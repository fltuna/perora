//! Windows: "Open with Perora" on images in the file manager. In the user's own part of the registry
//! (nothing here needs more rights than the user has): an entry of the menu an image is right-clicked
//! for, on every kind of file the system counts as an image, and Perora among the programs the system
//! offers to open such a file with.
//!
//! The entry is of the menu as it has long been: Windows 11 shows it under "Show more options"
//! (or with Shift and a right click), not in its own shorter menu. That one takes a package signed
//! with a certificate the system trusts (owner, 2026-11-02: 「証明書は無視でいいよROI悪い」).

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
use windows::core::PCWSTR;

use super::startup::{command_in, set_command_in, text_in};

/// Where what is said of kinds of files is kept, in the user's part of the registry.
pub const CLASSES: &str = r"Software\Classes";

/// The entry of the menu, under what the system says of every image.
const VERB: &str = r"SystemFileAssociations\image\shell\Perora";

/// The entry of the menu under one kind of file. Every image is not every kind the system counts
/// as an image: it goes by the kinds it knows of itself (PNG, JPEG and so on), not those it is told
/// of in the registry later (WebP, HEIC, AVIF, JPEG XR), on which the entry is not shown (owner,
/// 2026-11-07: 「webpは出てこないかも右栗」). So it is under each kind Perora opens too; on a kind
/// that has both, the system shows it once.
fn verb_of(kind: &str) -> String {
    format!(r"SystemFileAssociations\{kind}\shell\Perora")
}

/// Every place the entry is kept: under every image, and under each kind.
fn verbs() -> impl Iterator<Item = String> {
    std::iter::once(VERB.to_owned()).chain(KINDS.into_iter().map(verb_of))
}

/// What Perora is started with to open a file: the file follows.
pub const OPEN: &str = "--open";

/// The kinds of file Perora is offered for among the programs to open a file with. (What reads them
/// is the system's: more kinds may open than these, when they come through the menu.)
pub const KINDS: [&str; 16] = [
    ".png", ".jpg", ".jpeg", ".jfif", ".gif", ".bmp", ".dib", ".tif", ".tiff", ".ico", ".webp",
    ".heic", ".heif", ".avif", ".jxr", ".wdp",
];

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// What the system runs to open a file with this program.
pub fn command(program: &str) -> String {
    format!("\"{program}\" {OPEN} \"%1\"")
}

/// The program's own key among those the system offers to open files with: by the name of its file.
fn application(classes: &str, program: &str) -> String {
    let name = std::path::Path::new(program)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "perora.exe".to_owned());
    format!(r"{classes}\Applications\{name}")
}

/// What runs when "Open with Perora" is chosen on an image; `None` when there is no such entry.
pub fn registered(classes: &str) -> Option<String> {
    command_in(&format!(r"{classes}\{VERB}\command"), "")
}

/// Puts "Open with Perora" (`label`, in the user's language) on every image, opening it with
/// `program`; and Perora among the programs to open images with. Whatever was there of Perora's
/// before is replaced: another program (another copy of Perora) may have been there.
pub fn register(classes: &str, program: &str, label: &str) -> Result<(), String> {
    unregister(classes, program)?;
    let opened = command(program);
    for verb in verbs() {
        let verb = format!(r"{classes}\{verb}");
        set_command_in(&verb, "", Some(label))?;
        set_command_in(&verb, "Icon", Some(&format!("\"{program}\",0")))?;
        // (Not on several files at once: one image is opened at a time.)
        set_command_in(&verb, "MultiSelectModel", Some("Single"))?;
        set_command_in(&format!(r"{verb}\command"), "", Some(&opened))?;
    }

    let application = application(classes, program);
    set_command_in(&application, "FriendlyAppName", Some("Perora"))?;
    set_command_in(
        &format!(r"{application}\shell\open\command"),
        "",
        Some(&opened),
    )?;
    for kind in KINDS {
        set_command_in(&format!(r"{application}\SupportedTypes"), kind, Some(""))?;
    }
    Ok(())
}

/// Takes all of that away again. What is not there is away already.
pub fn unregister(classes: &str, program: &str) -> Result<(), String> {
    for verb in verbs() {
        remove_tree(&format!(r"{classes}\{verb}"))?;
    }
    remove_tree(&application(classes, program))
}

/// Whether the label of the entry is that one, everywhere it is kept: for telling whether it is to
/// be written anew (the user's language changed, or it was written where it is under every image
/// only, as Perora did before).
pub fn labelled(classes: &str, label: &str) -> bool {
    verbs().all(|verb| {
        text_in(&format!(r"{classes}\{verb}"), "")
            .ok()
            .flatten()
            .is_some_and(|there| there == label)
    })
}

