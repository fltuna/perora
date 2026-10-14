//! Windows: the key combinations with the Windows key that the system's shell keeps for itself
//! (Win+Shift+S for its own snipping, Win+S for searching), and letting go of them.
//!
//! The shell takes them as it starts, all of them, and nothing says which program they go to:
//! there is no giving one to another program. What there is, is a list of letters in the user's
//! registry that the shell reads as it starts and then leaves alone, with every combination of the
//! Windows key and that letter. Those are free then, for whoever asks for them first. So a change
//! here counts from the next time the shell starts: the user signs in again, or the shell is
//! started anew.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;

use super::startup::{set_command_in, text_in};

/// Where the shell's own settings are, and the letters it leaves alone among them.
const SHELL: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const LEFT_ALONE: &str = "DisabledHotkeys";

/// Whether the shell is told to leave the combinations of the Windows key and that letter alone.
/// (Told: it does so from the next time it starts.)
pub fn is_left_alone(letter: char) -> bool {
    text_in(SHELL, LEFT_ALONE)
        .ok()
        .flatten()
        .is_some_and(|letters| has(&letters, letter))
}

/// Tells the shell to leave the combinations of the Windows key and that letter alone, or to take
/// them again. The other letters it was told to leave alone stay as they are: a list that cannot
/// be read is not written over.
pub fn leave_alone(letter: char, alone: bool) -> Result<(), String> {
    let letters = text_in(SHELL, LEFT_ALONE)?.unwrap_or_default();
    let now = with(&letters, letter, alone);
    if now == letters {
        return Ok(());
    }
    set_command_in(SHELL, LEFT_ALONE, (!now.is_empty()).then_some(now.as_str()))
}

fn has(letters: &str, letter: char) -> bool {
    letters.chars().any(|one| one.eq_ignore_ascii_case(&letter))
}

/// The letters with that one among them, or without it; the others as they were.
fn with(letters: &str, letter: char, among: bool) -> String {
    let mut now: String = letters
        .chars()
        .filter(|one| !one.eq_ignore_ascii_case(&letter))
        .collect();
    if among && has(letters, letter) {
        return letters.to_owned();
    }
    if among {
        now.push(letter.to_ascii_uppercase());
    }
    now
}

/// A program that is run for what it does, without a window of its own flashing up.
const WITHOUT_A_WINDOW: u32 = 0x0800_0000;
/// A program that is not kept with the one that started it: should whatever started Perora end
/// everything it started along with it, the shell is not among that.
const ON_ITS_OWN: u32 = 0x0100_0000;

/// Where the system is: its own programs are run from there by their whole names, not whatever
/// has their name in the folder Perora is in.
fn system_root() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

/// One of the system's own tools, asked about the shell of this user's session only: someone else
/// signed in on this machine has a shell of their own, which is none of Perora's business.
fn about_the_shell(tool: &str, session: u32) -> Command {
    let mut command = Command::new(system_root().join("System32").join(tool));
    command
        .args(["/FI", "IMAGENAME eq explorer.exe"])
        .args(["/FI", &format!("SESSION eq {session}")])
        .creation_flags(WITHOUT_A_WINDOW);
    command
}

fn shell_is_running(session: u32) -> bool {
    about_the_shell("tasklist.exe", session)
        .args(["/FO", "CSV", "/NH"])
        .output()
        .is_ok_and(|listed| {
            String::from_utf8_lossy(&listed.stdout)
                .to_ascii_lowercase()
                .contains("explorer.exe")
        })
}

/// How often the shell is looked for, how long it is given to be gone once ended, and how long to
/// be back on its own after that.
const LOOKED_FOR_EVERY: Duration = Duration::from_millis(250);
const GONE_WITHIN: u32 = 8;
const BACK_WITHIN: u32 = 16;

/// Starts the shell anew (the taskbar is gone for a moment, and the folders that were open are
/// closed): it reads its settings as it starts. The system starts it again on its own when it is
/// ended; should it not have by a few seconds later, it is started from here.
pub fn restart_shell() -> Result<(), String> {
    let mut session = 0u32;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }
        .map_err(|error| error.message())?;

    // (What the tool says of how it went is not asked: whether the shell is there is looked at.)
    about_the_shell("taskkill.exe", session)
        .arg("/F")
        .status()
        .map_err(|error| error.to_string())?;
    let mut gone = false;
    for _ in 0..GONE_WITHIN {
        std::thread::sleep(LOOKED_FOR_EVERY);
        if !shell_is_running(session) {
            gone = true;
            break;
        }
    }
    for _ in 0..BACK_WITHIN {
        if shell_is_running(session) {
            // (Seen gone and back, or never seen gone: back so quickly, or never ended. Either
            // way there is a shell, and a second one would only open a folder.)
            return Ok(());
        }
        std::thread::sleep(LOOKED_FOR_EVERY);
    }
    if !gone {
        return Ok(());
    }
    // As the system would start it, as far as that can be had: from its own folder, knowing
    // nothing of Perora, with nothing of Perora's to read or write, and on its own. (It goes on
    // when Perora ends either way: nothing ends what a program started just because the program
    // ends. On its own is for whatever started Perora, should that keep its programs together;
    // where that does not let one go, the shell is started with them after all.)
    let root = system_root();
    let shell = |on_its_own: bool| {
        let mut shell = Command::new(root.join("explorer.exe"));
        shell
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("PERORA_") {
                shell.env_remove(name);
            }
        }
        if on_its_own {
            shell.creation_flags(ON_ITS_OWN);
        }
        shell.spawn().map(|_| ())
    };
    shell(true)
        .or_else(|_| shell(false))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{has, with};

    #[test]
    fn a_letter_is_put_among_those_left_alone_or_taken_out_and_the_others_stay() {
        assert_eq!(with("", 'S', true), "S");
        assert_eq!(with("VX", 'S', true), "VXS");
        assert_eq!(with("VSX", 'S', true), "VSX");
        assert_eq!(with("vsx", 's', true), "vsx");
        assert_eq!(with("VSX", 'S', false), "VX");
        assert_eq!(with("vsx", 'S', false), "vx");
        assert_eq!(with("S", 'S', false), "");
        assert_eq!(with("VX", 'S', false), "VX");

        assert!(has("vsx", 'S'));
        assert!(!has("VX", 'S'));
    }
}
