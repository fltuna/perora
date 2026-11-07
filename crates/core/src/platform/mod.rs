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

/// Why an image file could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// The system cannot read it (not an image, a kind it has nothing to read with, damaged, or
    /// gone), with what the system said, when it said something.
    Unreadable(String),
    /// More pixels than can be drawn on and leave.
    TooLarge { width: u32, height: u32 },
}

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
    fn windows(&self) -> Result<Vec<Window>> {
        self.windows_with(&[])
    }

    /// The same, with those of this application's own windows that are named (`own`) among them,
    /// in their places: for when the application's windows are in the picture too.
    fn windows_with(&self, own: &[WindowId]) -> Result<Vec<Window>>;

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

/// Calls `back`, from a thread of its own, every time the shell (the taskbar and its notification
/// area) is there anew: started again, or there at last when it was not.
#[cfg(windows)]
pub fn watch_shell(back: impl Fn() + Send + 'static) {
    windows::notification_area::watch_shell(back);
}

/// Whether the notification area has an icon of this window of this process, shown or hidden
/// among those it hides.
#[cfg(windows)]
pub fn icon_there(window: isize) -> bool {
    windows::notification_area::icon_there(window)
}

/// Asks the window of an icon (tray-icon's) to put its icon in the notification area again.
#[cfg(windows)]
pub fn put_icon_back(window: isize) {
    windows::notification_area::put_icon_back(window);
}

/// The names of the font families the system has, in order, each once.
#[cfg(windows)]
pub fn font_families() -> Vec<String> {
    windows::fonts::families()
}

/// Lines laid out as the shapes of their letters (see `windows::letters`): in the first of
/// `families` the system has, at `size` pixels, curves within `flat` pixels.
#[cfg(windows)]
pub fn lay_out(
    lines: &[&str],
    families: &[&str],
    size: f32,
    bold: bool,
    italic: bool,
    flat: f32,
) -> Option<windows::letters::Laid> {
    windows::letters::lay_out(lines, families, size, bold, italic, flat)
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

/// Reads the image in a file, as it is to be seen: its first picture, turned as the camera said.
/// Refused when the system cannot read it, or when it has more than `most` pixels or more than
/// `longest` a side.
#[cfg(windows)]
pub fn read_image(
    path: &std::path::Path,
    most: u64,
    longest: u32,
) -> std::result::Result<Frame, ImageError> {
    windows::imaging::read(path, most, longest)
}

/// Lets the user choose an image file to open, over `owner` (which cannot be used meanwhile); `images`
/// and `all` name the lists of files offered (the kinds of image first). Waits for the answer: not to be
/// called where the windows are drawn. `None` when no file was chosen.
#[cfg(windows)]
pub fn choose_image(
    owner: Option<WindowId>,
    images: &str,
    all: &str,
) -> std::result::Result<Option<std::path::PathBuf>, String> {
    windows::choosing::choose_image(owner.map(|owner| owner.0), &IMAGE_KINDS, images, all)
}

/// What Perora is started with to open a file in its editor: the file follows.
#[cfg(windows)]
pub const OPEN_ARGUMENT: &str = windows::explorer::OPEN;

/// The kinds of image file (as `.png`) that are surely read: what Perora is offered for among the
/// programs to open a file with, and what a choice of file shows first.
#[cfg(windows)]
pub const IMAGE_KINDS: [&str; 16] = windows::explorer::KINDS;

/// What the file manager runs to open an image with Perora ("Open with Perora" on it); `None` when
/// it offers no such thing.
#[cfg(windows)]
pub fn opened_from_explorer() -> Option<String> {
    windows::explorer::registered(windows::explorer::CLASSES)
}

/// What the file manager is to run for that, with this program.
#[cfg(windows)]
pub fn explorer_command(program: &str) -> String {
    windows::explorer::command(program)
}

/// Whether the file manager's entry says that.
#[cfg(windows)]
pub fn explorer_labelled(label: &str) -> bool {
    windows::explorer::labelled(windows::explorer::CLASSES, label)
}

/// Has the file manager offer "Open with Perora" (`label`) on images, opening them with `program`;
/// or offer it no more (`None`). Refused with the reason when the system would not have it.
#[cfg(windows)]
pub fn open_from_explorer(program: &str, label: Option<&str>) -> std::result::Result<(), String> {
    match label {
        Some(label) => windows::explorer::register(windows::explorer::CLASSES, program, label),
        None => windows::explorer::unregister(windows::explorer::CLASSES, program),
    }
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

/// How large the picture on the clipboard says it is (across, down), from its header alone: before
/// anything is made of it. `None` when there is none, or its header cannot be read.
#[cfg(windows)]
pub fn clipboard_picture_size() -> Option<(u32, u32)> {
    windows::clipboard::picture_size()
}

/// The letter key (A to Z) held down now, by its place on the keyboard (as the US layout names it),
/// when one is: what a key with Ctrl means, whatever letter the user's layout has there.
#[cfg(windows)]
pub fn letter_held() -> Option<char> {
    windows::desktop::letter_held()
}

/// The key a shortcut could end in that is held as the key press being handled was made, by the
/// name a shortcut is written with (`H`, `F5`, `ArrowUp`, `Numpad1`, `Backquote`); `None` when none
/// is. By where the key is, not by what it writes (with Shift, `1` writes `!`).
#[cfg(windows)]
pub fn held_key() -> Option<&'static str> {
    windows::keys::held_key()
}

/// One of the keys a shortcut can end in that a window is never told were pressed (PrintScreen,
/// Pause, ScrollLock, NumLock) that is down at this moment, with which of Ctrl, Alt, Shift and the
/// Windows key are down with it: for asking again and again while a shortcut is recorded.
#[cfg(windows)]
pub fn unheard_key() -> Option<(&'static str, [bool; 4])> {
    windows::keys::unheard_key()
}

/// Which of Ctrl, Alt, Shift and the Windows key are held as the key press being handled was made,
/// in that order.
#[cfg(windows)]
pub fn held_modifiers() -> [bool; 4] {
    windows::keys::held_modifiers()
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

/// Opens the folder a file is in, with the file chosen there (as the file manager's "show in
/// folder"). Refused with the reason when the system would not.
#[cfg(windows)]
pub fn reveal_in_folder(path: &std::path::Path) -> std::result::Result<(), String> {
    windows::explorer::reveal(path)
}

/// Puts a picture on the clipboard, in place of whatever is there: as the PNG it was made into
/// (`png`), and as a bitmap. Refused with the reason when the clipboard would not take it.
#[cfg(windows)]
pub fn put_picture_on_clipboard(frame: &Frame, png: &[u8]) -> std::result::Result<(), String> {
    windows::clipboard::put_picture(frame, png)
}

/// Empties the clipboard when it still holds what was put there at that count of its changes
/// (`clipboard_sequence`). Whether it was emptied; refused with the reason when it could not be had.
#[cfg(windows)]
pub fn clear_clipboard_if(written: u32) -> std::result::Result<bool, String> {
    windows::clipboard::clear_if(written)
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
