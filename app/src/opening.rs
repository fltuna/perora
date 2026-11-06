//! Opening an image in the editor (P5 of docs/gpui-rewrite.md, docs/gpui-parity.md 「画像を開く」):
//! a file (chosen, dropped on a window, opened from Explorer's menu), or what is on the clipboard.
//! The editor opened with nothing in it waits for an image.
// (Stubs: the main window's home calls them, and they are being written.)
#![allow(dead_code)]

use std::path::PathBuf;

use gpui::App;

/// The editor: with the image that is previewed when there is one, otherwise empty, waiting for an
/// image.
pub fn open_editor(cx: &mut App) {
    let _ = cx;
    eprintln!("open the editor: not yet (P5)");
}

/// What is on the clipboard, in the editor (an image, or a file of one).
pub fn open_clipboard(cx: &mut App) {
    let _ = cx;
    eprintln!("open the clipboard: not yet (P5)");
}

/// An image file the user chooses, in the editor.
pub fn choose_file(cx: &mut App) {
    let _ = cx;
    eprintln!("choose a file: not yet (P5)");
}

/// An image file, in the editor (`--open`, dropped on a window, Explorer's menu).
pub fn open_file(path: PathBuf, cx: &mut App) {
    let _ = cx;
    eprintln!("open {}: not yet (P5)", path.display());
}
