//! Capturing: the shortcut (or the icon's menu) asks, the screen is taken at once, and the selection
//! screen is put over it. Pressed again while that is up, the screen is taken anew without it.
//!
//! The screen is taken on a thread of the background, so that the UI thread can meanwhile make the
//! GPU's device again (Perora lets go of it while no window is open: 25–36 ms to make).

use std::sync::Arc;
use std::time::{Duration, Instant};

use std::path::PathBuf;

use gpui::{App, Entity, Global};
use perora_core::geometry::{Point, Rect};
use perora_core::language::Language;
use perora_core::notice::Sound;
use perora_core::platform::{self, Desktop, Frame, ScreenCapture, WindowId};
use perora_core::selection::Mode;
use perora_core::settings::{After, Leaves, Settings};
use perora_core::{copying, saving, shots};

use crate::notice::{self, Lasting};
use crate::selection::{self, Selection, Taken};
use crate::state::Perora;
use crate::{account, corner, gpu, large, main_window, preview};

/// How long a window that was just hidden may still be on screen: the compositor draws the next
/// frame without it only after a moment. The main window and the notice (as in the Tauri version),
/// and the selection screen taken away for the next.
const SETTLE_MAIN: Duration = Duration::from_millis(60);
const SETTLE: Duration = Duration::from_millis(40);
/// How long the question over what is drawn takes to be gone from the screen (it fades out as it
/// closes, as windows do) before the screen is taken.
const ASKED_GONE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Capturing {
    /// The screen is being taken: a press meanwhile (the key held down) is nothing.
    busy: bool,
    /// The selection screen that is up.
    current: Option<Entity<Selection>>,
    last_id: u64,
    /// The main window, put out of the way for the capture: back when it is over.
    put_aside: Option<WindowId>,
}

impl Global for Capturing {}

/// Takes the screen and puts the selection screen over it.
pub fn start(cx: &mut App) {
    if cx.default_global::<Capturing>().busy {
        return;
    }
    // A capture that goes to the preview takes the place of the image there: when something is
    // drawn on that, the user says first whether that is what they want (owner, 2026-11-02: 「撮影
    // 入る前に出そうか」). Not when the capture goes elsewhere (only copied), nor while the selection
    // screen is up (agreed to already, and the question would be behind it).
    let to_preview = Perora::get(cx).settings().after_capture != After::Copy;
    let selecting = cx.global::<Capturing>().current.is_some();
    let language = Perora::get(cx).language();
    if to_preview
        && !selecting
        && let Some(answer) = large::ask_over_drawing(
            &language.capture_over_drawing(),
            language.capture_anyway(),
            cx,
        )
    {
        cx.global_mut::<Capturing>().busy = true;
        cx.spawn(async move |cx| {
            let go = answer.await == Some(0);
            if go {
                cx.background_executor().timer(ASKED_GONE).await;
            }
            cx.update(|cx| {
                cx.global_mut::<Capturing>().busy = false;
                if go {
                    start_now(cx);
                }
            });
        })
        .detach();
        return;
    }
    start_now(cx);
}

/// The capture is taken (asked about first when that was needed).
fn start_now(cx: &mut App) {
    let asked = Instant::now();
    let capturing = cx.default_global::<Capturing>();
    if capturing.busy {
        return;
    }
    capturing.busy = true;
    let settings = Perora::get(cx).settings();
    let hide = settings.hide_own_windows;
    let start_mode = settings.start_mode;

    // The selection screen that is up is not in the picture: out of sight at once, closed once
    // this one is there.
    let before = cx.global_mut::<Capturing>().current.take();
    if let Some(before) = &before {
        selection::put_away(before, cx);
    }
    // Nor is a notice, ever; nor are Perora's own windows, unless the user wants them in it.
    // (Pressed again while a selection screen is up, they were put out of the way for that one
    // already.)
    let noticed = notice::put_away(cx);
    let previewed = hide && preview::put_away(cx);
    let put_aside = if hide {
        main_window::step_aside(cx)
    } else {
        None
    };
    if put_aside.is_some() {
        cx.global_mut::<Capturing>().put_aside = put_aside;
    }
    let own: Vec<WindowId> = if hide {
        Vec::new()
    } else {
        main_window::on_screen_now(cx)
            .into_iter()
            .chain(preview::on_screen_now(cx))
            .collect()
    };
    let settle = if put_aside.is_some() || noticed || previewed {
        Some(SETTLE_MAIN)
    } else {
        before.is_some().then_some(SETTLE)
    };

    let executor = cx.background_executor().clone();
    let taking = cx.background_executor().spawn(async move {
        if let Some(settle) = settle {
            executor.timer(settle).await;
        }
        take(&own, start_mode != Mode::AllMonitors)
    });
    // While the screen is taken: the GPU back, and the windows of the selection screen opened (out
    // of sight, empty). Not for every monitor at once: no screen is put up.
    let attaching = Instant::now();
    let prepared = if start_mode != Mode::AllMonitors {
        gpu::attach(cx);
        selection::prepare(cx)
    } else {
        None
    };
    let attached = attaching.elapsed();

    cx.spawn(async move |cx| {
        let taken = taking.await;
        cx.update(|cx| {
            eprintln!(
                "capture: taken {:.0} ms after the shortcut (the GPU back and the windows opened in {:.0} ms meanwhile)",
                asked.elapsed().as_secs_f64() * 1000.0,
                attached.as_secs_f64() * 1000.0
            );
            arrived(taken, prepared, before, hide, start_mode, asked, cx);
        });
    })
    .detach();
}

