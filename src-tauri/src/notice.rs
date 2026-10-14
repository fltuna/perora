//! What Perora has to say when no window of its own is the place to say it: that a capture was saved
//! or uploaded, who signed in, what went wrong. Said in a small window of Perora's own, in a corner
//! of the screen the user chooses, rather than through the system's notifications: those queue up,
//! each waiting for the one before it to leave, so that "uploaded" would come long after the upload.
//!
//! A notice is never in a capture, nor over the screen a capture is selected on: it waits until
//! that is over.
//!
//! There is one notice at a time. A new one takes the place of the one on screen at once, and stays
//! for as long as it is to, whatever was left of the time of the one before; a notice leaves on its
//! own when its time is over, or when it is clicked. Like the small preview, the window never
//! takes the keyboard.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{Perora, preview, recovery};

pub const LABEL: &str = "notice";

/// The window, in logical pixels: as wide as the small preview, which it lines up with.
const SIZE: (f64, f64) = (360.0, 76.0);

/// A corner of the primary monitor's work area: where a notice appears, and where the small preview
/// does. In the same corner, the notice keeps clear of the preview while there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    #[default]
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

impl Corner {
    pub const ALL: [Corner; 4] = [
        Corner::BottomRight,
        Corner::BottomLeft,
        Corner::TopRight,
        Corner::TopLeft,
    ];

    /// The name used in settings, language files and between the windows and the application.
    pub const fn name(self) -> &'static str {
        match self {
            Corner::BottomRight => "bottomRight",
            Corner::BottomLeft => "bottomLeft",
            Corner::TopRight => "topRight",
            Corner::TopLeft => "topLeft",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|corner| corner.name() == name)
    }

    pub const fn is_right(self) -> bool {
        matches!(self, Corner::BottomRight | Corner::TopRight)
    }

    pub const fn is_bottom(self) -> bool {
        matches!(self, Corner::BottomRight | Corner::BottomLeft)
    }
}

/// The sound a notice comes with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sound {
    None,
    /// One of the sounds the system comes with, by its name.
    System(String),
    /// A file of the user's own (which one is a setting of its own: it is remembered while
    /// another sound is chosen).
    Custom,
}

/// The system's sound for a notification, on the Windows of today.
const DEFAULT_SOUND: &str = "Windows Notify System Generic";

impl Default for Sound {
    fn default() -> Self {
        Sound::System(DEFAULT_SOUND.to_owned())
    }
}

impl Sound {
    /// As the window is told it: a sound of the system's that this system does not have (or that
    /// is not one to choose from) is no sound, which is also what is heard of it.
    pub fn shown(&self) -> String {
        match self {
            Sound::System(name) if perora_core::platform::system_sound(name).is_none() => {
                Sound::None.name()
            }
            sound => sound.name(),
        }
    }

    /// The name used in settings and between the windows and the application: `none`, `custom`,
    /// or `system:` and the name of one of the system's sounds.
    pub fn name(&self) -> String {
        match self {
            Sound::None => "none".to_owned(),
            Sound::Custom => "custom".to_owned(),
            Sound::System(name) => format!("system:{name}"),
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Sound::None),
            "custom" => Some(Sound::Custom),
            _ => name
                .strip_prefix("system:")
                .filter(|name| !name.is_empty())
                .map(|name| Sound::System(name.to_owned())),
        }
    }
}

/// What a sound is heard at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occasion {
    /// A notice appears.
    Notice,
    /// Something was captured.
    Capture,
}

impl Occasion {
    /// The name used between the windows and the application.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "notice" => Some(Occasion::Notice),
            "capture" => Some(Occasion::Capture),
            _ => None,
        }
    }
}

/// Plays the sound of an occasion, as the settings have it. Nothing, when there is none to play:
/// none was chosen, no file was, or the system has no sound of that name.
pub fn sound(app: &AppHandle, occasion: Occasion) {
    let (sound, file) = app.state::<Perora>().setting(|settings| match occasion {
        Occasion::Notice => (
            settings.notice_sound.clone(),
            settings.notice_sound_file.clone(),
        ),
        Occasion::Capture => (
            settings.capture_sound.clone(),
            settings.capture_sound_file.clone(),
        ),
    });
    let file = match sound {
        Sound::None => None,
        Sound::System(name) => perora_core::platform::system_sound(&name),
        Sound::Custom => file,
    };
    if let Some(file) = file {
        perora_core::platform::play_sound(&file);
    }
}

