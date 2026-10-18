//! The preview: what was just captured, shown before anything is done with it. Nothing leaves the
//! machine until the user says so here.
//!
//! Two windows show the same capture. The small one appears in a corner of the primary monitor without
//! taking the keyboard from whatever is in front (a game, usually); the large one opens from it on
//! request. Both are created once, hidden, and shown again for every capture: a web view takes a moment
//! to start, and a preview should not.
//!
//! The image is on the clipboard as soon as the preview appears: pasting it somewhere is the most
//! common thing to do with a capture, and needs no choice. The preview is for the rest: upload it, save
//! it to a file, or throw it away.
//!
//! Choosing closes the preview at once. An upload goes on behind it and ends in a notification, with
//! the link on the clipboard (in place of the image); only when it fails does the preview come back,
//! to say why and to offer another try.
//!
//! The large window is also where the capture is drawn on: lines, arrows, shapes, letters. What is
//! drawn is kept here as a list (`perora_core::edit`), told by the window each time it changes, for
//! the window to ask for again. The image with the drawing in it is made by that window, which is
//! what shows it: it hands the image over with its request to upload, save or copy, and that image
//! is what leaves. Nothing is drawn here. The capture as it was taken is then no longer what the
//! clipboard is to hold: saving puts the drawn image there instead, uploading the link. (What the
//! system keeps of the clipboard's past is the system's and the user's: nothing is taken out of it.)
//!
//! The state lives here, in Rust. The windows ask for it (`preview_state`), are told when it changed
//! (`preview-changed`) and send back what the user chose. The image itself reaches them through the
//! `perora:` protocol (see `serve` in the crate root), which gives out the capture being previewed and
//! nothing else; the window that is drawn in also asks for it as a file (`preview_image`), to draw on.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use perora_core::edit::{Edit, is_a_drawing, size_after};
use perora_core::output::{FITS, Format, Output};
use perora_core::platform::{Frame, ScreenCapture, clipboard_sequence, screen_capture};
use perora_core::shots;
use serde::Serialize;
use tauri::image::Image;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::Perora;
use crate::notice::{Corner, Lasting};
use crate::settings::{After, Leaves, Look};

pub const SMALL: &str = "preview";
pub const LARGE: &str = "preview-large";

/// The small window, in logical pixels. It has the shape of its capture: its picture is as wide as
/// there is room for and as tall as the capture then is, and where that would be taller than a
/// window in a corner should be, as tall as that and narrower instead. Within bounds, both ways:
/// the buttons need their width, and a strip of a capture still needs a picture to click on. Past
/// the bounds the picture is filled with the middle of the capture rather than left partly empty.
///
/// How wide the picture can be, and how tall; and what the window has around it (across, and
/// down: the status and the buttons).
const PICTURE_WIDTH: (f64, f64) = (324.0, 336.0);
const PICTURE_HEIGHT: (f64, f64) = (48.0, 420.0);
const AROUND_PICTURE: (f64, f64) = (24.0, 89.0);

/// The small window's distance from the corner of the work area.
const MARGIN: f64 = 16.0;

/// The longest sides of the image the small window shows: twice what it has room for, for sharpness.
const THUMBNAIL_FIT: (u32, u32) = (720, 840);

/// How large the small window is for a capture of that many pixels, in logical pixels.
fn small_size(width: u32, height: u32) -> (f64, f64) {
    let shape = f64::from(width.max(1)) / f64::from(height.max(1));
    let (mut across, mut down) = (PICTURE_WIDTH.1, (PICTURE_WIDTH.1 / shape).round());
    if down > PICTURE_HEIGHT.1 {
        down = PICTURE_HEIGHT.1;
        across = (down * shape).round().max(PICTURE_WIDTH.0);
    }
    (
        AROUND_PICTURE.0 + across,
        AROUND_PICTURE.1 + down.max(PICTURE_HEIGHT.0),
    )
}

#[derive(Default)]
pub struct State {
    pending: Mutex<Option<Pending>>,
    last_id: AtomicU64,
    /// The capture the large window last said it has drawn.
    drawn: AtomicU64,
    /// Whether the small window is on screen with no capture, only to show where it appears; and
    /// which showing of that it is (the last is the one that is timed).
    sample: AtomicBool,
    sample_turn: AtomicU64,
    /// Which hiding of the large window for a capture is the last: the one it is out of sight for.
    out_of_sight: AtomicU64,
    /// Which fading in of the large window is the one going on: one that began later, or the
    /// window going away, ends the ones before it.
    appearing: AtomicU64,
    /// The captures the user last discarded, the latest last: what is saved of one after that (it
    /// was saved as it was taken, and that took a moment) is taken away again, as it would have been
    /// had it been there. (A few: another capture may have been taken and discarded meanwhile.)
    discarded: Mutex<Vec<u64>>,
}

/// How many of the captures discarded last are kept in mind.
const DISCARDED_KEPT: usize = 8;

impl State {
    /// Keeps in mind that the user discarded a capture.
    fn was_discarded(&self, id: u64) {
        let mut discarded = self
            .discarded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        discarded.push(id);
        let over = discarded.len().saturating_sub(DISCARDED_KEPT);
        discarded.drain(..over);
    }

    /// Whether the user discarded a capture, lately.
    fn is_discarded(&self, id: u64) -> bool {
        self.discarded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&id)
    }
}

struct Pending {
    id: u64,
    frame: Arc<Frame>,
    png: Arc<Vec<u8>>,
    /// The small picture for the corner: of the capture as it is drawn on, made anew when the
    /// small window is to show a drawing it does not show yet. Which making it is (the windows ask
    /// for it by that number, to get the new one), and the drawing it shows.
    thumbnail: Arc<Vec<u8>>,
    thumbnail_made: u64,
    thumbnail_edits: Vec<Edit>,
    progress: Progress,
    /// Whether the image went to the clipboard when the preview appeared: `None` when the user has
    /// that turned off, otherwise the clipboard's count of changes right after it did, or why it did
    /// not.
    copied: Option<Result<Option<u32>, String>>,
    /// What was drawn on the capture as it went to the clipboard: nothing at first, more when it
    /// is copied again later.
    copied_edits: Vec<Edit>,
    /// What it was made into as it went there: the kind of file, how large at most.
    copied_as: Output,
    /// What the user chose for this capture alone, of each way it can leave, in place of what the
    /// settings say: kept as the window that chose said it, for the other window to show the
    /// same. (What leaves is what the window asks for as it asks: this is only remembered.)
    once: serde_json::Map<String, serde_json::Value>,
    /// What the user drew on the capture, in the order it was drawn.
    edits: Vec<Edit>,
    /// The image the large window last made of the capture with a drawing in it, and the drawing
    /// that is: what leaves, for as long as the drawing is still that one.
    drawn: Option<Picture>,
    /// Whether anything was done about the capture: shown large, or asked to leave (whatever came
    /// of that). It is not given up on its own after that.
    touched: bool,
    /// Whether it was shown in the large window: put back on screen, it is there again.
    large: bool,
    /// The files it was saved as, as it was taken, when the user has every capture saved.
    saved: Vec<PathBuf>,
    /// Which time it is that the small preview was left to: one that began later counts, not the
    /// ones before it.
    timer: u64,
}

/// The capture with a drawing in it, as the window that shows it made it: a PNG file.
struct Picture {
    png: Arc<Vec<u8>>,
    edits: Vec<Edit>,
}

/// No image a window hands over is longer than this, in bytes.
const LONGEST_PICTURE: usize = 256 * 1024 * 1024;

/// The image that leaves, taken out of the state to be dealt with elsewhere: that takes a moment.
struct Leaving {
    /// The capture as it was taken.
    frame: Arc<Frame>,
    /// The file that leaves: the capture, or when it was drawn on, the image the window made of it.
    png: Arc<Vec<u8>>,
    edits: Vec<Edit>,
    /// Whether the capture went to the clipboard, and whether what went there is no longer what it
    /// looks like now (it was drawn on since).
    copied: bool,
    copy_is_old: bool,
}

impl Leaving {
    /// The file that leaves, as the user has captures leave that way: the kind of file, and no
    /// larger than they chose. The PNG at hand, when that is what is asked for already.
    fn file(&self, how: Output) -> Result<Arc<Vec<u8>>, String> {
        let (width, height) =
            Frame::png_size(&self.png).unwrap_or((self.frame.width(), self.frame.height()));
        if how.keeps(width, height) {
            return Ok(self.png.clone());
        }
        self.pixels()?.to_file(how).map(Arc::new)
    }

    /// Its pixels, for the clipboard. Never those of the capture without the drawing: what was
    /// drawn over is not to be seen.
    fn pixels(&self) -> Result<Arc<Frame>, String> {
        if self.edits.is_empty() {
            return Ok(self.frame.clone());
        }
        let most = u64::from(self.frame.width()) * u64::from(self.frame.height());
        Frame::from_png(&self.png, most)
            .map(Arc::new)
            .ok_or_else(|| NOT_MADE.to_owned())
    }
}

/// What a window asks to be done with a capture: which capture, and from the window that is drawn
/// in, what is drawn and the image it made of that (no image when nothing is drawn).
struct Asked {
    id: u64,
    drawing: Option<(Vec<Edit>, Vec<u8>)>,
    once: Once,
}

/// What a window asks for this one time, in place of what the settings say of how a capture
/// leaves: the kind of file, the square it is to fit (`Some(None)` for none), or neither.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Once {
    format: Option<Format>,
    /// Several kinds at once, for saving: one file of each.
    formats: Option<Vec<Format>>,
    fit: Option<Option<u32>>,
}

impl Once {
    /// The kinds of file a capture is saved as, this time: those asked for, or what the settings
    /// say. Never none.
    fn saved_as(&self, app: &AppHandle) -> Vec<Format> {
        match (&self.formats, self.format) {
            (Some(kinds), _) if !kinds.is_empty() => kinds.clone(),
            (_, Some(kind)) => vec![kind],
            _ => app
                .state::<Perora>()
                .setting(|settings| settings.save_formats.clone()),
        }
    }

    /// What a capture is made into as it leaves that way, this time.
    fn output(&self, app: &AppHandle, leaves: Leaves) -> Output {
        let usual = app
            .state::<Perora>()
            .setting(|settings| settings.output(leaves));
        Output {
            format: self.format.unwrap_or(usual.format),
            fit: self.fit.unwrap_or(usual.fit),
            ..usual
        }
    }
}