/// The screen as it was at the press, and the windows on it; with each monitor's part ready to be
/// drawn when the selection screen is to show it.
fn take(own: &[WindowId], parts: bool) -> Result<Taken, String> {
    let started = Instant::now();
    let capture = platform::screen_capture();
    let still = capture.capture_all().map_err(|error| error.to_string())?;
    let captured = started.elapsed();
    let layout = capture.layout().map_err(|error| error.to_string())?;
    let desktop = platform::desktop();
    // In the picture, Perora's own windows are there to be picked like any other, where they are
    // in the stacking order.
    let windows = desktop.windows_with(own).unwrap_or_default();
    let pointer = desktop
        .pointer()
        .unwrap_or_else(|_| Point::new(layout.bounds.x, layout.bounds.y));
    let listed = started.elapsed();
    // Each monitor's part, all at once.
    let parts = if parts {
        std::thread::scope(|scope| {
            let cutting: Vec<_> = layout
                .monitors
                .iter()
                .map(|monitor| {
                    let still = &still;
                    scope.spawn(move || still.crop(monitor.bounds).and_then(selection::drawable))
                })
                .collect();
            cutting
                .into_iter()
                .map(|part| part.join().ok().flatten())
                .collect()
        })
    } else {
        Vec::new()
    };
    let ms = |time: Duration| time.as_secs_f64() * 1000.0;
    eprintln!(
        "capture: the screen in {:.0} ms, the windows by {:.0} ms, the parts to draw by {:.0} ms",
        ms(captured),
        ms(listed),
        ms(started.elapsed())
    );
    Ok(Taken {
        still: Arc::new(still),
        layout,
        windows,
        pointer,
        parts,
        own: own.to_vec(),
    })
}

fn arrived(
    taken: Result<Taken, String>,
    prepared: Option<selection::Prepared>,
    before: Option<Entity<Selection>>,
    hide: bool,
    start_mode: Mode,
    asked: Instant,
    cx: &mut App,
) {
    cx.global_mut::<Capturing>().busy = false;
    let was_up = before.is_some();
    if let Some(before) = before {
        selection::close(&before, cx);
    }
    let taken = match taken {
        Ok(taken) => taken,
        Err(detail) => {
            if let Some(prepared) = prepared {
                selection::discard(prepared, cx);
            }
            let message = Perora::get(cx).language().capture_failed(&detail);
            eprintln!("{message}");
            come_back(cx);
            gpu::release_soon(cx);
            notice::say(message, Lasting::Long, None, cx);
            // (What was being previewed is back, when it was put out of the way: left in the
            // picture, it never was.)
            if hide || was_up {
                preview::restore(cx);
            }
            return;
        }
    };
    // Every monitor is the one mode with nothing to choose: no screen for it.
    if start_mode == Mode::AllMonitors {
        come_back(cx);
        let area = taken.still.area;
        deliver(taken.still, area, cx);
        return;
    }
    // Perora's own windows were left in the picture: out of the way now, under the selection
    // screen (which shows them as they were), and back as that is done with.
    if !hide {
        preview::put_away(cx);
        if let Some(window) = main_window::step_aside(cx) {
            cx.global_mut::<Capturing>().put_aside = Some(window);
        }
    }
    let id = {
        let capturing = cx.global_mut::<Capturing>();
        capturing.last_id += 1;
        capturing.last_id
    };
    // (Let go of meanwhile, should the last window have closed just before the press: windows
    // cannot be opened without it.)
    gpu::attach(cx);
    match selection::open(id, taken, prepared, start_mode, asked, cx) {
        Some(selection) => cx.global_mut::<Capturing>().current = Some(selection),
        None => {
            eprintln!("selection {id}: no window could be opened");
            come_back(cx);
            gpu::release_soon(cx);
            preview::restore(cx);
        }
    }
}

