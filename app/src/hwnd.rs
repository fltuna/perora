//! The system's own handle of a GPUI window, for what GPUI has no call for: hiding a window at once,
//! keeping it out of sight until it has drawn, its look in the system's animations.

use gpui::Window;
use perora_core::platform::WindowId;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, IsIconic, IsWindowVisible, SW_HIDE, SW_SHOW,
    SetWindowLongPtrW, ShowWindow, WS_EX_NOACTIVATE,
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
pub fn show(window: WindowId) {
    unsafe {
        let _ = ShowWindow(hwnd(window), SW_SHOW);
    }
}
