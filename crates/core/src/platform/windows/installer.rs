//! Windows: Perora's installer, as an update runs it, and where it installed Perora.

use std::path::{Path, PathBuf};

/// Where the installer notes the folder it installed into (its `MANUPRODUCTKEY`: the key's own
/// value), in the user's registry.
const INSTALLED_INTO: &str = r"Software\f2a\Perora";

/// The folder Perora is installed in, as its installer noted it; none when it is not installed.
pub fn installed_folder() -> Option<PathBuf> {
    super::startup::command_in(INSTALLED_INTO, "")
        .map(|folder| PathBuf::from(folder.trim().trim_matches('"')))
        .filter(|folder| !folder.as_os_str().is_empty())
}

/// Whether `program` is the installed Perora: in the folder the installer installed into.
pub fn is_installed(program: &Path) -> bool {
    let (Some(folder), Some(here)) = (installed_folder(), program.parent()) else {
        return false;
    };
    let same = |a: &Path, b: &Path| {
        let canonical = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.into());
        canonical(a)
            .to_string_lossy()
            .eq_ignore_ascii_case(&canonical(b).to_string_lossy())
    };
    same(&folder, here)
}

/// Writes an installer out, into a folder of its own among the temporary files, and runs it with
/// `parameters`. It is running when this returns `Ok`: Perora is then to end, as the installer
/// replaces it. (The folder stays: the installer runs from it.)
pub fn run(installer: &[u8], version: &str, parameters: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;
    use windows::core::{HSTRING, w};

    // (The version is semver's: letters, digits, dots, hyphens and pluses, nothing for a path to
    // trip on.)
    let mut random = [0u8; 6];
    getrandom::fill(&mut random).map_err(|error| error.to_string())?;
    let random: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let folder = std::env::temp_dir().join(format!("Perora-{version}-updater-{random}"));
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let path = folder.join(format!("Perora-{version}-installer.exe"));
    std::fs::write(&path, installer).map_err(|error| error.to_string())?;

    let ran = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(path.as_os_str()),
            &HSTRING::from(parameters),
            None,
            SW_SHOW,
        )
    };
    // (Above 32 is a success, as the call has always said.)
    if ran.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("the installer could not be run ({})", ran.0 as usize))
    }
}
