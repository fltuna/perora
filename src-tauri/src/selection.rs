//! The selection screen: the moment the shortcut was pressed, held still on every monitor, for the user
//! to say what of it to keep.
//!
//! One borderless window per monitor shows that monitor's part of the capture, dimmed, with what would
//! be chosen left bright. The windows only draw and report: "the button went down", "the pointer
//! moved", "a mode was picked". Where the pointer is comes from the system, not from them, and what a
//! press or a drag means is decided by `perora_core::selection`, in the coordinates of the whole
//! virtual screen. So a drag can start on one monitor and end on another, and monitors of different
//! scaling need no arithmetic on the page's side beyond drawing.
//!
//! In the window mode the screen is only a backdrop: there is a list of the windows, each with its
//! picture and its name, and one is picked from it. That also reaches a window hidden behind others,
//! which no click on the screen can. (Clicking a window on the screen is what the smart mode does.)
//!
//! While the screen is up, each window's own picture is taken in the background. Choosing a window
//! then gives that window alone, without whatever lay in front of it; a window that gives no picture
//! of itself is cut out of the capture of the screen instead. These pictures are taken one after the
//! other in the first moments the screen is up, so a window whose content moves (a video, a game) is
//! in its picture a few tenths of a second later than in the capture of the screen.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use perora_core::geometry::Rect;
use perora_core::platform::{Desktop, Frame, ScreenCapture, WindowId, desktop, screen_capture};
use perora_core::selection::{Choice, Highlight, Mode, Session};
use serde::Serialize;
use tauri::window::Color;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::{Perora, main_window, preview};

const LABEL_PREFIX: &str = "overlay-";

/// How long a preview window that was just hidden may still be on screen: the compositor draws the
/// next frame without it only after a moment.
const SETTLE: Duration = Duration::from_millis(40);

/// A window is shown once its page has drawn the capture. One that has not said so by now is shown
/// anyway: a screen that never appears is worse than one that flickers.
const SHOW_ANYWAY_AFTER: Duration = Duration::from_millis(600);

/// The pictures of single windows kept while the screen is up, at most: front to back, until this
/// many bytes are used. The rest are cut out of the capture of the screen.
const WINDOW_PICTURES_BUDGET: usize = 512 * 1024 * 1024;

#[derive(Default)]
pub struct State {
    active: Mutex<Option<Active>>,
    last_id: AtomicU64,
}

struct Active {
    id: u64,
    /// Every monitor, as captured.
    still: Arc<Frame>,
    /// Each monitor's part of it, as a file for its window to show.
    stills: Vec<Arc<Vec<u8>>>,
    session: Session,
    /// The pictures of single windows taken so far.
    pictures: Arc<Mutex<HashMap<WindowId, Arc<Frame>>>>,
    /// The monitor the pointer was on at the start: where the mode bar is, and the keyboard.
    home: usize,
    /// The windows already shown.
    shown: HashSet<usize>,
    /// What was last drawn as the highlight, so that only changes are told.
    highlight: Option<Highlight>,
    /// The small pictures of windows for the list of them, as far as they are made: in the
    /// background, each once.
    thumbnails: Arc<Mutex<HashMap<usize, Arc<Vec<u8>>>>>,
    /// Whether the screen is showing its tour: nothing on any monitor is chosen meanwhile, and
    /// nothing is shown as what would be.
    touring: bool,
}

impl State {
    fn lock(&self) -> MutexGuard<'_, Option<Active>> {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn is_active(&self) -> bool {
        self.lock().is_some()
    }
}

/// What one of the windows needs to draw itself.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    language: String,
    texts: HashMap<String, String>,
    selection: Option<SelectionView>,
    /// The tours that were seen, by their names: one that was not is shown as the screen comes up.
    tours_seen: Vec<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionView {
    id: u64,
    /// Which monitor this window covers.
    monitor: usize,
    /// That monitor on the virtual screen, in physical pixels, and its scaling.
    area: Area,
    scale: f64,
    /// Whether this is the window with the mode bar.
    home: bool,
    modes: Vec<&'static str>,
    /// The windows that can be chosen, front to back: for the list of them.
    windows: Vec<WindowView>,
    #[serde(flatten)]
    now: Now,
}

