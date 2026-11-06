//! Putting a capture on the clipboard as the user has captures copied: no larger than they chose,
//! and as a picture (a PNG and a bitmap, to whatever asks the clipboard for one) or, of another
//! kind, as a file of that kind, which is pasted as files are. From the Tauri version's `preview.rs`,
//! without its window.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::output::{Format, Output};
use crate::platform::Frame;

/// One at a time has the clipboard and the folder of what is copied.
static COPIED: Mutex<()> = Mutex::new(());

/// The turn of each copy, in the order they were asked for; and the latest that was put on the
/// clipboard. A copy that took longer to make (a larger capture) is not put there over a later one.
static ASKED: AtomicU64 = AtomicU64::new(0);
static PUT: AtomicU64 = AtomicU64::new(0);

/// A copy's turn, taken when it is asked for.
pub fn turn() -> u64 {
    ASKED.fetch_add(1, Ordering::AcqRel) + 1
}

/// What came of a copy.
pub struct Copied {
    /// A later copy was on the clipboard already: this one was not put there.
    pub passed: bool,
    /// The clipboard's count of changes right after, where the system says.
    pub sequence: Option<u32>,
    /// The PNG it was put there as, when that is the picture as it was (not made smaller): for
    /// saving without making it again.
    pub png: Option<Vec<u8>>,
}

/// The folder of Perora's own that holds the file on the clipboard (when a copy is not a PNG), and
/// nothing else: `%LOCALAPPDATA%\dev.f2a.perora\copied`, as the Tauri version had it.
pub fn copied_folder() -> Option<PathBuf> {
    Some(crate::settings::cache_directory()?.join("copied"))
}

/// Puts the picture on the clipboard, in place of what is there, unless a later copy (`turn`) is
/// there already. Refused with the reason when it could not be put there.
pub fn to_clipboard(frame: &Frame, how: Output, turn: u64) -> Result<Copied, String> {
    let fitted = how.fit.and_then(|side| frame.fitted(side));
    let frame = fitted.as_ref().unwrap_or(frame);
    // The file of a copy is made first (that takes a moment); then one at a time has the clipboard
    // and the folder of what is copied.
    let file = match how.format {
        Format::Png => frame.to_png().map_err(|error| error.to_string())?,
        _ => frame.to_file(Output { fit: None, ..how })?,
    };
    let _one_at_a_time = COPIED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let as_it_was = fitted.is_none();
    if PUT.load(Ordering::Acquire) > turn {
        // (A later copy is there: what is said of that one is what counts.)
        return Ok(Copied {
            passed: true,
            sequence: None,
            png: (how.format == Format::Png && as_it_was).then_some(file),
        });
    }
    // (The turn is the clipboard's once the copy is there: one that failed holds back none before
    // it.)
    let put = put(frame, file, how.format, as_it_was);
    if put.is_ok() {
        PUT.store(turn, Ordering::Release);
    }
    put
}

/// (By whoever has the clipboard and the folder.)
fn put(frame: &Frame, file: Vec<u8>, format: Format, as_it_was: bool) -> Result<Copied, String> {
    if format == Format::Png {
        crate::platform::put_picture_on_clipboard(frame, &file)?;
        // (No file of Perora's is on the clipboard any more.)
        forget_copied_held(None);
        return Ok(Copied {
            passed: false,
            sequence: crate::platform::clipboard_sequence(),
            png: as_it_was.then_some(file),
        });
    }

    // A file has to be somewhere: in the folder that holds the one on the clipboard. The one before
    // it goes once the new one is there, not before: should the new one not get there, the old one
    // is still what the clipboard names.
    let folder = copied_folder().ok_or("no folder for what is copied")?;
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let path = (1..100)
        .map(|attempt| {
            folder.join(format!(
                "Perora {}{}.{}",
                crate::platform::local_timestamp(),
                if attempt == 1 {
                    String::new()
                } else {
                    format!(" ({attempt})")
                },
                format.name()
            ))
        })
        .find(|path| !path.exists())
        .ok_or("no name for what is copied")?;
    std::fs::write(&path, file).map_err(|error| error.to_string())?;
    if let Err(error) = crate::platform::put_file_on_clipboard(&path) {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    forget_copied_held(Some(&path));
    Ok(Copied {
        passed: false,
        sequence: crate::platform::clipboard_sequence(),
        png: None,
    })
}

/// Takes away the files Perora put on the clipboard before, all but `kept`: they are no longer what
/// the clipboard names, and nobody asked for them to be kept.
pub fn forget_copied(kept: Option<&Path>) {
    let _one_at_a_time = COPIED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    forget_copied_held(kept);
}

/// (By whoever has the folder already.)
fn forget_copied_held(kept: Option<&Path>) {
    let Some(folder) = copied_folder() else {
        return;
    };
    let Ok(files) = std::fs::read_dir(folder) else {
        return;
    };
    for file in files.flatten() {
        if Some(file.path().as_path()) != kept {
            let _ = std::fs::remove_file(file.path());
        }
    }
}