/// How long a notice that says how something ended is on screen, in seconds, until the user
/// chooses otherwise; and the longest they can choose. (With 0, it stays until it is clicked.)
pub const DEFAULT_SECONDS: u32 = 7;
pub const MOST_SECONDS: u32 = 3600;

/// How long a notice is on screen, which depends on what it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lasting {
    /// Something went well: read at a glance.
    Brief,
    /// Something went wrong: there is a reason to read.
    Long,
    /// Something is under way, and another notice will say how it ended: until that one comes (or,
    /// should it never, until this is plainly stale).
    UntilTheNext,
}

impl Lasting {
    /// What says how something ended stays as long as the user chose (`seconds`), whichever way
    /// it ended; with 0, for good: `None`, and it leaves when it is clicked or another is said.
    fn time(self, seconds: u32) -> Option<Duration> {
        match self {
            Lasting::Brief | Lasting::Long => {
                (seconds > 0).then(|| Duration::from_secs(seconds.into()))
            }
            Lasting::UntilTheNext => Some(Duration::from_secs(90)),
        }
    }
}

/// How often a notice that waits for a capture to be over looks whether it is.
const WAITS: Duration = Duration::from_millis(50);

/// The moment the page is given to draw a notice before the window is shown: shown at once, it
/// would still show the one before for a frame.
const DRAWN_AFTER: Duration = Duration::from_millis(40);

/// The page is loaded twice before it is to be seen (see `preview::not_a_browser`). Elsewhere, once.
const LOADS_BEFORE_SHOWN: u8 = if cfg!(windows) { 2 } else { 1 };

#[derive(Default)]
pub struct State {
    /// How often the page has finished loading.
    loads: AtomicU8,
    said: Mutex<Said>,
}

/// What a click on a notice does, besides sending it away.
#[derive(Debug, Clone)]
pub enum Click {
    /// Shows the file the notice is about in its folder.
    Reveal(std::path::PathBuf),
    /// Opens the main window: what the notice is about is shown there.
    OpenMain,
}

#[derive(Default)]
struct Said {
    /// What is on screen, or about to be.
    message: Option<String>,
    /// What a click on it does, when it does more than send it away.
    click: Option<Click>,
    /// Which notice that is, counted from the first: what is to happen to one notice later (it
    /// leaves) does not happen to the one that took its place.
    turn: u64,
}

impl State {
    fn said(&self) -> std::sync::MutexGuard<'_, Said> {
        self.said
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Serialize)]
pub struct View {
    language: String,
    message: Option<String>,
    /// Whether a click on it does more than send it away: it shows the file it is about, or opens
    /// the main window.
    opens: bool,
    /// Which notice this is, of all that were said: a click says which one it was on.
    turn: u64,
    /// The colour the user chose for their system, as `#rrggbb`.
    accent: Option<String>,
}

/// Creates the window, hidden.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let not_a_browser = preview::not_a_browser();
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Perora")
        .inner_size(SIZE.0, SIZE.1)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // Never takes the keyboard: not when it appears, not when it is clicked.
        .focusable(false)
        .focused(false)
        .visible(false)
        .devtools(false)
        .zoom_hotkeys_enabled(false)
        .on_page_load(move |window, payload| {
            if payload.event() == PageLoadEvent::Finished && payload.url().scheme() != "about" {
                let notice = &window.app_handle().state::<Perora>().notice;
                let loads = notice.loads.load(Ordering::Relaxed).saturating_add(1);
                notice.loads.store(loads, Ordering::Relaxed);
            }
            not_a_browser(window, payload);
        })
        .build()?;
    preview::without_transitions(&window);
    recovery::watch(&window);
    Ok(())
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Says something, in place of whatever is being said. Returns whether it could be: not before the
/// window's page is there, and not once Perora has given up on its windows. Then it is the caller's
/// to say it another way.
///
/// The window is shown from a thread of its own: whoever has something to say is not kept waiting,
/// and need not mind what it holds while saying it.
pub fn say(app: &AppHandle, message: String, lasting: Lasting, click: Option<Click>) -> bool {
    let notice = &app.state::<Perora>().notice;
    if recovery::windows_in_doubt() || notice.loads.load(Ordering::Relaxed) < LOADS_BEFORE_SHOWN {
        return false;
    }
    if window(app).is_none() {
        return false;
    }
    let turn = {
        let mut said = notice.said();
        said.turn += 1;
        said.message = Some(message);
        said.click = click;
        said.turn
    };
    changed(app);

    let app = app.clone();
    std::thread::spawn(move || {
        let perora = app.state::<Perora>();
        let still = |perora: &Perora| perora.notice.said().turn == turn;

        std::thread::sleep(DRAWN_AFTER);
        // Not while the screen is being captured or a capture is being selected: the notice would
        // be in the picture, or in the way. It is said when that is over.
        while still(&perora)
            && (perora.capturing.load(Ordering::Acquire) || perora.selection.is_active())
        {
            std::thread::sleep(WAITS);
        }
        if !still(&perora) || recovery::windows_in_doubt() {
            return;
        }
        if let Some(window) = window(&app) {
            // In the corner the user chose. Where that is the small preview's, above it when a
            // capture is being previewed (shown, or to be shown again): the two are never in each
            // other's way.
            let corner = perora.setting(|settings| settings.notice_corner);
            let clear_of = (corner == perora.setting(|settings| settings.preview_corner))
                .then(|| preview::small_in_its_corner(&app))
                .flatten();
            preview::appear_in_corner(&window, corner, clear_of);
            // Put away while it was being placed: that came before the window was there to hide.
            if !still(&perora) && perora.notice.said().message.is_none() {
                let _ = window.hide();
            }
            // Heard as it appears. Not what only says that something is under way: what it ends
            // in is said a moment later, and is heard.
            if still(&perora) && lasting != Lasting::UntilTheNext {
                sound(&app, Occasion::Notice);
            }
        }

        // (As long as the settings say when it appears: not changed under it afterwards.)
        if let Some(time) = lasting.time(perora.setting(|settings| settings.notice_seconds)) {
            std::thread::sleep(time);
            put_away_if(&app, Some(turn));
        }
    });
    true
}

