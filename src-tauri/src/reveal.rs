//! The screen on which the part of the monitor that brings the selection screen's mode bar back is
//! set: opened from the settings, over the monitor the main window is on, with a still of it behind
//! and the bar drawn where it is. The part is drawn and adjusted there as the editor's crop is
//! (owner, 2026-11-02: 「設定ボタン押したらキャプチャ画面みたいなのが出てくるイメージ！」).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use perora_core::platform::{Desktop, ScreenCapture, desktop, screen_capture};
use serde::Serialize;
use tauri::window::Color;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::settings::Zone;
use crate::{Perora, main_window, preview};

pub const LABEL: &str = "reveal";

/// How long the main window takes to be off the screen once hidden, before the still is taken.
const SETTLE: Duration = Duration::from_millis(80);

/// A window that has not said it drew the still by now is shown anyway.
const SHOW_ANYWAY: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct State {
    /// How many stills there were.
    next: AtomicU64,
    /// Whether the screen is on its way up: from the press to the still being kept.
    opening: AtomicBool,
    /// The still behind the screen while it is up, and which one it is (a new one each time, so
    /// that the page does not show the one before).
    still: Mutex<Option<(u64, Arc<Vec<u8>>)>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    language: String,
    texts: std::collections::HashMap<String, String>,
    /// The part as it is set now.
    zone: Zone,
    /// The modes, for the bar drawn as it is on the selection screen.
    modes: Vec<&'static str>,
    /// Which still is behind it; none while the screen is not up.
    still: Option<u64>,
}

/// Whether the screen is up, or on its way: no capture is taken meanwhile (it would take this
/// screen, and bring the main window back over it).
pub fn is_up(app: &AppHandle) -> bool {
    let state = &app.state::<Perora>().reveal;
    state.opening.load(Ordering::Acquire) || state.still.lock().unwrap().is_some()
}

/// From the settings: the screen comes up. Off the main thread: the still takes a moment. Once:
/// pressed again while it is coming or up, nothing more happens.
#[tauri::command]
pub fn main_edit_reveal_area(app: AppHandle) {
    if is_up(&app)
        || app
            .state::<Perora>()
            .reveal
            .opening
            .swap(true, Ordering::AcqRel)
    {
        return;
    }
    std::thread::spawn(move || {
        let result = open(&app);
        app.state::<Perora>()
            .reveal
            .opening
            .store(false, Ordering::Release);
        if let Err(error) = result {
            eprintln!("the screen for the bar's area: {error}");
            close(&app);
        }
    });
}