fn remove_tree(key: &str) -> Result<(), String> {
    let key = wide(key);
    let removed = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
    if removed == ERROR_FILE_NOT_FOUND || removed == ERROR_PATH_NOT_FOUND {
        return Ok(());
    }
    // (The key itself, should what took its tree have left it.)
    removed.ok().map_err(|error| error.message())?;
    let removed = unsafe {
        windows::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()))
    };
    if removed == ERROR_FILE_NOT_FOUND || removed == ERROR_PATH_NOT_FOUND {
        return Ok(());
    }
    removed.ok().map_err(|error| error.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A place of this test's own for what would be kept with the kinds of files, gone when the test
    /// is over: not what the system reads.
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
            let _ = remove_tree(&self.0);
        }
    }

    const PROGRAM: &str = r"C:\Users\someone\AppData\Local\Perora\perora.exe";

    #[test]
    fn the_command_opens_the_file_with_the_program() {
        assert_eq!(
            command(PROGRAM),
            r#""C:\Users\someone\AppData\Local\Perora\perora.exe" --open "%1""#
        );
    }

    #[test]
    fn the_entry_is_written_read_back_and_taken_away() {
        let classes = Scratch::new("explorer");
        assert_eq!(registered(&classes.0), None);

        register(&classes.0, PROGRAM, "Perora で開く").unwrap();
        assert_eq!(registered(&classes.0), Some(command(PROGRAM)));
        assert!(labelled(&classes.0, "Perora で開く"));
        assert!(!labelled(&classes.0, "Open with Perora"));
        let verb = format!(r"{}\{VERB}", classes.0);
        assert_eq!(
            command_in(&verb, "Icon").as_deref(),
            Some(r#""C:\Users\someone\AppData\Local\Perora\perora.exe",0"#)
        );
        assert_eq!(
            command_in(&verb, "MultiSelectModel").as_deref(),
            Some("Single")
        );
        let application = format!(r"{}\Applications\perora.exe", classes.0);
        assert_eq!(
            command_in(&format!(r"{application}\shell\open\command"), ""),
            Some(command(PROGRAM))
        );
        assert_eq!(
            text_in(&format!(r"{application}\SupportedTypes"), ".webp"),
            Ok(Some(String::new()))
        );
        // Under each kind too: WebP is not among the images the system knows of itself.
        let webp = format!(r"{}\{}", classes.0, verb_of(".webp"));
        assert_eq!(
            command_in(&format!(r"{webp}\command"), ""),
            Some(command(PROGRAM))
        );
        assert_eq!(text_in(&webp, ""), Ok(Some("Perora で開く".to_owned())));
        // As Perora wrote it before (under every image only): it is to be written anew.
        remove_tree(&webp).unwrap();
        assert!(!labelled(&classes.0, "Perora で開く"));
        register(&classes.0, PROGRAM, "Perora で開く").unwrap();
        assert!(labelled(&classes.0, "Perora で開く"));

        // Another copy of Perora, elsewhere: it is that one that opens images now.
        let elsewhere = r"D:\work\perora\target\debug\perora.exe";
        register(&classes.0, elsewhere, "Open with Perora").unwrap();
        assert_eq!(registered(&classes.0), Some(command(elsewhere)));
        assert!(labelled(&classes.0, "Open with Perora"));

        unregister(&classes.0, elsewhere).unwrap();
        assert_eq!(registered(&classes.0), None);
        assert_eq!(command_in(&format!(r"{webp}\command"), ""), None);
        assert_eq!(
            command_in(&format!(r"{application}\shell\open\command"), ""),
            None
        );
        // (Taken away where it is not: nothing to do, and nothing wrong.)
        unregister(&classes.0, elsewhere).unwrap();
    }
}

/// Opens the folder a file is in, with the file chosen there (what the file manager's "show in
/// folder" does), in a window of the file manager the user has: through the shell, which hands it
/// to whatever took the file manager's place (the owner has Files do that). Not by starting
/// `explorer.exe /select`, which opens the system's own past that. Refused with the reason when the
/// shell would not.
///
/// As the Tauri version's opener did it: the folder and the file in it apart, and the thread left
/// set up for COM.
pub fn reveal(path: &std::path::Path) -> Result<(), String> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};
    use windows::core::HSTRING;

    let folder = path
        .parent()
        .ok_or_else(|| format!("{} is in no folder", path.display()))?;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let folder = ILCreateFromPathW(&HSTRING::from(folder.as_os_str()));
        let item = ILCreateFromPathW(&HSTRING::from(path.as_os_str()));
        let shown = if folder.is_null() || item.is_null() {
            Err(format!("{} is not there", path.display()))
        } else {
            SHOpenFolderAndSelectItems(folder, Some(&[item.cast_const()]), 0)
                .map_err(|error| error.message())
        };
        for list in [folder, item] {
            if !list.is_null() {
                ILFree(Some(list));
            }
        }
        shown.or_else(|why| explore(path.parent().unwrap_or(path)).map_err(|_| why))
    }
}

/// Opens a web page in the user's browser, through the shell (as tauri-plugin-opener did). Only an
/// address of the web (`https://`, or `http://` for a server on this machine while developing):
/// whatever else the shell would run is not handed to it.
pub fn open_in_browser(url: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};

    let parsed = url::Url::parse(url).map_err(|error| error.to_string())?;
    if !matches!(parsed.scheme(), "https" | "http") {
        return Err(format!("not a web address: {url}"));
    }
    let opened = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(parsed.as_str()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // (Above 32 is a success, as the call has always said.)
    if opened.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!(
            "the browser could not be opened ({})",
            opened.0 as usize
        ))
    }
}

/// Opens a folder, through the shell (as tauri-plugin-opener does when the file cannot be shown in
/// it).
fn explore(folder: &std::path::Path) -> Result<(), ()> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};

    let folder = HSTRING::from(folder.as_os_str());
    let opened = unsafe { ShellExecuteW(None, w!("explore"), &folder, None, None, SW_SHOWNORMAL) };
    // (Above 32 is a success, as the call has always said.)
    if opened.0 as usize > 32 { Ok(()) } else { Err(()) }
}

#[cfg(test)]
mod reveal_tests {
    /// Opens a window of the file manager: only when asked for (`cargo test -- --ignored reveal`).
    #[test]
    #[ignore]
    fn reveal_opens_the_folder_with_the_file_chosen() {
        let file = std::env::temp_dir().join("perora-reveal.txt");
        std::fs::write(&file, b"reveal").unwrap();
        super::reveal(&file).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
