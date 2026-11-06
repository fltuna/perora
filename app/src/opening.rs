//! Opening an image in the editor (P5 of docs/gpui-rewrite.md, docs/gpui-parity.md 「画像を開く」):
//! a file (chosen, dropped on a window, opened from Explorer's menu, which starts Perora with
//! `--open` and the file), or what is on the clipboard. The editor opened with nothing in it waits
//! for an image. Whatever kind of file it is, it is read with what the system reads images with;
//! what it cannot read is said so after the fact (owner, 2026-11-02: 「いろんな画像タイプに対応した
//! ほうがいいだろうから、開いてから対応の有無だそう」).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{App, Global};
use perora_core::geometry::Rect;
use perora_core::platform::{self, Frame, ImageError};
use perora_editor::Picture;

use crate::notice::{self, Lasting};
use crate::state::Perora;
use crate::{large, preview};

/// No image is opened that has more pixels than this, or is longer than this a side (as in the
/// Tauri version: the GPU takes no picture longer than this a side, and the editor holds the image
/// and its halves).
const MOST_PIXELS: u64 = 64 * 1024 * 1024;
const LONGEST_SIDE: u32 = 16_384;

/// How often the clipboard is asked for again while another program has it, and how long between.
const CLIPBOARD_ASKED: u32 = 5;
const CLIPBOARD_ASKED_EVERY: Duration = Duration::from_millis(100);

/// An image read, ready for the editor: what it is called when the user is asked about it.
struct Ready {
    frame: Frame,
    picture: Picture,
    name: String,
}

/// One image is put in the editor at a time: what is asked about one (a drawing it would take the
/// place of) is answered before the next is looked at.
#[derive(Default)]
struct Opening {
    asking: bool,
    next: VecDeque<Ready>,
}

impl Global for Opening {}

/// The editor: with the image that is previewed when there is one, otherwise empty, waiting for an
/// image.
pub fn open_editor(cx: &mut App) {
    preview::open_editor(cx);
}

/// What is on the clipboard, in the editor: the file copied in the file manager, when one was (as it
/// is, its camera's turn too), otherwise the image (owner, 2026-11-02: 「クリップボードの画像を開くも
/// 用意しておきたいかもね」). Nothing to open there is said so.
pub fn open_clipboard(cx: &mut App) {
    let executor = cx.background_executor().clone();
    let reading = cx.background_executor().spawn(async move {
        // (A file that is no longer there, or a folder, is passed over: the image, if there is one.)
        if let Some(file) = arboard::Clipboard::new()
            .ok()
            .and_then(|mut clipboard| clipboard.get().file_list().ok())
            .and_then(|files| files.into_iter().next())
            .filter(|file| file.is_file())
        {
            return OnClipboard::File(file);
        }
        // Another program may have the clipboard for a moment: asked again a few times, not told
        // that there is nothing there.
        let mut asked = 0;
        let image = loop {
            let image = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_image());
            if !matches!(image, Err(arboard::Error::ClipboardOccupied)) || asked == CLIPBOARD_ASKED
            {
                break image;
            }
            asked += 1;
            executor.timer(CLIPBOARD_ASKED_EVERY).await;
        };
        let Ok(image) = image else {
            return OnClipboard::Nothing;
        };
        let (width, height) = (image.width as u32, image.height as u32);
        if too_large(width, height) {
            return OnClipboard::TooLarge(width, height);
        }
        let frame = Frame {
            area: Rect::new(0, 0, width, height),
            rgba: image.bytes.into_owned(),
        };
        if frame.rgba.len() != width as usize * height as usize * 4 {
            return OnClipboard::Nothing;
        }
        match Picture::new(&frame) {
            Some(picture) => OnClipboard::Image(frame, picture),
            None => OnClipboard::Nothing,
        }
    });
    cx.spawn(async move |cx| {
        let found = reading.await;
        cx.update(|cx| {
            let language = Perora::get(cx).language();
            match found {
                OnClipboard::File(file) => open_file(file, cx),
                OnClipboard::Image(frame, picture) => put_in(
                    Ready {
                        frame,
                        picture,
                        name: language.clipboard_image(),
                    },
                    cx,
                ),
                OnClipboard::TooLarge(width, height) => notice::say(
                    language.clipboard_too_large(width, height),
                    Lasting::Long,
                    None,
                    cx,
                ),
                OnClipboard::Nothing => {
                    notice::say(language.nothing_on_clipboard(), Lasting::Long, None, cx)
                }
            }
        });
    })
    .detach();
}

enum OnClipboard {
    File(PathBuf),
    Image(Frame, Picture),
    TooLarge(u32, u32),
    Nothing,
}