/// The selection screen that is up.
pub fn current(cx: &mut App) -> Option<Entity<Selection>> {
    cx.default_global::<Capturing>().current.clone()
}

/// Whether the screen is being taken or a capture selected: no notice is shown meanwhile.
pub fn busy(cx: &mut App) -> bool {
    let capturing = cx.default_global::<Capturing>();
    capturing.busy || capturing.current.is_some()
}

/// The selection screen is down (chosen or given up): the main window comes back.
pub fn ended(selection: &Entity<Selection>, cx: &mut App) {
    let capturing = cx.default_global::<Capturing>();
    if capturing
        .current
        .as_ref()
        .is_some_and(|current| current == selection)
    {
        capturing.current = None;
        come_back(cx);
    }
}

fn come_back(cx: &mut App) {
    if let Some(window) = cx.default_global::<Capturing>().put_aside.take() {
        main_window::come_back(window);
    }
}

/// What becomes of a capture, as the user has it: the capture's sound the moment there is one, then
/// the small preview (P4: also when the user has captures drawn on at once, until the large window
/// is there), or the clipboard and nothing else, or straight to the server when the user has
/// captures uploaded at once (and uploading is on offer: switched on, someone signed in).
pub fn deliver(source: Arc<Frame>, area: Rect, cx: &mut App) {
    let perora = Perora::get(cx);
    let settings = perora.settings();
    sound(&settings);
    if settings.upload_immediately && account::uploads(cx) {
        return upload_alone(source, area, settings, cx);
    }
    match settings.after_capture {
        After::Copy => copy_alone(source, area, settings, cx),
        After::Preview | After::Edit => to_preview(source, area, settings, cx),
    }
}

/// Straight to the server, as the user has captures uploaded (the kind of file, and how large at
/// most), with no preview: a notice says it was captured and is on its way, and another how that
/// ended, with the link on the clipboard. Saved as well when the user has every capture saved. What
/// was being previewed before is still to be dealt with: it comes back.
fn upload_alone(source: Arc<Frame>, area: Rect, settings: Settings, cx: &mut App) {
    preview::restore(cx);
    let Some(account) = account::account(cx) else {
        return;
    };
    let language = Perora::get(cx).language();
    notice::say(language.captured(), Lasting::UntilTheNext, None, cx);
    let how = settings.output(Leaves::Upload);
    let saves = settings.save_on_capture;
    let cropping = cx
        .background_executor()
        .spawn(async move { source.crop(area).map(Arc::new) });
    cx.spawn(async move |cx| {
        // (Somewhere on it: whoever chose it saw to that.)
        let Some(frame) = cropping.await else {
            cx.update(|cx| {
                let said = language.capture_failed("nothing was chosen");
                notice::say(said, Lasting::Long, None, cx);
            });
            return;
        };
        if saves {
            let frame = frame.clone();
            cx.update(|cx| save_as_taken(None, frame, None, settings.clone(), cx));
        }
        let sending = cx.background_executor().spawn(async move {
            let file = perora_core::preview::Pending::new(0, frame, None, None, how)
                .file(how)
                .map_err(|detail| (language.capture_failed(&detail), false))?;
            shots::upload(&account, &file).map_err(|error| {
                let signed_out = matches!(error, shots::ApiError::SignedOut);
                (language.upload_failed(&error), signed_out)
            })
        });
        let outcome = sending.await;
        cx.update(|cx| {
            let language = Perora::get(cx).language();
            let (said, lasting) = match outcome {
                Ok(shot) => {
                    account::uploaded(cx);
                    // (Only an address of the server's own goes on the clipboard.)
                    let copied = if account::is_at_server(&shot.url, cx) {
                        arboard::Clipboard::new()
                            .and_then(|mut clipboard| clipboard.set_text(shot.url.clone()))
                    } else {
                        Err(arboard::Error::ContentNotAvailable)
                    };
                    match copied {
                        Ok(()) => (language.uploaded(), Lasting::Brief),
                        Err(error) => {
                            eprintln!("clipboard: the link was not put there: {error}");
                            (language.uploaded_but_not_copied(&shot.url), Lasting::Long)
                        }
                    }
                }
                Err((said, signed_out)) => {
                    if signed_out {
                        account::signed_out_by_server(cx);
                    }
                    (said, Lasting::Long)
                }
            };
            notice::say(said, lasting, None, cx);
        });
    })
    .detach();
}

