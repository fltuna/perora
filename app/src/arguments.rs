//! What Perora is started with. 0.1.x wrote these into the registry (start at sign-in, "Open with
//! Perora", the uninstaller's hook), so they keep meaning what they meant.

use std::path::{Path, PathBuf};

use perora_core::settings;

/// Start in the notification area, without the main window (start at sign-in).
pub const START_IN_TRAY: &str = "--tray";
/// Asks the Perora that is running to capture.
pub const CAPTURE: &str = "--capture";
/// Run by the uninstaller (`perora.exe --uninstalling`): give back what Perora took, then end.
pub const UNINSTALLING: &str = "--uninstalling";
/// The letter of the system's combination for snipping, Win + Shift + S.
pub const SNIPPING_LETTER: char = 'S';

/// What one start asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Open this image in the editor (`--open <file>`); the main window is not shown for it.
    Open(PathBuf),
    Capture,
    /// Nothing but to be there (`--tray`).
    Tray,
    /// Show the main window.
    Main,
}

/// Reads `arguments` (the program's name first, as the system gives them). `from`: the folder the
/// start was made in, which a relative file name is in.
pub fn asked(arguments: &[String], from: &Path) -> Asked {
    if let Some(file) = file_in(arguments, from) {
        Asked::Open(file)
    } else if arguments.iter().any(|argument| argument == CAPTURE) {
        Asked::Capture
    } else if arguments.iter().any(|argument| argument == START_IN_TRAY) {
        Asked::Tray
    } else {
        Asked::Main
    }
}

fn file_in(arguments: &[String], from: &Path) -> Option<PathBuf> {
    let at = arguments
        .iter()
        .position(|argument| argument == perora_core::platform::OPEN_ARGUMENT)?;
    let file = PathBuf::from(arguments.get(at + 1)?);
    Some(if file.is_absolute() {
        file
    } else {
        from.join(file)
    })
}

/// Perora is being uninstalled: the system's combination for snipping is the shell's again, when
/// Perora had it (the settings say so; otherwise the letter, if it is there, is someone else's).
/// From the next time the shell starts, as taking it was. "Open with Perora" goes too, when it is
/// this program's. (What starts Perora at sign-in the uninstaller takes away itself.)
pub fn uninstalling() {
    if settings::read_without_app().is_some_and(|settings| settings.take_snipping)
        && let Err(detail) = perora_core::platform::leave_hotkeys_alone(SNIPPING_LETTER, false)
    {
        eprintln!("the system's combination for snipping could not be given back: {detail}");
    }
    if let Ok(program) = std::env::current_exe() {
        let program = program.display().to_string();
        if explorer_entry_is_ours(&program)
            && let Err(detail) = perora_core::platform::open_from_explorer(&program, None)
        {
            eprintln!("the file manager's entry could not be taken away: {detail}");
        }
    }
}

/// Whether "Open with Perora" in the file manager opens with this very program.
pub fn explorer_entry_is_ours(program: &str) -> bool {
    perora_core::platform::opened_from_explorer().is_some_and(|there| {
        there.eq_ignore_ascii_case(&perora_core::platform::explorer_command(program))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn each_start_asks_for_one_thing() {
        let here = Path::new(r"C:\Users\someone");
        assert_eq!(asked(&args(&["perora.exe"]), here), Asked::Main);
        assert_eq!(asked(&args(&["perora.exe", "--tray"]), here), Asked::Tray);
        assert_eq!(
            asked(&args(&["perora.exe", "--capture"]), here),
            Asked::Capture
        );
        assert_eq!(
            asked(&args(&["perora.exe", "--open", r"D:\a b\c.png"]), here),
            Asked::Open(PathBuf::from(r"D:\a b\c.png"))
        );
        // A relative name is in the folder the start was made in.
        assert_eq!(
            asked(&args(&["perora.exe", "--open", "c.png"]), here),
            Asked::Open(PathBuf::from(r"C:\Users\someone\c.png"))
        );
        // Opening wins: the file is what was asked for.
        assert_eq!(
            asked(
                &args(&["perora.exe", "--tray", "--open", r"D:\c.png"]),
                here
            ),
            Asked::Open(PathBuf::from(r"D:\c.png"))
        );
        // `--open` with nothing after it opens nothing.
        assert_eq!(asked(&args(&["perora.exe", "--open"]), here), Asked::Main);
    }
}
