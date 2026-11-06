//! Capturing: the shortcut (or the icon's menu) asks, the screen is taken at once, and the selection
//! screen is put over it. Pressed again while that is up, the screen is taken anew without it.
//!
//! The screen is taken on a thread of the background, so that the UI thread can meanwhile make the
//! GPU's device again (Perora lets go of it while no window is open: 25–36 ms to make).

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, Entity, Global};
use perora_core::geometry::Point;
use perora_core::notice::Sound;
use perora_core::platform::{self, Desktop, Frame, ScreenCapture, WindowId};
use perora_core::selection::Mode;

use crate::selection::{self, Selection, Taken};
use crate::state::Perora;
use crate::{gpu, main_window};

/// How long a window that was just hidden may still be on screen: the compositor draws the next
/// frame without it only after a moment. The main window (as in the Tauri version), and the
/// selection screen taken away for the next.
const SETTLE_MAIN: Duration = Duration::from_millis(60);
const SETTLE: Duration = Duration::from_millis(40);

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
    // Nor are Perora's own windows, unless the user wants them in it. (Pressed again while a
    // selection screen is up, they were put out of the way for that one already.)
    let put_aside = if hide { main_window::step_aside(cx) } else { None };
    if put_aside.is_some() {
        cx.global_mut::<Capturing>().put_aside = put_aside;
    }
    let own: Vec<WindowId> = if hide {
        Vec::new()
    } else {
        main_window::on_screen_now(cx).into_iter().collect()
    };
    let settle = if put_aside.is_some() {
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
    // The GPU back while the screen is taken.
    let attaching = Instant::now();
    gpu::attach(cx);
    let attached = attaching.elapsed();

    cx.spawn(async move |cx| {
        let taken = taking.await;
        cx.update(|cx| {
            eprintln!(
                "capture: taken {:.0} ms after the shortcut (the GPU back in {:.0} ms meanwhile)",
                asked.elapsed().as_secs_f64() * 1000.0,
                attached.as_secs_f64() * 1000.0
            );
            arrived(taken, before, hide, start_mode, asked, cx);
        });
    })
    .detach();
}

/// The screen as it was at the press, and the windows on it; with each monitor's part ready to be
/// drawn when the selection screen is to show it.
fn take(own: &[WindowId], parts: bool) -> Result<Taken, String> {
    let capture = platform::screen_capture();
    let still = capture.capture_all().map_err(|error| error.to_string())?;
    let layout = capture.layout().map_err(|error| error.to_string())?;
    let desktop = platform::desktop();
    // In the picture, Perora's own windows are there to be picked like any other, where they are
    // in the stacking order.
    let windows = desktop.windows_with(own).unwrap_or_default();
    let pointer = desktop
        .pointer()
        .unwrap_or_else(|_| Point::new(layout.bounds.x, layout.bounds.y));
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
    before: Option<Entity<Selection>>,
    hide: bool,
    start_mode: Mode,
    asked: Instant,
    cx: &mut App,
) {
    cx.global_mut::<Capturing>().busy = false;
    if let Some(before) = before {
        selection::close(&before, cx);
    }
    let taken = match taken {
        Ok(taken) => taken,
        Err(detail) => {
            // P3: the notice.
            eprintln!("{}", Perora::get(cx).language().capture_failed(&detail));
            come_back(cx);
            return;
        }
    };
    // Every monitor is the one mode with nothing to choose: no screen for it.
    if start_mode == Mode::AllMonitors {
        come_back(cx);
        deliver(Frame::clone(&taken.still), cx);
        return;
    }
    // Perora's own windows were left in the picture: out of the way now, under the selection
    // screen (which shows them as they were), and back as that is done with.
    if !hide && let Some(window) = main_window::step_aside(cx) {
        cx.global_mut::<Capturing>().put_aside = Some(window);
    }
    let id = {
        let capturing = cx.global_mut::<Capturing>();
        capturing.last_id += 1;
        capturing.last_id
    };
    match selection::open(id, taken, start_mode, asked, cx) {
        Some(selection) => cx.global_mut::<Capturing>().current = Some(selection),
        None => {
            eprintln!("selection {id}: no window could be opened");
            come_back(cx);
        }
    }
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

/// What the user chose, as P2 has it: the capture's sound, and the picture on the clipboard.
pub fn deliver(frame: Frame, cx: &mut App) {
    let settings = Perora::get(cx).settings();
    // Heard the moment there is a capture, whatever becomes of it.
    let sound = match settings.capture_sound {
        Sound::None => None,
        Sound::System(name) => platform::system_sound(&name),
        Sound::Custom => settings.capture_sound_file,
    };
    if let Some(sound) = sound {
        platform::play_sound(&sound);
    }
    cx.background_executor()
        .spawn(async move {
            let started = Instant::now();
            let copied = frame
                .to_png()
                .map_err(|error| error.to_string())
                .and_then(|png| platform::put_picture_on_clipboard(&frame, &png));
            match copied {
                Ok(()) => eprintln!(
                    "copied {}x{} in {:.0} ms",
                    frame.area.width,
                    frame.area.height,
                    started.elapsed().as_secs_f64() * 1000.0
                ),
                Err(detail) => eprintln!("clipboard: the capture was not put there: {detail}"),
            }
        })
        .detach();
}