/// Reads what a window asks. The small window names the capture, `{"id": Ã¢â‚¬Â¦}`. The large one hands
/// over bytes: four that say how long what follows them is (least first), then that, which names the
/// capture and what is drawn (`{"id": Ã¢â‚¬Â¦, "edits": [Ã¢â‚¬Â¦]}`), then the image as a PNG file. All in one,
/// so that the image and the drawing it shows cannot come apart on the way.
fn asked(body: &InvokeBody) -> Result<Asked, &'static str> {
    #[derive(serde::Deserialize)]
    struct Named {
        id: u64,
        #[serde(default)]
        edits: Vec<Edit>,
        /// For this once: `{"format": "webp", "fit": 2048}`, either or neither (a fit of 0 is
        /// none). What is not a kind of file, or not a square there is to choose, is not asked.
        #[serde(default, rename = "as")]
        once: Option<NamedOnce>,
    }
    #[derive(serde::Deserialize)]
    struct NamedOnce {
        format: Option<String>,
        /// (For saving: several kinds at once.)
        formats: Option<Vec<String>>,
        fit: Option<u64>,
    }
    let once = |named: &Named| {
        let Some(once) = &named.once else {
            return Once::default();
        };
        Once {
            format: once.format.as_deref().and_then(Format::from_name),
            formats: once.formats.as_ref().map(|names| {
                crate::settings::several(names.iter().filter_map(|name| Format::from_name(name)))
            }),
            fit: once.fit.and_then(|side| match u32::try_from(side) {
                Ok(0) => Some(None),
                Ok(side) if FITS.contains(&side) => Some(Some(side)),
                _ => None,
            }),
        }
    };
    match body {
        InvokeBody::Json(named) => {
            let named: Named = serde_json::from_value(named.clone()).map_err(|_| NOT_ASKED)?;
            Ok(Asked {
                id: named.id,
                drawing: None,
                once: once(&named),
            })
        }
        InvokeBody::Raw(bytes) => {
            // (Longer than an image with a drawing gets: not looked at, and not kept.)
            if bytes.len() > LONGEST_PICTURE {
                return Err(NOT_MADE);
            }
            let length = bytes.get(..4).ok_or(NOT_ASKED)?;
            let length = u32::from_le_bytes(length.try_into().map_err(|_| NOT_ASKED)?) as usize;
            let named = bytes
                .get(4..)
                .and_then(|rest| rest.get(..length))
                .ok_or(NOT_ASKED)?;
            let named: Named = serde_json::from_slice(named).map_err(|_| NOT_ASKED)?;
            Ok(Asked {
                id: named.id,
                once: once(&named),
                drawing: Some((named.edits, bytes[4 + length..].to_vec())),
            })
        }
    }
}

/// Takes what a window says is drawn on the capture and the image it made of it, when it says so.
/// Refused when the drawing is more than a drawing is, or the image is not one of this capture
/// with that drawing in it (as large as the drawing leaves it).
fn take(pending: &mut Pending, drawing: Option<(Vec<Edit>, Vec<u8>)>) -> Result<(), &'static str> {
    let Some((edits, png)) = drawing else {
        return Ok(());
    };
    if !is_a_drawing(&edits) {
        return Err(TOO_MUCH);
    }
    if edits.is_empty() {
        pending.drawn = None;
    } else {
        let left = size_after(&edits, pending.frame.width(), pending.frame.height());
        if Frame::png_size(&png) != Some(left) {
            return Err(NOT_MADE);
        }
        pending.drawn = Some(Picture {
            png: Arc::new(png),
            edits: edits.clone(),
        });
    }
    pending.edits = edits;
    Ok(())
}

/// What a window is told when its drawing is more than a drawing is: nothing was taken.
const TOO_MUCH: &str = "tooMuch";

/// What a window is told when there is no image of the capture as it is drawn on now: nothing
/// leaves, least of all the capture without the drawing.
const NOT_MADE: &str = "notMade";

/// Nothing leaves, for want of an image of the capture as it is drawn on: said to the window that
/// asked, and the large window is shown, where the image can be made. Not to be called with the
/// state held (see `out_of_sight`).
fn not_made(app: &AppHandle) -> &'static str {
    show_large(app);
    NOT_MADE
}

/// What a window is told when what it asked cannot be read.
const NOT_ASKED: &str = "notAsked";

impl Pending {
    /// The image that would leave now. `None` when the capture is drawn on and no image of it with
    /// that drawing has come from the window that shows it.
    fn leaving(&self) -> Option<Leaving> {
        let png = if self.edits.is_empty() {
            self.png.clone()
        } else {
            self.drawn
                .as_ref()
                .filter(|drawn| drawn.edits == self.edits)?
                .png
                .clone()
        };
        Some(Leaving {
            frame: self.frame.clone(),
            png,
            edits: self.edits.clone(),
            copied: matches!(self.copied, Some(Ok(_))),
            copy_is_old: self.copied_edits != self.edits,
        })
    }

    /// Whether the small picture shows the capture as it is drawn on now.
    fn thumbnail_is_current(&self) -> bool {
        self.thumbnail_edits == self.edits
    }

    /// Whether the image this preview put on the clipboard is still what is there: nothing was put
    /// there since, by the user or by Perora (the link of an upload that finished meanwhile).
    fn on_clipboard(&self) -> bool {
        matches!(
            &self.copied,
            Some(Ok(Some(written))) if clipboard_sequence() == Some(*written)
        )
    }
}

enum Progress {
    /// Shown, nothing chosen yet.
    Waiting,
    /// On its way to the server, the windows hidden.
    Uploading,
    /// The upload did not work; what to tell the user. Shown again, to be tried again.
    Failed(String),
}

impl State {
    fn lock(&self) -> MutexGuard<'_, Option<Pending>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Whether a capture is in hand: shown in a preview, or being uploaded. What would end Perora
/// (installing a new version) waits until it is done with.
pub fn holds_a_capture(app: &AppHandle) -> bool {
    app.state::<Perora>().preview.lock().is_some()
}

/// What the windows show: the texts in the user's language, and the capture if there is one.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    language: String,
    texts: HashMap<String, String>,
    capture: Option<CaptureView>,
    /// Whether the small window is on screen with no capture, only to show where it appears.
    sample: bool,
    /// Whether uploading is among what can be done with a capture: the user chose so, and someone
    /// is signed in.
    offers_upload: bool,
    /// The colour the user chose for their system, as `#rrggbb`: one of the colours to draw with.
    accent: Option<String>,
    /// The kinds of file a capture can leave as, the squares it can be made to fit (pixels a
    /// side), and what the settings say of each way of leaving: for choosing otherwise, once.
    formats: Vec<&'static str>,
    fits: Vec<u32>,
    leaves: HashMap<&'static str, LeavesView>,
    /// The tours that were seen, by their names: one that was not is shown as its window opens.
    tours_seen: Vec<&'static str>,
    /// Which way of making a part unreadable the editor starts with: `last` (the one used last),
    /// `mosaic` or `blur`.
    default_hider: &'static str,
}

#[derive(Serialize)]
struct LeavesView {
    format: &'static str,
    /// (More than one only for saving: one file of each.)
    formats: Vec<&'static str>,
    /// Pixels a side; 0 for as large as it is.
    fit: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureView {
    id: u64,
    width: u32,
    height: u32,
    progress: &'static str,
    /// With `failed`: why.
    message: Option<String>,
    /// Whether the image is on the clipboard; why not, when it could not be put there.
    copied: bool,
    copy_failure: Option<String>,
    /// What is drawn on it so far.
    edits: Vec<Edit>,
    /// Whether it was drawn on since it went to the clipboard: what is there is an older look of it.
    copy_is_old: bool,
    /// What it was made into as it went there: asked for as something else, it is not there.
    copied_as: LeavesView,
    /// What was chosen for this capture alone, of each way it can leave, as a window said it.
    once: serde_json::Map<String, serde_json::Value>,
    /// Which making of the small picture is the one to show.
    thumbnail: u64,
    /// How large the image is as it would leave: cut down, when it was.
    kept_width: u32,
    kept_height: u32,
}

/// Creates both windows, hidden.
pub fn create_windows(app: &AppHandle) -> tauri::Result<()> {
    let small = WebviewWindowBuilder::new(app, SMALL, WebviewUrl::App("index.html".into()))
        .title("Perora")
        .inner_size(small_size(16, 9).0, small_size(16, 9).1)
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
        .on_page_load(not_a_browser())
        .build()?;

    // Gone the moment a choice is made: the button was the feedback, a fade-out only delays it.
    without_transitions(&small);
    crate::recovery::watch(&small);

    let large = WebviewWindowBuilder::new(app, LARGE, WebviewUrl::App("index.html".into()))
        .title("Perora")
        .inner_size(1200.0, 800.0)
        // (No smaller than can be drawn in: the bar of tools and what is beside it, and a picture
        // under them.)
        .min_inner_size(1200.0, 800.0)
        .center()
        .visible(false)
        .devtools(false)
        .zoom_hotkeys_enabled(false)
        .on_page_load(not_a_browser())
        .build()?;
    crate::recovery::watch(&large);
    Ok(())
}

/// Shows a window that is a part of the screen, not something to switch to: it is kept out of what
/// the user switches between (Alt+Tab, the task view). That is said to the system each time the
/// window is shown, and right after: showing a window sets what the system knows of it anew.
pub fn show_beside_tasks(window: &WebviewWindow) {
    show_beside_tasks_seen(window, true);
}

/// As [`show_beside_tasks`]; with `seen` false, the window is shown with nothing of it seen yet
/// (whoever asks lets it be seen, little by little: see `appear_in_corner`). Out of sight while it
/// is shown and made so, and in that order: showing a window sets its styles anew.
fn show_beside_tasks_seen(window: &WebviewWindow, seen: bool) {
    let shown = window.clone();
    let _ = window.run_on_main_thread(move || {
        #[cfg(windows)]
        let handle = shown
            .hwnd()
            .ok()
            .map(|handle| perora_core::platform::WindowId(handle.0 as isize));
        #[cfg(windows)]
        if let Some(handle) = handle
            && !seen
        {
            perora_core::platform::set_cloaked(handle, true);
        }
        let _ = shown.show();
        #[cfg(windows)]
        if let Some(handle) = handle {
            perora_core::platform::not_among_tasks(handle);
            // (Seen wholly, whatever a fading in that was broken off left of it.)
            perora_core::platform::set_opacity(handle, (!seen).then_some(0));
            perora_core::platform::set_cloaked(handle, false);
        }
    });
}

/// Makes a window appear and disappear at once, without the system's animation.
pub fn without_transitions(window: &WebviewWindow) {
    #[cfg(windows)]
    if let Ok(handle) = window.hwnd() {
        perora_core::platform::without_transitions(perora_core::platform::WindowId(
            handle.0 as isize,
        ));
    }
}

/// Turns off what a web view has of a browser: its keyboard shortcuts (developer tools, reload, print,
/// find, view source), its developer tools and its context menu. These windows are parts of an
/// application; the page in them also refuses those keys, but the page is not asked about all of them.
///
/// The settings hold from the next page on. So this waits for the first page to have loaded (before
/// that there is nothing to load again: the web view is still on its blank page), sets them, and loads
/// the page once more, while the window is still hidden.
pub fn not_a_browser() -> impl Fn(WebviewWindow, PageLoadPayload<'_>) + Send + Sync + 'static {
    let done = AtomicBool::new(false);
    move |window, payload| {
        let first_page = payload.event() == PageLoadEvent::Finished
            && payload.url().scheme() != "about"
            && !done.swap(true, Ordering::Relaxed);
        if first_page {
            turn_off_browser_features(&window);
        }
    }
}

#[cfg(windows)]
fn turn_off_browser_features(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows_core::Interface;

    let _ = window.with_webview(|webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else {
            return;
        };
        let Ok(settings) = core.Settings() else {
            return;
        };
        let _ = settings.SetAreDevToolsEnabled(false);
        let _ = settings.SetAreDefaultContextMenusEnabled(false);
        if let Ok(settings) = settings.cast::<ICoreWebView2Settings3>() {
            let _ = settings.SetAreBrowserAcceleratorKeysEnabled(false);
        }
        let _ = core.Reload();
    });
}

#[cfg(not(windows))]
fn turn_off_browser_features(_: &WebviewWindow) {}

/// Gives every window the look the user chose: light, dark, or as the system is set.
///
/// Said to the windows, for their frames, and to the web views themselves: they share one profile,
/// which is told whether its pages are light or dark. (Left to follow their windows, they follow
/// into light and do not come back.)
pub fn show_look(app: &AppHandle, look: Look) {
    app.set_theme(look.theme());
    #[cfg(windows)]
    if let Some(window) = app.webview_windows().into_values().next() {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO, COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
            COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT, ICoreWebView2_13,
        };
        use windows_core::Interface;

        let scheme = match look {
            Look::System => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
            Look::Light => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
            Look::Dark => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
        };
        let _ = window.with_webview(move |webview| unsafe {
            let profile = webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_13>())
                .and_then(|core| core.Profile());
            if let Ok(profile) = profile {
                let _ = profile.SetPreferredColorScheme(scheme);
            }
        });
    }
}

