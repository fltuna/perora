//! Windows: the virtual screen through GDI.
//!
//! Everything here works in physical pixels. That only holds in a process that is per-monitor DPI aware;
//! otherwise Windows hands out scaled coordinates and stretched pixels for monitors whose scaling differs
//! from the primary one's. [`ensure_dpi_awareness`] sees to that.

pub mod clipboard;
pub mod desktop;
pub mod fonts;
pub mod hotkeys;
pub mod secrets;
pub mod sound;
pub mod startup;

use std::mem::size_of;
use std::sync::Once;

use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Globalization::{
    GetUserDefaultLocaleName, GetUserDefaultUILanguage, LCIDToLocaleName,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC,
    GetDIBits, GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ, HMONITOR, MONITORINFO, MONITORINFOEXW,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

use crate::geometry::Rect;
use crate::platform::{Frame, Monitor, PlatformError, Result, ScreenCapture, ScreenLayout};

/// Makes the process per-monitor DPI aware, unless something (an application manifest, the window
/// toolkit) already decided its awareness, in which case that stands.
pub fn ensure_dpi_awareness() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Fails when the awareness was set before: fine, there is nothing to do then.
        let _ =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    });
}

/// Captures by copying from the screen's device context: the whole virtual screen in one call, exactly as
/// composed at that moment.
pub struct GdiScreenCapture;

impl GdiScreenCapture {
    pub fn new() -> Self {
        ensure_dpi_awareness();
        Self
    }
}

impl Default for GdiScreenCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenCapture for GdiScreenCapture {
    fn layout(&self) -> Result<ScreenLayout> {
        ScreenLayout::new(monitors()?)
    }

    fn capture_all(&self) -> Result<Frame> {
        let layout = self.layout()?;
        let mut frame = capture(layout.bounds)?;
        frame.clear_outside(
            &layout
                .monitors
                .iter()
                .map(|monitor| monitor.bounds)
                .collect::<Vec<_>>(),
        );
        Ok(frame)
    }
}

pub fn ui_language() -> Option<String> {
    let mut name = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    let language = u32::from(unsafe { GetUserDefaultUILanguage() });
    let length = unsafe { LCIDToLocaleName(language, Some(&mut name), 0) };
    // The length counts the terminating zero.
    (length > 1).then(|| String::from_utf16_lossy(&name[..length as usize - 1]))
}

pub fn local_timestamp() -> String {
    let now = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}{:02}{:02}",
        now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond
    )
}

pub fn regional_locale() -> Option<String> {
    let mut name = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
    let length = unsafe { GetUserDefaultLocaleName(&mut name) };
    (length > 1).then(|| String::from_utf16_lossy(&name[..length as usize - 1]))
}

fn os_error(operation: &'static str) -> PlatformError {
    PlatformError::Os {
        operation,
        detail: windows::core::Error::from_thread().message(),
    }
}

fn monitors() -> Result<Vec<Monitor>> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let handles = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        handles.push(monitor);
        true.into()
    }

    let mut handles: Vec<HMONITOR> = Vec::new();
    if !unsafe { EnumDisplayMonitors(None, None, Some(collect), LPARAM(&raw mut handles as isize)) }
        .as_bool()
    {
        return Err(os_error("EnumDisplayMonitors"));
    }

    let mut monitors = Vec::with_capacity(handles.len());
    for handle in handles {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if !unsafe { GetMonitorInfoW(handle, (&raw mut info).cast::<MONITORINFO>()) }.as_bool() {
            continue; // unplugged between the two calls
        }

        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        // On failure the scale stays at 100 %.
        let _ = unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };

        let bounds = info.monitorInfo.rcMonitor;
        let work = info.monitorInfo.rcWork;
        let name_length = info
            .szDevice
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(info.szDevice.len());
        monitors.push(Monitor {
            name: String::from_utf16_lossy(&info.szDevice[..name_length]),
            bounds: Rect::new(
                bounds.left,
                bounds.top,
                (bounds.right - bounds.left) as u32,
                (bounds.bottom - bounds.top) as u32,
            ),
            work_area: Rect::new(
                work.left,
                work.top,
                (work.right - work.left) as u32,
                (work.bottom - work.top) as u32,
            ),
            scale: f64::from(dpi_x) / 96.0,
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }

    // Left to right, then top to bottom: an order that does not depend on how Windows numbers them.
    monitors.sort_by_key(|monitor| (monitor.bounds.x, monitor.bounds.y));
    Ok(monitors)
}

