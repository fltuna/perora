//! What the user chose and Perora remembers between runs: a small JSON file in the application's
//! configuration directory. Nothing in it is secret.
//!
//! The file is read one setting at a time: a value that is missing or of the wrong kind leaves that
//! setting as it would be without the file, and nothing else. What the file holds that this version
//! does not know (the settings of a newer one) is written back as it was read. A file that is not
//! readable as settings at all is put aside before a new one takes its place.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use perora_core::output::{FITS, Format, Output, Quality};
use perora_core::selection::Mode;
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};

use crate::Perora;
use crate::notice::{Corner, DEFAULT_SECONDS, MOST_SECONDS, Sound};

/// What becomes of a capture once it is taken, before the user says anything: the small preview in
/// a corner, the large window where it is drawn on, or the clipboard and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum After {
    #[default]
    Preview,
    Edit,
    Copy,
}

impl After {
    pub const ALL: [After; 3] = [After::Preview, After::Edit, After::Copy];

    /// The name used in settings, language files and between the windows and the application.
    pub const fn name(self) -> &'static str {
        match self {
            After::Preview => "preview",
            After::Edit => "edit",
            After::Copy => "copy",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|after| after.name() == name)
    }
}

/// A tour: what one of the windows shows of itself, a step at a time, the first time it is seen
/// (and again whenever it is asked for).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tour {
    /// Of the selection screen.
    Capture,
    /// Of the small preview in the corner.
    Preview,
    /// Of the large window, where a capture is drawn on.
    Editor,
}

impl Tour {
    pub const ALL: [Tour; 3] = [Tour::Capture, Tour::Preview, Tour::Editor];

    /// The name used in settings and between the windows and the application.
    pub const fn name(self) -> &'static str {
        match self {
            Tour::Capture => "capture",
            Tour::Preview => "preview",
            Tour::Editor => "editor",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tour| tour.name() == name)
    }
}

/// Which of their two looks the windows have: the one the system is set to, or one of them whatever
/// the system is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Look {
    #[default]
    System,
    Light,
    Dark,
}

impl Look {
    pub const ALL: [Look; 3] = [Look::System, Look::Light, Look::Dark];

    /// The name used in settings, language files and between the windows and the application.
    pub const fn name(self) -> &'static str {
        match self {
            Look::System => "system",
            Look::Light => "light",
            Look::Dark => "dark",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|look| look.name() == name)
    }

    /// As the windows are told it: one of the two, or none for the system's.
    pub const fn theme(self) -> Option<tauri::Theme> {
        match self {
            Look::System => None,
            Look::Light => Some(tauri::Theme::Light),
            Look::Dark => Some(tauri::Theme::Dark),
        }
    }
}

/// A way a capture leaves: each has the kind of file it leaves as, and how large at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaves {
    /// To the clipboard.
    Copy,
    /// To the server.
    Upload,
    /// To a file.
    Save,
}

impl Leaves {
    pub const ALL: [Leaves; 3] = [Leaves::Copy, Leaves::Upload, Leaves::Save];

    /// The name used in settings and between the windows and the application.
    pub const fn name(self) -> &'static str {
        match self {
            Leaves::Copy => "copy",
            Leaves::Upload => "upload",
            Leaves::Save => "save",
        }
    }
}

/// Kinds of file, each once, in the order they are always named in.
pub fn several(kinds: impl Iterator<Item = Format>) -> Vec<Format> {
    let named: Vec<Format> = kinds.collect();
    Format::ALL
        .into_iter()
        .filter(|kind| named.contains(kind))
        .collect()
}

