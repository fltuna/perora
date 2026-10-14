//! The main window: what Perora is set to do, and who it uploads as. It appears when Perora is started
//! (unless started with `--tray`), and closing it leaves Perora in the notification area: a click on
//! the icon there brings it back, and puts it away again.
//!
//! Like the other windows it is created once, hidden, and holds no state of its own: it asks for
//! everything (`main_state`), is told when something changed (`main-changed`), and what the user sets
//! in it takes effect at once.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use perora_core::output::{FITS, Format, Quality};
use perora_core::platform::accent_colour;
use perora_core::selection::Mode;
use perora_core::shots;
use serde::Serialize;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::notice::{Corner, Lasting, MOST_SECONDS, Occasion, Sound};
use crate::settings::{After, Leaves, Look};
use crate::{Perora, preview, recovery, settings};

/// How many of the user's uploads the account page shows.
const RECENT_UPLOADS: u32 = 8;

pub const LABEL: &str = "main";

/// The page is loaded twice before it is to be seen: once as created, and once more after the web
/// view's browser features were turned off (see `preview::not_a_browser`). Elsewhere, once.
const LOADS_BEFORE_SHOWN: u8 = if cfg!(windows) { 2 } else { 1 };

#[derive(Default)]
pub struct State {
    /// How often the page has finished loading.
    loads: AtomicU8,
    /// The page has drawn itself and can be shown.
    ready: AtomicBool,
    /// The window is to be on screen: asked for before it was ready, it is shown when it is.
    wanted: AtomicBool,
    /// The window went out of the way for a capture started from it, and comes back when that is over.
    aside: AtomicBool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    language: String,
    languages: Vec<LanguageChoice>,
    texts: HashMap<String, String>,
    account: AccountView,
    settings: SettingsView,
    /// The modes the selection screen can start in.
    modes: Vec<&'static str>,
    /// The kinds of file a capture can leave as, the squares it can be made to fit (in pixels a
    /// side), and how good a file that loses something can be.
    formats: Vec<&'static str>,
    fits: Vec<u32>,
    qualities: Vec<&'static str>,
    /// The looks the windows can have.
    looks: Vec<&'static str>,
    /// The corners of the screen a notice can appear in.
    corners: Vec<&'static str>,
    /// The sounds the system comes with, by name: each can be the sound of a notice.
    sounds: Vec<String>,
    shortcut: ShortcutView,
    /// The version this is, as the application was built with it.
    version: String,
    /// A newer version, and what is being done about it.
    update: crate::update::View,
    /// The colour the user chose for their system, as `#rrggbb`.
    accent: Option<String>,
}

