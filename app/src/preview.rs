//! The small preview: what was just captured, in a corner of the primary monitor, before anything
//! else is done with it (owner: 「撮影プレビュー画面欲しいかも！」). It never takes the keyboard from
//! whatever is in front (a game, usually), nor is it among the windows the user switches between.
//!
//! The capture is on the clipboard as soon as the preview appears (unless the user would rather
//! not): pasting it somewhere is the most common thing to do with it, and needs no choice. The
//! preview is for the rest: save it, copy it again (as another kind of file, or smaller), throw it
//! away. Saving closes it; a capture nothing was done about is given up when its time is over (the
//! clipboard keeps it), not while the pointer is over it.
//!
//! The window is opened as the preview appears and closed as it goes: while it is open, the GPU is
//! held. Not over a selection screen: it is put away as a capture starts, and back (with its time
//! anew) when that comes to nothing.
//!
//! A click on the picture takes the capture to the large window (`large`), where it is drawn on; a
//! capture the user has drawn on at once goes there instead of the corner. Saving and copying are the
//! same from either window: from the large one, with what is drawn on the capture (`burn`).
//!
//! P3 of docs/gpui-rewrite.md, and P4 for the large window. Not yet: uploading (P5: never on offer
//! until then); asking where to save (P5: saved where the settings say); the tour (P6).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, BoxShadow, Context, CursorStyle, Global, Hsla, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, RenderImage, SharedString,
    StatefulInteractiveElement, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, div, img, point, px, relative, rgb,
};
use perora_core::geometry::Rect;
use perora_core::language::Language;
use perora_core::output::{FITS, Format, Output};
use perora_core::platform::{self, Desktop, Frame};
use perora_core::preview::{Discarded, Once, Pending, picture_size, small_size};
use perora_core::settings::{Leaves, Settings};
use perora_core::{copying, saving};
use perora_ui::tour::{Asked, At, Button as TourButton, Marks, Step, Tour, Words};
use perora_ui::{Theme, Tip, motion};

use crate::notice::{self, Lasting};
use crate::state::Perora;
use crate::{burn, capture, corner, display, gpu, hwnd, large};

/// How often a preview whose time is over looks whether the pointer has left it.
const POINTED_AT: Duration = Duration::from_millis(250);
/// How often the window looks whether the clipboard still holds the capture.
const LOOKS_AT_CLIPBOARD: Duration = Duration::from_millis(500);
/// How long after a press beside the open choice a click is not one (it only put the choice away).
const SWALLOWED_FOR: Duration = Duration::from_millis(600);

/// A capture for the preview, as it was made ready on its way there.
pub struct Shown {
    /// Which capture it is: taken (`ask`) as it was chosen, so that one that took longer to make
    /// ready does not take the place of a later one.
    pub id: u64,
    pub frame: Arc<Frame>,
    /// The capture as a PNG, when it went to the clipboard as one, as it was.
    pub png: Option<Arc<Vec<u8>>>,
    /// Whether it went to the clipboard (see `Pending::copied`), and as what.
    pub copied: Option<Result<Option<u32>, String>>,
    pub copied_as: Output,
    /// The picture the window shows.
    pub picture: Option<Arc<RenderImage>>,
    /// Something is done about it from the start: the user has captures drawn on at once. It is
    /// not given up on its own.
    pub touched: bool,
    /// It is shown in the large window at once, to be drawn on; with the picture the editor shows,
    /// made on the way.
    pub large: bool,
    pub editor_picture: Option<perora_editor::Picture>,
}

#[derive(Default)]
struct Previews {
    pending: Option<Pending>,
    picture: Option<Arc<RenderImage>>,
    /// Why saving it, or copying it again, did not work, when it did not: said in the window until
    /// that is tried again.
    save_failure: Option<String>,
    copy_failure: Option<String>,
    /// The capture that is being saved or copied: nothing else is asked of it meanwhile.
    busy: Option<u64>,
    /// The last capture asked for, and the last one shown.
    asked: u64,
    presented: u64,
    window: Option<WindowHandle<Small>>,
    /// The capture is in the large window (and stays there until it is gone); the picture the
    /// editor is to show it with, when it was made on the way.
    large: bool,
    editor_picture: Option<perora_editor::Picture>,
    /// Which showing of the window is the last: one that comes into its corner stops where a later
    /// showing put it.
    placing: u64,
    discarded: Discarded,
    /// The tour of the small window was seen since Perora started (before the settings say so).
    toured: bool,
    /// The window is on screen with no capture, to show where it appears (the user just chose the
    /// corner); and which showing of that is the last.
    sampling: bool,
    sampled: u64,
}

/// How long the window is on screen to show where it appears.
const SHOWN_WHERE_FOR: Duration = Duration::from_secs(5);

/// The user chose another corner for the small window. With a capture in it, it is there at once, if
/// it is on screen. With none, the window is shown there for a moment all the same, to say that this
/// is where it appears: the choice is seen at once, as that of where notices are said is.
pub fn moved(cx: &mut App) {
    let previews = cx.default_global::<Previews>();
    if previews.pending.is_some() {
        if !previews.large && previews.window.is_some() {
            show(cx);
        }
        return;
    }
    if capture::busy(cx) {
        return;
    }
    let previews = cx.default_global::<Previews>();
    previews.sampling = true;
    previews.sampled += 1;
    let sampled = previews.sampled;
    show(cx);
    changed(cx);
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SHOWN_WHERE_FOR).await;
        cx.update(|cx| {
            let previews = cx.default_global::<Previews>();
            // (Shown anew since, or a capture came: not this one's to take away.)
            if previews.sampled != sampled || !previews.sampling {
                return;
            }
            previews.sampling = false;
            if previews.pending.is_none() {
                close(cx);
            }
        });
    })
    .detach();
}

impl Global for Previews {}

/// A number for a capture that is on its way to the preview.
pub fn ask(cx: &mut App) -> u64 {
    let previews = cx.default_global::<Previews>();
    previews.asked += 1;
    previews.asked
}

/// The picture of a capture as the small window shows it, at the size it is shown at on a monitor of
/// that scale: of the capture's middle, where the capture is of a shape the window cannot follow.
/// (Made at the size it is drawn at: the GPU makes a picture smaller by leaving pixels out.)
pub fn picture_of(frame: &Frame, scale: f64) -> Option<Arc<RenderImage>> {
    let (across, down) = picture_size(frame.width(), frame.height());
    let middle = perora_core::preview::middle(frame.width(), frame.height(), across, down);
    let part = if middle.width == frame.width() && middle.height == frame.height() {
        None
    } else {
        frame.crop(Rect::new(
            frame.area.x + middle.x,
            frame.area.y + middle.y,
            middle.width,
            middle.height,
        ))
    };
    let part = part.as_ref().unwrap_or(frame);
    let fitted = part.scaled_to_fit(
        (across * scale).round().max(1.0) as u32,
        (down * scale).round().max(1.0) as u32,
    );
    crate::selection::drawable(fitted)
}