/// The capture's sound, as the user has it (none, one of the system's, or a file of their own).
/// Also with the small preview shown where it appears, as a capture shows it (owner, 2026-11-07:
/// 「プレビューのやつは通知音も一緒に鳴らすようにしたいね、気づかないかも人が」).
pub fn sound(settings: &Settings) {
    let sound = match &settings.capture_sound {
        Sound::None => None,
        Sound::System(name) => platform::system_sound(name),
        Sound::Custom => settings.capture_sound_file.clone(),
    };
    if let Some(sound) = sound {
        platform::play_sound(&sound);
    }
}

/// To the preview: on the clipboard first (unless the user would rather not), so that it is there
/// to paste by the time the preview is seen; saved as well when the user has every capture saved.
fn to_preview(source: Arc<Frame>, area: Rect, settings: Settings, cx: &mut App) {
    let id = preview::ask(cx);
    let scale = corner::monitor().map_or(1.0, |monitor| monitor.scale);
    let how = settings.output(Leaves::Copy);
    let touched = settings.after_capture == After::Edit;
    // (In the order of the captures: the last one taken is what stays on the clipboard.)
    let turn = settings.copy_to_clipboard.then(copying::turn);
    let making = cx.background_executor().spawn(async move {
        let started = Instant::now();
        // (Somewhere on it: whoever chose it saw to that.)
        let frame = Arc::new(source.crop(area)?);
        drop(source);
        let (copied, png) = match turn.map(|turn| copying::to_clipboard(&frame, how, turn)) {
            None => (None, None),
            Some(Ok(copied)) => (
                Some(Ok((!copied.passed).then_some(copied.sequence).flatten())),
                copied.png.map(Arc::new),
            ),
            Some(Err(detail)) => {
                eprintln!("clipboard: the capture was not put there: {detail}");
                (Some(Err(detail)), None)
            }
        };
        let copying_took = started.elapsed();
        // (Drawn on at once: the picture of the large window, not of the corner.)
        let (picture, editor_picture) = if touched {
            (None, perora_editor::Picture::new(&frame))
        } else {
            (preview::picture_of(&frame, scale), None)
        };
        eprintln!(
            "deliver: {}x{} copied in {:.0} ms, its picture made in {:.0} ms more",
            frame.width(),
            frame.height(),
            copying_took.as_secs_f64() * 1000.0,
            (started.elapsed() - copying_took).as_secs_f64() * 1000.0
        );
        Some(preview::Shown {
            id,
            frame,
            png,
            copied,
            copied_as: how,
            picture,
            touched,
            large: touched,
            editor_picture,
        })
    });
    cx.spawn(async move |cx| {
        let Some(shown) = making.await else {
            // (Nothing to show: what was being previewed before is back.)
            cx.update(preview::restore);
            return;
        };
        // (Saved once it is shown: the preview is not kept waiting for the files.)
        let taken = settings
            .save_on_capture
            .then(|| (shown.frame.clone(), shown.png.clone()));
        cx.update(|cx| {
            preview::present(shown, cx);
            if let Some((frame, png)) = taken {
                save_as_taken(Some(id), frame, png, settings, cx);
            }
        });
    })
    .detach();
}

/// Saves a capture as it was taken, apart from whatever else becomes of it, and says where it went:
/// unless it was discarded meanwhile, and taken away with that (`id`: the preview it is shown in,
/// when it is).
fn save_as_taken(
    id: Option<u64>,
    frame: Arc<Frame>,
    png: Option<Arc<Vec<u8>>>,
    settings: Settings,
    cx: &mut App,
) {
    let saving = cx.background_executor().spawn(async move {
        saving::save_as_taken(&settings, &frame, png.as_deref().map(Vec::as_slice))
    });
    cx.spawn(async move |cx| {
        let saved = saving.await;
        cx.update(|cx| {
            let language = Perora::get(cx).language();
            match saved {
                Ok(files) => {
                    if id.is_some_and(|id| !preview::saved_as_taken(id, &files, cx)) {
                        return;
                    }
                    let Some(first) = files.first().cloned() else {
                        return;
                    };
                    let message =
                        language.saved_as_taken(&first.display().to_string(), files.len() - 1);
                    notice::say_of_file(message, Lasting::Brief, first, cx);
                }
                Err(detail) => notice::say(
                    language.not_saved_as_taken(&detail),
                    Lasting::Long,
                    None,
                    cx,
                ),
            }
        });
    })
    .detach();
}

