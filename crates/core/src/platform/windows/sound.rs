//! Windows: playing a sound file, and the sounds the system comes with.
//!
//! Played through the system's media control interface, which knows wave files and whatever else
//! the system has a decoder for (MP3, WMA, AAC). One sound at a time: a new one cuts the one before
//! short, and none plays for longer than a signal does: a piece of music chosen by mistake is cut
//! off. The device that plays belongs to the thread that opened it, so one thread does all of it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use windows::Win32::Media::Multimedia::mciSendStringW;
use windows::core::HSTRING;

/// What the one sound that plays is called, to the media control interface.
const ALIAS: &str = "perora_sound";

/// Where the system keeps its sounds.
fn directory() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("WINDIR")?).join("Media"))
}

/// The sounds of the system that say what a notice says: that something arrived, that something
/// wants attention, that something went wrong. The system's folder holds many more (alarms, ring
/// tones, the sounds of its own windows), which are not for this.
const NOTICES: [&str; 9] = [
    "Windows Notify System Generic",
    "Windows Notify Messaging",
    "Windows Notify Email",
    "Windows Notify Calendar",
    "Windows Ding",
    "Windows Balloon",
    "Windows Exclamation",
    "Windows Error",
    "Windows Critical Stop",
];

/// The names of the system's sounds a notice can come with, in order: those of them this system
/// has. Each is a wave file in the system's folder of sounds, named without its ending.
pub fn system_sounds() -> Vec<String> {
    NOTICES
        .into_iter()
        .filter(|name| system_sound(name).is_some())
        .map(str::to_owned)
        .collect()
}

/// The file of one of the system's sounds a notice can come with; `None` for any other name, and
/// for one this system does not have. (Only those names: this never leads anywhere else.)
pub fn system_sound(name: &str) -> Option<PathBuf> {
    let name = NOTICES.into_iter().find(|known| *known == name)?;
    let path = directory()?.join(format!("{name}.wav"));
    path.is_file().then_some(path)
}

/// The endings of the files that are played: sound, in the kinds the system has a decoder for.
pub const KINDS: [&str; 5] = ["wav", "mp3", "wma", "m4a", "aac"];

fn is_wave(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ending| ending.eq_ignore_ascii_case("wav"))
}

/// Whether a path is a sound file to play: a file that is there, named in full, of one of the
/// kinds. Nothing else is handed to the system's player, which would as readily show a film or
/// fetch an address.
fn is_a_sound(path: &Path) -> bool {
    let kind = path.extension().and_then(|ending| ending.to_str());
    path.is_absolute()
        && kind.is_some_and(|kind| KINDS.iter().any(|known| known.eq_ignore_ascii_case(kind)))
        && path.is_file()
}

/// How often the player looks whether the sound has ended, to let go of its file.
const LOOKS: Duration = Duration::from_millis(100);

/// The longest a sound plays: what goes on after that is cut off.
const LONGEST: Duration = Duration::from_secs(3);

/// Plays a sound file from its start, in place of whatever is playing, for three seconds at most;
/// returns at once. What is
/// not a sound file (see `KINDS`), is not there or cannot be decoded is silence.
pub fn play(path: &Path) {
    if !is_a_sound(path) {
        return;
    }
    static PLAYER: OnceLock<Sender<PathBuf>> = OnceLock::new();
    let player = PLAYER.get_or_init(|| {
        let (player, sounds) = channel::<PathBuf>();
        std::thread::spawn(move || {
            // Since when the one sound is open, while it is.
            let mut open_since: Option<Instant> = None;
            loop {
                match sounds.recv_timeout(LOOKS) {
                    Ok(path) => {
                        let _ = send(&format!("close {ALIAS}"));
                        open_since = open(&path).is_ok().then(Instant::now);
                        if open_since.is_none() || send(&format!("play {ALIAS} from 0")).is_err() {
                            eprintln!("sound: {} could not be played", path.display());
                        }
                    }
                    // Played to its end, or for as long as a sound may: closed, which is also
                    // what stops it. The file is let go of (while it is open, it cannot be moved
                    // or deleted).
                    Err(RecvTimeoutError::Timeout) => {
                        if let Some(since) = open_since
                            && (since.elapsed() >= LONGEST || mode().as_deref() != Some("playing"))
                        {
                            let _ = send(&format!("close {ALIAS}"));
                            open_since = None;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        player
    });
    let _ = player.send(path.to_owned());
}

/// Opens a sound file as the one sound. (A file that is there has no quotation mark in its name,
/// and only such a file gets here: the path is safe between two.)
fn open(path: &Path) -> Result<(), u32> {
    let kind = if is_wave(path) {
        "waveaudio"
    } else {
        "mpegvideo"
    };
    send(&format!(
        "open \"{}\" type {kind} alias {ALIAS}",
        path.display()
    ))
}

/// What the one sound is doing (`playing`, `stopped`, …); `None` when there is none.
fn mode() -> Option<String> {
    let mut answer = [0u16; 64];
    let command = HSTRING::from(format!("status {ALIAS} mode"));
    if unsafe { mciSendStringW(&command, Some(&mut answer), None) } != 0 {
        return None;
    }
    let length = answer.iter().position(|unit| *unit == 0).unwrap_or(0);
    Some(String::from_utf16_lossy(&answer[..length]))
}

fn send(command: &str) -> Result<(), u32> {
    match unsafe { mciSendStringW(&HSTRING::from(command), None, None) } {
        0 => Ok(()),
        code => Err(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_systems_sounds_are_plain_names_of_files_that_are_there() {
        let names = system_sounds();
        // (A system without any is possible; one with sounds has them as wave files.)
        for name in &names {
            let path = system_sound(name).unwrap_or_else(|| panic!("{name} is not found again"));
            assert!(is_wave(&path));
        }
        assert_eq!(system_sound(""), None);
        assert_eq!(system_sound("tada"), None);
        assert_eq!(system_sound("..\\system32\\notepad"), None);
        assert_eq!(system_sound("C:\\Windows\\Media\\ding"), None);
        assert_eq!(system_sound("no such sound, surely"), None);
    }

    #[test]
    fn only_a_sound_file_that_is_there_is_played() {
        if let Some(path) = system_sounds().first().and_then(|name| system_sound(name)) {
            assert!(is_a_sound(&path));
            // The same file, not named in full; a film beside it that is not there; a folder.
            assert!(!is_a_sound(Path::new(path.file_name().unwrap())));
            assert!(!is_a_sound(&path.with_extension("mp4")));
            assert!(!is_a_sound(path.parent().unwrap()));
        }
        assert!(!is_a_sound(Path::new("http://example.org/a.mp3")));
        assert!(!is_a_sound(Path::new(r"C:\no such folder\a.mp3")));
        assert!(!is_a_sound(Path::new("")));
    }

    #[test]
    fn a_system_sound_can_be_opened_without_a_sound_being_made() {
        let Some(path) = system_sounds().first().and_then(|name| system_sound(name)) else {
            return;
        };
        // (Opened and closed, not played: a test is not to be heard.)
        let opened = open(&path);
        let _ = send(&format!("close {ALIAS}"));
        // (Opening reads the file; it takes no device to play on.)
        assert_eq!(opened, Ok(()));
    }
}
