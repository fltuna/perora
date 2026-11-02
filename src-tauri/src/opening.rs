//! Opening an image file in the editor: one chosen in the file manager ("Open with Perora", which
//! starts Perora with `--open` and the file), dropped on Perora's main window or on the editor, or
//! chosen in the editor that waits for one. Whatever kind of file it is, it is read with what the
//! system reads images with; what it cannot read is said so after the fact (owner, 2026-11-02:
//! 「いろんな画像タイプに対応したほうがいいだろうから、開いてから対応の有無だそう」).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use perora_core::geometry::Rect;
use perora_core::platform::Frame;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::Perora;
use crate::notice::Lasting;
use crate::preview;

/// No image is opened that has more pixels than this, or is longer than this a side: what is drawn
/// on is drawn on a canvas as large as the image, and the web view has no larger canvas.
const MOST_PIXELS: u64 = 64 * 1024 * 1024;
const LONGEST_SIDE: u32 = 16_384;

/// How often the clipboard is asked for again while another program has it, and how long between.
const CLIPBOARD_ASKED: u32 = 5;
const CLIPBOARD_ASKED_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// One image is put in the editor at a time: what is asked about one (a drawing it would take the
/// place of) is asked and answered before the next is looked at.
static PUTTING_IN: Mutex<()> = Mutex::new(());

/// The file Perora was started (or asked by a second start) to open: what follows `--open`. One
/// that is not whole is taken from the folder it was started in.
pub fn file_in(arguments: &[String], from: &Path) -> Option<PathBuf> {
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

/// Opens the image in that file in the editor, in the background (reading it takes a moment). One
/// that cannot be opened is said so; one that is to take the place of a drawing is asked about first.
pub fn open(app: &AppHandle, file: PathBuf) {
    let app = app.clone();
    std::thread::spawn(move || open_now(&app, &file));
}

/// Opens what is on the clipboard in the editor, in the background: the file copied in the file
/// manager, when one was (as it is, its camera's turn too), otherwise the image (owner, 2026-11-02:
/// 「クリップボードの画像を開くも用意しておきたいかもね」). Nothing to open there is said so.
pub fn open_clipboard(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let language = app.state::<Perora>().language();
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            crate::notify(&app, Lasting::Long, language.nothing_on_clipboard());
            return;
        };
        // (A file that is no longer there, or a folder, is passed over: the image, if there is one.)
        if let Some(file) = clipboard
            .get()
            .file_list()
            .ok()
            .and_then(|files| files.into_iter().next())
            .filter(|file| file.is_file())
        {
            return open_now(&app, &file);
        }
        // Another program may have the clipboard for a moment: asked again a few times, not told
        // that there is nothing there.
        let mut image = clipboard.get_image();
        for _ in 0..CLIPBOARD_ASKED {
            if !matches!(image, Err(arboard::Error::ClipboardOccupied)) {
                break;
            }
            std::thread::sleep(CLIPBOARD_ASKED_EVERY);
            image = clipboard.get_image();
        }
        let Ok(image) = image else {
            crate::notify(&app, Lasting::Long, language.nothing_on_clipboard());
            return;
        };
        let (width, height) = (image.width as u32, image.height as u32);
        if u64::from(width) * u64::from(height) > MOST_PIXELS
            || width > LONGEST_SIDE
            || height > LONGEST_SIDE
        {
            crate::notify(
                &app,
                Lasting::Long,
                language.clipboard_too_large(width, height),
            );
            return;
        }
        let frame = Frame {
            area: Rect::new(0, 0, width, height),
            rgba: image.bytes.into_owned(),
        };
        if frame.rgba.len() != width as usize * height as usize * 4 {
            crate::notify(&app, Lasting::Long, language.nothing_on_clipboard());
            return;
        }
        put_in(&app, frame, &language.clipboard_image());
    });
}