/// Shows a capture in the preview, in place of whatever was previewed. Not one that a later capture
/// was shown in place of already.
pub fn present(shown: Shown, cx: &mut App) {
    let previews = cx.default_global::<Previews>();
    if shown.id <= previews.presented {
        return;
    }
    previews.presented = shown.id;
    // (Not there to show where it appears any more: here is a capture.)
    previews.sampling = false;
    let mut pending = Pending::new(
        shown.id,
        shown.frame,
        shown.png,
        shown.copied,
        shown.copied_as,
    );
    pending.touched = shown.touched;
    previews.pending = Some(pending);
    previews.picture = shown.picture;
    previews.save_failure = None;
    previews.copy_failure = None;
    previews.busy = None;
    previews.large = shown.large;
    previews.editor_picture = shown.editor_picture;
    // (In the corner, it takes the place of the one the large window was showing.)
    if !shown.large {
        large::close(cx);
    }
    show(cx);
    leave_to_its_time(cx);
}

/// Puts the capture being previewed back on screen, if there is one: after a selection screen that
/// came to nothing, or one that was up when the preview wanted to appear. It has its time anew. (With
/// none, the large window that was waiting for an image is back, waiting still: not over a selection
/// screen, where it would be in the way and in the picture.)
pub fn restore(cx: &mut App) {
    if cx.default_global::<Previews>().pending.is_none() {
        if large::awaiting(cx) && !capture::busy(cx) {
            large::await_again(cx);
        }
        return;
    }
    show(cx);
    leave_to_its_time(cx);
}

/// Puts an image the user opened (a file, the clipboard) in the large window, to be drawn on: as a
/// capture is that is drawn on at once, but with none of what becomes of a capture as it is taken
/// (not copied, saved or uploaded: what it leaves as is what the user chooses there). `picture` is
/// the editor's, made on the way; `id`, what it was asked for by as it began to be opened (a capture
/// asked for later takes its place).
pub fn present_opened(id: u64, frame: Frame, picture: perora_editor::Picture, cx: &mut App) {
    let copied_as = Perora::get(cx).settings().output(Leaves::Copy);
    present(
        Shown {
            id,
            frame: Arc::new(frame),
            png: None,
            copied: None,
            copied_as,
            picture: None,
            touched: true,
            large: true,
            editor_picture: Some(picture),
        },
        cx,
    );
}

/// The editor, from the main window: the capture being previewed, in the large window; or, when there
/// is none, the large window with no image, for one to be dropped on it, chosen or pasted.
pub fn open_editor(cx: &mut App) {
    let previewed = cx
        .default_global::<Previews>()
        .pending
        .as_ref()
        .map(|pending| pending.id);
    match previewed {
        Some(id) => enlarge(id, cx),
        None => large::await_image(cx),
    }
}

/// Takes the preview off the screen for a capture (the capture being previewed stays until the new
/// one takes its place). Whether it was on screen: a window that was just hidden is on screen for
/// one more frame, which matters to whoever captures the screen next.
pub fn put_away(cx: &mut App) -> bool {
    let previews = cx.default_global::<Previews>();
    // (Its time is over for now: it has it anew as it comes back.)
    if let Some(pending) = previews.pending.as_mut() {
        pending.timer += 1;
    }
    let small = close(cx);
    let large = large::hide(cx);
    small || large
}

/// The small window and the large one, those on screen: Perora's own, in the picture when the user
/// wants that.
pub fn on_screen_now(cx: &mut App) -> Vec<platform::WindowId> {
    let small = cx.default_global::<Previews>().window.and_then(|window| {
        window
            .update(cx, |_, window, _| hwnd::of(window))
            .ok()
            .flatten()
    });
    small.into_iter().chain(large::on_screen_now(cx)).collect()
}

/// How tall the small window is (physical pixels) while it is on screen with a capture, in the
/// corner the settings say: what shares that corner keeps clear of it.
pub fn in_its_corner(cx: &mut App) -> Option<(perora_core::notice::Corner, i32)> {
    let previews = cx.default_global::<Previews>();
    previews.pending.as_ref()?;
    let window = previews.window?;
    let handle = window
        .update(cx, |_, window, _| hwnd::of(window))
        .ok()
        .flatten()?;
    let corner = Perora::get(cx).settings().preview_corner;
    Some((corner, hwnd::rect(handle)?.height as i32))
}

/// Keeps in mind what a capture was saved as, as it was taken, for discarding to take away. When it
/// was discarded meanwhile (and the user has discarding take that away), it is taken away at once.
/// Whether it is still there.
pub fn saved_as_taken(id: u64, files: &[PathBuf], cx: &mut App) -> bool {
    let takes_away = Perora::get(cx).settings().delete_saved_on_discard;
    let previews = cx.default_global::<Previews>();
    let discarded = previews.discarded.has(id);
    if let Some(pending) = previews
        .pending
        .as_mut()
        .filter(|pending| pending.id == id && !discarded)
    {
        pending.saved = files.to_vec();
    }
    if discarded && takes_away {
        saving::take_away(files);
        return false;
    }
    true
}

/// The small window in its corner, at the size of the capture. Not over a selection screen: it would
/// be in the way and in the picture (`restore` shows it once that is gone).
fn show(cx: &mut App) {
    if capture::busy(cx) {
        return;
    }
    let previews = cx.default_global::<Previews>();
    if previews.large {
        let picture = previews.editor_picture.take();
        let Some((id, frame)) = previews
            .pending
            .as_ref()
            .map(|pending| (pending.id, pending.frame.clone()))
        else {
            return;
        };
        close(cx);
        large::show(id, frame, picture, cx);
        return;
    }
    // (Showing where it appears, it is as large as a capture of a monitor is.)
    let Some((width, height)) = previews
        .pending
        .as_ref()
        .map(|pending| (pending.frame.width(), pending.frame.height()))
        .or(previews.sampling.then_some((16, 9)))
    else {
        return;
    };
    let Some(monitor) = corner::monitor() else {
        return;
    };
    let corner = Perora::get(cx).settings().preview_corner;
    let to = corner::spot(&monitor, corner, small_size(width, height), None);
    let previews = cx.default_global::<Previews>();
    previews.placing += 1;
    let placing = previews.placing;

    // On screen already: there at once, at its size, showing this capture.
    if let Some(window) = previews.window {
        let _ = window.update(cx, |small, window, cx| {
            corner::put(window, to, cx);
            small.anew();
            cx.notify();
        });
        return;
    }

    let Some(screen) = display::of(&monitor, cx) else {
        return;
    };
    let from = corner::from(&monitor, corner, to);
    gpu::attach(cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(display::bounds_on(
            &monitor,
            from.unwrap_or(to),
        ))),
        titlebar: None,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        display_id: Some(screen.id()),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let still: Rc<dyn Fn(&App) -> bool> = Rc::new(move |cx: &App| {
        cx.try_global::<Previews>()
            .is_some_and(|previews| previews.placing == placing && previews.window.is_some())
    });
    let language = Perora::get(cx).language();
    let opened = cx.open_window(options, |window, cx| {
        if let Some(handle) = hwnd::of(window) {
            platform::without_transitions(handle);
            // Never takes the keyboard: not when it appears, not when it is clicked.
            hwnd::never_active(handle);
            platform::set_cloaked(handle, true);
        }
        // Closed (Alt+F4, should it ever have the keyboard): thrown away, as the bin does.
        window.on_window_should_close(cx, |_, cx| {
            cx.defer(|cx| discard_by_user(None, cx));
            false
        });
        cx.new(|cx| Small::new(language, corner::Appearing { to, from, still }, cx))
    });
    match opened {
        Ok(window) => cx.default_global::<Previews>().window = Some(window),
        Err(error) => eprintln!("the preview could not be shown: {error}"),
    }
}

