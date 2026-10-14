//! Windows: the windows on the desktop, front to back, each one's own picture, and the pointer.

use std::mem::size_of;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAK, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_TRANSITIONS_FORCEDISABLED,
    DwmGetColorizationColor, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDIBits,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetClassNameW, GetCursorPos, GetWindowLongW, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsHungAppWindow, IsIconic, IsWindowVisible,
    LWA_ALPHA, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SetLayeredWindowAttributes, SetWindowLongW, SystemParametersInfoW, WS_EX_APPWINDOW,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use super::{Bitmap, MemoryDc, ScreenDc, ensure_dpi_awareness, os_error};
use crate::geometry::{Point, Rect};
use crate::platform::{Desktop, Frame, PlatformError, Result, Window, WindowId};

/// `PrintWindow` asking the compositor for the window as it is drawn, hardware-accelerated content
/// included (browsers, most applications of the last decade). Not in the headers as a name.
const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);

/// The shell's own surfaces behind everything: the desktop is not a window to pick.
const DESKTOP_CLASSES: [&str; 2] = ["Progman", "WorkerW"];

pub struct Win32Desktop;

impl Win32Desktop {
    pub fn new() -> Self {
        ensure_dpi_awareness();
        Self
    }
}

impl Default for Win32Desktop {
    fn default() -> Self {
        Self::new()
    }
}

impl Desktop for Win32Desktop {
    fn windows(&self) -> Result<Vec<Window>> {
        unsafe extern "system" fn collect(window: HWND, data: LPARAM) -> BOOL {
            let handles = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
            handles.push(window);
            true.into()
        }

        // Top-level windows come in their stacking order, front first.
        let mut handles: Vec<HWND> = Vec::new();
        unsafe { EnumWindows(Some(collect), LPARAM(&raw mut handles as isize)) }.map_err(
            |error| PlatformError::Os {
                operation: "EnumWindows",
                detail: error.message(),
            },
        )?;

        let own_process = unsafe { GetCurrentProcessId() };
        Ok(handles
            .into_iter()
            .filter_map(|handle| describe(handle, own_process))
            .collect())
    }

    fn capture_window(&self, id: WindowId) -> Result<Frame> {
        let handle = HWND(id.0 as *mut _);
        // Drawing a window asks the window to draw: one that has stopped answering would keep
        // this call waiting for as long as it stays that way.
        if unsafe { IsHungAppWindow(handle) }.as_bool() {
            return Err(PlatformError::Os {
                operation: "PrintWindow",
                detail: "the window is not responding".into(),
            });
        }
        let bounds = frame_bounds(handle).ok_or(PlatformError::EmptyArea)?;

        // The window is drawn in its own coordinates: those of its full rectangle, which on current
        // Windows is larger than what is seen of it (invisible borders to grab for resizing).
        let mut full = RECT::default();
        unsafe { GetWindowRect(handle, &mut full) }.map_err(|error| PlatformError::Os {
            operation: "GetWindowRect",
            detail: error.message(),
        })?;
        let (width, height) = (full.right - full.left, full.bottom - full.top);
        if width <= 0 || height <= 0 {
            return Err(PlatformError::EmptyArea);
        }

        let screen = ScreenDc::get()?;
        let memory = MemoryDc::compatible_with(&screen)?;
        let bitmap = Bitmap::compatible_with(&screen, width, height)?;
        {
            let _selected = memory.select(&bitmap);
            if !unsafe { PrintWindow(handle, memory.0, PW_RENDERFULLCONTENT) }.as_bool() {
                return Err(os_error("PrintWindow"));
            }
        }

        let mut rgba = vec![0u8; width as usize * height as usize * 4];
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
                height as u32,
                Some(rgba.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if rows != height {
            return Err(os_error("GetDIBits"));
        }

        // A window that does not let itself be drawn this way (a game in exclusive fullscreen,
        // protected content) comes back as nothing but black.
        if rgba.chunks_exact(4).all(|pixel| pixel[..3] == [0, 0, 0]) {
            return Err(PlatformError::Os {
                operation: "PrintWindow",
                detail: "the window gave no picture of itself".into(),
            });
        }
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }

        let whole = Frame {
            area: Rect::new(full.left, full.top, width as u32, height as u32),
            rgba,
        };
        whole.crop(bounds).ok_or(PlatformError::EmptyArea)
    }

    fn pointer(&self) -> Result<Point> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }.map_err(|error| PlatformError::Os {
            operation: "GetCursorPos",
            detail: error.message(),
        })?;
        Ok(Point::new(point.x, point.y))
    }
}

pub fn accent_colour() -> Option<(u8, u8, u8)> {
    let mut colour = 0u32; // 0xAARRGGBB
    let mut opaque = BOOL::default();
    unsafe { DwmGetColorizationColor(&mut colour, &mut opaque) }.ok()?;
    Some(((colour >> 16) as u8, (colour >> 8) as u8, colour as u8))
}

/// How large, in pixels, the system draws a small icon (the notification area's): 16 at 100 %
/// scaling, more at more.
pub fn small_icon_side() -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    match unsafe { GetSystemMetrics(SM_CXSMICON) } {
        side if side > 0 => side as u32,
        _ => 16,
    }
}