/// Puts a capture on the clipboard and nothing else: what becomes of one when the user chose to
/// have no preview. (Whether or not a capture that is previewed is put there: here it is all there
/// is.) Refused with the reason when it could not be put there.
pub fn copy_alone(app: &AppHandle, frame: &Frame) -> Result<(), String> {
    to_clipboard(app, frame, Once::default().output(app, Leaves::Copy)).map(|_| ())
}

/// Puts a picture on the clipboard as the user has captures copied: no larger than they chose,
/// and as a picture (a PNG, to whatever asks the clipboard for one) or, of another kind, as a file
/// of that kind, which is pasted as files are. Returns the clipboard's count of changes right
/// after, where the system says; refused with the reason when it could not be put there.
fn to_clipboard(app: &AppHandle, frame: &Frame, how: Output) -> Result<Option<u32>, String> {
    let fitted = how.fit.and_then(|side| frame.fitted(side));
    let frame = fitted.as_ref().unwrap_or(frame);
    // The file of a copy that is not a PNG is made first (that takes a moment); then one at a
    // time has the clipboard and the folder of what is copied.
    let file = match how.format {
        Format::Png => None,
        _ => Some(frame.to_file(Output { fit: None, ..how })?),
    };
    let _one_at_a_time = COPIED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(file) = file else {
        app.clipboard()
            .write_image(&Image::new(&frame.rgba, frame.width(), frame.height()))
            .map_err(|error| error.to_string())?;
        // (No file of Perora's is on the clipboard any more.)
        forget_copied_held(app, None);
        return Ok(clipboard_sequence());
    };

    // A file has to be somewhere: in a folder of Perora's own that holds the one that is on the
    // clipboard, and nothing else. The one before it goes once the new one is there, not before:
    // should the new one not get there, the old one is still what the clipboard names.
    let folder = copied_folder(app).ok_or("no folder for what is copied")?;
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let path = (1..)
        .map(|attempt| {
            folder.join(format!(
                "Perora {}{}.{}",
                perora_core::platform::local_timestamp(),
                if attempt == 1 {
                    String::new()
                } else {
                    format!(" ({attempt})")
                },
                how.format.name()
            ))
        })
        .find(|path| !path.exists())
        .ok_or("no name for what is copied")?;
    std::fs::write(&path, file).map_err(|error| error.to_string())?;
    if let Err(error) = perora_core::platform::put_file_on_clipboard(&path) {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    forget_copied_held(app, Some(&path));
    Ok(clipboard_sequence())
}

/// One at a time in the folder of what is copied.
static COPIED: Mutex<()> = Mutex::new(());

fn copied_folder(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_cache_dir().ok()?.join("copied"))
}

/// Takes away the files Perora put on the clipboard before, all but `kept`: they are no longer
/// what the clipboard names, and nobody asked for them to be kept.
fn forget_copied(app: &AppHandle, kept: Option<&Path>) {
    let _one_at_a_time = COPIED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    forget_copied_held(app, kept);
}

/// (By whoever has the folder already.)
fn forget_copied_held(app: &AppHandle, kept: Option<&Path>) {
    let Some(folder) = copied_folder(app) else {
        return;
    };
    let Ok(files) = std::fs::read_dir(folder) else {
        return;
    };
    for file in files.flatten() {
        if Some(file.path().as_path()) != kept {
            let _ = std::fs::remove_file(file.path());
        }
    }
}

/// How often a small preview whose time is over looks whether the pointer has left it.
const POINTED_AT: Duration = Duration::from_millis(250);

impl Pending {
    /// Whether nothing was done about the capture yet: not drawn on, not shown large, not asked to
    /// leave, not on its way to the server nor back from there with a reason.
    fn is_untouched(&self) -> bool {
        matches!(self.progress, Progress::Waiting) && !self.touched && self.edits.is_empty()
    }
}

/// The small preview is on screen (anew, or again): from now it has as long as the user chose, and
/// the capture is given up when that is over with nothing done about it. Not one that something
/// was done about, and not under the pointer: whoever points at it is about to press something on
/// it. A preview taken off the screen meanwhile (another capture is being selected) has its time
/// again when it is back.
fn leave_to_its_time(app: &AppHandle) {
    let perora = app.state::<Perora>();
    // (For as long as the settings say now: not changed under it afterwards.)
    let seconds = perora.setting(|settings| settings.preview_seconds);
    if seconds == 0 {
        return;
    }
    let Some((id, timer)) = perora
        .preview
        .lock()
        .as_mut()
        .filter(|pending| pending.is_untouched())
        .map(|pending| {
            pending.timer += 1;
            (pending.id, pending.timer)
        })
    else {
        return;
    };

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(seconds.into()));
        loop {
            let perora = app.state::<Perora>();
            let still = matches!(
                &*perora.preview.lock(),
                Some(pending)
                    if pending.id == id && pending.timer == timer && pending.is_untouched()
            );
            let Some(small) = app.get_webview_window(SMALL) else {
                return;
            };
            if !still || !small.is_visible().unwrap_or(false) {
                return;
            }
            let pointed_at = match (
                app.cursor_position(),
                small.outer_position(),
                small.outer_size(),
            ) {
                (Ok(pointer), Ok(corner), Ok(size)) => {
                    let (across, down) = (
                        pointer.x - f64::from(corner.x),
                        pointer.y - f64::from(corner.y),
                    );
                    (0.0..f64::from(size.width)).contains(&across)
                        && (0.0..f64::from(size.height)).contains(&down)
                }
                _ => false,
            };
            if !pointed_at {
                // Only if it is still left alone, seen and decided in one: not what was just
                // asked to leave. (Not as the user discards it: what is on the clipboard stays.)
                let gone = {
                    let mut pending = perora.preview.lock();
                    let still = matches!(
                        &*pending,
                        Some(pending)
                            if pending.id == id
                                && pending.timer == timer
                                && pending.is_untouched()
                    );
                    if still {
                        *pending = None;
                    }
                    still
                };
                if gone {
                    hide(&app);
                    changed(&app);
                }
                return;
            }
            std::thread::sleep(POINTED_AT);
        }
    });
}

/// Shows a capture in the small window, in place of whatever was previewed before. Returns the id
/// it is previewed by.
pub fn present(app: &AppHandle, frame: Frame, png: Vec<u8>) -> u64 {
    let perora = app.state::<Perora>();
    // A thumbnail that cannot be made is no reason to lose the capture: the whole image serves.
    let png = Arc::new(png);
    let thumbnail = frame
        .scaled_to_fit(THUMBNAIL_FIT.0, THUMBNAIL_FIT.1)
        .to_png()
        .map(Arc::new)
        .unwrap_or_else(|_| png.clone());

    // To the clipboard first (unless the user would rather not): it is there to paste by the time the
    // preview is seen.
    let copied_as = Once::default().output(app, Leaves::Copy);
    let copied = perora
        .setting(|settings| settings.copy_to_clipboard)
        .then(|| to_clipboard(app, &frame, copied_as));
    if let Some(Err(error)) = &copied {
        eprintln!("clipboard: the capture was not put there: {error}");
    }

    // (The small window is no longer there only to show where it appears.)
    perora.preview.sample.store(false, Ordering::Relaxed);
    // As the user has captures shown: in the corner, or in the large window at once, where they
    // are drawn on.
    let large_at_once = perora.setting(|settings| settings.after_capture) == After::Edit;
    let id = perora.preview.last_id.fetch_add(1, Ordering::Relaxed) + 1;
    *perora.preview.lock() = Some(Pending {
        id,
        frame: Arc::new(frame),
        png,
        thumbnail,
        thumbnail_made: 0,
        thumbnail_edits: Vec::new(),
        progress: Progress::Waiting,
        copied,
        copied_edits: Vec::new(),
        copied_as,
        once: serde_json::Map::new(),
        edits: Vec::new(),
        drawn: None,
        // (Shown large from the start, it is not one to be given up on its own.)
        touched: large_at_once,
        large: large_at_once,
        timer: 0,
        saved: Vec::new(),
    });

    if large_at_once {
        // Told of the capture before it is in sight, and shown once it says it has drawn it (or
        // has not said so in a moment): what the window showed last is another capture's, and
        // would be seen first.
        changed(app);
        let (waited, before) = (
            Instant::now(),
            perora.preview.appearing.load(Ordering::Relaxed),
        );
        while perora.preview.drawn.load(Ordering::Relaxed) != id && waited.elapsed() < DRAWN_WITHIN
        {
            std::thread::sleep(Duration::from_millis(8));
        }
        // (The windows were taken off the screen meanwhile, for the next capture: this one is
        // shown when that comes to nothing, or not at all.)
        if perora.preview.appearing.load(Ordering::Relaxed) != before {
            return id;
        }
        show(app);
        return id;
    }
    show_small(app);
    changed(app);
    leave_to_its_time(app);
    id
}