/// Closes the window. Whether it was open.
fn close(cx: &mut App) -> bool {
    let previews = cx.default_global::<Previews>();
    previews.placing += 1;
    let Some(window) = previews.window.take() else {
        return false;
    };
    let _ = window.update(cx, |_, window, _| {
        // Out of sight at once: closing takes a turn of the message loop.
        if let Some(handle) = hwnd::of(window) {
            hwnd::hide(handle);
        }
        window.remove_window();
    });
    true
}

/// Tells the windows that what they show changed.
fn changed(cx: &mut App) {
    if let Some(window) = cx.default_global::<Previews>().window {
        let _ = window.update(cx, |_, _, cx| cx.notify());
    }
    large::changed(cx);
}

/// What the large window says of the capture.
pub struct Said {
    pub width: u32,
    pub height: u32,
    /// The clipboard holds this capture as copying would put it there (if not with what is drawn
    /// now); and with what is drawn now.
    pub on_clipboard: bool,
    pub copied: bool,
    pub failure: Option<String>,
    /// What it is saved and copied as, as its buttons' tooltips say.
    pub save_as: String,
    pub copy_as: String,
}

pub fn said_of(id: u64, edits: &[perora_core::edit::Edit], cx: &mut App) -> Option<Said> {
    let settings = Perora::get(cx).settings();
    let language = Perora::get(cx).language();
    let previews = cx.default_global::<Previews>();
    let pending = previews
        .pending
        .as_ref()
        .filter(|pending| pending.id == id)?;
    let now = platform::clipboard_sequence();
    let failure = previews
        .save_failure
        .as_ref()
        .map(|detail| language.text("preview.saveFailed", &[("detail", detail)]))
        .or_else(|| {
            previews
                .copy_failure
                .as_deref()
                .or(pending.copy_failure())
                .map(|detail| language.text("preview.copyFailed", &[("detail", detail)]))
        });
    Some(Said {
        width: pending.frame.width(),
        height: pending.frame.height(),
        on_clipboard: pending.on_clipboard(now) && pending.copied_as_asked(&settings),
        copied: pending.copied_now(now, &settings, edits),
        failure,
        save_as: leaves_as_said(
            &language,
            &settings,
            pending.once.of(Leaves::Save),
            Leaves::Save,
        ),
        copy_as: leaves_as_said(
            &language,
            &settings,
            pending.once.of(Leaves::Copy),
            Leaves::Copy,
        ),
    })
}

/// The preview is on screen (anew, or again): from now it has as long as the user chose, and the
/// capture is given up when that is over with nothing done about it. Not one that something was done
/// about, and not under the pointer: whoever points at it is about to press something on it.
fn leave_to_its_time(cx: &mut App) {
    // (For as long as the settings say now: not changed under it afterwards.)
    let seconds = Perora::get(cx).settings().preview_seconds;
    if seconds <= 0.0 {
        return;
    }
    let Some((id, timer)) = cx
        .default_global::<Previews>()
        .pending
        .as_mut()
        .filter(|pending| pending.is_untouched())
        .map(|pending| {
            pending.timer += 1;
            (pending.id, pending.timer)
        })
    else {
        return;
    };
    let still = move |cx: &mut App| {
        matches!(
            &cx.default_global::<Previews>().pending,
            Some(pending) if pending.id == id && pending.timer == timer && pending.is_untouched()
        )
    };
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_secs_f64(seconds))
            .await;
        loop {
            let pointed_at = cx.update(|cx| {
                if !still(cx) {
                    return None;
                }
                let window = cx.default_global::<Previews>().window?;
                let handle = window
                    .update(cx, |_, window, _| hwnd::of(window))
                    .ok()
                    .flatten()?;
                let pointer = platform::desktop().pointer().ok();
                Some(
                    hwnd::rect(handle)
                        .zip(pointer)
                        .is_some_and(|(rect, pointer)| rect.contains(pointer)),
                )
            });
            match pointed_at {
                // Gone, done something about, put away: not this time's to end.
                None => return,
                Some(true) => cx.background_executor().timer(POINTED_AT).await,
                // (Not as the user discards it: what is on the clipboard stays.)
                Some(false) => {
                    cx.update(|cx| discard(Some(id), cx));
                    return;
                }
            }
        }
    })
    .detach();
}

/// The language changed: what the small window says, in the new one.
pub fn relanguage(cx: &mut App) {
    let language = Perora::get(cx).language();
    if let Some(window) = cx.default_global::<Previews>().window {
        let _ = window.update(cx, |small, _, cx| {
            small.language = language;
            cx.notify();
        });
    }
}

/// Throws the capture away and closes the window: `id` when it is still the one previewed, whichever
/// is previewed when no id is given.
fn discard(id: Option<u64>, cx: &mut App) {
    let previews = cx.default_global::<Previews>();
    match &previews.pending {
        Some(pending) if id.is_none_or(|id| id == pending.id) => {}
        _ => return,
    }
    previews.pending = None;
    previews.picture = None;
    previews.busy = None;
    previews.large = false;
    previews.editor_picture = None;
    close(cx);
    large::close(cx);
}

/// Discarding as the user does it (the bin, closing the window): when they asked for that, the image
/// is taken off the clipboard as well (only the image: whatever came to the clipboard after it
/// stays), and what was saved of it as it was taken is taken away.
pub fn discard_by_user(id: Option<u64>, cx: &mut App) {
    let settings = Perora::get(cx).settings();
    let previews = cx.default_global::<Previews>();
    let Some(pending) = previews
        .pending
        .as_ref()
        .filter(|pending| id.is_none_or(|id| id == pending.id))
    else {
        return;
    };
    let on_clipboard = match pending.copied {
        Some(Ok(Some(written))) if pending.on_clipboard(platform::clipboard_sequence()) => {
            Some(written)
        }
        _ => None,
    };
    let saved = pending.saved.clone();
    let id = pending.id;
    previews.discarded.add(id);
    discard(Some(id), cx);
    if let Some(written) = on_clipboard
        && settings.clear_clipboard_on_discard
    {
        match platform::clear_clipboard_if(written) {
            Ok(true) => copying::forget_copied(None),
            Ok(false) => {}
            Err(error) => {
                eprintln!("clipboard: the discarded capture was not taken off it: {error}")
            }
        }
    }
    if settings.delete_saved_on_discard {
        saving::take_away(&saved);
    }
}

/// The capture, when it is still `id` and nothing is being done with it: something is to be done.
fn to_do(id: u64, cx: &mut App) -> Option<&mut Pending> {
    let previews = cx.default_global::<Previews>();
    if previews.busy.is_some() {
        return None;
    }
    previews.pending.as_mut().filter(|pending| pending.id == id)
}