#[derive(Serialize)]
struct WindowView {
    index: usize,
    title: String,
    width: u32,
    height: u32,
    /// Where it is seen to be on the virtual screen (physical pixels): the tour shows one of them.
    area: Area,
}

/// The part that changes while the screen is up; also what `selection-changed` carries.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Now {
    id: u64,
    mode: &'static str,
    highlight: Option<Area>,
    dragging: bool,
    /// Whether the tour is on (on the monitor with the bar): no key does anything on the others.
    touring: bool,
}

#[derive(Serialize, Clone, Copy)]
struct Area {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl From<Rect> for Area {
    fn from(rect: Rect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

impl Active {
    fn now(&self) -> Now {
        Now {
            id: self.id,
            mode: self.session.mode().name(),
            highlight: self
                .highlight
                .filter(|_| !self.touring)
                .map(|highlight| highlight.area.into()),
            dragging: !self.touring && self.highlight.is_some_and(|highlight| highlight.dragging),
            touring: self.touring,
        }
    }
}

fn label(monitor: usize) -> String {
    format!("{LABEL_PREFIX}{monitor}")
}

fn monitor_of(window: &WebviewWindow) -> Option<usize> {
    window.label().strip_prefix(LABEL_PREFIX)?.parse().ok()
}

fn overlay(app: &AppHandle, monitor: usize) -> Option<WebviewWindow> {
    app.get_webview_window(&label(monitor))
}

/// Creates the windows for the monitors there are now, hidden: a web view takes a moment to start,
/// and the screen should be there when the key is pressed.
pub fn create_windows(app: &AppHandle) {
    let monitors = screen_capture()
        .layout()
        .map(|layout| layout.monitors.len())
        .unwrap_or(1);
    for monitor in 0..monitors {
        let _ = ensure_window(app, monitor);
    }
}

fn ensure_window(app: &AppHandle, monitor: usize) -> tauri::Result<WebviewWindow> {
    if let Some(window) = overlay(app, monitor) {
        return Ok(window);
    }
    let window =
        WebviewWindowBuilder::new(app, label(monitor), WebviewUrl::App("index.html".into()))
            .title("Perora")
            .decorations(false)
            .resizable(false)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            // What shows before the capture is drawn: dark, not the white of an empty page.
            .background_color(Color(0, 0, 0, 255))
            .devtools(false)
            .zoom_hotkeys_enabled(false)
            .on_page_load(preview::not_a_browser())
            .build()?;
    // The screen comes and goes at once: a fade would show the desktop moving under a still of itself.
    preview::without_transitions(&window);
    crate::recovery::watch(&window);
    Ok(window)
}

fn hide_windows(app: &AppHandle) {
    for (name, window) in app.webview_windows() {
        if name.starts_with(LABEL_PREFIX) {
            let _ = window.hide();
        }
    }
}

/// Captures every monitor and puts the selection screen over them. Called off the main thread; a
/// screen that is already up is replaced (the shortcut pressed again: capture anew).
pub fn start(app: &AppHandle) -> Result<(), String> {
    let perora = app.state::<Perora>();
    // A selection screen or a preview still on screen would be in the picture.
    let was_up = perora.selection.lock().take().is_some();
    hide_windows(app);
    if preview::hide_for_capture(app) || was_up {
        std::thread::sleep(SETTLE);
    }

    // The screen first: it is the moment of the key press that counts, whatever happens next.
    let capture = screen_capture();
    let captured = capture
        .capture_all()
        .and_then(|still| Ok((still, capture.layout()?)));
    let (still, layout) = match captured {
        Ok((still, layout)) => (Arc::new(still), layout),
        Err(error) => {
            // Nothing came of it: what was being previewed before is still there to be dealt with.
            preview::restore(app);
            main_window::come_back(app);
            return Err(error.to_string());
        }
    };
    let desktop = desktop();
    let windows = desktop.windows().unwrap_or_default();
    let pointer = desktop
        .pointer()
        .unwrap_or_else(|_| perora_core::geometry::Point::new(layout.bounds.x, layout.bounds.y));
    let home = layout
        .monitors
        .iter()
        .position(|monitor| monitor.bounds.contains(pointer))
        .or_else(|| layout.monitors.iter().position(|monitor| monitor.primary))
        .unwrap_or(0);

    // Each monitor's part, all at once.
    let stills: Vec<Arc<Vec<u8>>> = std::thread::scope(|scope| {
        let parts: Vec<_> = layout
            .monitors
            .iter()
            .map(|monitor| {
                let still = &still;
                scope.spawn(move || {
                    Arc::new(
                        still
                            .crop(monitor.bounds)
                            .map(|part| part.to_bmp())
                            .unwrap_or_default(),
                    )
                })
            })
            .collect();
        parts
            .into_iter()
            .map(|part| part.join().unwrap_or_default())
            .collect()
    });

    let start_mode = perora.setting(|settings| settings.start_mode);
    // Every monitor is the one mode with nothing to choose: no screen for it, the capture goes on.
    if start_mode == Mode::AllMonitors {
        main_window::come_back(app);
        let app = app.clone();
        let whole = Frame::clone(&still);
        std::thread::spawn(move || crate::deliver(&app, whole));
        return Ok(());
    }
    let session = Session::new(layout.clone(), windows, start_mode, pointer);
    let id = perora.selection.last_id.fetch_add(1, Ordering::Relaxed) + 1;
    let pictures = Arc::new(Mutex::new(HashMap::new()));
    let thumbnails: Arc<Mutex<HashMap<usize, Arc<Vec<u8>>>>> = Arc::default();
    let windows: Vec<(WindowId, Rect)> = session
        .windows()
        .iter()
        .map(|window| (window.id, window.bounds))
        .collect();

    let highlight = session.highlight();
    *perora.selection.lock() = Some(Active {
        id,
        still: still.clone(),
        stills,
        session,
        pictures: pictures.clone(),
        home,
        shown: HashSet::new(),
        highlight,
        thumbnails: thumbnails.clone(),
        touring: false,
    });
    // Only now: the thread ends as soon as it finds its selection gone.
    take_window_pictures(app, id, still, windows, pictures, thumbnails);

    // Each window over its monitor, still hidden; it is shown when its page says the capture is drawn.
    for (index, monitor) in layout.monitors.iter().enumerate() {
        let Ok(window) = ensure_window(app, index) else {
            continue;
        };
        let _ = window.set_position(PhysicalPosition::new(monitor.bounds.x, monitor.bounds.y));
        let _ = window.set_size(PhysicalSize::new(
            monitor.bounds.width,
            monitor.bounds.height,
        ));
    }
    changed(app);

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHOW_ANYWAY_AFTER);
        let monitors = match &*app.state::<Perora>().selection.lock() {
            Some(active) if active.id == id => active.session.layout().monitors.len(),
            _ => return,
        };
        for monitor in 0..monitors {
            show(&app, id, monitor);
        }
    });
    Ok(())
}

