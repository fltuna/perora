//! What Perora needs from the operating system, behind traits, so that each system gets its own
//! implementation and everything else stays unaware of which one it runs on. Windows is the only
//! implementation so far; the traits take their shape from it and will be revisited with the second one.

use std::path::PathBuf;

use crate::geometry::{Point, Rect};

#[cfg(windows)]
pub mod windows;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// The operating system refused or failed a call.
    #[error("{operation} failed: {detail}")]
    Os {
        operation: &'static str,
        detail: String,
    },

    #[error("there is no monitor to capture")]
    NoMonitors,

    #[error("the area to capture is empty")]
    EmptyArea,
}

pub type Result<T> = std::result::Result<T, PlatformError>;

/// A monitor as the operating system arranges it.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// The system's name for it (on Windows: `\\.\DISPLAY1`). Stable while the arrangement is.
    pub name: String,
    /// Where it sits on the virtual screen, in physical pixels.
    pub bounds: Rect,
    /// The part of it that windows are meant to use: without the taskbar and other docked bars.
    pub work_area: Rect,
    /// The display scaling the user chose for it: 1.0 for 100 %, 1.5 for 150 %.
    pub scale: f64,
    pub primary: bool,
}

/// Every monitor, and the virtual screen they make up together.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenLayout {
    pub monitors: Vec<Monitor>,
    /// The smallest rectangle that holds every monitor. Parts of it may belong to no monitor (monitors
    /// of different heights, or staggered).
    pub bounds: Rect,
}

impl ScreenLayout {
    pub fn new(monitors: Vec<Monitor>) -> Result<Self> {
        let bounds = Rect::bounding(monitors.iter().map(|monitor| monitor.bounds))
            .ok_or(PlatformError::NoMonitors)?;
        Ok(Self { monitors, bounds })
    }

    pub fn primary(&self) -> Option<&Monitor> {
        self.monitors.iter().find(|monitor| monitor.primary)
    }

    /// The monitor showing this point of the virtual screen.
    pub fn monitor_at(&self, point: Point) -> Option<&Monitor> {
        self.monitors
            .iter()
            .find(|monitor| monitor.bounds.contains(point))
    }
}

/// Pixels taken from the screen: 8-bit RGBA rows, top to bottom, without padding.
#[derive(Clone, PartialEq)]
pub struct Frame {
    /// The area of the virtual screen the pixels show.
    pub area: Rect,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("area", &self.area)
            .field("bytes", &self.rgba.len())
            .finish()
    }
}

/// A window on the desktop, as the system knows it. The number means nothing outside the run of the
/// system it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub isize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub id: WindowId,
    pub title: String,
    /// Where it is seen to be on the virtual screen, in physical pixels. It may reach beyond the screen.
    pub bounds: Rect,
}

/// What is on the desktop besides pixels.
pub trait Desktop {
    /// The windows a user would call windows, front to back: shown, not minimized, not this
    /// application's own, not the desktop itself.
    fn windows(&self) -> Result<Vec<Window>>;

    /// One window's own picture, whatever lies in front of it. Fails for a window that does not let
    /// itself be drawn that way; cutting it out of a capture of the screen is the fallback then.
    fn capture_window(&self, id: WindowId) -> Result<Frame>;

    /// Where the pointer is on the virtual screen, in physical pixels.
    fn pointer(&self) -> Result<Point>;
}

pub trait ScreenCapture {
    fn layout(&self) -> Result<ScreenLayout>;

    /// Every monitor in one frame, laid out as on the virtual screen. What belongs to no monitor is
    /// transparent.
    fn capture_all(&self) -> Result<Frame>;
}

/// Where the sign-in is kept between runs: one refresh token, readable by this user of this machine
/// only. Each save replaces what was there.
pub trait TokenStore: Send + Sync {
    /// `None` when nothing is stored, or when what is stored can no longer be read.
    fn load(&self) -> Result<Option<String>>;

    fn save(&self, refresh_token: &str) -> Result<()>;

    fn clear(&self) -> Result<()>;
}

/// The screen capture of the system this was built for.
#[cfg(windows)]
pub fn screen_capture() -> impl ScreenCapture {
    windows::GdiScreenCapture::new()
}

/// The desktop of the system this was built for.
#[cfg(windows)]
pub fn desktop() -> impl Desktop {
    windows::desktop::Win32Desktop::new()
}

/// Makes a window of this application appear and disappear at once, without the system's animation.
#[cfg(windows)]
pub fn without_transitions(window: WindowId) {
    windows::desktop::without_transitions(window);
}

/// The date and time on the user's clock, as `2026-10-12 023455`: for naming a file after the moment
/// it was made, in a form every file system takes and that sorts by time.
#[cfg(windows)]
pub fn local_timestamp() -> String {
    windows::local_timestamp()
}

/// The colour the user chose to accent their system with, as red, green and blue; `None` when the
/// system does not say.
#[cfg(windows)]
pub fn accent_colour() -> Option<(u8, u8, u8)> {
    windows::desktop::accent_colour()
}

/// How large, in pixels, the system draws a small icon, as in the notification area.
#[cfg(windows)]
pub fn small_icon_side() -> u32 {
    windows::desktop::small_icon_side()
}