#[derive(Serialize)]
struct LanguageChoice {
    code: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountView {
    /// `signedOut`, `signingIn` (the browser is awaited) or `signedIn`.
    state: &'static str,
    /// Who is signed in, when the server could be asked.
    name: Option<String>,
    /// Whether this account may upload; `None` when the server could not be asked.
    may_upload: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutView {
    label: String,
    /// Whether the system gave Perora the key (another application may hold it).
    registered: bool,
    /// The system's own combination for snipping (Win+Shift+S): whether the user wants it to
    /// capture with Perora, whether it does, and whether the system's shell has to start anew
    /// before things are as the user last chose.
    take_snipping: bool,
    snipping_registered: bool,
    shell_is_behind: bool,
    /// The shell is being started anew: not to be asked for again meanwhile.
    shell_restarting: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    upload_immediately: bool,
    enable_upload: bool,
    start_mode: &'static str,
    copy_to_clipboard: bool,
    clear_clipboard_on_discard: bool,
    /// Where "save" writes, whether chosen or not.
    save_directory: String,
    /// Whether that is the folder Perora uses when none was chosen.
    save_directory_is_default: bool,
    ask_where_to_save: bool,
    /// `system`, `light` or `dark`.
    theme: &'static str,
    /// The kind of file a capture leaves as, each way it leaves (`png`, `jpg` or `webp`), and the
    /// square it is made to fit (pixels a side; 0 for as large as it is).
    copy_format: &'static str,
    copy_fit: u32,
    upload_format: &'static str,
    upload_fit: u32,
    save_format: &'static str,
    save_fit: u32,
    /// The kinds of file a capture is saved as, one file of each.
    save_formats: Vec<&'static str>,
    /// How good a file that loses something is: `best`, `high`, `standard` or `light`.
    quality: &'static str,
    /// Whether Perora starts when the user signs in: what the system says, not a setting kept here.
    start_at_sign_in: bool,
    /// Whether it starts in the tray then, without its window. (As it would, while it does not
    /// start at sign-in at all.)
    start_minimized: bool,
    /// How long a notice stays, in seconds; 0 for until it is clicked.
    notice_seconds: u32,
    notice_corner: &'static str,
    /// What becomes of a capture once it is taken: `preview`, `edit` or `copy`.
    after_capture: &'static str,
    /// How long the small preview stays, in seconds; 0 for until something is done with it.
    preview_seconds: u32,
    preview_corner: &'static str,
    /// Each of the sounds: `none`, `custom`, or `system:` and the name of one of the system's.
    capture_sound: String,
    capture_sound_file: Option<String>,
    notice_sound: String,
    /// The file of the user's own that can be that sound, when one was chosen.
    notice_sound_file: Option<String>,
}

/// One of the user's uploads, for the account page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Upload {
    id: String,
    /// The page to share.
    url: String,
    /// The image itself.
    image_url: String,
    width: u32,
    height: u32,
    created_at: String,
}

/// Creates the window, hidden.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let not_a_browser = preview::not_a_browser();
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Perora")
        .inner_size(1200.0, 800.0)
        .min_inner_size(1200.0, 800.0)
        .center()
        .visible(false)
        .devtools(false)
        .zoom_hotkeys_enabled(false)
        .on_page_load(move |window, payload| {
            if payload.event() == PageLoadEvent::Finished && payload.url().scheme() != "about" {
                let main = &window.app_handle().state::<Perora>().main;
                let loads = main.loads.load(Ordering::Relaxed).saturating_add(1);
                main.loads.store(loads, Ordering::Relaxed);
            }
            not_a_browser(window, payload);
        })
        .build()?;
    recovery::watch(&window);
    Ok(())
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Puts the window on screen, in front, with the keyboard; as soon as it can be, when its page is not
/// there yet.
pub fn open(app: &AppHandle) {
    if recovery::windows_lost(app) {
        return;
    }
    let main = &app.state::<Perora>().main;
    main.wanted.store(true, Ordering::Relaxed);
    if main.ready.load(Ordering::Relaxed) {
        show(app);
    }
}

/// A click on the icon in the notification area: the window, when it is not on screen; out of sight
/// again, when it is.
pub fn toggle(app: &AppHandle) {
    if is_on_screen(app) {
        close(app)
    } else {
        open(app)
    }
}

/// Whether the window is on screen: shown, and not made small into the taskbar.
pub fn is_on_screen(app: &AppHandle) -> bool {
    window(app).is_some_and(|window| {
        window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false)
    })
}

/// Whether the window is to be on screen: opened, and not closed since.
pub fn is_wanted(app: &AppHandle) -> bool {
    app.state::<Perora>().main.wanted.load(Ordering::Relaxed)
}