/// Saves the capture as a file (or several, of each kind the user saves as) in the folder for
/// saving, named after the moment, and closes the preview; a notice says where it went, and a click
/// on that shows the file. The preview stays, with the reason, when it could not be written. From the
/// large window, what is drawn on it is in the file, and the window is out of sight at once.
pub fn save(id: u64, cx: &mut App) {
    // (What is being written is written first: it is part of what is drawn.)
    if !large::done_writing(id, cx) {
        return;
    }
    let settings = Perora::get(cx).settings();
    let language = Perora::get(cx).language();
    let edits = large::drawing_of(id, cx).unwrap_or_default();
    let Some(pending) = to_do(id, cx) else {
        return;
    };
    pending.touched = true;
    let once = pending.once.of(Leaves::Save).clone();
    let (frame, png) = (pending.frame.clone(), pending.png.clone());
    let previews = cx.default_global::<Previews>();
    previews.busy = Some(id);
    previews.save_failure = None;
    changed(cx);
    let drawn = !edits.is_empty();
    if drawn {
        large::leaving(id, true, cx);
    }

    // As the user has captures saved, or asked for this once: the kinds of file (one of each, all
    // by one name), and how large at most.
    let how = once.output(&settings, Leaves::Save);
    let kinds = once.saved_as(&settings);
    cx.spawn(async move |cx| {
        let (frame, png) = if drawn {
            match burn::burn(frame, edits, cx).await {
                Ok(burned) => (Arc::new(burned), None),
                Err(detail) => {
                    eprintln!("save: the drawing could not be made into an image: {detail}");
                    cx.update(|cx| {
                        let unmade = Perora::get(cx).language().text("editor.unmade", &[]);
                        not_left(id, |previews| previews.save_failure = Some(unmade), cx);
                    });
                    return;
                }
            }
        } else {
            (frame, png)
        };
        let saving = cx.background_executor().spawn(async move {
            let leaving = Pending::new(0, frame, png, None, how);
            let directory = saving::save_directory(&settings).ok_or("no folder to save to")?;
            // Every kind is made before any is written: one that cannot be made leaves no file.
            let files = kinds
                .iter()
                .map(|kind| {
                    Ok((
                        kind.name(),
                        leaving.file(Output {
                            format: *kind,
                            ..how
                        })?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let files: Vec<_> = files
                .iter()
                .map(|(kind, file)| (*kind, file.as_slice()))
                .collect();
            std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            let name = format!("Perora {}", platform::local_timestamp());
            saving::write_each(&directory, &name, &files, false)
                .map(|first| (first, kinds.len()))
                .map_err(|not_all| match not_all.first {
                    // Some of them are there: a notice says which, and the window why the rest is
                    // not.
                    Some(first) => {
                        let kind = not_all.kind.to_uppercase();
                        let name = first.file_name().unwrap_or_default().to_string_lossy();
                        let said = language.saved_but_not(&first.display().to_string(), &kind);
                        let refused =
                            language.not_all_saved(&kind, &not_all.error.to_string(), &name);
                        format!("{refused}\u{0}{said}\u{0}{}", first.display())
                    }
                    None => not_all.error.to_string(),
                })
        });
        let saved = saving.await;
        cx.update(|cx| {
            let previews = cx.default_global::<Previews>();
            if previews.busy == Some(id) {
                previews.busy = None;
            }
            let language = Perora::get(cx).language();
            match saved {
                Ok((path, kinds)) => {
                    discard(Some(id), cx);
                    let shown = path.display().to_string();
                    let message = match kinds {
                        1 => language.saved(&shown),
                        several => language.saved_several(&shown, several - 1),
                    };
                    notice::say_of_file(message, Lasting::Brief, path, cx);
                }
                Err(failure) => {
                    let mut parts = failure.split('\u{0}');
                    let refused = parts.next().unwrap_or_default().to_owned();
                    if let (Some(said), Some(first)) = (parts.next(), parts.next()) {
                        notice::say_of_file(said.to_owned(), Lasting::Long, first.into(), cx);
                    }
                    not_left(id, |previews| previews.save_failure = Some(refused), cx);
                }
            }
        });
    })
    .detach();
}

/// The capture did not leave: nothing is being done with it now, the window says why, and the large
/// one is in sight and drawn on again.
fn not_left(id: u64, why: impl FnOnce(&mut Previews), cx: &mut App) {
    let previews = cx.default_global::<Previews>();
    if previews.busy == Some(id) {
        previews.busy = None;
    }
    if previews
        .pending
        .as_ref()
        .is_some_and(|pending| pending.id == id)
    {
        why(previews);
    }
    large::stays(id, cx);
    changed(cx);
}

/// Puts the capture on the clipboard again, as it is asked for now, and keeps the preview: after
/// something else was copied, or to have it as another kind of file. Nothing is done when the
/// clipboard holds just that already. From the large window, with what is drawn on it.
pub fn copy(id: u64, cx: &mut App) {
    if !large::done_writing(id, cx) {
        return;
    }
    let settings = Perora::get(cx).settings();
    let edits = large::drawing_of(id, cx).unwrap_or_default();
    let Some(pending) = to_do(id, cx) else {
        return;
    };
    pending.touched = true;
    let copy_as = pending
        .once
        .of(Leaves::Copy)
        .output(&settings, Leaves::Copy);
    let there = pending.on_clipboard(platform::clipboard_sequence())
        && pending.copied_as.format == copy_as.format
        && pending.copied_as.fit == copy_as.fit
        && pending.copied_edits == edits;
    let frame = pending.frame.clone();
    let previews = cx.default_global::<Previews>();
    previews.copy_failure = None;
    if there {
        changed(cx);
        return;
    }
    previews.busy = Some(id);
    let drawn = !edits.is_empty();
    if drawn {
        large::leaving(id, false, cx);
    }
    let turn = copying::turn();
    cx.spawn(async move |cx| {
        let frame = if drawn {
            match burn::burn(frame, edits.clone(), cx).await {
                Ok(burned) => Arc::new(burned),
                Err(detail) => {
                    eprintln!("copy: the drawing could not be made into an image: {detail}");
                    cx.update(|cx| {
                        let unmade = Perora::get(cx).language().text("editor.unmade", &[]);
                        not_left(id, |previews| previews.copy_failure = Some(unmade), cx);
                    });
                    return;
                }
            }
        } else {
            frame
        };
        let copied = cx
            .background_executor()
            .spawn(async move { copying::to_clipboard(&frame, copy_as, turn) })
            .await;
        cx.update(|cx| {
            let previews = cx.default_global::<Previews>();
            if previews.busy == Some(id) {
                previews.busy = None;
            }
            match (
                previews.pending.as_mut().filter(|pending| pending.id == id),
                copied,
            ) {
                (Some(pending), Ok(copied)) => {
                    pending.copied =
                        Some(Ok((!copied.passed).then_some(copied.sequence).flatten()));
                    pending.copied_as = copy_as;
                    pending.copied_edits = edits;
                    // (Only the PNG of the capture as it was taken is saved as it is.)
                    if pending.png.is_none() && !drawn {
                        pending.png = copied.png.map(Arc::new);
                    }
                    if let Some(window) = previews.window {
                        let _ = window.update(cx, |small, _, cx| {
                            small.ticked = Some(Instant::now());
                            cx.notify();
                        });
                    }
                    large::stays(id, cx);
                    changed(cx);
                }
                (Some(_), Err(detail)) => {
                    eprintln!("clipboard: the capture was not put there: {detail}");
                    not_left(id, |previews| previews.copy_failure = Some(detail), cx);
                }
                // Gone in the moment it was written: it is not left on the clipboard.
                (None, Ok(copied)) => {
                    if !copied.passed
                        && let Some(written) = copied.sequence
                        && platform::clear_clipboard_if(written) == Ok(true)
                    {
                        copying::forget_copied(None);
                    }
                }
                (None, Err(_)) => {}
            }
        });
    })
    .detach();
}

/// The user chose what the capture leaves as this once, one way: remembered for it (and something
/// was done about it: it is not given up on its own after that).
pub(crate) fn choose(id: u64, leaves: Leaves, chosen: impl FnOnce(&mut Once), cx: &mut App) {
    if let Some(pending) = cx
        .default_global::<Previews>()
        .pending
        .as_mut()
        .filter(|pending| pending.id == id)
    {
        pending.touched = true;
        chosen(pending.once.of_mut(leaves));
    }
    changed(cx);
}

/// Something is being done about the capture (a choice was opened): it is not given up on its own.
pub(crate) fn touch(id: u64, cx: &mut App) {
    if let Some(pending) = cx
        .default_global::<Previews>()
        .pending
        .as_mut()
        .filter(|pending| pending.id == id)
    {
        pending.touched = true;
    }
}

/// The picture was clicked: the capture in the large window, to be drawn on (and there it stays).
fn enlarge(id: u64, cx: &mut App) {
    let previews = cx.default_global::<Previews>();
    let Some(pending) = previews.pending.as_mut().filter(|pending| pending.id == id) else {
        return;
    };
    pending.touched = true;
    previews.large = true;
    show(cx);
}

/// What the window shows, read in one go.
struct Look {
    id: u64,
    width: u32,
    height: u32,
    picture: Option<Arc<RenderImage>>,
    /// Whether the clipboard holds the capture, as copying would put it there now.
    copied: bool,
    failure: Option<String>,
    once: perora_core::preview::Chosen,
}

/// The window. What it shows is the application's (`Previews`); what is only about how it is shown
/// is its own.
pub struct Small {
    language: Language,
    appearing: Option<corner::Appearing>,
    /// The capture it shows: what is its own starts anew with another.
    id: u64,
    /// Whether things move (the system's "animation effects").
    moves: bool,
    /// The pointer is over the picture; and when it came or went, with how much of the dark over the
    /// picture was seen then (it fades in and out).
    over: bool,
    over_changed: Option<(Instant, f32)>,
    /// The choice of what the capture leaves as this once, open for that way, and since when.
    choosing: Option<Leaves>,
    opened: Instant,
    /// A press beside the open choice put it away: the click that ends it is not one.
    swallow_until: Option<Instant>,
    /// Whether the clipboard held the capture as last drawn, and since when (for this capture: what
    /// it says of that is seen to change).
    copied: bool,
    copied_since: Option<Instant>,
    /// When a copy made while the capture was on screen was done: its tick comes in with a bounce.
    pub(crate) ticked: Option<Instant>,
    /// The clipboard's count of changes, as last looked at (to draw anew when it changes).
    clipboard: Option<u32>,
    /// The tour of the window, while it is on; where the things it shows are; whether the choice
    /// is open for it (the tour's to put away).
    tour: Option<Tour>,
    marks: Marks,
    chosen_by_tour: bool,
}

/// How long the dark over the picture takes to come and go; what is said of the clipboard to fade
/// out, and in; the sheets of the copy button to go; the tick to wait, and to come; the choice to
/// come up.
const OVER_FADES: Duration = Duration::from_millis(150);
const SAID_OUT: Duration = Duration::from_millis(200);
const SAID_IN: Duration = Duration::from_millis(280);
const SHEETS_GO: Duration = Duration::from_millis(160);
const TICK_WAITS: Duration = Duration::from_millis(100);
const TICK_COMES: Duration = Duration::from_millis(420);
const CHOICE_COMES: Duration = Duration::from_millis(160);
/// How the tick comes: `cubic-bezier(0.2, 0.8, 0.3, 1)`.
const BOUNCE: motion::Curve = motion::Curve(0.2, 0.8, 0.3, 1.0);

impl Small {
    fn new(language: Language, appearing: corner::Appearing, cx: &mut Context<Self>) -> Self {
        // What is on the clipboard changes without Perora: the window looks now and then.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(LOOKS_AT_CLIPBOARD).await;
                let looked = this.update(cx, |this, cx| {
                    let now = platform::clipboard_sequence();
                    if now != this.clipboard {
                        this.clipboard = now;
                        cx.notify();
                    }
                });
                if looked.is_err() {
                    return;
                }
            }
        })
        .detach();
        Self {
            language,
            appearing: Some(appearing),
            id: 0,
            moves: platform::animations_enabled(),
            over: false,
            over_changed: None,
            choosing: None,
            opened: Instant::now(),
            swallow_until: None,
            copied: false,
            copied_since: None,
            ticked: None,
            clipboard: platform::clipboard_sequence(),
            tour: None,
            marks: Marks::default(),
            chosen_by_tour: false,
        }
    }

    /// The tour of the window, from its first step. (Shown, something is being done about the
    /// capture: it is not given up on its own meanwhile.)
    fn start_tour(&mut self, cx: &mut Context<Self>) {
        let t = |key: &str| self.language.text(key, &[]);
        let offers_upload = false;
        let steps = vec![
            Step::new(At::Mark("picture"), t("tour.preview.picture")),
            Step::new(
                At::Mark("actions"),
                t(if offers_upload {
                    "tour.preview.actions"
                } else {
                    "tour.preview.actionsWithoutUpload"
                }),
            ),
            Step::new(At::Mark("as"), t("tour.preview.as")),
            Step::new(At::Mark("menu"), t("tour.choice")),
            Step::new(At::Mark("guide"), t("tour.again")),
        ];
        self.tour = Some(Tour::new(steps, self.moves, true));
        self.choose(None);
        let id = self.id;
        cx.defer(move |cx| touch(id, cx));
        cx.notify();
    }

    /// One of the tour's buttons pressed.
    fn tour_button(&mut self, button: TourButton, cx: &mut Context<Self>) {
        let Some(tour) = self.tour.as_mut() else {
            return;
        };
        let asked = match button {
            TourButton::Skip => Asked::End,
            TourButton::Back => tour.back(),
            TourButton::Next => tour.next(),
        };
        if asked == Asked::End {
            self.tour = None;
            cx.defer(|cx| saw_tour(cx));
        }
        cx.notify();
    }

    /// How much of the dark over the picture is seen now.
    fn dark(&self) -> f32 {
        let to = if self.over { 1.0 } else { 0.0 };
        match self.over_changed {
            Some((since, from)) => {
                let along = motion::share(since, Duration::ZERO, OVER_FADES);
                from + (to - from) * motion::EASE.at(along)
            }
            None => to,
        }
    }

    /// The pointer came onto the picture, or left it.
    fn hovered(&mut self, over: bool) {
        if over == self.over {
            return;
        }
        let seen = self.dark();
        self.over = over;
        self.over_changed = self.moves.then(|| (Instant::now(), seen));
    }

    /// The choice opened (for that way) or put away.
    fn choose(&mut self, leaves: Option<Leaves>) {
        if leaves.is_some() && leaves != self.choosing {
            self.opened = Instant::now();
        }
        self.choosing = leaves;
    }

    /// Another capture: nothing of the one before is left.
    fn anew(&mut self) {
        self.id = 0;
    }

    fn look(&mut self, settings: &Settings, cx: &mut App) -> Option<Look> {
        let previews = cx.default_global::<Previews>();
        // Showing where it appears: as it does with a capture of a monitor in it, copied (nothing in
        // it does anything).
        if previews.pending.is_none() && previews.sampling {
            return Some(Look {
                id: 0,
                width: 1920,
                height: 1080,
                picture: None,
                copied: true,
                failure: None,
                once: Default::default(),
            });
        }
        let pending = previews.pending.as_ref()?;
        let language = &self.language;
        let failure = previews
            .save_failure
            .as_ref()
            .map(|detail| language.text("preview.saveFailed", &[("detail", detail)]))
            .or_else(|| {
                previews
                    .copy_failure
                    .as_deref()
                    .or(pending.copy_failure())
                    .map(|detail| language.text("preview.copyFailed", &[("detail", detail)]))
            });
        Some(Look {
            id: pending.id,
            width: pending.frame.width(),
            height: pending.frame.height(),
            picture: previews.picture.clone(),
            copied: pending.copied_now(platform::clipboard_sequence(), settings, &[]),
            failure,
            once: pending.once.clone(),
        })
    }
}

/// A tooltip that says this.
fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let text = text.into();
    move |_, cx| Tip::view(text.clone(), cx)
}