/// The shortcut Perora asks for until the user chooses another.
pub const DEFAULT_SHORTCUT: &str = "Ctrl+Shift+H";

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The code of the language chosen; `None` when none was, and the system decides.
    pub language: Option<String>,
    /// Whether the windows are light or dark.
    pub look: Look,
    /// Whether Perora uploads at all: it does only when the user chose so (and is signed in).
    pub upload_enabled: bool,
    /// Whether what was selected is uploaded at once, without the preview. Counts only while
    /// Perora uploads.
    pub upload_immediately: bool,
    /// The mode the selection screen starts in.
    pub start_mode: Mode,
    /// Whether Perora's own windows (the main window, the previews) are out of the picture: hidden
    /// before the screen is captured. Otherwise they are captured too, and hidden only once they are.
    pub hide_own_windows: bool,
    /// Whether what was selected is put on the clipboard as the preview appears.
    pub copy_to_clipboard: bool,
    /// Whether discarding a capture also takes it off the clipboard.
    pub clear_clipboard_on_discard: bool,
    /// Where "save" writes; `None` for a folder of Perora's own in the user's pictures.
    pub save_directory: Option<PathBuf>,
    /// Whether "save" asks where to, each time.
    pub ask_where_to_save: bool,
    /// The global shortcut that captures, as `Ctrl+Shift+H`.
    pub shortcut: String,
    /// Whether the system's own combination for snipping (Win+Shift+S) captures with Perora too:
    /// taken from the system's shell, which has to let go of it for that.
    pub take_snipping: bool,
    /// What becomes of a capture once it is taken.
    pub after_capture: After,
    /// How long the small preview stays before the capture is given up, in seconds; 0 for until
    /// something is done with it.
    pub preview_seconds: u32,
    /// The corner of the screen the small preview appears in.
    pub preview_corner: Corner,
    /// The sound a capture comes with, and the file of the user's own that is that sound when it
    /// is one of their own.
    pub capture_sound: Sound,
    pub capture_sound_file: Option<PathBuf>,
    /// How long a notice that says how something ended stays, in seconds; 0 for until it is
    /// clicked.
    pub notice_seconds: u32,
    /// The corner of the screen Perora's notices appear in.
    pub notice_corner: Corner,
    /// The sound a notice comes with.
    pub notice_sound: Sound,
    /// The file of the user's own that is that sound, when it is one of their own; kept while
    /// another sound is chosen.
    pub notice_sound_file: Option<PathBuf>,
    /// The kind of file a capture leaves as, and the square it is made to fit when it does not
    /// (`None` for as large as it is): for each way of leaving, in the order of `Leaves::ALL`.
    pub formats: [Format; 3],
    pub fits: [Option<u32>; 3],
    /// The kinds of file a capture is saved as: one file of each, and at least one (the first is
    /// the one `formats` has for saving).
    pub save_formats: Vec<Format>,
    /// How good a file that loses something is, whichever way it leaves.
    pub quality: Quality,
    /// The tours that were seen to the end, or put away: each in order, once.
    pub tours_seen: Vec<Tour>,
    /// The newer version the user chose to skip: its band is not shown (a newer one's is).
    pub skipped_version: Option<String>,
    /// Whatever else the file holds.
    rest: Map<String, Value>,
}

impl Settings {
    /// Has the tour seen: it is not shown again unless it is asked for.
    pub fn saw(&mut self, tour: Tour) {
        if !self.tours_seen.contains(&tour) {
            self.tours_seen.push(tour);
            self.tours_seen = Tour::ALL
                .into_iter()
                .filter(|one| self.tours_seen.contains(one))
                .collect();
        }
    }

    /// The names of the tours seen, for the windows.
    pub fn tours_seen(&self) -> Vec<&'static str> {
        self.tours_seen.iter().map(|tour| tour.name()).collect()
    }

    /// Has a capture saved as these kinds of file, one of each; as it was, when none is named.
    pub fn save_as(&mut self, kinds: &[Format]) {
        let kinds = several(kinds.iter().copied());
        if let Some(first) = kinds.first() {
            self.formats[Leaves::Save as usize] = *first;
            self.save_formats = kinds;
        }
    }

    /// What a capture is made into as it leaves that way.
    pub fn output(&self, leaves: Leaves) -> Output {
        let which = leaves as usize;
        Output {
            format: self.formats[which],
            quality: self.quality,
            fit: self.fits[which],
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_file(Map::new())
    }
}