fn show(app: &AppHandle) {
    if let Some(window) = window(app) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Closing the window: it goes out of sight, Perora stays.
pub fn close(app: &AppHandle) {
    app.state::<Perora>()
        .main
        .wanted
        .store(false, Ordering::Relaxed);
    if let Some(window) = window(app) {
        let _ = window.hide();
    }
}

/// Takes the window off the screen for a capture, if it is on it: it is not what is to be captured.
/// At once, without the system's fading it out (a window that is still fading is in the picture).
/// Unlike closing, this is for the moment only. Returns whether there was a window to take away.
///
/// Not to be called on the main thread: it asks the window whether it is visible, and waits for the
/// answer.
pub fn step_aside(app: &AppHandle) -> bool {
    let Some(window) = window(app) else {
        return false;
    };
    if !window.is_visible().unwrap_or(false) {
        return false;
    }
    app.state::<Perora>()
        .main
        .aside
        .store(true, Ordering::Relaxed);
    animate(&window, false);
    let _ = window.hide();
    true
}

/// The capture is over, whichever way: the window is back where it was, if it stepped aside for it
/// and was not closed since.
pub fn come_back(app: &AppHandle) {
    let main = &app.state::<Perora>().main;
    if !main.aside.swap(false, Ordering::Relaxed) {
        return;
    }
    if main.wanted.load(Ordering::Relaxed) {
        show(app);
    }
    // Opened and closed by the user, it moves the way windows do.
    if let Some(window) = window(app) {
        animate(&window, true);
    }
}

/// Whether the system animates the window's appearing and disappearing.
fn animate(window: &WebviewWindow, enabled: bool) {
    #[cfg(windows)]
    if let Ok(handle) = window.hwnd() {
        perora_core::platform::set_transitions(
            perora_core::platform::WindowId(handle.0 as isize),
            enabled,
        );
    }
}

/// Tells the window to ask for the state again.
pub fn changed(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "main-changed", ());
}

#[tauri::command]
pub fn main_state(app: AppHandle) -> View {
    let perora = app.state::<Perora>();
    let language = perora.language();
    let signed_in = perora.signed_in.lock().unwrap().clone();
    let account = if perora.login.lock().unwrap().is_some() {
        AccountView {
            state: "signingIn",
            name: None,
            may_upload: None,
        }
    } else {
        match signed_in {
            Some(who) => AccountView {
                state: "signedIn",
                name: who.name,
                may_upload: who.may_upload,
            },
            None => AccountView {
                state: "signedOut",
                name: None,
                may_upload: None,
            },
        }
    };

    View {
        language: language.code().to_owned(),
        languages: perora
            .languages
            .all()
            .iter()
            .map(|choice| LanguageChoice {
                code: choice.code().to_owned(),
                name: choice.name(),
            })
            .collect(),
        texts: language.texts(),
        account,
        settings: perora.setting(|settings| SettingsView {
            upload_immediately: settings.upload_immediately,
            enable_upload: settings.upload_enabled,
            start_mode: settings.start_mode.name(),
            copy_to_clipboard: settings.copy_to_clipboard,
            clear_clipboard_on_discard: settings.clear_clipboard_on_discard,
            save_directory: preview::save_directory(&app, settings)
                .map(|directory| directory.display().to_string())
                .unwrap_or_default(),
            save_directory_is_default: settings.save_directory.is_none(),
            ask_where_to_save: settings.ask_where_to_save,
            theme: settings.look.name(),
            copy_format: settings.output(Leaves::Copy).format.name(),
            copy_fit: settings.output(Leaves::Copy).fit.unwrap_or(0),
            upload_format: settings.output(Leaves::Upload).format.name(),
            upload_fit: settings.output(Leaves::Upload).fit.unwrap_or(0),
            save_format: settings.output(Leaves::Save).format.name(),
            save_fit: settings.output(Leaves::Save).fit.unwrap_or(0),
            save_formats: settings
                .save_formats
                .iter()
                .map(|kind| kind.name())
                .collect(),
            quality: settings.quality.name(),
            start_at_sign_in: crate::starts_at_sign_in().is_some(),
            start_minimized: crate::starts_at_sign_in().unwrap_or(true),
            notice_seconds: settings.notice_seconds,
            notice_corner: settings.notice_corner.name(),
            after_capture: settings.after_capture.name(),
            preview_seconds: settings.preview_seconds,
            preview_corner: settings.preview_corner.name(),
            capture_sound: settings.capture_sound.shown(),
            capture_sound_file: settings
                .capture_sound_file
                .as_ref()
                .map(|file| file.display().to_string()),
            notice_sound: settings.notice_sound.shown(),
            notice_sound_file: settings
                .notice_sound_file
                .as_ref()
                .map(|file| file.display().to_string()),
        }),
        modes: Mode::ALL.iter().map(|mode| mode.name()).collect(),
        formats: Format::ALL.iter().map(|format| format.name()).collect(),
        fits: FITS.to_vec(),
        qualities: Quality::ALL.iter().map(|quality| quality.name()).collect(),
        looks: Look::ALL.iter().map(|look| look.name()).collect(),
        corners: Corner::ALL.iter().map(|corner| corner.name()).collect(),
        sounds: perora_core::platform::system_sounds(),
        shortcut: {
            let snipping = crate::snipping(&app);
            ShortcutView {
                // (The one that captures now: the system's for snipping while that is Perora's.)
                label: perora.shortcut_shown(),
                registered: perora.shortcut_label().is_some(),
                take_snipping: snipping.wanted,
                snipping_registered: snipping.registered,
                shell_is_behind: snipping.behind,
                shell_restarting: snipping.restarting,
            }
        },
        version: app.package_info().version.to_string(),
        update: crate::update::view(&app),
        accent: accent_colour()
            .map(|(red, green, blue)| format!("#{red:02x}{green:02x}{blue:02x}")),
    }
}