/// Segoe Fluent Icons, as the Tauri version's style sheet has them.
const PEN: &str = "\u{E70F}";
const CHEVRON: &str = "\u{E70D}";
const SHEETS: &str = "\u{E8C8}";
const TICK: &str = "\u{E73E}";
const BIN: &str = "\u{E74D}";
const GUIDE: &str = "\u{E946}";

/// The bin's colour: a subdued red, the same in both themes (owner: 「#a1473f あたり？」), and a
/// little darker under the pointer.
const BIN_COLOUR: u32 = 0xa1473f;
const BIN_HOVER: u32 = 0x94413a;

fn icon(glyph: &'static str, size: f32) -> gpui::Div {
    div()
        .font_family("Segoe Fluent Icons")
        .text_size(px(size))
        .line_height(px(size))
        .child(glyph)
}

/// What the capture leaves as one way, as a tooltip says it: the kinds of file, and how large at most.
fn leaves_as_said(language: &Language, settings: &Settings, once: &Once, leaves: Leaves) -> String {
    let (_, kinds, fit) = leaves_as(settings, once, leaves);
    let kinds = kinds
        .iter()
        .map(|kind| language.text(&format!("main.output.format.{}", kind.name()), &[]))
        .collect::<Vec<_>>()
        .join(" + ");
    if fit == 0 {
        kinds
    } else {
        let within = language.text("main.output.fit.within", &[("side", &fit.to_string())]);
        format!("{kinds}, {within}")
    }
}