impl Settings {
    fn from_file(mut file: Map<String, Value>) -> Self {
        let mut flag = |key: &str, otherwise: bool| match file.remove(key) {
            Some(Value::Bool(value)) => value,
            _ => otherwise,
        };
        let upload_immediately = flag("uploadImmediately", false);
        let upload_enabled = flag("enableUpload", false);
        let copy_to_clipboard = flag("copyToClipboard", true);
        let clear_clipboard_on_discard = flag("clearClipboardOnDiscard", false);
        let ask_where_to_save = flag("askWhereToSave", false);
        // (Notices could once be switched off. What a file says of that is let go of: not kept
        // for a version that does not know it, as other things it does not know are.)
        flag("noticeEnabled", true);
        // (Once a switch, for a preview or none: off, it was what is now "to the clipboard only".)
        let preview_enabled = flag("previewEnabled", true);
        let take_snipping = flag("takeSnipping", false);
        let hide_own_windows = flag("hideOwnWindows", true);

        let mut text = |key: &str| match file.remove(key) {
            Some(Value::String(value)) if !value.is_empty() => Some(value),
            _ => None,
        };
        let language = text("language");
        let skipped_version = text("skippedVersion");
        let after_capture = text("afterCapture")
            .and_then(|name| After::from_name(&name))
            .unwrap_or(match preview_enabled {
                true => After::Preview,
                false => After::Copy,
            });
        let look = text("theme")
            .and_then(|name| Look::from_name(&name))
            .unwrap_or_default();
        let start_mode = text("startMode")
            .and_then(|name| Mode::from_name(&name))
            .unwrap_or(Mode::Smart);
        let save_directory = text("saveDirectory").map(PathBuf::from);
        let shortcut = text("shortcut").unwrap_or_else(|| DEFAULT_SHORTCUT.into());
        let notice_corner = text("noticeCorner")
            .and_then(|name| Corner::from_name(&name))
            .unwrap_or_default();
        let notice_sound = text("noticeSound")
            .and_then(|name| Sound::from_name(&name))
            .unwrap_or_default();
        let notice_sound_file = text("noticeSoundFile").map(PathBuf::from);
        let preview_corner = text("previewCorner")
            .and_then(|name| Corner::from_name(&name))
            .unwrap_or_default();
        let capture_sound = text("captureSound")
            .and_then(|name| Sound::from_name(&name))
            .unwrap_or_default();
        let capture_sound_file = text("captureSoundFile").map(PathBuf::from);
        let formats = Leaves::ALL.map(|leaves| {
            text(&format!("{}Format", leaves.name()))
                .and_then(|name| Format::from_name(&name))
                .unwrap_or_default()
        });
        let quality = text("quality")
            .and_then(|name| Quality::from_name(&name))
            .unwrap_or_default();
        // (Saved as several kinds at once, when the file names more than one; else as the one.)
        let mut formats = formats;
        let save_formats = match file.remove("saveFormats") {
            Some(Value::Array(names)) => several(
                names
                    .iter()
                    .filter_map(|name| name.as_str().and_then(Format::from_name)),
            ),
            _ => Vec::new(),
        };
        let mut tours_seen = Vec::new();
        if let Some(Value::Array(names)) = file.remove("toursSeen") {
            for name in names.iter().filter_map(Value::as_str) {
                if let Some(tour) = Tour::from_name(name) {
                    tours_seen.push(tour);
                }
            }
        }
        let tours_seen = Tour::ALL
            .into_iter()
            .filter(|tour| tours_seen.contains(tour))
            .collect();
        let save_formats = match save_formats.first() {
            Some(first) => {
                formats[Leaves::Save as usize] = *first;
                save_formats
            }
            None => vec![formats[Leaves::Save as usize]],
        };

        // (A whole number of seconds, no more than can be chosen.)
        let mut seconds = |key: &str| match file.remove(key) {
            Some(Value::Number(seconds)) => seconds
                .as_u64()
                .and_then(|seconds| u32::try_from(seconds).ok())
                .filter(|seconds| *seconds <= MOST_SECONDS),
            _ => None,
        };
        let notice_seconds = seconds("noticeSeconds").unwrap_or(DEFAULT_SECONDS);
        let preview_seconds = seconds("previewSeconds").unwrap_or(0);
        // (One of the squares there are to choose from; anything else is no square.)
        let fits = Leaves::ALL.map(
            |leaves| match file.remove(&format!("{}Fit", leaves.name())) {
                Some(Value::Number(side)) => side
                    .as_u64()
                    .and_then(|side| u32::try_from(side).ok())
                    .filter(|side| FITS.contains(side)),
                _ => None,
            },
        );

        Self {
            language,
            look,
            upload_immediately,
            upload_enabled,
            start_mode,
            hide_own_windows,
            copy_to_clipboard,
            clear_clipboard_on_discard,
            save_directory,
            ask_where_to_save,
            shortcut,
            take_snipping,
            notice_seconds,
            notice_corner,
            notice_sound,
            notice_sound_file,
            after_capture,
            preview_seconds,
            preview_corner,
            capture_sound,
            capture_sound_file,
            formats,
            fits,
            save_formats,
            quality,
            tours_seen,
            skipped_version,
            rest: file,
        }
    }