/// The page has drawn itself. Counts only once the page is the one that stays (see
/// `LOADS_BEFORE_SHOWN`): the window is shown then, if it is wanted.
#[tauri::command]
pub fn main_ready(app: AppHandle) {
    let main = &app.state::<Perora>().main;
    if main.loads.load(Ordering::Relaxed) < LOADS_BEFORE_SHOWN {
        return;
    }
    main.ready.store(true, Ordering::Relaxed);
    if main.wanted.load(Ordering::Relaxed) {
        show(&app);
    }
}

#[tauri::command]
pub fn main_set_language(app: AppHandle, code: String) {
    if let Some(language) = app.state::<Perora>().languages.by_code(&code) {
        crate::speak(&app, language);
    }
}

/// Changes one of the settings that is a switch or a choice. A name or a value that is not one is
/// ignored: what is set is always something Perora understands.
#[tauri::command]
pub fn main_set(app: AppHandle, setting: String, value: serde_json::Value) {
    let switch = value.as_bool();
    let mode = value.as_str().and_then(Mode::from_name);
    let corner = value.as_str().and_then(Corner::from_name);
    // (How the windows look is set on its own: no other value is one of its names.)
    if let ("theme", Some(look)) = (setting.as_str(), value.as_str().and_then(Look::from_name)) {
        settings::update(&app, |settings| settings.look = look);
        preview::show_look(&app, look);
        return changed(&app);
    }
    // (Whether Perora starts at sign-in is the system's to remember, not the settings'.)
    // Switched on, it starts in the tray, until the user would rather see the window; and how it
    // starts is only to be said while it does.
    match (setting.as_str(), switch) {
        ("startAtSignIn", Some(on)) => {
            crate::start_at_sign_in(on.then_some(true));
            return changed(&app);
        }
        ("takeSnipping", Some(on)) => {
            crate::take_snipping(&app, on);
            return changed(&app);
        }
        ("startMinimized", Some(quietly)) => {
            if crate::starts_at_sign_in().is_some() {
                crate::start_at_sign_in(Some(quietly));
            }
            return changed(&app);
        }
        _ => {}
    }
    // What a capture is made into as it leaves, each way it leaves: the kind of file, the square
    // it is to fit (0 for none), and how good a file that loses something is.
    // (Saved as several kinds at once: named together.)
    if setting == "saveFormats" {
        let kinds: Vec<Format> = value
            .as_array()
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| name.as_str().and_then(Format::from_name))
                    .collect()
            })
            .unwrap_or_default();
        settings::update(&app, |settings| settings.save_as(&kinds));
        preview::changed(&app);
        return changed(&app);
    }
    let leaves = Leaves::ALL.into_iter().find(|leaves| {
        setting
            .strip_prefix(leaves.name())
            .is_some_and(|rest| rest == "Format" || rest == "Fit")
    });
    if let Some(leaves) = leaves {
        let which = leaves as usize;
        if setting.ends_with("Format") {
            if let Some(format) = value.as_str().and_then(Format::from_name) {
                settings::update(&app, |settings| match leaves {
                    Leaves::Save => settings.save_as(&[format]),
                    _ => settings.formats[which] = format,
                });
            }
        } else if let Some(side) = value.as_u64().and_then(|side| u32::try_from(side).ok())
            && (side == 0 || FITS.contains(&side))
        {
            settings::update(&app, |settings| {
                settings.fits[which] = (side != 0).then_some(side);
            });
        }
        // (A capture being previewed is offered what the settings say now.)
        preview::changed(&app);
        return changed(&app);
    }
    if setting == "afterCapture" {
        if let Some(after) = value.as_str().and_then(After::from_name) {
            settings::update(&app, |settings| settings.after_capture = after);
        }
        return changed(&app);
    }
    if setting == "quality" {
        if let Some(quality) = value.as_str().and_then(Quality::from_name) {
            settings::update(&app, |settings| settings.quality = quality);
        }
        return changed(&app);
    }
    // (How long a notice stays, and the small preview, are the ones that are numbers: of seconds,
    // no more than can be chosen. They count from the next one that appears.)
    if let "noticeSeconds" | "previewSeconds" = setting.as_str() {
        let seconds = value
            .as_u64()
            .and_then(|seconds| u32::try_from(seconds).ok())
            .filter(|seconds| *seconds <= MOST_SECONDS);
        if let Some(seconds) = seconds {
            settings::update(&app, |settings| match setting.as_str() {
                "noticeSeconds" => settings.notice_seconds = seconds,
                _ => settings.preview_seconds = seconds,
            });
        }
        return changed(&app);
    }
    // (A sound of the system's is one only if the system has it.)
    let sound = value
        .as_str()
        .and_then(Sound::from_name)
        .filter(|sound| match sound {
            Sound::System(name) => perora_core::platform::system_sound(name).is_some(),
            _ => true,
        });
    let mut moved = false;
    let mut placed = false;
    let mut heard = None;
    settings::update(&app, |settings| {
        match (setting.as_str(), switch, mode, corner) {
            ("uploadImmediately", Some(on), ..) => settings.upload_immediately = on,
            ("enableUpload", Some(on), ..) => settings.upload_enabled = on,
            ("copyToClipboard", Some(on), ..) => settings.copy_to_clipboard = on,
            ("clearClipboardOnDiscard", Some(on), ..) => settings.clear_clipboard_on_discard = on,
            ("askWhereToSave", Some(on), ..) => settings.ask_where_to_save = on,
            ("startMode", _, Some(mode), _) => settings.start_mode = mode,
            ("noticeCorner", _, _, Some(corner)) => {
                settings.notice_corner = corner;
                moved = true;
            }
            ("previewCorner", _, _, Some(corner)) => {
                settings.preview_corner = corner;
                placed = true;
            }
            _ => match (setting.as_str(), sound) {
                ("noticeSound", Some(sound)) => {
                    settings.notice_sound = sound;
                    heard = Some(Occasion::Notice);
                }
                ("captureSound", Some(sound)) => {
                    settings.capture_sound = sound;
                    heard = Some(Occasion::Capture);
                }
                _ => {}
            },
        }
    });
    if let Some(occasion) = heard {
        // Heard as it is chosen.
        crate::notice::sound(&app, occasion);
    }
    if placed {
        preview::moved(&app);
    }
    if moved {
        // Said where it is said from now on: the choice is seen at once.
        let said = app.state::<Perora>().language().notices_here();
        crate::notify(&app, Lasting::Brief, said);
    }
    changed(&app);
    // A capture being previewed is offered what the settings say now.
    preview::changed(&app);
}