/// What the capture `id` leaves as, one way, as `leaves_as` says (while it is the one previewed).
pub(crate) fn leaves_now(id: u64, leaves: Leaves, cx: &mut App) -> Option<(Format, Vec<Format>, u32)> {
    let settings = Perora::get(cx).settings();
    let pending = cx
        .default_global::<Previews>()
        .pending
        .as_ref()
        .filter(|pending| pending.id == id)?;
    Some(leaves_as(&settings, pending.once.of(leaves), leaves))
}

/// What the capture leaves as, one way: what was chosen this once, or what the settings say. (The
/// kinds of file, for saving: several at once.)
fn leaves_as(settings: &Settings, once: &Once, leaves: Leaves) -> (Format, Vec<Format>, u32) {
    let output = once.output(settings, leaves);
    let kinds = match leaves {
        Leaves::Save => once
            .formats
            .clone()
            .filter(|kinds| !kinds.is_empty())
            .unwrap_or_else(|| settings.save_formats.clone()),
        _ => vec![output.format],
    };
    (output.format, kinds, output.fit.unwrap_or(0))
}

impl Render for Small {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(appearing) = self.appearing.take() {
            corner::appear_when_drawn(appearing, window);
        }
        let theme = Theme::get(cx).clone();
        let settings = Perora::get(cx).settings();
        self.marks.turn();
        let root = div()
            .id("preview")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(10.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .text_size(theme.text_size_small)
            .line_height(relative(1.43));
        let Some(look) = self.look(&settings, cx) else {
            return root;
        };
        let sampling = look.id == 0;
        if look.id != self.id && !sampling {
            // The tour comes on its own a moment after the first capture is seen here, unless it
            // was seen; on over another capture, that one is not given up on its own either.
            if self.tour.is_some() {
                let id = look.id;
                cx.defer(move |cx| touch(id, cx));
            } else if !tour_seen(cx) {
                let id = look.id;
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(TOUR_COMES_AFTER).await;
                    let _ = this.update(cx, |small, cx| {
                        if small.id == id && small.tour.is_none() && !tour_seen(cx) {
                            small.start_tour(cx);
                        }
                    });
                })
                .detach();
            }
            // Another capture starts with what it has to say, nothing moving.
            self.id = look.id;
            self.over = false;
            self.over_changed = None;
            self.choosing = None;
            self.swallow_until = None;
            self.copied = look.copied;
            self.copied_since = None;
            self.ticked = None;
        } else if look.copied != self.copied {
            self.copied = look.copied;
            self.copied_since = self.moves.then(Instant::now);
        }
        if !look.copied || !self.moves {
            self.ticked = None;
        }
        let id = look.id;
        let t = |key: &str| self.language.text(key, &[]);
        let dark = self.dark();
        if let Some((since, _)) = self.over_changed {
            motion::along(since, Duration::ZERO, OVER_FADES, window);
        }

        // A press anywhere but on the open choice and the row it belongs to puts it away, and is
        // nothing more: the click it ends in does not also open the large window.
        let root = root.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.choosing.is_some() {
                    this.choose(None);
                    this.swallow_until = Some(Instant::now() + SWALLOWED_FOR);
                    cx.notify();
                }
            }),
        );

        // The picture: the way to the large window, where it is seen whole and drawn on. With the
        // pointer over it, it darkens and says what a click does.
        let picture = div()
            .id("picture")
            .relative()
            .flex_1()
            .min_h_0()
            .border_1()
            .border_color(theme.border)
            .rounded(px(6.0))
            .overflow_hidden()
            .bg(theme.surface)
            .cursor(CursorStyle::PointingHand)
            .children(look.picture.clone().map(|picture| img(picture).size_full()))
            .when(sampling, |picture| {
                picture
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(12.0))
                    .text_color(theme.muted)
                    .child(t("preview.here"))
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.0))
                    .text_color(gpui::white())
                    .bg(Hsla::from(gpui::rgba(0x0000_0073)))
                    .opacity(dark)
                    .child(icon(PEN, 18.0))
                    .child(t("preview.clickToEdit")),
            )
            .tooltip(tip(t("preview.clickToEdit")))
            .child(self.marks.mark("picture"))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered(*hovered);
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                if this
                    .swallow_until
                    .take()
                    .is_some_and(|until| Instant::now() < until)
                {
                    return;
                }
                this.hovered(false);
                cx.notify();
                cx.defer(move |cx| enlarge(id, cx));
            }));

        // What is on the clipboard, and how large the capture is; or what went wrong last (in two
        // lines at most, the whole of it in its tooltip).
        let status = match look.failure.clone() {
            Some(failure) => div()
                .id("status")
                .min_h(px(18.2))
                .text_color(theme.bad)
                .line_clamp(2)
                .tooltip(tip(failure.clone()))
                .child(failure),
            None => div()
                .id("status")
                .min_h(px(18.2))
                .flex()
                .items_center()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_color(theme.muted)
                .child(self.said(&theme, window))
                .when(look.copied, |status| status.child("\u{a0}·\u{a0}"))
                .child(format!("{} × {}", look.width, look.height)),
        };

        // The tour opens the choice beside saving for its step, and puts it away after.
        let tour_opens = self.tour.as_ref().is_some_and(|tour| tour.at() == 3);
        if tour_opens && self.choosing.is_none() {
            self.choose(Some(Leaves::Save));
            self.chosen_by_tour = true;
        } else if !tour_opens && self.chosen_by_tour {
            self.choose(None);
            self.chosen_by_tour = false;
        }
        let actions = self.actions(&look, &settings, &theme, window, cx);
        let words = tour_words(&self.language);
        let this = cx.entity().downgrade();
        let marks = self.marks.clone();
        let tour = self.tour.as_mut().map(|tour| {
            tour.view(&marks, &words, &theme, window, move |button, _, cx| {
                let _ = this.update(cx, |small, cx| small.tour_button(button, cx));
            })
        });
        root.child(picture)
            .child(status)
            .child(actions)
            // (Showing where it appears, nothing in it is pressed: a cover over all of it.)
            .when(sampling, |root| {
                root.child(div().id("sample").absolute().inset_0().occlude())
            })
            .children(tour.map(|tour| gpui::deferred(tour).with_priority(3)))
    }
}