/// Turns the system's animation of a window's appearing and disappearing on or off: off for the
/// moment a window has to be gone at once, on again afterwards.
#[cfg(windows)]
pub fn set_transitions(window: WindowId, enabled: bool) {
    windows::desktop::set_transitions(window, enabled);
}

/// Keeps a window of this process out of what the user switches between (Alt+Tab, the task view):
/// for a window that is a part of the screen, not a task.
#[cfg(windows)]
pub fn not_among_tasks(window: WindowId) {
    windows::desktop::not_among_tasks(window);
}

/// The names of the font families the system has, in order, each once.
#[cfg(windows)]
pub fn font_families() -> Vec<String> {
    windows::fonts::families()
}

/// The names of the system's sounds a notice can come with, in order.
#[cfg(windows)]
pub fn system_sounds() -> Vec<String> {
    windows::sound::system_sounds()
}

/// The endings of the sound files that are played.
#[cfg(windows)]
pub const SOUND_FILES: [&str; 5] = windows::sound::KINDS;

/// The file of one of the system's sounds a notice can come with; `None` for any other name.
#[cfg(windows)]
pub fn system_sound(name: &str) -> Option<PathBuf> {
    windows::sound::system_sound(name)
}

/// Plays a sound file from its start, in place of whatever is playing; returns at once. A file that
/// cannot be played is silence.
#[cfg(windows)]
pub fn play_sound(path: &std::path::Path) {
    windows::sound::play(path);
}

/// The command the system starts under that name when the user signs in; `None` when there is
/// none, or the user switched it off in the system's own settings.
#[cfg(windows)]
pub fn started_at_sign_in(name: &str) -> Option<String> {
    windows::startup::command(name)
}

/// Has the system start that command under that name when the user signs in, or nothing under it
/// any more (`None`). Refused with the reason when the system would not have it.
#[cfg(windows)]
pub fn start_at_sign_in(name: &str, command: Option<&str>) -> std::result::Result<(), String> {
    windows::startup::set_command(name, command)
}

/// Tells the system's shell to leave the key combinations of the Windows key and that letter alone
/// (they are free for another program then), or to take them again. It does so from the next time
/// it starts. Refused with the reason when the system would not have it.
#[cfg(windows)]
pub fn leave_hotkeys_alone(letter: char, alone: bool) -> std::result::Result<(), String> {
    windows::hotkeys::leave_alone(letter, alone)
}

/// Whether the shell is told to leave them alone.
#[cfg(windows)]
pub fn hotkeys_left_alone(letter: char) -> bool {
    windows::hotkeys::is_left_alone(letter)
}

/// Starts the system's shell anew, which makes it read its settings: the taskbar is gone for a
/// moment, and the folders that were open are closed.
#[cfg(windows)]
pub fn restart_shell() -> std::result::Result<(), String> {
    windows::hotkeys::restart_shell()
}

/// Whether the user has the system animate what is in its windows; without that, nothing of
/// Perora's moves into its place either.
#[cfg(windows)]
pub fn animations_enabled() -> bool {
    windows::desktop::animations_enabled()
}

/// Takes a window of this process out of sight at once, or puts it back, without hiding it: it goes
/// on with what it is doing (a hidden window is slowed down by the system), unseen.
#[cfg(windows)]
pub fn set_cloaked(window: WindowId, cloaked: bool) {
    windows::desktop::set_cloaked(window, cloaked);
}

/// How much of a window of this application is seen, from 0 to 255; `None` for all of it, as a
/// window like any other.
#[cfg(windows)]
pub fn set_opacity(window: WindowId, opacity: Option<u8>) {
    windows::desktop::set_opacity(window, opacity);
}

/// Whether the user has the system animate what is in its windows.
#[cfg(windows)]
pub fn animates() -> bool {
    windows::desktop::animations_enabled()
}

/// Puts a file on the clipboard, as copying it in the file manager does, in place of whatever is
/// there. Refused with the reason when the clipboard would not take it.
#[cfg(windows)]
pub fn put_file_on_clipboard(path: &std::path::Path) -> std::result::Result<(), String> {
    windows::clipboard::put_file(path)
}

/// A number that changes whenever what is on the clipboard does: for telling whether what was put
/// there is still what is there. `None` when the system does not say.
#[cfg(windows)]
pub fn clipboard_sequence() -> Option<u32> {
    windows::desktop::clipboard_sequence()
}

/// The token store of the system this was built for, keeping its file at `path`.
#[cfg(windows)]
pub fn token_store(path: PathBuf) -> SystemTokenStore {
    windows::secrets::ProtectedFile::new(path)
}

#[cfg(windows)]
pub type SystemTokenStore = windows::secrets::ProtectedFile;

/// The language the user reads their system in, as a tag like `ja-JP`; `None` when the system does not
/// say.
#[cfg(windows)]
pub fn ui_language() -> Option<String> {
    windows::ui_language()
}

/// The locale the user has dates and numbers formatted for, as a tag like `ja-JP`. It can differ from
/// [`ui_language`]: people run their system in one language and live in another.
#[cfg(windows)]
pub fn regional_locale() -> Option<String> {
    windows::regional_locale()
}