/// Puts the capture being previewed back on screen, if there is one waiting to be dealt with: after a
/// selection screen that came to nothing, or one that was up when the preview wanted to appear.
pub fn restore(app: &AppHandle) {
    let waiting = matches!(
        &*app.state::<Perora>().preview.lock(),
        Some(pending) if !matches!(pending.progress, Progress::Uploading)
    );
    if waiting {
        show(app);
        changed(app);
        leave_to_its_time(app);
    }
}

/// The preview on screen, in the window it was in: the large one once it was shown there, the
/// small one in its corner until then. Neither over a selection screen (see `show_small`).
fn show(app: &AppHandle) {
    let perora = app.state::<Perora>();
    let large = matches!(&*perora.preview.lock(), Some(pending) if pending.large);
    if !large {
        return show_small(app);
    }
    if !perora.selection.is_active() {
        show_large(app);
    }
}

/// The small window in its corner, the large one out of the way. Not over a selection screen: it
/// would be in the way and in the picture; `restore` shows it when that screen is gone.
///
/// The small window shows the capture as it is drawn on, and what leaves from it is that. Drawn on
/// with no image of it made yet (the large window makes one only when something is to leave), it is
/// the large window that is shown: there the image can be made.
fn show_small(app: &AppHandle) {
    let perora = app.state::<Perora>();
    if perora.selection.is_active() {
        return;
    }
    // The small picture is to show the capture as it is drawn on now (never an older look of it:
    // that may show what was since made unreadable). Made anew from the image the large window
    // made, here and not while the state is held: that takes a moment.
    let to_draw = match &*perora.preview.lock() {
        Some(pending) if !pending.thumbnail_is_current() => Some(pending.leaving()),
        _ => None,
    };
    if let Some(leaving) = to_draw {
        let thumbnail = leaving.as_ref().and_then(|leaving| {
            leaving
                .pixels()
                .ok()?
                .scaled_to_fit(THUMBNAIL_FIT.0, THUMBNAIL_FIT.1)
                .to_png()
                .ok()
        });
        let shown = match (perora.preview.lock().as_mut(), leaving, thumbnail) {
            (Some(pending), Some(leaving), Some(thumbnail)) if pending.edits == leaving.edits => {
                pending.thumbnail = Arc::new(thumbnail);
                pending.thumbnail_edits = leaving.edits;
                pending.thumbnail_made += 1;
                true
            }
            // No capture any more: nothing to show either way.
            (None, ..) => true,
            _ => false,
        };
        if !shown {
            return show_large(app);
        }
    }
    if let Some(large) = app.get_webview_window(LARGE) {
        let _ = large.hide();
    }
    // As the capture is shaped now (cut down, it has another shape than it was taken with).
    let shape = perora.preview.lock().as_ref().map(|pending| {
        size_after(
            &pending.edits,
            pending.frame.width(),
            pending.frame.height(),
        )
    });
    if let Some(small) = app.get_webview_window(SMALL) {
        if let Some((width, height)) = shape {
            let (across, down) = small_size(width, height);
            let _ = small.set_size(LogicalSize::new(across, down));
        }
        let corner = perora.setting(|settings| settings.preview_corner);
        appear_in_corner(&small, corner, None);
    }
}

/// How long after it was hidden for a capture the large window is put back in sight (hidden still):
/// longer than the system takes to fade a window out.
const OUT_OF_SIGHT_FOR: Duration = Duration::from_millis(400);

/// Takes the preview windows off the screen before it is captured again (the capture being previewed
/// stays until the new one takes its place). Returns whether anything was on screen.
///
/// The large window goes the way windows close, fading out, and would still be in the picture,
/// faintly: when it is on screen it is taken out of sight first, which is at once, and hidden
/// behind that. It is put back in sight a moment later, hidden by then, and not as it is next
/// shown: the system opens a window the way windows open only when nothing else about how it is
/// seen was changed just before. (For the same reason a large window that is not on screen is
/// left alone.)
pub fn hide_for_capture(app: &AppHandle) -> bool {
    // (Before looking at what is on screen: what is shown from here on is not shown after all.)
    let perora = app.state::<Perora>();
    perora.preview.appearing.fetch_add(1, Ordering::Relaxed);
    let on_screen = |label: &str| {
        app.get_webview_window(label)
            .is_some_and(|window| window.is_visible().unwrap_or(false))
    };
    let (small, large) = (on_screen(SMALL), on_screen(LARGE));
    let turn = perora.preview.out_of_sight.fetch_add(1, Ordering::Relaxed) + 1;
    if large {
        out_of_sight(app, true);
    }
    for label in [SMALL, LARGE] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    if large && let Some(window) = app.get_webview_window(LARGE) {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(OUT_OF_SIGHT_FOR);
            // Put back where the windows are shown and hidden from, and only if it is still this
            // hiding that it is out of sight for, and it is hidden still: taken out of sight again
            // since, or shown, it was seen to there.
            let (during, hidden) = (app.clone(), window.clone());
            let _ = window.run_on_main_thread(move || {
                let still = during
                    .state::<Perora>()
                    .preview
                    .out_of_sight
                    .load(Ordering::Relaxed)
                    == turn;
                if still && !hidden.is_visible().unwrap_or(true) {
                    out_of_sight(&during, false);
                }
            });
        });
    }
    small || large
}

fn hide(app: &AppHandle) {
    // (Whatever was still fading in is not any more.)
    app.state::<Perora>()
        .preview
        .appearing
        .fetch_add(1, Ordering::Relaxed);
    for label in [SMALL, LARGE] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
    // (Hidden for good now, the large window is no longer merely out of sight.)
    out_of_sight(app, false);
}

/// Takes the large window out of sight, or puts it back, without hiding it. Out of sight is where
/// it is from the moment the user chose to upload or save until the application has what it needs
/// from it: the window still has to make the image that leaves, which takes a moment, and a choice
/// is to close the window at once. (Hidden, it would be slowed down by the system while it does.)
///
/// Asks the window for its handle, which waits for the thread the windows live on: not to be
/// called with the state held, which that thread may be waiting for.
fn out_of_sight(app: &AppHandle, out: bool) {
    #[cfg(windows)]
    if let Some(handle) = app
        .get_webview_window(LARGE)
        .and_then(|large| large.hwnd().ok())
    {
        perora_core::platform::set_cloaked(perora_core::platform::WindowId(handle.0 as isize), out);
    }
}

/// The large window is about to hand over the capture, to upload or to save: it is taken out of
/// sight meanwhile, as if it had closed already. Not when saving asks where to: the question is
/// asked over the window. Whatever comes of it, the window says when it is done (`preview_appear`).
#[tauri::command]
pub fn preview_vanish(app: AppHandle, id: u64, to: String) {
    let perora = app.state::<Perora>();
    let there = matches!(
        &*perora.preview.lock(),
        Some(pending) if pending.id == id && !matches!(pending.progress, Progress::Uploading)
    );
    let asks = to == "save" && perora.setting(|settings| settings.ask_where_to_save);
    if there && !asks {
        out_of_sight(&app, true);
    }
}

/// The large window is done handing over the capture: back in sight, if it is still shown at all
/// (it stayed because nothing left, or it is hidden by now and this is for the next time).
#[tauri::command]
pub fn preview_appear(app: AppHandle) {
    out_of_sight(&app, false);
}

/// How tall the small window is, in pixels of the screen, while it is on screen with a capture:
/// what shares its corner keeps clear of it by that much. Not for a capture whose small window is
/// out of sight (shown large, on its way to the server): room kept for a window that is not there
/// is a gap nobody can account for.
pub fn small_in_its_corner(app: &AppHandle) -> Option<u32> {
    if app.state::<Perora>().preview.lock().is_none() {
        return None;
    }
    let small = app.get_webview_window(SMALL)?;
    if !small.is_visible().unwrap_or(false) {
        return None;
    }
    Some(small.outer_size().ok()?.height)
}

/// The room between the small window and what is put above it, in logical pixels.
const ABOVE: f64 = 8.0;

/// How long the small window is on screen to show where it appears.
const SHOWN_WHERE_FOR: Duration = Duration::from_secs(5);

/// The user chose another corner for the small window. With a capture in it, it is there at
/// once, if it is on screen. With none, the window is shown there for a moment all the same, to
/// say that this is where it appears: the choice is seen at once, as that of where notices are
/// said is.
pub fn moved(app: &AppHandle) {
    let perora = app.state::<Perora>();
    let corner = perora.setting(|settings| settings.preview_corner);
    let Some(small) = app.get_webview_window(SMALL) else {
        return;
    };
    if perora.preview.lock().is_some() {
        if small.is_visible().unwrap_or(false) {
            // (On its way in, it stops fading in where this puts it: seen wholly there.)
            place_in_corner(&small, corner, None);
            show_beside_tasks(&small);
        }
        return;
    }
    if perora.selection.is_active() {
        return;
    }
    perora.preview.sample.store(true, Ordering::Relaxed);
    let turn = perora.preview.sample_turn.fetch_add(1, Ordering::Relaxed) + 1;
    let (across, down) = small_size(16, 9);
    let _ = small.set_size(LogicalSize::new(across, down));
    changed(app);
    appear_in_corner(&small, corner, None);

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SHOWN_WHERE_FOR);
        let perora = app.state::<Perora>();
        // (Shown anew since, or a capture came: not this one's to take away.)
        if perora.preview.sample_turn.load(Ordering::Relaxed) != turn
            || !perora.preview.sample.swap(false, Ordering::Relaxed)
        {
            return;
        }
        if perora.preview.lock().is_none()
            && let Some(small) = app.get_webview_window(SMALL)
        {
            let _ = small.hide();
        }
        changed(&app);
    });
}

