//! Windows: what the system starts when the user signs in, as the user's own list of it has it (the
//! `Run` key of the user's registry: nothing here needs more rights than the user has), and whether
//! the user switched an entry of that list off in the system's own settings.

use std::ffi::c_void;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};
use windows::core::PCWSTR;

/// What is started at sign-in: a name, and the command that is run under it.
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Where the system notes which of those the user switched off (in the task manager, in the
/// settings): by the same name, some bytes of which the first is odd for "off".
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// The command started at sign-in under that name; `None` when there is none, or the user
/// switched it off in the system's settings.
pub fn command(name: &str) -> Option<String> {
    if switched_off(APPROVED, name) {
        return None;
    }
    command_in(RUN, name)
}

/// Has that command started at sign-in under that name, or nothing any more (`None`). Having it
/// started also undoes its having been switched off in the system's settings: the user asked for
/// it here.
pub fn set_command(name: &str, command: Option<&str>) -> Result<(), String> {
    set_command_in(RUN, name, command)?;
    // (After the command is there: switched on again with an old command still under the name,
    // what would start is not what was asked for.)
    if command.is_some() {
        remove(APPROVED, name)?;
    }
    Ok(())
}

/// The text under a name in a key of the user's registry; `None` when there is none.
pub(super) fn command_in(key: &str, name: &str) -> Option<String> {
    text_in(key, name).ok().flatten()
}

/// The text under a name in a key of the user's registry: none when there is none, and the reason
/// when what is there cannot be read (it is not a text, or the system would not say). For whoever
/// writes there next: what could not be read is not nothing.
pub(super) fn text_in(key: &str, name: &str) -> Result<Option<String>, String> {
    let (key, name) = (wide(key), wide(name));
    let mut length = 0u32;
    let asked = |data: Option<*mut c_void>, length: &mut u32| unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            data,
            Some(length),
        )
    };
    let there = asked(None, &mut length);
    if there == ERROR_FILE_NOT_FOUND || there == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    there.ok().map_err(|error| error.message())?;
    if length == 0 {
        return Ok(None);
    }
    // (In bytes, with the end of the text.)
    let mut text = vec![0u16; (length as usize).div_ceil(2)];
    asked(Some(text.as_mut_ptr().cast()), &mut length)
        .ok()
        .map_err(|error| error.message())?;
    let end = text
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(text.len());
    Ok(Some(String::from_utf16_lossy(&text[..end])))
}

/// Puts a text under a name in a key of the user's registry, or takes the name out of it.
pub(super) fn set_command_in(key: &str, name: &str, command: Option<&str>) -> Result<(), String> {
    let Some(command) = command else {
        return remove(key, name);
    };
    let (key, name, command) = (wide(key), wide(name), wide(command));
    let set = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(command.as_ptr().cast()),
            (command.len() * 2) as u32,
        )
    };
    set.ok().map_err(|error| error.message())
}

/// Takes a name out of a key. One that is not there is out of it already.
fn remove(key: &str, name: &str) -> Result<(), String> {
    let (key, name) = (wide(key), wide(name));
    let removed = unsafe {
        RegDeleteKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
        )
    };
    if removed == ERROR_FILE_NOT_FOUND || removed == ERROR_PATH_NOT_FOUND {
        return Ok(());
    }
    removed.ok().map_err(|error| error.message())
}

fn switched_off(key: &str, name: &str) -> bool {
    let (key, name) = (wide(key), wide(name));
    let mut noted = [0u8; 64];
    let mut length = noted.len() as u32;
    let asked = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_BINARY,
            None,
            Some(noted.as_mut_ptr().cast()),
            Some(&mut length),
        )
    };
    asked == ERROR_SUCCESS && length > 0 && noted[0] % 2 == 1
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteKeyW};
    use windows::core::PCWSTR;

    use super::{command_in, remove, set_command_in, switched_off, wide};

    /// A key of this test's own, gone when the test is over: not the list the system reads.
    struct Scratch(String);

    impl Scratch {
        fn new(name: &str) -> Self {
            Self(format!(
                r"Software\Perora tests {}-{name}",
                std::process::id()
            ))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let key = wide(&self.0);
            unsafe {
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()));
            }
        }
    }

    #[test]
    fn a_command_is_written_read_back_and_taken_out() {
        let key = Scratch::new("command");
        assert_eq!(command_in(&key.0, "Perora"), None);

        let command = r#""C:\Program Files\Perora\perora.exe" --tray"#;
        set_command_in(&key.0, "Perora", Some(command)).unwrap();
        assert_eq!(command_in(&key.0, "Perora").as_deref(), Some(command));
        assert_eq!(command_in(&key.0, "Another"), None);

        set_command_in(&key.0, "Perora", None).unwrap();
        assert_eq!(command_in(&key.0, "Perora"), None);
        // (Taken out of where it is not: nothing to do, and nothing wrong.)
        set_command_in(&key.0, "Perora", None).unwrap();
        remove(r"Software\Perora tests, no such key", "Perora").unwrap();
    }

    #[test]
    fn what_the_user_switched_off_is_told_by_an_odd_first_byte() {
        use windows::Win32::System::Registry::{REG_BINARY, RegSetKeyValueW};

        let key = Scratch::new("approved");
        assert!(!switched_off(&key.0, "Perora"));
        for (first, off) in [(2u8, false), (3, true), (6, false), (7, true)] {
            let noted = [first, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let (path, name) = (wide(&key.0), wide("Perora"));
            let set = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(path.as_ptr()),
                    PCWSTR(name.as_ptr()),
                    REG_BINARY.0,
                    Some(noted.as_ptr().cast()),
                    noted.len() as u32,
                )
            };
            assert!(set.is_ok());
            assert_eq!(switched_off(&key.0, "Perora"), off, "{first}");
        }
        remove(&key.0, "Perora").unwrap();
        assert!(!switched_off(&key.0, "Perora"));
    }
}
