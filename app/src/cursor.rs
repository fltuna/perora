//! The system's pointer, hidden over the selection screen while its + is drawn (two would be seen,
//! the + a frame behind: owner, 2026-11-02). GPUI has no pointer style for "none", and sets the one
//! it has on every move; the system's count of who wants the pointer shown (`ShowCursor`) is kept
//! apart from that, for this thread's windows, which the selection screen covers the monitors with.

use std::cell::Cell;

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos, ShowCursor};

thread_local! {
    static HIDDEN: Cell<bool> = const { Cell::new(false) };
}

/// The pointer looked at again where it is, as if it had moved: the system shows or hides it only as
/// it moves, so a screen that came up under a pointer standing still left it there to be seen until
/// it did (owner, 2026-11-06: 「カーソル動かすまでカーソル消えないw」). Put where it is, it is not moved.
pub fn refresh() {
    let mut at = POINT::default();
    unsafe {
        if GetCursorPos(&mut at).is_ok() {
            let _ = SetCursorPos(at.x, at.y);
        }
    }
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