    fn to_file(&self) -> Map<String, Value> {
        let mut file = self.rest.clone();
        if let Some(language) = &self.language {
            file.insert("language".into(), language.clone().into());
        }
        file.insert("theme".into(), self.look.name().into());
        file.insert("uploadImmediately".into(), self.upload_immediately.into());
        file.insert("enableUpload".into(), self.upload_enabled.into());
        file.insert("startMode".into(), self.start_mode.name().into());
        file.insert("hideOwnWindows".into(), self.hide_own_windows.into());
        file.insert("copyToClipboard".into(), self.copy_to_clipboard.into());
        file.insert(
            "clearClipboardOnDiscard".into(),
            self.clear_clipboard_on_discard.into(),
        );
        if let Some(directory) = &self.save_directory {
            file.insert(
                "saveDirectory".into(),
                directory.to_string_lossy().into_owned().into(),
            );
        }
        file.insert("askWhereToSave".into(), self.ask_where_to_save.into());
        file.insert("shortcut".into(), self.shortcut.clone().into());
        for leaves in Leaves::ALL {
            let output = self.output(leaves);
            file.insert(
                format!("{}Format", leaves.name()),
                output.format.name().into(),
            );
            file.insert(
                format!("{}Fit", leaves.name()),
                output.fit.unwrap_or(0).into(),
            );
        }
        file.insert(
            "saveFormats".into(),
            self.save_formats
                .iter()
                .map(|kind| Value::from(kind.name()))
                .collect::<Vec<_>>()
                .into(),
        );
        file.insert("quality".into(), self.quality.name().into());
        file.insert("toursSeen".into(), self.tours_seen().into());
        if let Some(version) = &self.skipped_version {
            file.insert("skippedVersion".into(), version.clone().into());
        }
        file.insert("takeSnipping".into(), self.take_snipping.into());
        file.insert("noticeSeconds".into(), self.notice_seconds.into());
        file.insert("noticeCorner".into(), self.notice_corner.name().into());
        file.insert("noticeSound".into(), self.notice_sound.name().into());
        file.insert("afterCapture".into(), self.after_capture.name().into());
        file.insert("previewSeconds".into(), self.preview_seconds.into());
        file.insert("previewCorner".into(), self.preview_corner.name().into());
        file.insert("captureSound".into(), self.capture_sound.name().into());
        if let Some(sound) = &self.capture_sound_file {
            file.insert(
                "captureSoundFile".into(),
                sound.to_string_lossy().into_owned().into(),
            );
        }
        if let Some(sound) = &self.notice_sound_file {
            file.insert(
                "noticeSoundFile".into(),
                sound.to_string_lossy().into_owned().into(),
            );
        }
        file
    }
}