fn open(app: &AppHandle) -> Result<(), String> {
    let capture = screen_capture();
    let layout = capture.layout().map_err(|error| error.to_string())?;
    // The monitor the main window is on (its middle), else the pointer's, else the first.
    let middle = app
        .get_webview_window(main_window::LABEL)
        .and_then(|window| {
            let at = window.outer_position().ok()?;
            let size = window.outer_size().ok()?;
            Some(perora_core::geometry::Point::new(
                at.x + size.width as i32 / 2,
                at.y + size.height as i32 / 2,
            ))
        });
    let pointer = desktop().pointer().ok();
    let monitor = layout
        .monitors
        .iter()
        .find(|monitor| middle.is_some_and(|at| monitor.bounds.contains(at)))
        .or_else(|| {
            layout
                .monitors
                .iter()
                .find(|monitor| pointer.is_some_and(|at| monitor.bounds.contains(at)))
        })
        .or_else(|| layout.monitors.first())
        .ok_or("no monitor")?
        .bounds;

    // The still without the main window in it: it is out of the way meanwhile, and back after.
    if main_window::step_aside(app) {
        std::thread::sleep(SETTLE);
    }
    let still = capture
        .capture_all()
        .map_err(|error| error.to_string())?
        .crop(monitor)
        .map(|part| part.to_bmp())
        .ok_or("the monitor is not in the capture")?;
    let perora = app.state::<Perora>();
    let id = perora.reveal.next.fetch_add(1, Ordering::Relaxed) + 1;
    *perora.reveal.still.lock().unwrap() = Some((id, Arc::new(still)));

    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => {
            let window =
                WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
                    .title("Perora")
                    .decorations(false)
                    .resizable(false)
                    .shadow(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .visible(false)
                    .background_color(Color(0, 0, 0, 255))
                    .devtools(false)
                    .zoom_hotkeys_enabled(false)
                    .on_page_load(preview::not_a_browser())
                    .build()
                    .map_err(|error| error.to_string())?;
            preview::without_transitions(&window);
            window
        }
    };
    let _ = window.set_position(PhysicalPosition::new(monitor.x, monitor.y));
    let _ = window.set_size(PhysicalSize::new(monitor.width, monitor.height));
    // Told to look again (it may have been up before), and shown once it has drawn the still.
    let _ = tauri::Emitter::emit_to(app, LABEL, "reveal-changed", ());
    let shown_for = app
        .state::<Perora>()
        .reveal
        .still
        .lock()
        .unwrap()
        .as_ref()
        .map(|(id, _)| *id);
    let later = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_ANYWAY);
        let now = later
            .state::<Perora>()
            .reveal
            .still
            .lock()
            .unwrap()
            .as_ref()
            .map(|(id, _)| *id);
        if now.is_some() && now == shown_for {
            show(&later);
        }
    });
    Ok(())
}

fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Takes the screen down, and the main window comes back. (Also when the window is closed by the
/// system, as with Alt+F4.)
pub fn close(app: &AppHandle) {
    *app.state::<Perora>().reveal.still.lock().unwrap() = None;
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
    main_window::come_back(app);
}

#[tauri::command]
pub fn reveal_state(app: AppHandle) -> View {
    let perora = app.state::<Perora>();
    let language = perora.language();
    View {
        language: language.code().to_owned(),
        texts: language.texts(),
        zone: perora.setting(|settings| settings.bar_reveal_area),
        modes: perora_core::selection::Mode::ALL
            .iter()
            .map(|mode| mode.name())
            .collect(),
        still: perora
            .reveal
            .still
            .lock()
            .unwrap()
            .as_ref()
            .map(|(id, _)| *id),
    }
}

/// The page drew the still: the screen can be shown.
#[tauri::command]
pub fn reveal_ready(app: AppHandle, window: WebviewWindow, still: u64) {
    let now = app
        .state::<Perora>()
        .reveal
        .still
        .lock()
        .unwrap()
        .as_ref()
        .map(|(id, _)| *id);
    if window.label() == LABEL && now == Some(still) {
        show(&app);
    }
}

/// Done with the screen: the part is set when one is given (saved), not when none is (given up).
/// Only from the screen, while it is up, and once.
#[tauri::command]
pub fn reveal_done(app: AppHandle, window: WebviewWindow, zone: Option<serde_json::Value>) {
    if window.label() != LABEL
        || app
            .state::<Perora>()
            .reveal
            .still
            .lock()
            .unwrap()
            .take()
            .is_none()
    {
        return;
    }
    if let Some(zone) = zone.as_ref().and_then(Zone::from_value) {
        crate::settings::update(&app, |settings| settings.bar_reveal_area = zone);
        main_window::changed(&app);
    }
    close(&app);
}

/// The still behind the screen, for the page: `reveal/<id>.bmp`.
pub fn image(perora: &Perora, path: &[&str]) -> Option<(&'static str, Arc<Vec<u8>>)> {
    let kept = perora.reveal.still.lock().unwrap();
    let (id, still) = kept.as_ref()?;
    match path {
        [file] if file.strip_suffix(".bmp")?.parse() == Ok(*id) => {
            Some(("image/bmp", still.clone()))
        }
        _ => None,
    }
}