/// Makes another key combination the shortcut. Refused with `invalid` when it is not one, with
/// `taken` when another application holds it; the shortcut stays what it was then.
#[tauri::command]
pub fn main_set_shortcut(app: AppHandle, shortcut: String) -> Result<(), String> {
    let outcome = crate::set_shortcut(&app, &shortcut);
    changed(&app);
    outcome
}

/// A new shortcut is being recorded in the window (`suspended`), or no longer is.
#[tauri::command]
pub fn main_suspend_shortcut(app: AppHandle, suspended: bool) {
    crate::suspend_shortcut(&app, suspended);
    if !suspended {
        changed(&app);
    }
}

/// Starts the system's shell anew: asked for by the user, where the window says that the shell has
/// yet to read what it was told. The window is told when that is done.
#[tauri::command]
pub fn main_restart_shell(app: AppHandle) {
    crate::restart_shell(&app);
}

/// Lets the user choose the folder "save" writes to; with `reset`, goes back to Perora's own.
#[tauri::command]
pub async fn main_choose_save_directory(app: AppHandle, reset: bool) {
    let chosen = if reset {
        None
    } else {
        let current = app
            .state::<Perora>()
            .setting(|settings| preview::save_directory(&app, settings));
        let mut dialog = app.dialog().file();
        if let Some(current) = current {
            dialog = dialog.set_directory(current);
        }
        // Closed without choosing: nothing changes.
        match dialog
            .blocking_pick_folder()
            .and_then(|folder| folder.into_path().ok())
        {
            Some(folder) => Some(folder),
            None => return,
        }
    };
    settings::update(&app, |settings| settings.save_directory = chosen);
    changed(&app);
}