/// A tour was seen to the end, or put away: it is not shown again on its own.
#[tauri::command]
pub fn tour_seen(app: AppHandle, tour: String) {
    if let Some(tour) = Tour::from_name(&tour) {
        update(&app, |settings| settings.saw(tour));
    }
}

/// The application's identifier (`tauri.conf.json`): the name of the folder its settings are in.
const IDENTIFIER: &str = "dev.f2a.perora";

/// The settings as the file has them, found without the application (which is not started for
/// `--uninstalling`): where Tauri keeps them, the user's folder for settings as the system names it.
/// `None` when there is no file, or nothing in it that is settings.
pub fn read_without_app() -> Option<Settings> {
    read_in(&dirs::config_dir()?)
}

/// The settings in the application's folder in that folder for settings.
fn read_in(config: &Path) -> Option<Settings> {
    parse(&std::fs::read(config.join(IDENTIFIER).join("settings.json")).ok()?)
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("settings.json"))
}

/// The settings as the file has them: for the start, before anything else knows them.
pub fn read(app: &AppHandle) -> Settings {
    path(app)
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| parse(&bytes))
        .unwrap_or_default()
}

/// `None` for what is not a JSON object at all.
fn parse(bytes: &[u8]) -> Option<Settings> {
    serde_json::from_slice(bytes).ok().map(Settings::from_file)
}

/// One writer of the file at a time: two would share the file written beside it, and the later of
/// them could write down the older settings.
static WRITING: Mutex<()> = Mutex::new(());

/// Changes the settings: from now on for the running application, and written down for the next run.
/// Writing is best effort: a choice that cannot be written still holds until Perora quits.
pub fn update(app: &AppHandle, change: impl FnOnce(&mut Settings)) {
    let perora = app.state::<Perora>();
    change(&mut perora.settings.lock().unwrap());

    // What is written is the settings as they are when it is this change's turn to write, not as
    // they were when it was made: whoever writes last writes what the application runs with. The
    // settings themselves are not held meanwhile; they are read all the time.
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let settings = perora.settings.lock().unwrap().clone();
    if let Some(path) = path(app) {
        let _ = write(&path, &settings);
    }
}