/// Copies an area of the virtual screen. Whatever part of it belongs to no monitor comes back black and
/// opaque; the caller decides what to make of it.
fn capture(area: Rect) -> Result<Frame> {
    if area.is_empty() {
        return Err(PlatformError::EmptyArea);
    }

    let (width, height) = (area.width as i32, area.height as i32);
    let screen = ScreenDc::get()?;
    let memory = MemoryDc::compatible_with(&screen)?;
    let bitmap = Bitmap::compatible_with(&screen, width, height)?;

    let mut rgba = vec![0u8; area.width as usize * area.height as usize * 4];
    {
        let _selected = memory.select(&bitmap);
        // CAPTUREBLT: layered windows too (tooltips, menus with shadows).
        unsafe {
            BitBlt(
                memory.0,
                0,
                0,
                width,
                height,
                Some(screen.0),
                area.x,
                area.y,
                SRCCOPY | CAPTUREBLT,
            )
        }
        .map_err(|error| PlatformError::Os {
            operation: "BitBlt",
            detail: error.message(),
        })?;
    }

    // The bitmap must not be selected into a device context while its bits are read.
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // negative: rows from the top
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let rows = unsafe {
        GetDIBits(
            screen.0,
            bitmap.0,
            0,
            area.height,
            Some(rgba.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if rows != height {
        return Err(os_error("GetDIBits"));
    }

    // GDI gives blue, green, red and an unused byte; the screen has no transparency.
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }

    Ok(Frame { area, rgba })
}

struct ScreenDc(HDC);

impl ScreenDc {
    fn get() -> Result<Self> {
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            Err(os_error("GetDC"))
        } else {
            Ok(Self(dc))
        }
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe { ReleaseDC(None, self.0) };
    }
}

struct MemoryDc(HDC);

impl MemoryDc {
    fn compatible_with(screen: &ScreenDc) -> Result<Self> {
        let dc = unsafe { CreateCompatibleDC(Some(screen.0)) };
        if dc.is_invalid() {
            Err(os_error("CreateCompatibleDC"))
        } else {
            Ok(Self(dc))
        }
    }

    /// Selects the bitmap until the returned guard is dropped.
    fn select<'a>(&'a self, bitmap: &'a Bitmap) -> Selected<'a> {
        let previous = unsafe { SelectObject(self.0, HGDIOBJ(bitmap.0.0)) };
        Selected { dc: self, previous }
    }
}

impl Drop for MemoryDc {
    fn drop(&mut self) {
        let _ = unsafe { DeleteDC(self.0) };
    }
}

struct Selected<'a> {
    dc: &'a MemoryDc,
    previous: HGDIOBJ,
}

impl Drop for Selected<'_> {
    fn drop(&mut self) {
        unsafe { SelectObject(self.dc.0, self.previous) };
    }
}

struct Bitmap(HBITMAP);

impl Bitmap {
    fn compatible_with(screen: &ScreenDc, width: i32, height: i32) -> Result<Self> {
        let bitmap = unsafe { CreateCompatibleBitmap(screen.0, width, height) };
        if bitmap.is_invalid() {
            Err(os_error("CreateCompatibleBitmap"))
        } else {
            Ok(Self(bitmap))
        }
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(HGDIOBJ(self.0.0)) };
    }
}