fn open_now(app: &AppHandle, file: &Path) {
    let perora = app.state::<Perora>();
    let language = perora.language();
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.display().to_string());
    let frame = match perora_core::platform::read_image(file, MOST_PIXELS, LONGEST_SIDE) {
        Ok(frame) => frame,
        Err(error) => {
            let said = match error {
                perora_core::platform::ImageError::TooLarge { width, height } => {
                    language.too_large_to_open(&name, width, height)
                }
                perora_core::platform::ImageError::Unreadable(detail) => {
                    eprintln!("open: {} could not be read: {detail}", file.display());
                    language.not_opened(&name)
                }
            };
            crate::notify(app, Lasting::Long, said);
            return;
        }
    };
    put_in(app, frame, &name);
}

/// Puts an image that was read (`name` is what it is called when the user is asked about it) in the
/// editor.
fn put_in(app: &AppHandle, frame: Frame, name: &str) {
    let language = app.state::<Perora>().language();
    // (The window shows the image from a PNG of it.)
    let png = match frame.to_png() {
        Ok(png) => png,
        Err(error) => {
            eprintln!("open: {name} could not be made a PNG: {error}");
            crate::notify(app, Lasting::Long, language.not_opened(name));
            return;
        }
    };
    // What is drawn on the image in the editor now is lost when another takes its place: the user
    // says whether that is what they want. (Asked once the new one is read: what cannot be opened
    // is not asked about.) And asked again should another image, drawn on, be there by the time
    // the answer comes (a capture came meanwhile).
    let _one_at_a_time = PUTTING_IN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut agreed = None;
    while let Some(drawn) = preview::drawn_on(app) {
        if agreed == Some(drawn) {
            break;
        }
        let mut asked = app
            .dialog()
            .message(language.open_over_drawing(name))
            .title("Perora")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                language.open_anyway(),
                language.keep_drawing(),
            ));
        if let Some(editor) = app.get_webview_window(preview::LARGE) {
            asked = asked.parent(&editor);
        }
        if !asked.blocking_show() {
            return;
        }
        agreed = Some(drawn);
    }
    preview::present_opened(app, frame, png);
}

/// The editor, from the main window: with the image being previewed, or with none, for one to be
/// dropped on it or chosen.
#[tauri::command]
pub fn main_open_editor(app: AppHandle) {
    preview::open_editor(&app);
}

/// What is on the clipboard, in the editor: from the main window, or the editor that waits for an
/// image.
#[tauri::command]
pub fn main_open_clipboard(app: AppHandle) {
    open_clipboard(&app);
}

#[tauri::command]
pub fn preview_open_clipboard(app: AppHandle) {
    open_clipboard(&app);
}

/// Lets the user choose an image file in the editor that waits for one, and opens it.
#[tauri::command]
pub async fn preview_choose_file(app: AppHandle) {
    let language = app.state::<Perora>().language();
    let kinds: Vec<&str> = perora_core::platform::IMAGE_KINDS
        .iter()
        .map(|kind| kind.trim_start_matches('.'))
        .collect();
    let mut dialog = app
        .dialog()
        .file()
        .add_filter(language.images(), &kinds)
        .add_filter(language.all_files(), &["*"]);
    if let Some(editor) = app.get_webview_window(preview::LARGE) {
        dialog = dialog.set_parent(&editor);
    }
    // Closed without choosing: nothing happens.
    if let Some(file) = dialog
        .blocking_pick_file()
        .and_then(|file| file.into_path().ok())
    {
        open(&app, file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(of: &[&str]) -> Vec<String> {
        of.iter().map(|argument| argument.to_string()).collect()
    }

    #[test]
    fn the_file_to_open_follows_the_argument() {
        let from = Path::new(r"C:\Users\someone");
        assert_eq!(
            file_in(
                &arguments(&["perora.exe", "--open", r"D:\photos\cat.jpg"]),
                from
            ),
            Some(PathBuf::from(r"D:\photos\cat.jpg"))
        );
        // (Named from where it was started, it is found there.)
        assert_eq!(
            file_in(&arguments(&["perora.exe", "--open", "cat.jpg"]), from),
            Some(PathBuf::from(r"C:\Users\someone\cat.jpg"))
        );
        assert_eq!(file_in(&arguments(&["perora.exe", "--open"]), from), None);
        assert_eq!(file_in(&arguments(&["perora.exe", "--tray"]), from), None);
        assert_eq!(
            file_in(&arguments(&["perora.exe", r"D:\photos\cat.jpg"]), from),
            None
        );
    }
}