/// To the clipboard and nothing else (the user chose to have no preview): as copies are made (the
/// kind of file, and how large at most), whether or not previewed captures are copied; saved as
/// well when the user has every capture saved, and one notice says what became of it. What was
/// being previewed before is still to be dealt with: it comes back.
fn copy_alone(source: Arc<Frame>, area: Rect, settings: Settings, cx: &mut App) {
    preview::restore(cx);
    let language = Perora::get(cx).language();
    let how = settings.output(Leaves::Copy);
    let saves = settings.save_on_capture;
    // (In the order of the captures: the last one taken is what stays on the clipboard.)
    let turn = copying::turn();
    let working = cx.background_executor().spawn(async move {
        let started = Instant::now();
        // (Somewhere on it: whoever chose it saw to that.)
        let Some(frame) = source.crop(area) else {
            return (Err("nothing was chosen".to_owned()), None);
        };
        drop(source);
        let copied = copying::to_clipboard(&frame, how, turn);
        let copying_took = started.elapsed();
        // (Saved meanwhile too, when it is: one notice says both, and a click on it shows the file.
        // As the PNG the copy was made into, when that is the picture as it was.)
        let png = copied
            .as_ref()
            .ok()
            .and_then(|copied| copied.png.as_deref());
        let saved = saves.then(|| saving::save_as_taken(&settings, &frame, png));
        let copied = copied.map(|copied| copied.passed);
        eprintln!(
            "deliver: {}x{} copied in {:.0} ms{}",
            frame.area.width,
            frame.area.height,
            copying_took.as_secs_f64() * 1000.0,
            if saves {
                format!(
                    ", saved in {:.0} ms more",
                    (started.elapsed() - copying_took).as_secs_f64() * 1000.0
                )
            } else {
                String::new()
            }
        );
        if let Err(detail) = &copied {
            eprintln!("clipboard: the capture was not put there: {detail}");
        }
        (copied, saved)
    });
    cx.spawn(async move |cx| {
        let (copied, saved) = working.await;
        cx.update(|cx| tell(copied, saved, &language, cx));
    })
    .detach();
}

/// What became of each is said: one of them not done is no reason to leave out the other. (`copied`
/// is whether a later capture went past this one to the clipboard: then only what was saved of it
/// is said, if anything was.)
fn tell(
    copied: Result<bool, String>,
    saved: Option<Result<Vec<PathBuf>, String>>,
    language: &Language,
    cx: &mut App,
) {
    if copied == Ok(true) {
        match saved {
            Some(Ok(files)) => {
                let first = files.first().cloned().unwrap_or_default();
                let message =
                    language.saved_as_taken(&first.display().to_string(), files.len() - 1);
                notice::say_of_file(message, Lasting::Brief, first, cx);
            }
            Some(Err(detail)) => notice::say(
                language.not_saved_as_taken(&detail),
                Lasting::Long,
                None,
                cx,
            ),
            None => {}
        }
        return;
    }
    let copied = copied.map(|_| ());
    match (copied, saved) {
        (Ok(()), None) => notice::say(language.copied(), Lasting::Brief, None, cx),
        (Err(detail), None) => notice::say(language.not_copied(&detail), Lasting::Long, None, cx),
        (copied, Some(Ok(files))) => {
            let first = files.first().cloned().unwrap_or_default();
            let shown = first.display().to_string();
            let (lasting, message) = match copied {
                Ok(()) => (Lasting::Brief, language.copied_and_saved(&shown)),
                Err(detail) => (
                    Lasting::Long,
                    language.saved_but_not_copied(&shown, &detail),
                ),
            };
            notice::say_of_file(message, lasting, first, cx);
        }
        (copied, Some(Err(detail))) => {
            let message = match copied {
                Ok(()) => language.copied_but_not_saved(&detail),
                Err(not_copied) => format!(
                    "{}\n{}",
                    language.not_copied(&not_copied),
                    language.not_saved_as_taken(&detail)
                ),
            };
            notice::say(message, Lasting::Long, None, cx);
        }
    }
}