/// Takes the picture of each window, front to back, while the selection screen is up, until the
/// budget is used. Beside it, the small picture of every window is made for the list of them. Both
/// stop when the screen is gone.
fn take_window_pictures(
    app: &AppHandle,
    id: u64,
    still: Arc<Frame>,
    windows: Vec<(WindowId, Rect)>,
    pictures: Arc<Mutex<HashMap<WindowId, Arc<Frame>>>>,
    thumbnails: Arc<Mutex<HashMap<usize, Arc<Vec<u8>>>>>,
) {
    let still_up = {
        let app = app.clone();
        move || matches!(&*app.state::<Perora>().selection.lock(), Some(active) if active.id == id)
    };
    let started = std::time::Instant::now();

    // The small pictures are made here and not when the list asks for one: scaling a large picture
    // takes a moment. On a thread of their own, as the pictures come: those are what a choice needs,
    // and they are not to wait for these. Of every window: of its own picture, or when it gave none,
    // of what is seen of it on the screen.
    let (made, to_make) = std::sync::mpsc::channel::<(usize, Rect, Option<Arc<Frame>>)>();
    std::thread::spawn({
        let still_up = still_up.clone();
        move || {
            for (index, bounds, picture) in to_make {
                if !still_up() {
                    return;
                }
                let small = match picture {
                    Some(picture) => thumbnail(&picture),
                    None => still.crop(bounds).and_then(|seen| thumbnail(&seen)),
                };
                if let Some(thumbnail) = small {
                    thumbnails
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(index, thumbnail);
                }
            }
            trace(format_args!(
                "selection {id}: the list of windows ready after {:.0?}",
                started.elapsed()
            ));
        }
    });

    std::thread::spawn(move || {
        let desktop = desktop();
        let total = windows.len();
        let (mut used, mut taken) = (0usize, 0usize);
        for (index, &(window, bounds)) in windows.iter().enumerate() {
            if !still_up() {
                return;
            }
            // Out of room: the rest are cut out of the capture of the screen.
            let picture = if used > WINDOW_PICTURES_BUDGET {
                None
            } else {
                match desktop.capture_window(window) {
                    Ok(picture) => {
                        used += picture.rgba.len();
                        (used <= WINDOW_PICTURES_BUDGET).then(|| Arc::new(picture))
                    }
                    Err(error) => {
                        trace(format_args!(
                            "selection {id}: no picture of window {window:?}: {error}"
                        ));
                        None
                    }
                }
            };
            if let Some(picture) = &picture {
                pictures
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(window, picture.clone());
                taken += 1;
            }
            // The other thread is gone only when the screen is.
            if made.send((index, bounds, picture)).is_err() {
                return;
            }
        }
        trace(format_args!(
            "selection {id}: pictures of {taken} of {total} windows in {:.0?}",
            started.elapsed()
        ));
    });
}