/// Lets the user choose the sound file of their own that a notice (`of` is `notice`) or a capture
/// (`capture`) comes with; chosen, it is that sound from now on, and is heard once.
#[tauri::command]
pub async fn main_choose_sound(app: AppHandle, of: String) {
    let Some(occasion) = Occasion::from_name(&of) else {
        return;
    };
    let current = app.state::<Perora>().setting(|settings| match occasion {
        Occasion::Notice => settings.notice_sound_file.clone(),
        Occasion::Capture => settings.capture_sound_file.clone(),
    });
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("Audio", &perora_core::platform::SOUND_FILES);
    if let Some(folder) = current.as_deref().and_then(|file| file.parent()) {
        dialog = dialog.set_directory(folder);
    }
    // Closed without choosing: nothing changes.
    let Some(file) = dialog
        .blocking_pick_file()
        .and_then(|file| file.into_path().ok())
    else {
        return;
    };
    settings::update(&app, |settings| match occasion {
        Occasion::Notice => {
            settings.notice_sound_file = Some(file);
            settings.notice_sound = Sound::Custom;
        }
        Occasion::Capture => {
            settings.capture_sound_file = Some(file);
            settings.capture_sound = Sound::Custom;
        }
    });
    crate::notice::sound(&app, occasion);
    changed(&app);
}

/// The user's latest uploads. Refused with `signedOut` when nobody is signed in, with `unreachable`
/// when the server could not be asked.
#[tauri::command]
pub async fn main_recent(app: AppHandle) -> Result<Vec<Upload>, &'static str> {
    let perora = app.state::<Perora>();
    match shots::recent(&perora.account, RECENT_UPLOADS) {
        Ok(recent) => Ok(recent
            .into_iter()
            // Only what is the server's own: these addresses are shown, copied and opened.
            .filter(|shot| is_at(&perora, &shot.url) && is_at(&perora, &shot.image_url))
            .map(|shot| Upload {
                id: shot.id,
                url: shot.url,
                image_url: shot.image_url,
                width: shot.width,
                height: shot.height,
                created_at: shot.created_at,
            })
            .collect()),
        Err(shots::ApiError::SignedOut) => Err("signedOut"),
        Err(_) => Err("unreachable"),
    }
}

