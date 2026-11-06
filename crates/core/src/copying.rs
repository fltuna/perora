//! Putting a capture on the clipboard as the user has captures copied: no larger than they chose,
//! and as a picture (a PNG and a bitmap, to whatever asks the clipboard for one) or, of another
//! kind, as a file of that kind, which is pasted as files are. From the Tauri version's `preview.rs`,
//! without its window.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::output::{Format, Output};
use crate::platform::Frame;

/// One at a time has the clipboard and the folder of what is copied.
static COPIED: Mutex<()> = Mutex::new(());

/// The folder of Perora's own that holds the file on the clipboard (when a copy is not a PNG), and
/// nothing else: `%LOCALAPPDATA%\dev.f2a.perora\copied`, as the Tauri version had it.
pub fn copied_folder() -> Option<PathBuf> {
    Some(crate::settings::cache_directory()?.join("copied"))
}

/// Puts the picture on the clipboard, in place of what is there. Returns the clipboard's count of
/// changes right after, where the system says; refused with the reason when it could not be put
/// there.
pub fn to_clipboard(frame: &Frame, how: Output) -> Result<Option<u32>, String> {
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
    if how.format == Format::Png {
        crate::platform::put_picture_on_clipboard(frame, &file)?;
        // (No file of Perora's is on the clipboard any more.)
        forget_copied_held(None);
        return Ok(crate::platform::clipboard_sequence());
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
                how.format.name()
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
    Ok(crate::platform::clipboard_sequence())
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