/// Writes the settings the application runs with, on top of whatever the file holds besides them.
fn write(path: &Path, settings: &Settings) -> std::io::Result<()> {
    let mut merged = settings.clone();
    merged.rest = match std::fs::read(path) {
        Ok(bytes) => match parse(&bytes) {
            Some(on_disk) => on_disk.rest,
            // Something is there that is not settings (cut short, edited into nonsense): kept beside
            // the new file rather than written over.
            None => {
                let _ = std::fs::rename(path, path.with_extension("json.unreadable"));
                Map::new()
            }
        },
        Err(_) => Map::new(),
    };

    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    // Written beside the file and moved over it, so that the file is never half of one.
    let new = path.with_extension("json.new");
    std::fs::write(&new, serde_json::to_vec_pretty(&merged.to_file())?)?;
    std::fs::rename(new, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_default_save() -> Vec<Format> {
        Settings::default().save_formats
    }

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("perora-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory.join("settings.json")
    }

    #[test]
    fn without_a_file_everything_is_as_perora_comes() {
        let settings = parse(b"{}").unwrap();

        assert_eq!(settings, Settings::default());
        assert_eq!(settings.language, None);
        assert_eq!(settings.look, Look::System);
        assert_eq!(parse(br#"{"theme": "dark"}"#).unwrap().look, Look::Dark);
        assert_eq!(parse(br#"{"theme": "light"}"#).unwrap().look, Look::Light);
        assert_eq!(parse(br#"{"theme": "sepia"}"#).unwrap().look, Look::System);
        assert!(!settings.upload_immediately);
        assert!(!settings.upload_enabled);
        assert_eq!(settings.start_mode, Mode::Smart);
        assert!(settings.copy_to_clipboard);
        assert!(!settings.clear_clipboard_on_discard);
        assert_eq!(settings.save_directory, None);
        assert!(!settings.ask_where_to_save);
        assert_eq!(settings.shortcut, "Ctrl+Shift+H");
        for leaves in Leaves::ALL {
            assert_eq!(settings.output(leaves), Output::default(), "{leaves:?}");
        }
        assert_eq!(Output::default().format, Format::Png);
        assert_eq!(Output::default().quality, Quality::High);
        assert_eq!(Output::default().fit, None);
        assert!(!settings.take_snipping);
        assert!(parse(br#"{"takeSnipping": true}"#).unwrap().take_snipping);
        // Perora's own windows are out of the picture unless the user wants them in it.
        assert!(parse(b"{}").unwrap().hide_own_windows);
        assert!(
            !parse(br#"{"hideOwnWindows": false}"#)
                .unwrap()
                .hide_own_windows
        );
        assert_eq!(settings.notice_seconds, 7);
        assert_eq!(settings.notice_corner, Corner::BottomRight);
        assert_eq!(
            settings.notice_sound,
            Sound::System("Windows Notify System Generic".into())
        );
        assert_eq!(settings.notice_sound_file, None);
        assert_eq!(settings.after_capture, After::Preview);
        assert_eq!(settings.preview_seconds, 0);
        assert_eq!(settings.preview_corner, Corner::BottomRight);
        assert_eq!(settings.capture_sound, Sound::default());
        assert_eq!(settings.capture_sound_file, None);
        assert!(settings.tours_seen.is_empty());
    }

    #[test]
    fn each_setting_is_read_on_its_own() {
        let known = parse(
            br#"{"language": "ja", "uploadImmediately": true, "startMode": "rectangle",
                 "copyToClipboard": false, "saveDirectory": "D:\\shots", "shortcut": "Alt+F9",
                 "enableUpload": true, "noticeCorner": "topLeft", "noticeSound": "system:ding",
                 "noticeSoundFile": "D:\\sounds\\pop.mp3"}"#,
        )
        .unwrap();
        assert_eq!(known.language.as_deref(), Some("ja"));
        assert!(known.upload_immediately);
        assert!(known.upload_enabled);
        assert_eq!(known.start_mode, Mode::Rectangle);
        assert!(!known.copy_to_clipboard);
        assert_eq!(known.save_directory, Some(PathBuf::from("D:\\shots")));
        assert_eq!(known.shortcut, "Alt+F9");
        assert_eq!(known.notice_corner, Corner::TopLeft);
        assert_eq!(known.notice_sound, Sound::System("ding".into()));
        let capture = parse(
            br#"{"previewCorner": "topRight", "captureSound": "custom",
                 "captureSoundFile": "D:\\sounds\\click.wav"}"#,
        )
        .unwrap();
        assert_eq!(capture.preview_corner, Corner::TopRight);
        assert_eq!(capture.capture_sound, Sound::Custom);
        assert_eq!(
            capture.capture_sound_file,
            Some(PathBuf::from("D:\\sounds\\click.wav"))
        );
        assert_eq!(
            known.notice_sound_file,
            Some(PathBuf::from("D:\\sounds\\pop.mp3"))
        );
        assert_eq!(
            parse(br#"{"noticeSound": "none"}"#).unwrap().notice_sound,
            Sound::None
        );
        assert_eq!(
            parse(br#"{"noticeSound": "custom"}"#).unwrap().notice_sound,
            Sound::Custom
        );

        // (Notices could once be switched off: a file that says so is read as any other, and
        // what it says of that is not written back.)
        let notices = parse(br#"{"noticeEnabled": false, "noticeSeconds": 0}"#).unwrap();
        assert_eq!(notices.notice_seconds, 0);
        assert!(!notices.to_file().contains_key("noticeEnabled"));
        assert_eq!(
            parse(br#"{"noticeSeconds": 45}"#).unwrap().notice_seconds,
            45
        );
        let leaving = parse(
            br#"{"copyFormat": "jpg", "uploadFormat": "webp", "uploadFit": 2048, "saveFit": 1280,
                 "quality": "light", "copyFit": 2000, "saveFormat": "gif"}"#,
        )
        .unwrap();
        assert_eq!(
            leaving.output(Leaves::Copy),
            Output {
                format: Format::Jpeg,
                quality: Quality::Light,
                fit: None
            }
        );
        assert_eq!(
            leaving.output(Leaves::Upload),
            Output {
                format: Format::WebP,
                quality: Quality::Light,
                fit: Some(2048)
            }
        );
        assert_eq!(
            leaving.output(Leaves::Save),
            Output {
                format: Format::Png,
                quality: Quality::Light,
                fit: Some(1280)
            }
        );
        // Saved as several kinds at once: each once, in their order; the first is "the" kind.
        assert_eq!(settings_default_save(), [Format::Png]);
        assert_eq!(leaving.save_formats, [Format::Png]);
        let both = parse(br#"{"saveFormat": "webp", "saveFormats": ["jpg", "png", "jpg", "gif"]}"#)
            .unwrap();
        assert_eq!(both.save_formats, [Format::Png, Format::Jpeg]);
        assert_eq!(both.output(Leaves::Save).format, Format::Png);
        let one = parse(br#"{"saveFormat": "webp", "saveFormats": []}"#).unwrap();
        assert_eq!(one.save_formats, [Format::WebP]);
        let mut changed = one.clone();
        changed.save_as(&[]);
        assert_eq!(changed, one);
        changed.save_as(&[Format::WebP, Format::Jpeg]);
        assert_eq!(changed.save_formats, [Format::Jpeg, Format::WebP]);
        assert_eq!(changed.output(Leaves::Save).format, Format::Jpeg);
        // (What was once "no preview" is "to the clipboard only", until something else is chosen.)
        let previews = parse(br#"{"previewEnabled": false, "previewSeconds": 20}"#).unwrap();
        assert_eq!(previews.after_capture, After::Copy);
        assert_eq!(previews.preview_seconds, 20);
        assert!(!previews.to_file().contains_key("previewEnabled"));
        let edits = parse(br#"{"previewEnabled": false, "afterCapture": "edit"}"#).unwrap();
        assert_eq!(edits.after_capture, After::Edit);
        assert_eq!(
            parse(br#"{"afterCapture": "print"}"#)
                .unwrap()
                .after_capture,
            After::Preview
        );
        assert_eq!(
            parse(br#"{"previewSeconds": 3601, "previewEnabled": 0}"#).unwrap(),
            Settings::default()
        );
        // (Seconds that are not a whole number of them, or more than can be chosen, are none.)
        for seconds in ["\"5\"", "-1", "2.5", "3601", "true"] {
            let file = format!(r#"{{"noticeSeconds": {seconds}}}"#);
            let odd = parse(file.as_bytes()).unwrap();
            assert_eq!(odd.notice_seconds, 7, "{seconds}");
        }

        // A value of the wrong kind, or one that means nothing, costs that setting, not the others.
        let mixed = parse(
            br#"{"language": "ja", "uploadImmediately": "true", "startMode": "sideways",
                 "copyToClipboard": 0, "shortcut": "", "enableUpload": 1, "noticeCorner": "middle",
                 "noticeSound": "loud"}"#,
        )
        .unwrap();
        assert_eq!(mixed.language.as_deref(), Some("ja"));
        assert!(!mixed.upload_immediately);
        assert!(!mixed.upload_enabled);
        assert_eq!(mixed.start_mode, Mode::Smart);
        assert!(mixed.copy_to_clipboard);
        assert_eq!(mixed.shortcut, "Ctrl+Shift+H");
        assert_eq!(mixed.notice_corner, Corner::BottomRight);
        assert_eq!(mixed.notice_sound, Sound::default());

        // (The tours seen: each once, in their order; a name of none, or not a name, is let go of.)
        let toured = parse(br#"{"toursSeen": ["editor", "map", 3, "capture", "editor"]}"#).unwrap();
        assert_eq!(toured.tours_seen, [Tour::Capture, Tour::Editor]);
        assert!(
            parse(br#"{"toursSeen": "editor"}"#)
                .unwrap()
                .tours_seen
                .is_empty()
        );
        let mut seeing = Settings::default();
        seeing.saw(Tour::Editor);
        seeing.saw(Tour::Preview);
        seeing.saw(Tour::Editor);
        assert_eq!(seeing.tours_seen, [Tour::Preview, Tour::Editor]);
        assert_eq!(seeing.tours_seen(), ["preview", "editor"]);

        // (The version skipped: a name, or none; and written back as it was read.)
        let skipped = parse(br#"{"skippedVersion": "0.2.0"}"#).unwrap();
        assert_eq!(skipped.skipped_version.as_deref(), Some("0.2.0"));
        assert_eq!(
            skipped.to_file().get("skippedVersion"),
            Some(&Value::from("0.2.0"))
        );
        assert_eq!(
            parse(br#"{"skippedVersion": 2}"#).unwrap().skipped_version,
            None
        );
        assert_eq!(
            parse(br#"{"skippedVersion": ""}"#).unwrap().skipped_version,
            None
        );
        assert!(!Settings::default().to_file().contains_key("skippedVersion"));

        assert_eq!(parse(b"not json"), None);
        assert_eq!(parse(b"[1, 2]"), None);
    }

    #[test]
    fn writing_keeps_what_this_version_does_not_know() {
        let path = scratch("unknown");
        std::fs::write(&path, br#"{"language": "en", "somethingNew": {"a": 1}}"#).unwrap();

        let mut settings = parse(&std::fs::read(&path).unwrap()).unwrap();
        settings.upload_immediately = true;
        settings.start_mode = Mode::Monitor;
        settings.saw(Tour::Capture);
        write(&path, &settings).unwrap();

        let written: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["language"], "en");
        assert_eq!(written["uploadImmediately"], true);
        assert_eq!(written["enableUpload"], false);
        assert_eq!(written["startMode"], "monitor");
        assert_eq!(written["hideOwnWindows"], true);
        assert_eq!(written["noticeSeconds"], 7);
        assert_eq!(written["uploadFormat"], "png");
        assert_eq!(written["uploadFit"], 0);
        assert_eq!(written["quality"], "high");
        assert_eq!(written["afterCapture"], "preview");
        assert_eq!(written["previewSeconds"], 0);
        assert_eq!(written["toursSeen"], serde_json::json!(["capture"]));
        assert_eq!(written["somethingNew"]["a"], 1);
        assert!(!path.with_extension("json.new").exists());
        assert_eq!(
            parse(&std::fs::read(&path).unwrap()).unwrap().start_mode,
            Mode::Monitor
        );
    }

    #[test]
    fn a_file_that_is_not_settings_is_put_aside_not_written_over() {
        let path = scratch("unreadable");
        std::fs::write(&path, b"{ \"language\": \"ja\", ").unwrap();

        let settings = Settings {
            language: Some("en".into()),
            ..Settings::default()
        };
        write(&path, &settings).unwrap();

        let written = parse(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written.language.as_deref(), Some("en"));
        assert_eq!(
            std::fs::read(path.with_extension("json.unreadable")).unwrap(),
            b"{ \"language\": \"ja\", "
        );
    }

    #[test]
    fn settings_are_found_without_the_application() {
        let config = scratch("without-app").with_file_name("config");
        assert_eq!(read_in(&config), None);
        let folder = config.join(IDENTIFIER);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("settings.json"), b"{ \"takeSnipping\": true }").unwrap();
        assert!(read_in(&config).unwrap().take_snipping);
        std::fs::write(folder.join("settings.json"), b"not settings").unwrap();
        assert_eq!(read_in(&config), None);
    }

    #[test]
    fn the_folder_named_is_the_application_s() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], IDENTIFIER);
    }

    #[test]
    fn the_first_write_creates_the_file_and_its_directory() {
        let path = scratch("first")
            .with_file_name("deeper")
            .join("settings.json");

        write(&path, &Settings::default()).unwrap();

        assert_eq!(
            parse(&std::fs::read(&path).unwrap()),
            Some(Settings::default())
        );
    }
}
