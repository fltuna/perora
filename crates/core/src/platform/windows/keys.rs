//! Windows: which keys of the keyboard are held as the message being handled was sent, by where they
//! are rather than by what they write. For recording a shortcut: the letter a key writes depends on
//! Shift and on the layout, the key a shortcut is registered by does not.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, VIRTUAL_KEY, VK_0, VK_1, VK_2, VK_3, VK_4, VK_5, VK_6, VK_7,
    VK_8, VK_9, VK_A, VK_ADD, VK_B, VK_BACK, VK_C, VK_CONTROL, VK_D, VK_DECIMAL, VK_DELETE,
    VK_DIVIDE, VK_DOWN, VK_E, VK_END, VK_F, VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8,
    VK_F9, VK_F10, VK_F11, VK_F12, VK_F13, VK_F14, VK_F15, VK_F16, VK_F17, VK_F18, VK_F19, VK_F20,
    VK_F21, VK_F22, VK_F23, VK_F24, VK_G, VK_H, VK_HOME, VK_I, VK_INSERT, VK_J, VK_K, VK_L,
    VK_LEFT, VK_LWIN, VK_M, VK_MENU, VK_MULTIPLY, VK_N, VK_NEXT, VK_NUMLOCK, VK_NUMPAD0,
    VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3, VK_NUMPAD4, VK_NUMPAD5, VK_NUMPAD6, VK_NUMPAD7, VK_NUMPAD8,
    VK_NUMPAD9, VK_O, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7,
    VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS, VK_P, VK_PAUSE, VK_PRIOR, VK_Q, VK_R,
    VK_RETURN, VK_RIGHT, VK_RWIN, VK_S, VK_SCROLL, VK_SHIFT, VK_SNAPSHOT, VK_SPACE, VK_SUBTRACT,
    VK_T, VK_TAB, VK_U, VK_UP, VK_V, VK_W, VK_X, VK_Y, VK_Z,
};

/// The keys a shortcut can end in, each by the name the shortcut is written with (`Ctrl+Shift+H`,
/// as the Tauri version wrote it from the browser's names, and as `global-hotkey` reads it back to
/// the same key). Keys without such a name (the ones only a Japanese keyboard has besides these,
/// the menu key, F25 and on) are not among them.
const KEYS: &[(VIRTUAL_KEY, &str)] = &[
    (VK_A, "A"),
    (VK_B, "B"),
    (VK_C, "C"),
    (VK_D, "D"),
    (VK_E, "E"),
    (VK_F, "F"),
    (VK_G, "G"),
    (VK_H, "H"),
    (VK_I, "I"),
    (VK_J, "J"),
    (VK_K, "K"),
    (VK_L, "L"),
    (VK_M, "M"),
    (VK_N, "N"),
    (VK_O, "O"),
    (VK_P, "P"),
    (VK_Q, "Q"),
    (VK_R, "R"),
    (VK_S, "S"),
    (VK_T, "T"),
    (VK_U, "U"),
    (VK_V, "V"),
    (VK_W, "W"),
    (VK_X, "X"),
    (VK_Y, "Y"),
    (VK_Z, "Z"),
    (VK_0, "0"),
    (VK_1, "1"),
    (VK_2, "2"),
    (VK_3, "3"),
    (VK_4, "4"),
    (VK_5, "5"),
    (VK_6, "6"),
    (VK_7, "7"),
    (VK_8, "8"),
    (VK_9, "9"),
    (VK_F1, "F1"),
    (VK_F2, "F2"),
    (VK_F3, "F3"),
    (VK_F4, "F4"),
    (VK_F5, "F5"),
    (VK_F6, "F6"),
    (VK_F7, "F7"),
    (VK_F8, "F8"),
    (VK_F9, "F9"),
    (VK_F10, "F10"),
    (VK_F11, "F11"),
    (VK_F12, "F12"),
    (VK_F13, "F13"),
    (VK_F14, "F14"),
    (VK_F15, "F15"),
    (VK_F16, "F16"),
    (VK_F17, "F17"),
    (VK_F18, "F18"),
    (VK_F19, "F19"),
    (VK_F20, "F20"),
    (VK_F21, "F21"),
    (VK_F22, "F22"),
    (VK_F23, "F23"),
    (VK_F24, "F24"),
    (VK_NUMPAD0, "Numpad0"),
    (VK_NUMPAD1, "Numpad1"),
    (VK_NUMPAD2, "Numpad2"),
    (VK_NUMPAD3, "Numpad3"),
    (VK_NUMPAD4, "Numpad4"),
    (VK_NUMPAD5, "Numpad5"),
    (VK_NUMPAD6, "Numpad6"),
    (VK_NUMPAD7, "Numpad7"),
    (VK_NUMPAD8, "Numpad8"),
    (VK_NUMPAD9, "Numpad9"),
    (VK_ADD, "NumpadAdd"),
    (VK_DECIMAL, "NumpadDecimal"),
    (VK_DIVIDE, "NumpadDivide"),
    (VK_MULTIPLY, "NumpadMultiply"),
    (VK_SUBTRACT, "NumpadSubtract"),
    (VK_UP, "ArrowUp"),
    (VK_DOWN, "ArrowDown"),
    (VK_LEFT, "ArrowLeft"),
    (VK_RIGHT, "ArrowRight"),
    (VK_OEM_3, "Backquote"),
    (VK_OEM_5, "Backslash"),
    (VK_OEM_4, "BracketLeft"),
    (VK_OEM_6, "BracketRight"),
    (VK_OEM_COMMA, "Comma"),
    (VK_OEM_PLUS, "Equal"),
    (VK_OEM_MINUS, "Minus"),
    (VK_OEM_PERIOD, "Period"),
    (VK_OEM_7, "Quote"),
    (VK_OEM_1, "Semicolon"),
    (VK_OEM_2, "Slash"),
    (VK_BACK, "Backspace"),
    (VK_RETURN, "Enter"),
    (VK_SPACE, "Space"),
    (VK_TAB, "Tab"),
    (VK_DELETE, "Delete"),
    (VK_END, "End"),
    (VK_HOME, "Home"),
    (VK_INSERT, "Insert"),
    (VK_NEXT, "PageDown"),
    (VK_PRIOR, "PageUp"),
];