/// The number the system counts changes of the clipboard with; `None` when it gives none (zero: this
/// process may not look at the clipboard).
pub fn clipboard_sequence() -> Option<u32> {
    match unsafe { GetClipboardSequenceNumber() } {
        0 => None,
        sequence => Some(sequence),
    }
}

/// Makes a window of this process appear and disappear at once, without the system's fading and
/// zooming: for windows that are parts of a screen, not things that open and close.
pub fn without_transitions(window: WindowId) {
    set_transitions(window, false);
}

/// Turns the system's animation of a window's appearing and disappearing on or off.
pub fn set_transitions(window: WindowId, enabled: bool) {
    let disabled = BOOL::from(!enabled);
    let _ = unsafe {
        DwmSetWindowAttribute(
            HWND(window.0 as *mut _),
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const disabled).cast(),
            size_of::<BOOL>() as u32,
        )
    };
}

/// Whether the user has the system animate what is in its windows (the "animation effects" of the
/// accessibility settings). Taken to be so when the system does not say.
pub fn animations_enabled() -> bool {
    let mut enabled = BOOL::from(true);
    let asked = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&raw mut enabled).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    asked.is_err() || enabled.as_bool()
}

/// Keeps a window of this process out of what the user switches between (Alt+Tab, the task view,
/// the taskbar): for a window that is a part of the screen, not a task.
pub fn not_among_tasks(window: WindowId) {
    let handle = HWND(window.0 as *mut _);
    let style = unsafe { GetWindowLongW(handle, GWL_EXSTYLE) } as u32;
    let style = (style | WS_EX_TOOLWINDOW.0) & !WS_EX_APPWINDOW.0;
    unsafe { SetWindowLongW(handle, GWL_EXSTYLE, style as i32) };
}

/// How much of a window of this process is seen, all of it alike: from 0 (nothing) to 255 (all of
/// it), or `None` for a window like any other again, which is what it is to be left as.
pub fn set_opacity(window: WindowId, opacity: Option<u8>) {
    let handle = HWND(window.0 as *mut _);
    let style = unsafe { GetWindowLongW(handle, GWL_EXSTYLE) } as u32;
    match opacity {
        Some(opacity) => unsafe {
            if style & WS_EX_LAYERED.0 == 0 {
                SetWindowLongW(handle, GWL_EXSTYLE, (style | WS_EX_LAYERED.0) as i32);
            }
            let _ = SetLayeredWindowAttributes(handle, COLORREF(0), opacity, LWA_ALPHA);
        },
        None if style & WS_EX_LAYERED.0 != 0 => unsafe {
            SetWindowLongW(handle, GWL_EXSTYLE, (style & !WS_EX_LAYERED.0) as i32);
        },
        None => {}
    }
}

/// Takes a window out of sight, or puts it back, without hiding it: to the system and to itself it
/// is still shown, and goes on with what it is doing.
pub fn set_cloaked(window: WindowId, cloaked: bool) {
    let cloaked = BOOL::from(cloaked);
    let _ = unsafe {
        DwmSetWindowAttribute(
            HWND(window.0 as *mut _),
            DWMWA_CLOAK,
            (&raw const cloaked).cast(),
            size_of::<BOOL>() as u32,
        )
    };
}

/// A window as something to pick, or `None` when it is not: hidden, minimized, not really there, one of
/// this process's own, the desktop, or something that lies over the screen without being seen.
fn describe(handle: HWND, own_process: u32) -> Option<Window> {
    if !unsafe { IsWindowVisible(handle) }.as_bool() || unsafe { IsIconic(handle) }.as_bool() {
        return None;
    }

    let mut process = 0u32;
    unsafe { GetWindowThreadProcessId(handle, Some(&mut process)) };
    if process == own_process {
        return None;
    }

    // "Cloaked": kept by the system but not shown (windows on other virtual desktops, suspended
    // store applications).
    let mut cloaked = 0u32;
    let asked = unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
    };
    if asked.is_ok() && cloaked != 0 {
        return None;
    }

    let mut class = [0u16; 64];
    let length = unsafe { GetClassNameW(handle, &mut class) }.max(0) as usize;
    let class = String::from_utf16_lossy(&class[..length]);
    if DESKTOP_CLASSES.contains(&class.as_str()) {
        return None;
    }

    let mut title = [0u16; 256];
    let length = unsafe { GetWindowTextW(handle, &mut title) }.max(0) as usize;
    let title = String::from_utf16_lossy(&title[..length]);

    // Overlays (of graphics drivers, chat applications, screen recorders) cover the whole screen and
    // let every click through: nobody sees them, and they would be "the window in front" everywhere.
    let style = unsafe { GetWindowLongW(handle, GWL_EXSTYLE) } as u32;
    let click_through = style & WS_EX_TRANSPARENT.0 != 0;
    let nameless_layer = style & WS_EX_LAYERED.0 != 0 && title.is_empty();
    if click_through || nameless_layer {
        return None;
    }

    let bounds = frame_bounds(handle)?;
    Some(Window {
        id: WindowId(handle.0 as isize),
        title,
        bounds,
    })
}

/// Where a window is seen to be: without the invisible borders its full rectangle includes.
fn frame_bounds(handle: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let asked = unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    };
    if asked.is_err() {
        unsafe { GetWindowRect(handle, &mut rect) }.ok()?;
    }

    let bounds = Rect::new(
        rect.left,
        rect.top,
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
    );
    (!bounds.is_empty()).then_some(bounds)
}