/// A corner of the primary monitor's work area (which leaves out the taskbar); with `clear_of`, as
/// far from the edge as it takes to be clear of a window that tall there (above it at the bottom,
/// below it at the top).
pub fn place_in_corner(window: &WebviewWindow, corner: Corner, clear_of: Option<u32>) {
    let _ = place(window, corner, clear_of);
}

/// How far from its place a window starts that comes into a corner, in logical pixels (from the
/// side of the screen that corner is at), and how long it takes to be there.
const COMES_FROM: f64 = 24.0;
const COMES_IN: Duration = Duration::from_millis(200);
/// How often it is moved on its way.
const COMES_BY: Duration = Duration::from_millis(8);
/// How far along its way a window that comes into its place is seen wholly.
const SEEN_BY: f64 = 0.6;

/// Which placing of each of the windows that are put in a corner is the last: a window on its way
/// into its place stops where another placing put it.
static PLACINGS: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

fn placings(window: &WebviewWindow) -> &'static AtomicU64 {
    &PLACINGS[usize::from(window.label() == SMALL)]
}

/// Shows a window in a corner (see `place_in_corner`), beside the tasks. One that was not on
/// screen comes into its place from the side, quickly and slowing down, and fades in as it sets
/// out (it is seen wholly a little past halfway): it is noticed, and it is seen where it came to. One that is there already (something else is said, another capture is
/// shown) stays where it is. Nor does it move for a user who has the system move nothing.
pub fn appear_in_corner(window: &WebviewWindow, corner: Corner, clear_of: Option<u32>) {
    let there = window.is_visible().unwrap_or(false);
    let Some((to, placing)) = place(window, corner, clear_of) else {
        return show_beside_tasks(window);
    };
    if there || !perora_core::platform::animations_enabled() {
        return show_beside_tasks(window);
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let from = (COMES_FROM * scale).round() * if corner.is_right() { 1.0 } else { -1.0 };
    let _ = window.set_position(PhysicalPosition::new(to.x + from as i32, to.y));
    show_beside_tasks_seen(window, false);

    let window = window.clone();
    std::thread::spawn(move || {
        let began = Instant::now();
        loop {
            let done = (began.elapsed().as_secs_f64() / COMES_IN.as_secs_f64()).min(1.0);
            if placings(&window).load(Ordering::Acquire) != placing {
                return;
            }
            let left = (1.0 - done).powi(3);
            let step = PhysicalPosition::new(to.x + (from * left).round() as i32, to.y);
            // Moved where the windows are placed from, and only if it is still this placing
            // then: placed again meanwhile, somewhere else perhaps, that is where it is.
            // (Seen more and more on the way, and wholly, as a window like any other, at its end.)
            let seen = (done / SEEN_BY).min(1.0);
            let seen = (done < 1.0).then(|| ((1.0 - (1.0 - seen).powi(2)) * 255.0).round() as u8);
            let moving = window.clone();
            let _ = window.run_on_main_thread(move || {
                if placings(&moving).load(Ordering::Acquire) == placing {
                    let _ = moving.set_position(step);
                    #[cfg(windows)]
                    if let Ok(handle) = moving.hwnd() {
                        perora_core::platform::set_opacity(
                            perora_core::platform::WindowId(handle.0 as isize),
                            seen,
                        );
                    }
                }
            });
            if done >= 1.0 {
                return;
            }
            std::thread::sleep(COMES_BY);
        }
    });
}

/// Puts a window in its corner, and says where that is and which placing of the window this was.
/// `None` when the screen cannot be asked.
fn place(
    window: &WebviewWindow,
    corner: Corner,
    clear_of: Option<u32>,
) -> Option<(PhysicalPosition<i32>, u64)> {
    let layout = screen_capture().layout().ok()?;
    let monitor = layout.primary().or(layout.monitors.first())?;
    let size = window.outer_size().ok()?;
    let margin = (MARGIN * monitor.scale).round() as i32;
    let raised = clear_of.map_or(0, |tall| {
        tall as i32 + (ABOVE * monitor.scale).round() as i32
    });
    let area = monitor.work_area;
    let to = PhysicalPosition::new(
        if corner.is_right() {
            area.right() - size.width as i32 - margin
        } else {
            area.x + margin
        },
        if corner.is_bottom() {
            area.bottom() - size.height as i32 - margin - raised
        } else {
            area.y + margin + raised
        },
    );
    let placing = placings(window).fetch_add(1, Ordering::AcqRel) + 1;
    let _ = window.set_position(to);
    Some((to, placing))
}

/// Throws the capture away and hides both windows: `id` when it is still the one previewed, whichever
/// is previewed when no id is given. Also what closing a window means.
///
/// Not while it is being uploaded: the upload still needs it, to show it again should it fail.
pub fn discard(app: &AppHandle, id: Option<u64>) {
    let perora = app.state::<Perora>();
    {
        let mut pending = perora.preview.lock();
        match &*pending {
            Some(current)
                if id.is_none_or(|id| id == current.id)
                    && !matches!(current.progress, Progress::Uploading) =>
            {
                *pending = None;
            }
            _ => return,
        }
    }
    hide(app);
    changed(app);
}

/// Discarding as the user does it (the button, the key, closing the window): when they asked for
/// that, the image is taken off the clipboard as well. Only the image: whatever came to the
/// clipboard after it stays.
pub fn discard_by_user(app: &AppHandle, id: Option<u64>) {
    let perora = app.state::<Perora>();
    // Whether the image this preview put on the clipboard is still what is there; and what was
    // saved of it as it was taken.
    let (on_clipboard, saved) = match &*perora.preview.lock() {
        Some(current)
            if id.is_none_or(|id| id == current.id)
                && !matches!(current.progress, Progress::Uploading) =>
        {
            perora.preview.was_discarded(current.id);
            (current.on_clipboard(), current.saved.clone())
        }
        _ => (false, Vec::new()),
    };
    discard(app, id);
    if on_clipboard && perora.setting(|settings| settings.clear_clipboard_on_discard) {
        let _ = app.clipboard().clear();
        forget_copied(app, None);
    }
    if perora.setting(|settings| settings.delete_saved_on_discard) {
        take_away(&saved);
    }
}

/// Saves a capture as it was taken, as the user has captures saved: every kind of file they save
/// as, all by one name, in the folder for saving (never asking where: the user wants every capture
/// there). `png` is the capture as a PNG already, when there is one. The files written; refused with
/// the reason when one could not be (then none is there).
pub fn save_as_taken(
    app: &AppHandle,
    frame: &Frame,
    png: Option<&[u8]>,
) -> Result<Vec<PathBuf>, String> {
    let perora = app.state::<Perora>();
    // (As "save" has it, or as the user would rather have these saved.)
    let (kinds, how) = perora.setting(|settings| settings.as_taken());
    let directory = perora
        .setting(|settings| save_directory(app, settings))
        .ok_or("no folder to save to")?;
    // Every kind is made before any is written: one that cannot be made leaves no file behind.
    let files = kinds
        .iter()
        .map(|kind| {
            let how = Output {
                format: *kind,
                ..how
            };
            Ok((
                kind.name(),
                match png {
                    Some(png) if how.keeps(frame.width(), frame.height()) => png.to_vec(),
                    _ => frame.to_file(how)?,
                },
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let files: Vec<_> = files
        .iter()
        .map(|(kind, file)| (*kind, file.as_slice()))
        .collect();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let name = format!("Perora {}", perora_core::platform::local_timestamp());
    let first = write_each(&directory, &name, &files, false)
        .map_err(|not_all| not_all.error.to_string())?;
    Ok(files
        .iter()
        .map(|(kind, _)| first.with_extension(kind))
        .collect())
}

/// Keeps in mind what a capture was saved as, as it was taken, for discarding to take away. When it
/// was discarded meanwhile (and the user has discarding take that away), it is taken away at once.
/// Whether it is still there.
pub fn saved_as_taken(app: &AppHandle, id: u64, files: &[PathBuf]) -> bool {
    let perora = app.state::<Perora>();
    // (Looked at with the preview held: the user discarding it says so before it is gone, and
    // takes what is kept of it here then. Being discarded is what counts, gone or not yet.)
    let discarded = {
        let mut pending = perora.preview.lock();
        let discarded = perora.preview.is_discarded(id);
        if let Some(pending) = pending
            .as_mut()
            .filter(|pending| pending.id == id && !discarded)
        {
            pending.saved = files.to_vec();
        }
        discarded
    };
    if discarded && perora.setting(|settings| settings.delete_saved_on_discard) {
        take_away(files);
        return false;
    }
    true
}

/// Takes away files Perora saved, as far as they are still there.
fn take_away(files: &[PathBuf]) {
    for file in files {
        if let Err(error) = std::fs::remove_file(file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("save: {} was not taken away: {error}", file.display());
        }
    }
}

/// Tells the windows to ask for the state again: the capture, its progress, the language or what
/// can be done with a capture (a setting, who is signed in) changed.
pub fn changed(app: &AppHandle) {
    let _ = app.emit("preview-changed", ());
}

/// The image a preview window shows: `/capture/<id>/thumbnail.png` and `/capture/<id>/image.png` of the
/// capture being previewed. Any other id is not found.
pub fn image(perora: &Perora, path: &[&str]) -> Option<(&'static str, Arc<Vec<u8>>)> {
    let pending = perora.preview.lock();
    let pending = pending.as_ref()?;
    match path {
        [id, "thumbnail.png"] if id.parse() == Ok(pending.id) => {
            Some(("image/png", pending.thumbnail.clone()))
        }
        [id, "image.png"] if id.parse() == Ok(pending.id) => {
            Some(("image/png", pending.png.clone()))
        }
        _ => None,
    }
}

#[tauri::command]
pub fn preview_state(app: AppHandle) -> View {
    let perora = app.state::<Perora>();
    let language = perora.language();
    let capture = perora.preview.lock().as_ref().map(|pending| CaptureView {
        id: pending.id,
        width: pending.frame.width(),
        height: pending.frame.height(),
        progress: match pending.progress {
            Progress::Waiting => "waiting",
            Progress::Uploading => "uploading",
            Progress::Failed(_) => "failed",
        },
        message: match &pending.progress {
            Progress::Failed(message) => Some(message.clone()),
            _ => None,
        },
        copied: pending.on_clipboard(),
        copy_failure: pending.copied.clone().and_then(Result::err),
        edits: pending.edits.clone(),
        copy_is_old: pending.copied_edits != pending.edits,
        copied_as: LeavesView {
            format: pending.copied_as.format.name(),
            formats: vec![pending.copied_as.format.name()],
            fit: pending.copied_as.fit.unwrap_or(0),
        },
        once: pending.once.clone(),
        thumbnail: pending.thumbnail_made,
        kept_width: size_after(
            &pending.edits,
            pending.frame.width(),
            pending.frame.height(),
        )
        .0,
        kept_height: size_after(
            &pending.edits,
            pending.frame.width(),
            pending.frame.height(),
        )
        .1,
    });
    View {
        language: language.code().to_owned(),
        texts: language.texts(),
        sample: capture.is_none() && perora.preview.sample.load(Ordering::Relaxed),
        capture,
        offers_upload: perora.uploads(),
        accent: perora_core::platform::accent_colour()
            .map(|(red, green, blue)| format!("#{red:02x}{green:02x}{blue:02x}")),
        formats: Format::ALL.iter().map(|format| format.name()).collect(),
        fits: FITS.to_vec(),
        tours_seen: perora.setting(|settings| settings.tours_seen()),
        default_hider: perora.setting(|settings| settings.default_hider.name()),
        leaves: Leaves::ALL
            .into_iter()
            .map(|leaves| {
                let output = perora.setting(|settings| settings.output(leaves));
                let kinds = match leaves {
                    Leaves::Save => perora.setting(|settings| settings.save_formats.clone()),
                    _ => vec![output.format],
                };
                (
                    leaves.name(),
                    LeavesView {
                        format: output.format.name(),
                        formats: kinds.iter().map(|kind| kind.name()).collect(),
                        fit: output.fit.unwrap_or(0),
                    },
                )
            })
            .collect(),
    }
}

/// The fonts the system has, by the names of their families: what something can be written in.
#[tauri::command(async)]
pub fn preview_fonts() -> Vec<String> {
    perora_core::platform::font_families()
}

/// The capture's image as a file, for the window that draws on it: handed over as bytes, so that the
/// window can read back what it draws over it (an image it merely shows from another address it
/// cannot).
#[tauri::command]
pub fn preview_image(app: AppHandle, id: u64) -> Result<Response, ()> {
    match &*app.state::<Perora>().preview.lock() {
        Some(pending) if pending.id == id => Ok(Response::new(pending.png.to_vec())),
        _ => Err(()),
    }
}

/// What is drawn on the capture, whenever that changed: the whole list. The window that draws keeps
/// it too; here it is for a window that asks anew, and to know that an image made of the capture
/// before is of a drawing that is no longer. Refused (`tooMuch`) when it is more than a drawing is.
#[tauri::command]
pub fn preview_edits(app: AppHandle, id: u64, edits: Vec<Edit>) -> Result<(), &'static str> {
    let perora = app.state::<Perora>();
    let tell = match perora.preview.lock().as_mut() {
        Some(pending) if pending.id == id && !matches!(pending.progress, Progress::Uploading) => {
            if !is_a_drawing(&edits) {
                return Err(TOO_MUCH);
            }
            let size =
                |edits: &[Edit]| size_after(edits, pending.frame.width(), pending.frame.height());
            // The windows say whether the clipboard has the capture as it is drawn now, and how
            // large it would leave: told when one of those turns.
            let was = (pending.copied_edits != pending.edits, size(&pending.edits));
            let is = (pending.copied_edits != edits, size(&edits));
            pending.edits = edits;
            was != is
        }
        // About a capture that is gone, or on its way already: nothing to tell.
        _ => false,
    };
    if tell {
        changed(&app);
    }
    Ok(())
}

/// Uploads the capture the window shows. The preview closes at once; the upload ends in a
/// notification, with the link on the clipboard, or brings the preview back to say why it failed.
///
/// Every command names the capture it is about: a window may still show the one before while the next
/// is already here, and what the user clicked on is what they saw. The window that is drawn in hands
/// over what is drawn and the image it made of it (see `asked`), so that what leaves is what it
/// shows; refused, and nothing uploaded, when the drawing is more than a drawing is (`tooMuch`) or
/// there is no image of the capture as it is drawn on (`notMade`).
#[tauri::command(async)]
pub fn preview_upload(app: AppHandle, request: Request<'_>) -> Result<(), &'static str> {
    let Asked { id, drawing, once } = asked(request.body())?;
    let perora = app.state::<Perora>();
    let leaving = {
        let mut pending = perora.preview.lock();
        match pending.as_mut() {
            // Once at a time.
            Some(pending)
                if pending.id == id
                    && matches!(pending.progress, Progress::Waiting | Progress::Failed(_)) =>
            {
                pending.touched = true;
                take(pending, drawing)?;
                let leaving = pending.leaving();
                if leaving.is_some() {
                    pending.progress = Progress::Uploading;
                }
                leaving
            }
            _ => return Ok(()),
        }
    };
    // (Said with the state let go of: showing a window waits for the thread the windows live on,
    // which may be waiting for the state.)
    let Some(leaving) = leaving else {
        return Err(not_made(&app));
    };
    hide(&app);
    changed(&app);

    std::thread::spawn(move || {
        let perora = app.state::<Perora>();
        let language = perora.language();
        let _uploading = crate::recovery::Uploading::begin();
        // What went wrong, as the user is told it.
        // As the user has captures uploaded, or asked for this once: the kind of file, and how
        // large at most.
        let how = once.output(&app, Leaves::Upload);
        let outcome = leaving
            .file(how)
            .map_err(|detail| language.capture_failed(&detail))
            .and_then(|file| {
                shots::upload(&perora.account, &file).map_err(|error| {
                    if matches!(error, shots::ApiError::SignedOut) {
                        crate::show_status(&app, None);
                    }
                    language.upload_failed(&error)
                })
            });

        if outcome.is_ok() {
            // The main window lists the uploads.
            crate::main_window::changed(&app);
        }
        let said = match &outcome {
            Ok(shot) => match app.clipboard().write_text(shot.url.as_str()) {
                Ok(()) => language.uploaded(),
                Err(error) => {
                    eprintln!("clipboard: the link was not put there: {error}");
                    language.uploaded_but_not_copied(&shot.url)
                }
            },
            Err(said) => said.clone(),
        };

        // Its preview is still there, hidden, unless a new capture took its place meanwhile (nothing
        // else removes one that is uploading). Done with it when the upload worked; back on screen
        // with the reason when it did not, and only a notification when there is no preview to show.
        let shown_again = {
            let mut pending = perora.preview.lock();
            let current = matches!(&*pending, Some(pending) if pending.id == id);
            match (&outcome, current) {
                (Ok(_), true) => {
                    *pending = None;
                    false
                }
                (Err(_), true) => {
                    if let Some(pending) = pending.as_mut() {
                        pending.progress = Progress::Failed(said.clone());
                    }
                    true
                }
                (_, false) => false,
            }
        };
        if shown_again {
            show(&app);
        } else {
            let lasting = match (&outcome, &said) {
                (Ok(_), said) if *said == language.uploaded() => Lasting::Brief,
                _ => Lasting::Long,
            };
            crate::notify(&app, lasting, said);
        }
        changed(&app);
    });
    Ok(())
}

/// Saves the image as a file in the user's pictures, in a folder of Perora's own, named after the
/// moment, and closes the preview; a notification says where it went. The preview stays, with the
/// reason, when the file could not be written. A capture that was drawn on is saved as drawn (the
/// image the window made of it as it asked to save), and that is then also what the clipboard
/// holds, in place of the capture as it was taken.
#[tauri::command(async)]
pub fn preview_save(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let Asked { id, drawing, once } = asked(request.body())?;
    let perora = app.state::<Perora>();
    // Whether the capture is still the one previewed, and not on its way to the server.
    let still_there = |pending: &Option<Pending>| matches!(pending, Some(pending) if pending.id == id && !matches!(pending.progress, Progress::Uploading));
    let leaving = {
        let mut pending = perora.preview.lock();
        if !still_there(&pending) {
            return Ok(());
        }
        let Some(pending) = pending.as_mut() else {
            return Ok(());
        };
        pending.touched = true;
        take(pending, drawing)?;
        pending.leaving()
    };
    let Some(leaving) = leaving else {
        return Err(not_made(&app).into());
    };

    // As the user has captures saved, or asked for this once: the kind of file, and how large at
    // most.
    // Several kinds at once, when asked: one file of each, all by one name.
    let how = once.output(&app, Leaves::Save);
    let kinds = once.saved_as(&app);
    let made = |kind: Format| {
        leaving.file(Output {
            format: kind,
            ..how
        })
    };
    let kind = kinds[0].name();

    let name = format!("Perora {}", perora_core::platform::local_timestamp());
    let directory = perora
        .setting(|settings| save_directory(&app, settings))
        .ok_or("no folder to save to")?;

    let (folder, name, replaces) = if perora.setting(|settings| settings.ask_where_to_save) {
        // The user says where, each time. Giving up on the question is not a failure: the preview
        // just stays.
        let asked = app
            .dialog()
            .file()
            .set_directory(&directory)
            .set_file_name(format!("{name}.{kind}"))
            .add_filter(kind.to_uppercase(), &[kind])
            .blocking_save_file();
        let Some(path) = asked.and_then(|path| path.into_path().ok()) else {
            return Ok(());
        };
        // Here the user chose the name, and to replace what has it: every kind goes by that
        // name. That question was about one file, though. What else would be written over (a
        // file of another kind by the same name) is asked about as well, and nothing is saved
        // when the answer is no: the preview just stays.
        let (folder, name) = named(&path, &directory);
        let there: Vec<String> = kinds
            .iter()
            .map(|kind| folder.join(format!("{name}.{}", kind.name())))
            .filter(|file| {
                !file.as_os_str().eq_ignore_ascii_case(path.as_os_str()) && file.exists()
            })
            .filter_map(|file| Some(file.file_name()?.to_string_lossy().into_owned()))
            .collect();
        if !there.is_empty() {
            let language = perora.language();
            let replaces = app
                .dialog()
                .message(language.there_already(&there.join(", ")))
                .title("Perora")
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    language.replace(),
                    language.keep(),
                ))
                .blocking_show();
            if !replaces {
                return Ok(());
            }
        }
        // (Gone while the user was asked: nothing of it is written any more. And drawn on
        // meanwhile, the image at hand is of a drawing that is no longer: not written either.)
        match &*perora.preview.lock() {
            pending if !still_there(pending) => return Ok(()),
            Some(pending) if pending.edits != leaving.edits => return Err(NOT_MADE.into()),
            _ => {}
        }
        (folder, name, true)
    } else {
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        (directory, name, false)
    };
    // Every kind is made before any is written: one that cannot be made leaves no file behind.
    // (And made once it is known where to: the question is not kept waiting for it.)
    let files = kinds
        .iter()
        .map(|kind| Ok((kind.name(), made(*kind)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let files: Vec<_> = files
        .iter()
        .map(|(kind, file)| (*kind, file.as_slice()))
        .collect();
    let path = match write_each(&folder, &name, &files, replaces) {
        Ok(path) => path,
        // Some of them are there (written over what the user chose to replace, they are not
        // taken back): a notice says which, and the preview stays, with why the rest is not.
        Err(NotAll {
            first: Some(first),
            kind,
            error,
        }) => {
            let (kind, language) = (kind.to_uppercase(), perora.language());
            let said = language.saved_but_not(&first.display().to_string(), &kind);
            // (The window says it too, in a few words: a notice is gone in a moment.)
            let name = first.file_name().unwrap_or_default().to_string_lossy();
            let refused = language.not_all_saved(&kind, &error.to_string(), &name);
            crate::notify_of_file(&app, Lasting::Long, said, first);
            return Err(refused);
        }
        Err(NotAll { error, .. }) => return Err(error.to_string()),
    };

    // The clipboard holds the capture as it looked before what is drawn on it now: the one that
    // was saved takes its place there.
    if leaving.copied && leaving.copy_is_old {
        match leaving.pixels() {
            // (As captures are copied: the kind of file, and how large at most.)
            Ok(frame) => {
                let how = Once::default().output(&app, Leaves::Copy);
                if let Err(error) = to_clipboard(&app, &frame, how) {
                    eprintln!("clipboard: the saved image was not put there: {error}");
                }
            }
            Err(error) => eprintln!("clipboard: the saved image could not be read: {error}"),
        }
    }

    discard(&app, Some(id));
    // (A click on what says so shows the file in its folder.)
    let (language, shown) = (perora.language(), path.display().to_string());
    crate::notify_of_file(
        &app,
        Lasting::Brief,
        match kinds.len() {
            1 => language.saved(&shown),
            several => language.saved_several(&shown, several - 1),
        },
        path,
    );
    Ok(())
}

/// Where "save" writes: the folder the user chose, or one of Perora's own in their pictures. `None`
/// when the system has no pictures folder to offer.
pub fn save_directory(app: &AppHandle, settings: &crate::settings::Settings) -> Option<PathBuf> {
    match &settings.save_directory {
        Some(directory) => Some(directory.clone()),
        None => Some(app.path().picture_dir().ok()?.join("Perora")),
    }
}

/// The folder and the name of the files to write, from the file the user named: without what the
/// name ends in, when that is how a picture ends (each file ends in its own kind, whatever was
/// typed). Any other ending is part of the name.
fn named(path: &Path, otherwise: &Path) -> (PathBuf, String) {
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => otherwise,
    };
    let of_a_picture = path.extension().is_some_and(|ending| {
        ["png", "jpg", "jpeg", "webp"]
            .iter()
            .any(|one| ending.eq_ignore_ascii_case(one))
    });
    let name = if of_a_picture {
        path.file_stem()
    } else {
        path.file_name()
    };
    (
        folder.to_owned(),
        name.unwrap_or_default().to_string_lossy().into_owned(),
    )
}

/// Not every file was written: which was first, when some are there all the same, the kind that
/// was not, and why.
#[derive(Debug)]
struct NotAll {
    first: Option<PathBuf>,
    kind: &'static str,
    error: std::io::Error,
}

/// Writes one file of each kind into the folder, all by one name, and says where the first went.
///
/// Over what is there with `replaces`; what was written before one could not be stays then.
/// Otherwise never over a file: the name is the first that no file of any of the kinds has (a
/// second capture in the same second becomes `Ã¢â‚¬Â¦ (2)`), and should one not be written, the ones
/// that were are taken away again.
fn write_each(
    folder: &Path,
    name: &str,
    files: &[(&'static str, &[u8])],
    replaces: bool,
) -> Result<PathBuf, NotAll> {
    use std::io::Write;

    let named = |name: &str| -> Vec<PathBuf> {
        files
            .iter()
            .map(|(kind, _)| folder.join(format!("{name}.{kind}")))
            .collect()
    };
    if replaces {
        let paths = named(name);
        for (at, ((kind, contents), path)) in files.iter().zip(&paths).enumerate() {
            if let Err(error) = std::fs::write(path, contents) {
                let first = (at > 0).then(|| paths[0].clone());
                return Err(NotAll { first, kind, error });
            }
        }
        return Ok(paths.into_iter().next().unwrap_or_default());
    }

    'names: for attempt in 1..=99 {
        let paths = match attempt {
            1 => named(name),
            n => named(&format!("{name} ({n})")),
        };
        if paths.iter().any(|path| path.exists()) {
            continue;
        }
        for (at, ((kind, contents), path)) in files.iter().zip(&paths).enumerate() {
            let created = std::fs::File::create_new(path);
            // (Taken by another meanwhile: that one is not this capture's to take away.)
            let ours = if created.is_ok() { at + 1 } else { at };
            let Err(error) = created.and_then(|mut file| file.write_all(contents)) else {
                continue;
            };
            for written in &paths[..ours] {
                let _ = std::fs::remove_file(written);
            }
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                continue 'names;
            }
            return Err(NotAll {
                first: None,
                kind,
                error,
            });
        }
        return Ok(paths.into_iter().next().unwrap_or_default());
    }
    Err(NotAll {
        first: None,
        kind: files.first().map_or("", |(kind, _)| kind),
        error: std::io::ErrorKind::AlreadyExists.into(),
    })
}

/// One copying at a time: the image that is asked for last is the one that stays.
static COPYING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Puts the capture on the clipboard again, as it is drawn on now, and keeps the preview: for after
/// drawing on it, or after something else was copied. Nothing is done when the clipboard holds just
/// that already. Refused with the reason when the image could not be put there.
#[tauri::command(async)]
pub fn preview_copy(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let Asked { id, drawing, once } = asked(request.body())?;
    let perora = app.state::<Perora>();
    // As the user has captures copied, or asked for this once.
    let copy_as = once.output(&app, Leaves::Copy);
    let _copying = COPYING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Whether the capture is still the one previewed, and not on its way to the server.
    let still_there = |pending: &Option<Pending>| matches!(pending, Some(pending) if pending.id == id && !matches!(pending.progress, Progress::Uploading));
    let leaving = {
        let mut pending = perora.preview.lock();
        if !still_there(&pending) {
            return Ok(());
        }
        let Some(pending) = pending.as_mut() else {
            return Ok(());
        };
        pending.touched = true;
        take(pending, drawing)?;
        // What the clipboard holds is this capture as it looks now, made into what is asked for
        // now, already: nothing to do.
        if pending.on_clipboard()
            && pending.copied_edits == pending.edits
            && pending.copied_as == copy_as
        {
            None
        } else {
            Some(pending.leaving())
        }
    };
    let leaving = match leaving {
        // (The drawing may have come with this request: the windows say what the clipboard has.)
        None => {
            changed(&app);
            return Ok(());
        }
        Some(None) => return Err(not_made(&app).into()),
        Some(Some(leaving)) => leaving,
    };
    // The pixels of the file the window made: reading them takes a moment.
    let frame = leaving.pixels()?;
    // The capture may have been discarded or replaced meanwhile, and then nothing of it is to come
    // back to the clipboard.
    if !still_there(&perora.preview.lock()) {
        return Ok(());
    }

    let copied = to_clipboard(&app, &frame, copy_as).inspect_err(|error| {
        eprintln!("clipboard: the image was not put there: {error}");
    })?;

    match perora
        .preview
        .lock()
        .as_mut()
        .filter(|pending| pending.id == id)
    {
        Some(pending) => {
            pending.copied = Some(Ok(copied));
            pending.copied_edits = leaving.edits.clone();
            pending.copied_as = copy_as;
        }
        // Gone in the moment the image was written: it is not left on the clipboard.
        None => {
            if copied.is_some() && clipboard_sequence() == copied {
                let _ = app.clipboard().clear();
                forget_copied(&app, None);
            }
        }
    }
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn preview_discard(app: AppHandle, id: u64) {
    discard_by_user(&app, Some(id));
}

/// Moves the preview from the small window to the large one.
#[tauri::command]
pub fn preview_enlarge(app: AppHandle, id: u64) {
    let app = &app;
    match app.state::<Perora>().preview.lock().as_mut() {
        Some(pending) if pending.id == id => {
            pending.touched = true;
            pending.large = true;
        }
        _ => return,
    }
    show_large(app);
}

/// The user chose, in one of the windows, what the capture leaves as this once: remembered, and
/// the other window is told. (Something was done about the capture: it is not given up on its
/// own after that.)
#[tauri::command]
pub fn preview_choose(app: AppHandle, id: u64, once: serde_json::Map<String, serde_json::Value>) {
    match app.state::<Perora>().preview.lock().as_mut() {
        Some(pending) if pending.id == id => {
            pending.touched = true;
            pending.once = once;
        }
        _ => return,
    }
    changed(&app);
}

/// How long the large window is waited for to have drawn a capture it is to be shown with.
const DRAWN_WITHIN: Duration = Duration::from_millis(400);

/// The large window has drawn that capture (or could not, and has nothing more to draw of it).
#[tauri::command]
pub fn preview_drawn(app: AppHandle, id: u64) {
    app.state::<Perora>()
        .preview
        .drawn
        .store(id, Ordering::Relaxed);
}

/// How long the large window takes to be seen wholly, and in how many steps.
const FADE: (Duration, u32) = (Duration::from_millis(160), 12);

/// The large window, in place of the small one. It fades in: the system opens a window the way
/// windows open only the first time it is shown, and at once every time after.
///
/// Out of sight, the window is shown and made to be seen through wholly (in that order: showing a
/// window sets its styles anew), put back in sight, and then seen more and more. One that is on
/// screen already just comes to the front. At once where the system is asked not to animate.
fn show_large(app: &AppHandle) {
    if let Some(small) = app.get_webview_window(SMALL) {
        let _ = small.hide();
    }
    let Some(large) = app.get_webview_window(LARGE) else {
        return;
    };
    let perora = app.state::<Perora>();
    if large.is_visible().unwrap_or(false) {
        // (A fading in that is going on goes on.)
        out_of_sight(app, false);
        let _ = large.unminimize();
        let _ = large.set_focus();
        return;
    }
    let turn = perora.preview.appearing.fetch_add(1, Ordering::Relaxed) + 1;
    let fades = cfg!(windows) && perora_core::platform::animates();
    let (shown, during) = (large.clone(), app.clone());
    let _ = large.run_on_main_thread(move || {
        // (Hidden again since this was asked for: not shown after all.)
        if during
            .state::<Perora>()
            .preview
            .appearing
            .load(Ordering::Relaxed)
            != turn
        {
            return;
        }
        #[cfg(windows)]
        let window = shown
            .hwnd()
            .ok()
            .map(|handle| perora_core::platform::WindowId(handle.0 as isize));
        #[cfg(windows)]
        if let Some(window) = window
            && fades
        {
            perora_core::platform::set_cloaked(window, true);
        }
        let _ = shown.show();
        let _ = shown.unminimize();
        #[cfg(windows)]
        if let Some(window) = window {
            perora_core::platform::set_opacity(window, fades.then_some(0));
            perora_core::platform::set_cloaked(window, false);
        }
        let _ = shown.set_focus();
    });
    if !fades {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let perora = app.state::<Perora>();
        let still = || perora.preview.appearing.load(Ordering::Relaxed) == turn;
        // (Asked of the window after it was shown: this waits for that.)
        #[cfg(windows)]
        {
            let Some(window) = app
                .get_webview_window(LARGE)
                .and_then(|large| large.hwnd().ok())
                .map(|handle| perora_core::platform::WindowId(handle.0 as isize))
            else {
                return;
            };
            for step in 1..=FADE.1 {
                std::thread::sleep(FADE.0 / FADE.1);
                if !still() {
                    return;
                }
                // (Quickly at first, then settling.)
                let along = f64::from(step) / f64::from(FADE.1);
                let seen = 1.0 - (1.0 - along) * (1.0 - along);
                perora_core::platform::set_opacity(window, Some((seen * 255.0).round() as u8));
            }
            if still() {
                perora_core::platform::set_opacity(window, None);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{DISCARDED_KEPT, State, asked, named, take_away, write_each};
    use std::path::Path;
    use tauri::ipc::InvokeBody;

    #[test]
    fn what_a_window_asks_names_the_capture_and_from_the_large_one_brings_the_drawing_and_its_image()
     {
        let small = asked(&InvokeBody::Json(serde_json::json!({"id": 7}))).unwrap();
        assert_eq!((small.id, small.drawing.is_none()), (7, true));

        let named = br##"{"id": 8, "edits": [{"kind": "crop", "from": [0, 0], "to": [4, 4]}]}"##;
        let mut bytes = (named.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(named);
        bytes.extend_from_slice(b"the image");
        let large = asked(&InvokeBody::Raw(bytes.clone())).unwrap();
        let (edits, image) = large.drawing.unwrap();
        assert_eq!(
            (large.id, edits.len(), image.as_slice()),
            (8, 1, &b"the image"[..])
        );

        // Shorter than it says it is, not saying how long it is, or naming nothing: not asked.
        assert!(asked(&InvokeBody::Raw(bytes[..20].to_vec())).is_err());
        assert!(asked(&InvokeBody::Raw(vec![1, 2])).is_err());
        assert!(asked(&InvokeBody::Raw(vec![255, 255, 255, 255, b'{', b'}'])).is_err());
        assert!(asked(&InvokeBody::Json(serde_json::json!({}))).is_err());
    }

    #[test]
    fn a_saved_capture_never_replaces_a_file_that_is_there() {
        let directory = std::env::temp_dir().join(format!("perora-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let write_new = |kind: &'static str, contents: &[u8]| {
            let files = [(kind, contents)];
            write_each(&directory, "Perora 2026-10-12 023455", &files, false)
        };
        let first = write_new("png", b"one").unwrap();
        let second = write_new("png", b"two").unwrap();
        let other = write_new("webp", b"three").unwrap();
        assert_eq!(other.file_name().unwrap(), "Perora 2026-10-12 023455.webp");

        assert_eq!(first.file_name().unwrap(), "Perora 2026-10-12 023455.png");
        assert_eq!(
            second.file_name().unwrap(),
            "Perora 2026-10-12 023455 (2).png"
        );
        assert_eq!(std::fs::read(first).unwrap(), b"one");
        assert_eq!(std::fs::read(second).unwrap(), b"two");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn every_kind_is_saved_by_one_name_over_what_is_there_only_when_the_user_named_it() {
        let directory = std::env::temp_dir().join(format!("perora-kinds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let read = |name: &str| std::fs::read(directory.join(name)).unwrap();

        // Not named by the user: a name no file of any of the kinds has, the same for all.
        std::fs::write(directory.join("shot.png"), b"old").unwrap();
        let files = [("png", &b"png"[..]), ("jpg", &b"jpg"[..])];
        let first = write_each(&directory, "shot", &files, false).unwrap();
        assert_eq!(first.file_name().unwrap(), "shot (2).png");
        assert_eq!(
            (read("shot.png"), read("shot (2).png"), read("shot (2).jpg")),
            (b"old".to_vec(), b"png".to_vec(), b"jpg".to_vec())
        );
        assert!(!directory.join("shot.jpg").exists());
        std::fs::write(directory.join("only.jpg"), b"old").unwrap();
        let first = write_each(&directory, "only", &files, false).unwrap();
        assert_eq!(first.file_name().unwrap(), "only (2).png");
        assert!(!directory.join("only.png").exists() && directory.join("only (2).jpg").exists());

        // A name is passed over when any of the kinds has it. And with nowhere to write, that is
        // said of the first kind, with no file to point at.
        std::fs::create_dir_all(directory.join("none (2).jpg")).unwrap();
        std::fs::write(directory.join("none.png"), b"old").unwrap();
        let taken = [
            ("png", &b"png"[..]),
            ("jpg", &b"jpg"[..]),
            ("webp", &b"webp"[..]),
        ];
        let again = write_each(&directory, "none", &taken, false).unwrap();
        assert_eq!(again.file_name().unwrap(), "none (3).png");
        let nowhere = directory.join("gone");
        let not_all = write_each(&nowhere, "none", &files, false).unwrap_err();
        assert!(not_all.first.is_none() && not_all.kind == "png");

        // Named by the user: each kind by that name, over what is there.
        std::fs::write(directory.join("shot.jpg"), b"old").unwrap();
        let first = write_each(&directory, "shot", &files, true).unwrap();
        assert_eq!(first.file_name().unwrap(), "shot.png");
        assert_eq!(
            (read("shot.png"), read("shot.jpg")),
            (b"png".to_vec(), b"jpg".to_vec())
        );

        // One that cannot be written, named by the user: said with the first, which is there.
        std::fs::create_dir_all(directory.join("held.jpg")).unwrap();
        let not_all = write_each(&directory, "held", &files, true).unwrap_err();
        assert_eq!(
            (not_all.first.unwrap().file_name().unwrap(), not_all.kind),
            (std::ffi::OsStr::new("held.png"), "jpg")
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn the_name_the_user_gave_loses_its_ending_only_when_that_is_how_a_picture_ends() {
        let otherwise = Path::new("elsewhere");
        let of = |path: &str| named(Path::new(path), otherwise);
        let folder = Path::new("pictures").to_owned();
        assert_eq!(of("pictures/shot.png"), (folder.clone(), "shot".into()));
        assert_eq!(of("pictures/shot.JPEG"), (folder.clone(), "shot".into()));
        assert_eq!(of("pictures/v1.2.webp"), (folder.clone(), "v1.2".into()));
        assert_eq!(of("pictures/v1.2"), (folder.clone(), "v1.2".into()));
        assert_eq!(of("pictures/shot"), (folder, "shot".into()));
        assert_eq!(of("shot.jpg"), (otherwise.to_owned(), "shot".into()));
    }

    #[test]
    fn the_captures_discarded_lately_are_kept_in_mind_a_few_at_a_time() {
        let state = State::default();
        assert!(!state.is_discarded(1));
        // (Another taken and discarded while the first was still being saved: both count.)
        state.was_discarded(1);
        state.was_discarded(2);
        assert!(state.is_discarded(1) && state.is_discarded(2));
        for id in 3..3 + DISCARDED_KEPT as u64 {
            state.was_discarded(id);
        }
        assert!(!state.is_discarded(1) && !state.is_discarded(2));
        assert!(state.is_discarded(2 + DISCARDED_KEPT as u64));
    }

    #[test]
    fn what_is_saved_as_taken_is_named_as_saving_names_it_each_kind_beside_the_first() {
        let directory =
            std::env::temp_dir().join(format!("perora-as-taken-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let files: [(&'static str, &[u8]); 2] = [("png", b"a"), ("webp", b"b")];
        // (A second in the same second: each kind by the name with its number.)
        for want in ["Perora 2026-10-16 120000", "Perora 2026-10-16 120000 (2)"] {
            let first = write_each(&directory, "Perora 2026-10-16 120000", &files, false).unwrap();
            let all: Vec<_> = files
                .iter()
                .map(|(kind, _)| first.with_extension(kind))
                .collect();
            assert_eq!(
                all,
                [
                    directory.join(format!("{want}.png")),
                    directory.join(format!("{want}.webp"))
                ]
            );
            assert!(all.iter().all(|file| file.exists()));
        }
        take_away(&[
            directory.join("Perora 2026-10-16 120000.png"),
            directory.join("nothing.png"),
        ]);
        assert!(!directory.join("Perora 2026-10-16 120000.png").exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}

#[cfg(test)]
mod small_window {
    use super::small_size;

    #[test]
    fn the_small_window_has_the_shape_of_its_capture() {
        // As wide as there is room for, and as tall as the capture then is.
        assert_eq!(small_size(1920, 1080), (360.0, 89.0 + 189.0));
        assert_eq!(small_size(1000, 1000), (360.0, 89.0 + 336.0));
        assert_eq!(small_size(336, 100), (360.0, 89.0 + 100.0));
        // Taller than a window in a corner should be: as tall as that, and narrower.
        assert_eq!(small_size(780, 1000), (24.0 + 328.0, 89.0 + 420.0));
        // Within bounds: a tower, a strip, and what has no size at all.
        assert_eq!(small_size(1080, 2560), (24.0 + 324.0, 89.0 + 420.0));
        assert_eq!(small_size(5120, 100), (360.0, 89.0 + 48.0));
        assert_eq!(small_size(0, 0), (360.0, 89.0 + 336.0));
    }
}