/// Shows the window of a monitor, once; the one the pointer started on gets the keyboard.
fn show(app: &AppHandle, id: u64, monitor: usize) {
    let perora = app.state::<Perora>();
    // Under the lock to the end: taking the screen down needs the lock first, so its hiding of the
    // windows cannot come before this showing and leave a window up with nothing behind it.
    let mut guard = perora.selection.lock();
    let home = match guard.as_mut() {
        Some(active) if active.id == id => {
            if !active.shown.insert(monitor) {
                return;
            }
            active.home == monitor
        }
        _ => return,
    };
    if let Some(window) = overlay(app, monitor) {
        let _ = window.show();
        if home {
            let _ = window.set_focus();
        }
    }
}

/// Tells the windows what is to be drawn now. They ask for everything when it is about a selection
/// they have not seen yet, or about none.
fn changed(app: &AppHandle) {
    let now = app
        .state::<Perora>()
        .selection
        .lock()
        .as_ref()
        .map(Active::now);
    let _ = app.emit("selection-changed", now);
}

/// The texts changed (another language was chosen).
pub fn language_changed(app: &AppHandle) {
    changed(app);
}

/// Takes the screen down without choosing anything.
pub fn cancel(app: &AppHandle) {
    *app.state::<Perora>().selection.lock() = None;
    hide_windows(app);
    changed(app);
    // What was being previewed when the shortcut was pressed is still to be dealt with.
    preview::restore(app);
    main_window::come_back(app);
}