/// An image file the user chooses, in the editor: the system's window for that, over the editor
/// (which waits for an image). Closed without a choice, nothing happens.
pub fn choose_file(cx: &mut App) {
    let owner = large::handle(cx);
    let language = Perora::get(cx).language();
    let (images, all) = (language.images(), language.all_files());
    let (sender, receiver) = async_channel::bounded(1);
    // (A thread of its own: the system's window waits there for the choice, as long as it takes.)
    std::thread::spawn(move || {
        let _ = sender.send_blocking(platform::choose_image(owner, &images, &all));
    });
    cx.spawn(async move |cx| match receiver.recv().await {
        Ok(Ok(Some(file))) => cx.update(|cx| open_file(file, cx)),
        Ok(Ok(None)) | Err(_) => {}
        Ok(Err(detail)) => eprintln!("open: no file could be chosen: {detail}"),
    })
    .detach();
}

/// An image file, in the editor (`--open`, dropped on a window, Explorer's menu, chosen): read
/// beside the windows (reading takes a moment). One that cannot be opened is said so; one that is to
/// take the place of a drawing is asked about first.
pub fn open_file(path: PathBuf, cx: &mut App) {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let reading = cx.background_executor().spawn({
        let path = path.clone();
        async move {
            let frame = platform::read_image(&path, MOST_PIXELS, LONGEST_SIDE)?;
            let picture = Picture::new(&frame)
                .ok_or_else(|| ImageError::Unreadable("no picture could be made of it".into()))?;
            Ok::<_, ImageError>((frame, picture))
        }
    });
    cx.spawn(async move |cx| {
        let read = reading.await;
        cx.update(|cx| match read {
            Ok((frame, picture)) => put_in(
                Ready {
                    frame,
                    picture,
                    name,
                },
                cx,
            ),
            Err(error) => {
                let language = Perora::get(cx).language();
                let said = match error {
                    ImageError::TooLarge { width, height } => {
                        language.too_large_to_open(&name, width, height)
                    }
                    ImageError::Unreadable(detail) => {
                        eprintln!("open: {} could not be read: {detail}", path.display());
                        language.not_opened(&name)
                    }
                };
                notice::say(said, Lasting::Long, None, cx);
            }
        });
    })
    .detach();
}

fn too_large(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) > MOST_PIXELS
        || width > LONGEST_SIDE
        || height > LONGEST_SIDE
}

/// Puts an image that was read in the editor. What is drawn on the image in the editor now is lost
/// when another takes its place: the user says whether that is what they want (asked once the new one
/// is read: what cannot be opened is not asked about). And asked again should another image, drawn
/// on, be there by the time the answer comes (a capture came meanwhile).
fn put_in(ready: Ready, cx: &mut App) {
    let opening = cx.default_global::<Opening>();
    if opening.asking {
        opening.next.push_back(ready);
        return;
    }
    opening.asking = true;
    cx.spawn(async move |cx| {
        let mut agreed = None;
        let mut go = true;
        loop {
            let asked = cx.update(|cx| {
                let drawn = large::drawn_on(cx)?;
                if agreed == Some(drawn) {
                    return None;
                }
                let language = Perora::get(cx).language();
                let answer = large::ask_over_drawing(
                    &language.open_over_drawing(&ready.name),
                    language.open_anyway(),
                    cx,
                )?;
                Some((drawn, answer))
            });
            let Some((drawn, answer)) = asked else {
                break;
            };
            if answer.await != Some(0) {
                go = false;
                break;
            }
            agreed = Some(drawn);
        }
        cx.update(|cx| {
            if go {
                preview::present_opened(ready.frame, ready.picture, cx);
            }
            let opening = cx.default_global::<Opening>();
            opening.asking = false;
            if let Some(next) = opening.next.pop_front() {
                put_in(next, cx);
            }
        });
    })
    .detach();
}

/// Has the file manager offer "Open with Perora" on images, with this very program and in the user's
/// language, as the settings say; or not, when they say not. Written only where it is not already
/// so: at every start (the program that is started may be another copy than the one that wrote it
/// last, such as one that is worked on), when the language changes, and when the user switches it.
pub fn keep_explorer_entry(cx: &mut App) {
    let Some(program) = std::env::current_exe()
        .ok()
        .map(|program| program.display().to_string())
    else {
        return;
    };
    let perora = Perora::get(cx);
    let wanted = perora.settings().open_from_explorer;
    let label = perora.language().open_with_perora();
    let ours = explorer_entry_is_ours(&program);
    let done = if wanted {
        ours && platform::explorer_labelled(&label)
    } else {
        // (Off: what is there is taken away only when it is this program's. Another copy's is that
        // copy's to keep or take.)
        !ours
    };
    if done {
        return;
    }
    if let Err(detail) = platform::open_from_explorer(&program, wanted.then_some(label.as_str())) {
        eprintln!("the file manager's entry could not be set: {detail}");
    }
}

/// Whether the file manager's "Open with Perora" runs this program.
fn explorer_entry_is_ours(program: &str) -> bool {
    platform::opened_from_explorer()
        .is_some_and(|there| there.eq_ignore_ascii_case(&platform::explorer_command(program)))
}