/// A moment after the window came, so that it is seen to come first.
const TOUR_COMES_AFTER: Duration = Duration::from_millis(400);

/// Whether the tour of the small window was seen: the settings say so, or it was, since Perora
/// started.
fn tour_seen(cx: &mut App) -> bool {
    cx.default_global::<Previews>().toured
        || Perora::get(cx)
            .settings()
            .tours_seen
            .contains(&perora_core::settings::Tour::Preview)
}

/// The tour of the small window was seen (to the end, or put away): it does not come on its own
/// again.
fn saw_tour(cx: &mut App) {
    let previews = cx.default_global::<Previews>();
    if previews.toured {
        return;
    }
    previews.toured = true;
    let perora = Perora::get(cx);
    if let Some(path) = perora.settings_path.clone() {
        perora_core::settings::tour_seen(&path, &perora.settings, "preview");
    }
}

/// The words of a tour's own buttons, in the user's language.
pub fn tour_words(language: &Language) -> Words {
    let count = language.clone();
    let t = |key: &str| SharedString::from(language.text(key, &[]));
    Words {
        count: Box::new(move |at, of| {
            count.text(
                "tour.count",
                &[("at", &at.to_string()), ("of", &of.to_string())],
            )
        }),
        skip: t("tour.skip"),
        back: t("tour.back"),
        next: t("tour.next"),
        done: t("tour.done"),
    }
}

impl Small {
    /// What is said of the clipboard: a tick and "copied", or nothing. As it changes, what was said
    /// fades out where it is, and what is said now fades in from a little to the left (owner:
    /// 「既存の文字はそのままfadeout、新しい文字を左からうっすらfadeinさせてほしい」, 3 px).
    fn said(&self, theme: &Theme, window: &mut Window) -> impl IntoElement {
        let copied = |theme: &Theme| {
            div()
                .flex()
                .items_center()
                .whitespace_nowrap()
                .text_color(theme.good)
                .child(icon(TICK, 12.0).mr(px(5.0)))
                .child(self.language.text("preview.copied", &[]))
        };
        let (coming, going) = match self.copied_since {
            Some(since) => (
                motion::EASE_OUT.at(motion::along(since, Duration::ZERO, SAID_IN, window)),
                motion::EASE_OUT.at(motion::share(since, Duration::ZERO, SAID_OUT)),
            ),
            None => (1.0, 1.0),
        };
        div()
            .relative()
            .flex()
            // (What was said, while it fades: only "copied" says anything.)
            .when(!self.copied && going < 1.0, |said| {
                said.child(
                    copied(theme)
                        .absolute()
                        .left_0()
                        .top_0()
                        .opacity(1.0 - going),
                )
            })
            .when(self.copied, |said| {
                said.child(
                    copied(theme)
                        .relative()
                        .left(px(-3.0 * (1.0 - coming)))
                        .opacity(coming),
                )
            })
    }

    /// The copy button's mark: the sheets, or the tick while the clipboard holds the capture as it
    /// would be copied now. Copied, the sheets shrink away and the tick is there in their place; after
    /// a copy made just now it comes in larger than it is and settles.
    fn copy_mark(&self, theme: &Theme, window: &mut Window) -> impl IntoElement {
        let cell = || {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
        };
        // (Both ways, as the sheets went: shrinking away, or back.)
        let turned = self.copied_since.map_or(1.0, |since| {
            motion::EASE_IN.at(motion::along(since, Duration::ZERO, SHEETS_GO, window))
        });
        let (sheets_seen, sheets_size) = if self.copied {
            (1.0 - turned, 1.0 - 0.6 * turned)
        } else {
            (turned, 0.4 + 0.6 * turned)
        };
        let (tick_seen, tick_size) = match (self.copied, self.ticked) {
            (false, _) => (0.0, 1.0),
            (true, None) => (1.0, 1.0),
            // Up to 1.45 times its size by 55 % of the way, then back to it.
            (true, Some(since)) => {
                let along = motion::along(since, TICK_WAITS, TICK_COMES, window);
                if along < 0.55 {
                    let up = BOUNCE.at(along / 0.55);
                    (up, 1.45 * up)
                } else {
                    let back = BOUNCE.at((along - 0.55) / 0.45);
                    (1.0, 1.45 - 0.45 * back)
                }
            }
        };
        div()
            .relative()
            .size_full()
            .when(sheets_seen > 0.0, |mark| {
                mark.child(
                    cell()
                        .opacity(sheets_seen)
                        .child(icon(SHEETS, (16.0 * sheets_size).max(0.5))),
                )
            })
            .when(tick_seen > 0.0, |mark| {
                mark.child(
                    cell()
                        .opacity(tick_seen)
                        .child(icon(TICK, (16.0 * tick_size).max(0.5)).text_color(theme.good)),
                )
            })
    }

    /// The row of what can be done with the capture: the tour, then saving (the one that stands
    /// out while uploading is not on offer), copying and throwing away; and beside saving and
    /// copying the small arrow that opens the choice of what it leaves as this once.
    fn actions(
        &mut self,
        look: &Look,
        settings: &Settings,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let id = look.id;
        let t = |key: &str| self.language.text(key, &[]);
        let base = |name: &'static str| {
            div()
                .id(name)
                .h(px(30.0))
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .cursor(CursorStyle::PointingHand)
                .whitespace_nowrap()
        };
        let accent = theme.accent;