/// The user decided: the screen comes down and what was chosen goes on to the preview (or straight to
/// the server, when that is what the user set).
fn finish(app: &AppHandle, active: Active, choice: Choice) {
    hide_windows(app);
    main_window::come_back(app);
    changed(app);

    let frame = match choice {
        Choice::Area(area) => active.still.crop(area),
        Choice::Window(index) => active.session.windows().get(index).and_then(|window| {
            let picture = active
                .pictures
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&window.id)
                .cloned();
            trace(format_args!(
                "selection {}: chose window {index} {:?} {:?} at {:?}: {}",
                active.id,
                window.id,
                window.title,
                window.bounds,
                if picture.is_some() {
                    "its own picture"
                } else {
                    "cut out of the screen"
                }
            ));
            match picture {
                // The window alone, as it was; of it, what was on the screen, as highlighted (its own
                // picture also has the part that hung off the edge).
                Some(picture) => picture.crop(active.still.area),
                // No picture of its own (yet, or ever): what was seen of it on the screen.
                None => active.still.crop(window.bounds),
            }
        }),
    };

    let app = app.clone();
    match frame {
        Some(frame) => {
            std::thread::spawn(move || crate::deliver(&app, frame));
        }
        // The choice was all off the screen: the screen just comes down.
        None => preview::restore(&app),
    }
}

/// The longest sides of a window's picture in the list of windows. The list fills the monitor, so
/// with few windows a picture is shown large; beyond this it is drawn enlarged.
const THUMBNAIL_FIT: (u32, u32) = (1600, 1000);

fn thumbnail(picture: &Frame) -> Option<Arc<Vec<u8>>> {
    picture
        .scaled_to_fit(THUMBNAIL_FIT.0, THUMBNAIL_FIT.1)
        .to_png()
        .ok()
        .map(Arc::new)
}

/// The images the selection windows show, of the selection that is up: `/selection/<id>/monitor/<n>.bmp`,
/// a monitor's part of the capture, and `/selection/<id>/window/<n>.png`, a small picture of a window
/// for the list of windows. A small picture that is not made yet is not found: the list asks again.
/// Nothing is made here, where every window of the application waits for the answer.
pub fn image(perora: &Perora, path: &[&str]) -> Option<(&'static str, Arc<Vec<u8>>)> {
    let active = perora.selection.lock();
    let active = active.as_ref()?;
    match path {
        [id, "monitor", file] if id.parse() == Ok(active.id) => {
            let monitor: usize = file.strip_suffix(".bmp")?.parse().ok()?;
            Some(("image/bmp", active.stills.get(monitor)?.clone()))
        }
        [id, "window", file] if id.parse() == Ok(active.id) => {
            let index: usize = file.strip_suffix(".png")?.parse().ok()?;
            let thumbnail = active
                .thumbnails
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&index)
                .cloned()?;
            Some(("image/png", thumbnail))
        }
        _ => None,
    }
}

#[tauri::command]
pub fn selection_state(app: AppHandle, window: WebviewWindow) -> View {
    let perora = app.state::<Perora>();
    let language = perora.language();
    let selection = monitor_of(&window).and_then(|monitor| {
        let active = perora.selection.lock();
        let active = active.as_ref()?;
        let on = active.session.layout().monitors.get(monitor)?;
        Some(SelectionView {
            id: active.id,
            monitor,
            area: on.bounds.into(),
            scale: on.scale,
            home: active.home == monitor,
            modes: Mode::ALL.iter().map(|mode| mode.name()).collect(),
            windows: active
                .session
                .windows()
                .iter()
                .enumerate()
                .map(|(index, window)| WindowView {
                    index,
                    title: window.title.clone(),
                    width: window.bounds.width,
                    height: window.bounds.height,
                    area: window.bounds.into(),
                })
                .collect(),
            now: active.now(),
        })
    });
    View {
        language: language.code().to_owned(),
        texts: language.texts(),
        selection,
        tours_seen: perora.setting(|settings| settings.tours_seen()),
    }
}