/// Takes the notice off the screen, whichever it is. Returns whether there was one: a window that
/// was just hidden is on screen for one more frame, which matters to whoever captures the screen
/// next.
pub fn put_away(app: &AppHandle) -> bool {
    put_away_if(app, None)
}

/// Takes the notice off the screen: the one of that turn, or whichever there is. Asked for
/// whichever there is, the window is hidden even with nothing said: it is never left on screen.
fn put_away_if(app: &AppHandle, turn: Option<u64>) -> bool {
    taken_away(app, |said| {
        turn.is_none_or(|turn| turn == said.turn && said.message.is_some())
    })
    .0
}

/// Takes the notice off the screen when it is `the_one`, and says whether there was one, with what a
/// click on it does: taken in the same breath, so that it is never what another notice does.
fn taken_away(app: &AppHandle, the_one: impl FnOnce(&Said) -> bool) -> (bool, Option<Click>) {
    let (was, click) = {
        let perora = app.state::<Perora>();
        let mut said = perora.notice.said();
        if !the_one(&said) {
            return (false, None);
        }
        // Whatever was still to happen to it does not.
        said.turn += 1;
        let click = said.click.take();
        (said.message.take().is_some(), click)
    };
    // (With the state let go of.)
    if let Some(window) = window(app) {
        let _ = window.hide();
    }
    changed(app);
    (was, click)
}

/// Tells the window to ask for the state again: what is said, or the language, changed.
pub fn changed(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "notice-changed", ());
}

#[tauri::command]
pub fn notice_state(app: AppHandle) -> View {
    let perora = app.state::<Perora>();
    let (message, opens, turn) = {
        let said = perora.notice.said();
        (said.message.clone(), said.click.is_some(), said.turn)
    };
    View {
        language: perora.language().code().to_owned(),
        message,
        opens,
        turn,
        accent: perora_core::platform::accent_colour()
            .map(|(red, green, blue)| format!("#{red:02x}{green:02x}{blue:02x}")),
    }
}

/// The notice was clicked: read, and in the way. One that is about a file shows that file in its
/// folder as it goes; one about something the main window shows opens that window. The click says which notice the window showed (`turn`), with something
/// said or not: when another has taken its place meanwhile, that one stays, unread, and nothing
/// is shown. A window that has not yet heard what there is says no turn, and whatever there is
/// goes.
#[tauri::command(async)]
pub fn notice_dismiss(app: AppHandle, turn: Option<u64>) {
    let (_, click) = taken_away(&app, |said| turn.is_none_or(|turn| turn == said.turn));
    match click {
        Some(Click::Reveal(file)) => {
            if let Err(error) = tauri_plugin_opener::reveal_item_in_dir(&file) {
                eprintln!("the folder of what was saved could not be shown: {error}");
            }
        }
        Some(Click::OpenMain) => crate::main_window::open(&app),
        None => {}
    }
}