/// Whether an address is one of the server's (`https://host/…`): nothing else is copied or opened on
/// the word of a page.
fn is_at(perora: &Perora, url: &str) -> bool {
    url.strip_prefix(perora.account.config().server.as_str())
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Puts the link of an upload on the clipboard.
#[tauri::command]
pub fn main_copy_link(app: AppHandle, url: String) -> Result<(), String> {
    if !is_at(&app.state::<Perora>(), &url) {
        return Ok(());
    }
    let outcome = app
        .clipboard()
        .write_text(url)
        .map_err(|error| error.to_string());
    // A capture being previewed is no longer what is on the clipboard.
    preview::changed(&app);
    outcome
}

/// Opens the page of an upload in the browser.
#[tauri::command]
pub fn main_open_page(app: AppHandle, url: String) {
    if is_at(&app.state::<Perora>(), &url) {
        let _ = app.opener().open_url(url, None::<&str>);
    }
}

#[tauri::command]
pub fn main_sign_in(app: AppHandle) {
    crate::sign_in(&app);
}

/// Stops waiting for the browser to come back from signing in.
#[tauri::command]
pub fn main_cancel_sign_in(app: AppHandle) {
    app.state::<Perora>()
        .login_cancelled
        .store(true, Ordering::Relaxed);
}

#[tauri::command]
pub fn main_sign_out(app: AppHandle) {
    crate::sign_out(&app);
}

#[tauri::command]
pub fn main_capture(app: AppHandle) {
    // The window steps aside for every capture, however it was started, and is back afterwards.
    crate::capture(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_told_each_switch_by_the_name_it_sets_it_by() {
        let view = serde_json::to_value(SettingsView {
            upload_immediately: true,
            enable_upload: true,
            start_mode: "smart",
            copy_to_clipboard: true,
            clear_clipboard_on_discard: true,
            save_directory: String::new(),
            save_directory_is_default: true,
            ask_where_to_save: true,
            theme: "dark",
            copy_format: "png",
            copy_fit: 0,
            upload_format: "webp",
            upload_fit: 2048,
            save_format: "jpg",
            save_fit: 0,
            save_formats: vec!["jpg", "webp"],
            quality: "high",
            start_at_sign_in: true,
            start_minimized: true,
            notice_seconds: 12,
            notice_corner: "topLeft",
            notice_sound: "none".into(),
            notice_sound_file: None,
            after_capture: "edit",
            preview_seconds: 20,
            preview_corner: "topRight",
            capture_sound: "custom".into(),
            capture_sound_file: None,
        })
        .unwrap();

        // The names `main_set` takes.
        for name in [
            "uploadImmediately",
            "enableUpload",
            "copyToClipboard",
            "clearClipboardOnDiscard",
            "askWhereToSave",
            "startAtSignIn",
            "startMinimized",
        ] {
            assert_eq!(view[name], true, "{name}");
        }
        assert_eq!(view["startMode"], "smart");
        assert_eq!(view["afterCapture"], "edit");
        // (And what a capture leaves as, by the names of `Leaves` with what is set of each.)
        assert_eq!(view["uploadFormat"], "webp");
        assert_eq!(view["uploadFit"], 2048);
        assert_eq!(view["saveFormat"], "jpg");
        assert_eq!(view["saveFormats"], serde_json::json!(["jpg", "webp"]));
        assert_eq!(view["copyFit"], 0);
        assert_eq!(view["quality"], "high");
        assert_eq!(view["theme"], "dark");
        assert_eq!(view["noticeSeconds"], 12);
        assert_eq!(view["noticeCorner"], "topLeft");
        assert_eq!(view["noticeSound"], "none");
        assert_eq!(view["previewSeconds"], 20);
        assert_eq!(view["previewCorner"], "topRight");
        assert_eq!(view["captureSound"], "custom");
        assert!(view["captureSoundFile"].is_null());
        assert!(view["noticeSoundFile"].is_null());
    }
}
