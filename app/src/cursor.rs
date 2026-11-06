//! The system's pointer, hidden over the selection screen while its + is drawn (two would be seen,
//! the + a frame behind: owner, 2026-11-02). GPUI has no pointer style for "none", and sets the one
//! it has on every move; the system's count of who wants the pointer shown (`ShowCursor`) is kept
//! apart from that, for this thread's windows, which the selection screen covers the monitors with.

use std::cell::Cell;

use windows::Win32::UI::WindowsAndMessaging::ShowCursor;

thread_local! {
    static HIDDEN: Cell<bool> = const { Cell::new(false) };
}

/// Hidden or shown again; each once, so that the count is back where it was when the screen is
/// down.
pub fn set_hidden(hidden: bool) {
    HIDDEN.with(|now| {
        if now.get() != hidden {
            unsafe {
                ShowCursor(!hidden);
            }
            now.set(hidden);
        }
    });
}