fn held(key: VIRTUAL_KEY) -> bool {
    // (The high bit: down.)
    let state = unsafe { GetKeyState(key.0 as i32) };
    state < 0
}

/// The key a shortcut could end in that is held now, by its name; `None` when none is.
pub fn held_key() -> Option<&'static str> {
    KEYS.iter()
        .find(|(key, _)| held(*key))
        .map(|(_, name)| *name)
}

/// The keys a shortcut can end in that a window is never told were pressed (the system gives it
/// no press of them, only their letting go, or nothing), by the same names.
const UNHEARD: &[(VIRTUAL_KEY, &str)] = &[
    (VK_SNAPSHOT, "PrintScreen"),
    (VK_PAUSE, "Pause"),
    (VK_SCROLL, "ScrollLock"),
    (VK_NUMLOCK, "NumLock"),
];

fn down_now(key: VIRTUAL_KEY) -> bool {
    // (The high bit: down at this moment, whichever window has the keyboard.)
    let state = unsafe { GetAsyncKeyState(key.0 as i32) };
    state < 0
}

/// One of the keys a window is not told of that is down at this moment, by its name, with which of
/// Ctrl, Alt, Shift and the Windows key are down with it; `None` when none is.
pub fn unheard_key() -> Option<(&'static str, [bool; 4])> {
    let (_, name) = UNHEARD.iter().find(|(key, _)| down_now(*key))?;
    Some((
        name,
        [
            down_now(VK_CONTROL),
            down_now(VK_MENU),
            down_now(VK_SHIFT),
            down_now(VK_LWIN) || down_now(VK_RWIN),
        ],
    ))
}

/// Which of Ctrl, Alt, Shift and the Windows key are held now, in that order.
pub fn held_modifiers() -> [bool; 4] {
    [
        held(VK_CONTROL),
        held(VK_MENU),
        held(VK_SHIFT),
        held(VK_LWIN) || held(VK_RWIN),
    ]
}