/// A window has drawn the capture and can be shown.
#[tauri::command]
pub fn selection_ready(app: AppHandle, window: WebviewWindow, id: u64) {
    if let Some(monitor) = monitor_of(&window) {
        show(&app, id, monitor);
    }
}

/// The pointer did something over one of the windows: `down`, `move`, `up`, or `leave` (a press was
/// broken off). Where it is, is asked of the system.
#[tauri::command]
pub fn selection_pointer(app: AppHandle, id: u64, what: String) {
    let Ok(pointer) = desktop().pointer() else {
        return;
    };
    let perora = app.state::<Perora>();
    let mut guard = perora.selection.lock();
    let Some(active) = guard
        .as_mut()
        .filter(|active| active.id == id && !active.touring)
    else {
        return;
    };

    let choice = match what.as_str() {
        "down" => {
            active.session.pointer_down(pointer);
            None
        }
        "move" => {
            active.session.pointer_moved(pointer);
            None
        }
        "up" => active.session.pointer_up(pointer),
        "leave" => {
            active.session.release();
            None
        }
        _ => return,
    };

    if let Some(choice) = choice {
        let Some(active) = guard.take() else { return };
        drop(guard);
        return finish(&app, active, choice);
    }

    let highlight = active.session.highlight();
    let redraw = highlight != active.highlight;
    active.highlight = highlight;
    drop(guard);
    if redraw {
        changed(&app);
    }
}

/// A window was picked from the list of windows.
#[tauri::command]
pub fn selection_window(app: AppHandle, id: u64, window: usize) {
    let perora = app.state::<Perora>();
    let mut guard = perora.selection.lock();
    let listed = matches!(
        guard.as_ref(),
        Some(active) if active.id == id && window < active.session.windows().len()
    );
    if !listed {
        return;
    }
    let Some(active) = guard.take() else { return };
    drop(guard);
    finish(&app, active, Choice::Window(window));
}

#[tauri::command]
pub fn selection_mode(app: AppHandle, id: u64, mode: String) {
    let Some(mode) = Mode::from_name(&mode) else {
        return;
    };
    let perora = app.state::<Perora>();
    let mut guard = perora.selection.lock();
    // (Not while the tour is on: picking all the monitors would be the choice itself.)
    let Some(active) = guard
        .as_mut()
        .filter(|active| active.id == id && !active.touring)
    else {
        return;
    };

    if let Some(choice) = active.session.set_mode(mode) {
        let Some(active) = guard.take() else { return };
        drop(guard);
        return finish(&app, active, choice);
    }
    // The pointer has not moved, but what it points at means something else now.
    if let Ok(pointer) = desktop().pointer() {
        active.session.pointer_moved(pointer);
    }
    active.highlight = active.session.highlight();
    drop(guard);
    changed(&app);
}

/// The tour of the screen starts, or ends. While it is shown, what the pointer does on any monitor
/// chooses nothing: a press under way is given up, and nothing is shown as what would be chosen.
#[tauri::command]
pub fn selection_touring(app: AppHandle, id: u64, on: bool) {
    let perora = app.state::<Perora>();
    let mut guard = perora.selection.lock();
    let Some(active) = guard.as_mut().filter(|active| active.id == id) else {
        return;
    };
    active.touring = on;
    if on {
        active.session.release();
    } else if let Ok(pointer) = desktop().pointer() {
        // Over, what the pointer is on means what it meant before.
        active.session.pointer_moved(pointer);
    }
    active.highlight = active.session.highlight();
    drop(guard);
    changed(&app);
}

#[tauri::command]
pub fn selection_cancel(app: AppHandle, id: u64) {
    let current =
        matches!(&*app.state::<Perora>().selection.lock(), Some(active) if active.id == id);
    if current {
        cancel(&app);
    }
}

/// A line for whoever is developing this, on standard error; nothing in a release build.
fn trace(message: std::fmt::Arguments<'_>) {
    if cfg!(debug_assertions) {
        eprintln!("{message}");
    }
}
