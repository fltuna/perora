//! Windows: the fonts the system has, by the names of their families.

use std::collections::BTreeMap;

use windows::Win32::Foundation::LPARAM;
use windows::Win32::Graphics::Gdi::{
    DEFAULT_CHARSET, EnumFontFamiliesExW, GetDC, LOGFONTW, ReleaseDC, TEXTMETRICW,
};

/// The names of the font families the system has, in order, each once. Without those that only
/// write downwards (the system lists them a second time, with an `@` before the name), and without
/// any when the system cannot be asked.
pub fn families() -> Vec<String> {
    // By their names without regard to case, so that they are in the order a reader expects.
    let mut found: BTreeMap<String, String> = BTreeMap::new();
    let asked = LOGFONTW {
        lfCharSet: DEFAULT_CHARSET,
        ..Default::default()
    };
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Vec::new();
        }
        EnumFontFamiliesExW(
            screen,
            &asked,
            Some(one),
            LPARAM(&raw mut found as isize),
            0,
        );
        ReleaseDC(None, screen);
    }
    found.into_values().collect()
}

/// One font of a family: its family is one more of those found (told once for each of its fonts
/// and each kind of letters it has).
unsafe extern "system" fn one(
    font: *const LOGFONTW,
    _: *const TEXTMETRICW,
    _: u32,
    found: LPARAM,
) -> i32 {
    let found = unsafe { &mut *(found.0 as *mut BTreeMap<String, String>) };
    let face = unsafe { (*font).lfFaceName };
    let length = face
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(face.len());
    let name = String::from_utf16_lossy(&face[..length]);
    if !name.is_empty() && !name.starts_with('@') {
        found.entry(name.to_lowercase()).or_insert(name);
    }
    // (Go on.)
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_systems_fonts_are_named_once_each_in_order() {
        let names = families();
        // (A system that cannot be asked gives none: nothing to say of those.)
        if names.is_empty() {
            return;
        }
        assert!(
            names
                .iter()
                .all(|name| !name.is_empty() && !name.starts_with('@'))
        );
        let lower: Vec<String> = names.iter().map(|name| name.to_lowercase()).collect();
        assert!(lower.windows(2).all(|pair| pair[0] < pair[1]));
        // Every Windows has this one.
        assert!(names.iter().any(|name| name == "Segoe UI"));
    }
}