        // The tour, shown again.
        let guide = div()
            .id("guide")
            .relative()
            .w(px(20.0))
            .h(px(30.0))
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme.muted)
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.text_color(theme.text))
            .tooltip(tip(t("tour.open")))
            .child(icon(GUIDE, 16.0))
            .child(self.marks.mark("guide"))
            .on_click(cx.listener(|this, _, _, cx| this.start_tour(cx)));

        let save = base("save")
            .flex_1()
            .px(px(8.0))
            .rounded_r(px(6.0))
            .bg(theme.accent)
            .border_color(theme.accent)
            .text_color(theme.accent_text)
            .tooltip(tip(format!(
                "{} · {}",
                t("preview.save"),
                self.leaves_as_said(settings, look.once.of(Leaves::Save), Leaves::Save)
            )))
            .child(t("preview.save"))
            .on_click(move |_, _, cx| cx.defer(move |cx| save(id, cx)));

        // (Named after what it says: GPUI keeps a tooltip that is up as it was made, and one that
        // says "copy" after the copy is done is made anew.)
        let copy = base(if look.copied { "copied" } else { "copy" })
            .w(px(34.0))
            .rounded_r(px(6.0))
            .hover(|style| style.border_color(accent))
            .tooltip(tip(if look.copied {
                t("preview.copied")
            } else {
                t("preview.copy")
            }))
            .child(self.copy_mark(theme, window))
            .on_click(move |_, _, cx| cx.defer(move |cx| copy(id, cx)));

        let bin = base("discard")
            .w(px(34.0))
            .rounded(px(6.0))
            .bg(rgb(BIN_COLOUR))
            .border_color(rgb(BIN_COLOUR))
            .text_color(gpui::white())
            .hover(|style| style.bg(rgb(BIN_HOVER)))
            .tooltip(tip(t("preview.discard")))
            .child(icon(BIN, 16.0))
            .on_click(move |_, _, cx| cx.defer(move |cx| discard_by_user(Some(id), cx)));

        let mut row = div()
            .id("actions")
            .relative()
            .flex()
            .gap(px(6.0))
            .h(px(30.0))
            // A press on the row puts the choice away (not the arrows and the choice itself: they
            // see to that), and is the row's: it is not a press beside it.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.choosing.is_some() {
                        this.choose(None);
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .child(guide)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .child(
                        div()
                            .relative()
                            .child(self.chevron(Leaves::Save, id, theme, cx))
                            .child(self.marks.mark("as")),
                    )
                    .child(save),
            )
            .child(self.marks.mark("actions"))
            .child(
                div()
                    .flex()
                    .child(self.chevron(Leaves::Copy, id, theme, cx))
                    .child(copy),
            )
            .child(bin);
        if let Some(leaves) = self.choosing {
            row = row.child(self.menu(leaves, look, settings, theme, window));
        }
        row
    }

    /// What the capture leaves as, one way, in words: the kinds of file (all of them, for saving),
    /// and how large at most when it is made smaller.
    fn leaves_as_said(&self, settings: &Settings, once: &Once, leaves: Leaves) -> String {
        leaves_as_said(&self.language, settings, once, leaves)
    }

    /// The small arrow beside a way to leave: it opens (or puts away) the choice of what the capture
    /// leaves as that way, this once.
    fn chevron(
        &self,
        leaves: Leaves,
        id: u64,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.choosing == Some(leaves);
        let (text, accent) = (theme.text, theme.accent);
        let what = self.language.text(
            match leaves {
                Leaves::Save => "preview.save",
                Leaves::Upload => "preview.upload",
                Leaves::Copy => "preview.copy",
            },
            &[],
        );
        let said = self.language.text("preview.as", &[("what", &what)]);
        div()
            .id(SharedString::from(format!("as-{}", leaves.name())))
            .w(px(18.0))
            .h(px(30.0))
            .flex()
            .items_center()
            .justify_center()
            .border_1()
            .border_r_0()
            .border_color(theme.border)
            .rounded_l(px(6.0))
            .bg(theme.surface)
            .text_color(if open { theme.text } else { theme.muted })
            .cursor(CursorStyle::PointingHand)
            .hover(move |style| style.text_color(text).border_color(accent))
            .tooltip(tip(said))
            .child(icon(CHEVRON, 9.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.choose(if open { None } else { Some(leaves) });
                if !open {
                    // (Opened, something is being done about the capture: it is not given up on
                    // its own meanwhile.)
                    cx.defer(move |cx| touch(id, cx));
                }
                cx.notify();
            }))
    }

    /// The choice of what the capture leaves as this once, over the row (the small window has no
    /// room for a list): the kinds of file, and under them how large at most (the numbers alone).
    fn menu(
        &self,
        leaves: Leaves,
        look: &Look,
        settings: &Settings,
        theme: &Theme,
        window: &mut Window,
    ) -> impl IntoElement {
        let id = look.id;
        // It comes up into its place from the row it belongs to (6 px, 0.16 s): seen to open, and
        // one opened in place of another is seen to be another.
        let up = if self.moves {
            motion::EASE_OUT.at(motion::along(
                self.opened,
                Duration::ZERO,
                CHOICE_COMES,
                window,
            ))
        } else {
            1.0
        };
        let once = look.once.of(leaves).clone();
        let (format, kinds, fit) = leaves_as(settings, &once, leaves);
        let item = |name: SharedString, label: String, checked: bool| {
            let accent = theme.accent;
            div()
                .id(name)
                .flex_1()
                .h(px(26.0))
                .px(px(6.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.0))
                .rounded(px(5.0))
                .border_1()
                .border_color(if checked { theme.accent } else { theme.border })
                .when(checked, |item| {
                    item.text_color(theme.accent).shadow(vec![BoxShadow {
                        color: theme.accent,
                        offset: point(px(0.0), px(0.0)),
                        blur_radius: px(0.0),
                        spread_radius: px(1.0),
                        inset: true,
                    }])
                })
                .cursor(CursorStyle::PointingHand)
                .hover(move |style| style.border_color(accent))
                .whitespace_nowrap()
                .child(label)
        };
        let formats = Format::ALL.into_iter().map(|each| {
            let checked = match leaves {
                Leaves::Save => kinds.contains(&each),
                _ => format == each,
            };
            let label = self
                .language
                .text(&format!("main.output.format.{}", each.name()), &[]);
            let now = kinds.clone();
            item(
                SharedString::from(format!("format-{}", each.name())),
                label,
                checked,
            )
            .on_click(move |_, _, cx| {
                let now = now.clone();
                cx.defer(move |cx| {
                    choose(
                        id,
                        leaves,
                        |once| match leaves {
                            // Saved as several kinds at once, one file of each: ticked, not
                            // chosen. The last one ticked stays so.
                            Leaves::Save => {
                                let kinds: Vec<Format> = Format::ALL
                                    .into_iter()
                                    .filter(|one| now.contains(one) != (*one == each))
                                    .collect();
                                if !kinds.is_empty() {
                                    once.formats = Some(kinds);
                                }
                            }
                            _ => once.format = Some(each),
                        },
                        cx,
                    )
                });
            })
        });
        let fits = std::iter::once(0).chain(FITS).map(|side| {
            // (Room for the number alone: the rest is its tooltip.)
            let (label, said) = if side == 0 {
                (
                    self.language.text("preview.fit.none", &[]),
                    self.language.text("main.output.fit.none", &[]),
                )
            } else {
                (
                    side.to_string(),
                    self.language
                        .text("main.output.fit.within", &[("side", &side.to_string())]),
                )
            };
            item(
                SharedString::from(format!("fit-{side}")),
                label,
                fit == side,
            )
            .tooltip(tip(said))
            .on_click(move |_, _, cx| {
                cx.defer(move |cx| {
                    choose(
                        id,
                        leaves,
                        |once| once.fit = Some((side != 0).then_some(side)),
                        cx,
                    )
                })
            })
        });
        div()
            .id("menu")
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(36.0 - 6.0 * (1.0 - up)))
            .opacity(up)
            .flex()
            .flex_wrap()
            .gap(px(4.0))
            .p(px(6.0))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .rounded(px(8.0))
            .shadow(vec![BoxShadow {
                color: Hsla::from(gpui::rgba(0x0000_0047)),
                offset: point(px(0.0), px(4.0)),
                blur_radius: px(14.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(formats)
            // (The second row starts here.)
            .child(div().w_full().h_0())
            .children(fits)
            .child(self.marks.mark("menu"))
    }
}
