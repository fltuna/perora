//! The system's own handle of a GPUI window, for what GPUI has no call for: hiding a window at once,
//! keeping it out of sight until it has drawn, its look in the system's animations.

use gpui::Window;
use perora_core::geometry::Rect;
use perora_core::platform::WindowId;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible, SW_HIDE, SW_SHOW,
    SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    WS_EX_NOACTIVATE,
};

pub fn of(window: &Window) -> Option<WindowId> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(WindowId(handle.hwnd.get())),
        _ => None,
    }
}

fn hwnd(window: WindowId) -> HWND {
    HWND(window.0 as *mut _)
}

/// Shown, and not minimized.
pub fn on_screen(window: WindowId) -> bool {
    let window = hwnd(window);
    unsafe { IsWindowVisible(window).as_bool() && !IsIconic(window).as_bool() }
}

/// Out of sight at once (GPUI only closes windows).
pub fn hide(window: WindowId) {
    unsafe {
        let _ = ShowWindow(hwnd(window), SW_HIDE);
    }
}

/// Never the active window, not even when it is clicked: for what appears over a game or whatever
/// the user is in (the notice), which must not take the keyboard from it.
pub fn never_active(window: WindowId) {
    let window = hwnd(window);
    unsafe {
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
        SetWindowLongPtrW(window, GWL_EXSTYLE, style | WS_EX_NOACTIVATE.0 as isize);
    }
}

/// Back on screen, as it was.
/// Shown, without being made the window in front.
pub fn show_behind(window: WindowId) {
    unsafe {
        let _ = ShowWindow(hwnd(window), windows::Win32::UI::WindowsAndMessaging::SW_SHOWNA);
    }
}

/// Whether it is the window in front (the one with the keyboard).
pub fn in_front(window: WindowId) -> bool {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() == hwnd(window) }
}

pub fn show(window: WindowId) {
    unsafe {
        let _ = ShowWindow(hwnd(window), SW_SHOW);
    }
}

/// Where the window is, in physical pixels of the virtual screen.
pub fn rect(window: WindowId) -> Option<Rect> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd(window), &mut rect) }.ok()?;
    Some(Rect::new(
        rect.left,
        rect.top,
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
    ))
}

/// Moves the window there (physical pixels), as large as it is.
pub fn move_to(window: WindowId, x: i32, y: i32) {
    unsafe {
        let _ = SetWindowPos(
            hwnd(window),
            None,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Puts the window there, at that size (physical pixels).
pub fn place(window: WindowId, area: Rect) {
    unsafe {
        let _ = SetWindowPos(
            hwnd(window),
            None,
            area.x,
            area.y,
            area.width as i32,
            area.height as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Shows the window behind every other at that size (physical pixels), without making it the active
/// one: so that the system gives it its size and it has something to draw into. Its inside, as large
/// as the system let it be (it keeps a window within about the size of a monitor).
pub fn behind_everything(window: WindowId, area: Rect) -> (u32, u32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, HWND_BOTTOM, SWP_SHOWWINDOW};
    unsafe {
        let _ = SetWindowPos(
            hwnd(window),
            Some(HWND_BOTTOM),
            area.x,
            area.y,
            area.width as i32,
            area.height as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let mut inside = RECT::default();
        let _ = GetClientRect(hwnd(window), &mut inside);
        (
            (inside.right - inside.left).max(1) as u32,
            (inside.bottom - inside.top).max(1) as u32,
        )
    }
}
