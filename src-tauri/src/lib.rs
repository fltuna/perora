//! The application: a main window for what it is set to do, an icon in the notification area, one
//! global shortcut, the selection screen it brings up and the preview of what was selected.
//!
//! Pressing the shortcut captures every monitor and holds that moment still on screen; the user picks
//! a window, a rectangle, a monitor or all of it. What was picked is put on the clipboard and shown
//! in the preview, from where it is uploaded (its link put on the clipboard, a notification saying
//! so), saved to a file or thrown away. What all of that does lives in `perora-core`; this crate
//! connects it to the desktop.

mod main_window;
mod messages;
mod notice;
mod preview;
mod recovery;
mod selection;
mod settings;
mod update;

use std::borrow::Cow;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use perora_core::account::{Account, AccountError};
use perora_core::config::Config;
use perora_core::oidc::OidcError;
use perora_core::platform::{Frame, SystemTokenStore, token_store};
use perora_core::shots;
use tauri::http::{Request, Response, StatusCode};
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, UriSchemeContext, WindowEvent, Wry};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use messages::{Language, Languages};
use notice::Lasting;

/// How long the browser may take to come back from signing in.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How long quitting waits for a renewal of the sign-in that is under way.
const QUIT_PATIENCE: Duration = Duration::from_secs(3);

/// How long a window that was just hidden may still be on screen.
const SETTLE: Duration = Duration::from_millis(60);

/// Started with this, Perora goes to the notification area without showing its window: for being
/// started with the system, and for being restarted while it is worked on.
const START_IN_TRAY: &str = "--tray";

/// The name Perora is started under when the user signs in.
const STARTED_AS: &str = "Perora";

/// What the system is to run for that: this very program, in the tray without its window
/// (`quietly`) or with it.
fn started_by(quietly: bool) -> Option<String> {
    let program = std::env::current_exe().ok()?;
    Some(started_as(&program.display().to_string(), quietly))
}

fn started_as(program: &str, quietly: bool) -> String {
    if quietly {
        format!("\"{program}\" {START_IN_TRAY}")
    } else {
        format!("\"{program}\"")
    }
}

/// Whether Perora starts when the user signs in, and whether it does so in the tray, without its
/// window. This program: not one of the same name that was somewhere else (that one may be gone).
fn starts_at_sign_in() -> Option<bool> {
    let started = perora_core::platform::started_at_sign_in(STARTED_AS)?;
    [true, false].into_iter().find(|quietly| {
        started_by(*quietly).is_some_and(|this| started.eq_ignore_ascii_case(&this))
    })
}

/// Has Perora start when the user signs in (in the tray, or with its window), or no longer.
fn start_at_sign_in(how: Option<bool>) {
    let command = how.and_then(started_by);
    if how.is_some() && command.is_none() {
        return;
    }
    if let Err(detail) = perora_core::platform::start_at_sign_in(STARTED_AS, command.as_deref()) {
        eprintln!("starting at sign-in could not be set: {detail}");
    }
}

/// Started with this while it is running already, Perora captures, as the shortcut does.
const CAPTURE: &str = "--capture";

struct Perora {
    account: Account<SystemTokenStore>,
    language: Mutex<Language>,
    languages: Languages,
    /// Who is signed in, as far as is known.
    signed_in: Mutex<Option<SignedIn>>,
    /// What the user chose: read at the start, changed through `settings::update`.
    settings: Mutex<settings::Settings>,
    /// Whether the global shortcut could be registered (another application may hold it).
    shortcut_registered: AtomicBool,
    /// The user's shortcut is let go of for the moment, on purpose: another is being recorded.
    shortcut_suspended: AtomicBool,
    /// Whether the system's combination for snipping is Perora's now: asked for, and given.
    snipping_registered: AtomicBool,
    /// What the system's shell knew when it last started, as far as Perora can tell: whether it was
    /// to leave its combinations with S alone. (It reads that as it starts, and only then.)
    shell_left_alone: AtomicBool,
    /// The shell is being started anew, at the user's asking: not asked for a second time meanwhile.
    shell_restarting: AtomicBool,
    /// The screen is being captured and the selection screen put up: the shortcut does nothing until
    /// that is done (a key held down repeats).
    capturing: AtomicBool,
    /// The address of the sign-in the browser was sent to, while it is awaited.
    login: Mutex<Option<String>>,
    /// Set to end a wait for the browser: by the user, or by quitting.
    login_cancelled: AtomicBool,
    menu: MenuItems,
    main: main_window::State,
    notice: notice::State,
    preview: preview::State,
    selection: selection::State,
    update: update::State,
}

/// A sign-in is stored. What the server says about it, when it could be asked.
#[derive(Clone, Default)]
struct SignedIn {
    name: Option<String>,
    may_upload: Option<bool>,
}

struct MenuItems {
    /// All of it: what the icon in the notification area is given, each time it is put there.
    all: Menu<Wry>,
    capture: MenuItem<Wry>,
    open: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

impl Perora {
    fn language(&self) -> Language {
        self.language.lock().unwrap().clone()
    }

    /// One of the settings, as it is now.
    fn setting<T>(&self, of: impl FnOnce(&settings::Settings) -> T) -> T {
        of(&self.settings.lock().unwrap())
    }

    /// Whether uploading is among what Perora does: the user chose so, and someone is signed in.
    /// Without it, a capture is copied and saved, and nothing says "upload".
    fn uploads(&self) -> bool {
        let signed_in = self.signed_in.lock().unwrap().is_some();
        signed_in && self.setting(|settings| settings.upload_enabled)
    }

    /// The shortcut as the menu shows it; `None` when the system did not give it to Perora. While
    /// the system's combination for snipping is Perora's, that is the shortcut: the user's own is
    /// let go of meanwhile.
    fn shortcut_label(&self) -> Option<String> {
        if self.snipping_registered.load(Ordering::Relaxed) {
            return Some(SNIPPING_SHOWN.to_owned());
        }
        self.shortcut_registered
            .load(Ordering::Relaxed)
            .then(|| self.setting(|settings| settings.shortcut.clone()))
    }

    /// The combination that captures now, whether or not the system gave it to Perora: the
    /// system's for snipping while that is Perora's, otherwise the user's own.
    fn shortcut_shown(&self) -> String {
        if self.snipping_registered.load(Ordering::Relaxed) {
            SNIPPING_SHOWN.to_owned()
        } else {
            self.setting(|settings| settings.shortcut.clone())
        }
    }
}

/// Started with this by the uninstaller (not when updating, and before anything is removed),
/// Perora gives back what it took from the system, and does nothing else.
const UNINSTALLING: &str = "--uninstalling";

/// Perora is being uninstalled: the system's combination for snipping is the shell's again, when
/// Perora had it (the settings say so; otherwise the letter, if it is there, is someone else's).
/// From the next time the shell starts, as taking it was. (What starts Perora at sign-in the
/// uninstaller takes away itself.)
fn uninstalling() {
    if settings::read_without_app().is_some_and(|settings| settings.take_snipping)
        && let Err(detail) = perora_core::platform::leave_hotkeys_alone(SNIPPING_LETTER, false)
    {
        eprintln!("the system's combination for snipping could not be given back: {detail}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if std::env::args_os().any(|argument| argument == UNINSTALLING) {
        uninstalling();
        return;
    }
    tauri::Builder::default()
        // A second Perora would spend the first one's refresh token. Started again, it asks the one
        // that is running to capture (`--capture`) or to show its window.
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            if arguments.iter().any(|argument| argument == CAPTURE) {
                capture(app);
            } else if !arguments.iter().any(|argument| argument == START_IN_TRAY) {
                main_window::open(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        capture(app);
                    }
                })
                .build(),
        )
        .register_uri_scheme_protocol("perora", serve)
        .invoke_handler(tauri::generate_handler![
            main_window::main_state,
            main_window::main_ready,
            main_window::main_set_language,
            main_window::main_set,
            main_window::main_set_shortcut,
            main_window::main_suspend_shortcut,
            main_window::main_restart_shell,
            main_window::main_choose_save_directory,
            main_window::main_choose_sound,
            main_window::main_recent,
            main_window::main_copy_link,
            main_window::main_open_page,
            main_window::main_sign_in,
            main_window::main_cancel_sign_in,
            main_window::main_sign_out,
            main_window::main_capture,
            preview::preview_state,
            preview::preview_image,
            preview::preview_fonts,
            preview::preview_vanish,
            preview::preview_appear,
            preview::preview_edits,
            preview::preview_copy,
            preview::preview_upload,
            preview::preview_save,
            preview::preview_discard,
            preview::preview_enlarge,
            preview::preview_choose,
            preview::preview_drawn,
            notice::notice_state,
            notice::notice_dismiss,
            selection::selection_state,
            selection::selection_ready,
            selection::selection_pointer,
            selection::selection_mode,
            selection::selection_window,
            selection::selection_cancel,
            selection::selection_touring,
            settings::tour_seen,
            update::main_update_look,
            update::main_update_skip,
            update::main_update_close,
            update::main_update_install,
        ])
        .on_window_event(|window, event| {
            // No window is ever destroyed by being closed: each is kept, hidden, for the next time.
            // What closing means depends on the window.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle();
                if window.label() == main_window::LABEL {
                    // Out of sight; Perora stays in the notification area.
                    main_window::close(app);
                } else if window.label() == notice::LABEL {
                    notice::put_away(app);
                } else if app.state::<Perora>().selection.is_active() {
                    // Alt+F4 on the selection screen is "never mind".
                    selection::cancel(app);
                } else {
                    // A preview: the capture is thrown away (unless it is being uploaded).
                    preview::discard_by_user(app, None);
                }
            }
        })
        .setup(|app| {
            setup(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the application")
        .run(|app, event| {
            // No window is open most of the time; that is no reason to exit. Quitting from the menu
            // carries an exit code and goes through.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                match code {
                    None => api.prevent_exit(),
                    Some(_) => {
                        let perora = app.state::<Perora>();
                        perora.login_cancelled.store(true, Ordering::Relaxed);
                        // The process ends when this returns, whatever its threads are doing.
                        perora.account.settle(QUIT_PATIENCE);
                    }
                }
            }
        });
}

fn setup(app: &AppHandle) -> tauri::Result<()> {
    // A start that follows the web views' ending shows the main window if it was shown before.
    let window_shown_before = recovery::window_shown_before();
    // The built-in languages, and the language files the user put beside the settings.
    let languages = Languages::load(
        app.path()
            .app_config_dir()
            .ok()
            .map(|directory| directory.join("lang"))
            .as_deref(),
    );
    let mut chosen = settings::read(app);
    // The settings want the system's combination for snipping, and the shell was told to take it
    // back meanwhile (by an uninstall that was part of installing again, say): it is told to leave
    // it alone again, from the next time it starts. (Before what the shell knew is guessed below.)
    if chosen.take_snipping
        && !perora_core::platform::hotkeys_left_alone(SNIPPING_LETTER)
        && let Err(detail) = perora_core::platform::leave_hotkeys_alone(SNIPPING_LETTER, true)
    {
        eprintln!("the shell could not be told about its combinations: {detail}");
    }
    let language = chosen
        .language
        .as_deref()
        .and_then(|code| languages.by_code(code))
        .unwrap_or_else(|| languages.of_the_system());
    let config = Config::from_environment();

    // A sign-in at a development server is kept apart from the real one.
    let file = if config.is_production() {
        "sign-in"
    } else {
        "sign-in.development"
    };
    let account = Account::new(
        config,
        token_store(app.path().app_local_data_dir()?.join(file)),
    );

    // A shortcut in the file that means nothing (edited by hand) is as good as none chosen. The
    // settings say so too from here on: what they name is what is registered, shown, and let go of
    // when another is chosen.
    if chosen.shortcut.parse::<Shortcut>().is_err() {
        chosen.shortcut = settings::DEFAULT_SHORTCUT.to_owned();
    }
    let registered = chosen
        .shortcut
        .parse::<Shortcut>()
        .is_ok_and(|shortcut| app.global_shortcut().register(shortcut).is_ok());

    // The system's combination for snipping, when the user chose to have it capture with Perora:
    // Perora's if the shell has let go of it (it does when it starts, once told to).
    let main = chosen
        .shortcut
        .parse::<Shortcut>()
        .ok()
        .map(|main| main.id());
    let claimed = |combination: &str| {
        combination.parse::<Shortcut>().is_ok_and(|shortcut| {
            // (The user's own shortcut already: asked for once, above.)
            if main == Some(shortcut.id()) {
                registered
            } else {
                app.global_shortcut().register(shortcut).is_ok()
            }
        })
    };
    let snipping = chosen.take_snipping && claimed(SNIPPING);

    let capture = MenuItem::with_id(
        app,
        "capture",
        language.capture(registered.then_some(chosen.shortcut.as_str())),
        true,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(app, "open", language.open(), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", language.quit(), true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&capture, &open, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    // (Heard once, by the application: an icon that is put in the notification area again does
    // not bring a second listener with it.)
    app.on_menu_event(on_menu_event);
    put_in_tray(
        app,
        &menu,
        tooltip(
            &language,
            registered.then_some(chosen.shortcut.as_str()),
            &chosen.shortcut,
        ),
    )?;

    // A sign-in that is stored counts until the server says otherwise: the window is not to show
    // "sign in" for the moment it takes to ask.
    let signed_in = stored_sign_in(&account).then(SignedIn::default);

    // Before the windows: they ask for the state as soon as they have loaded.
    app.manage(Perora {
        account,
        language: Mutex::new(language),
        languages,
        signed_in: Mutex::new(signed_in),
        settings: Mutex::new(chosen),
        shortcut_registered: AtomicBool::new(registered),
        shortcut_suspended: AtomicBool::new(false),
        snipping_registered: AtomicBool::new(snipping),
        shell_left_alone: AtomicBool::new(perora_core::platform::hotkeys_left_alone(
            SNIPPING_LETTER,
        )),
        shell_restarting: AtomicBool::new(false),
        capturing: AtomicBool::new(false),
        login: Mutex::new(None),
        login_cancelled: AtomicBool::new(false),
        menu: MenuItems {
            all: menu,
            capture,
            open,
            quit,
        },
        main: main_window::State::default(),
        notice: notice::State::default(),
        preview: preview::State::default(),
        selection: selection::State::default(),
        update: update::State::default(),
    });
    main_window::create(app)?;
    preview::create_windows(app)?;
    notice::create(app)?;
    selection::create_windows(app);
    // (With the system's combination for snipping Perora's, the user's own shortcut is let go of.)
    claim_snipping(app);
    // Light or dark, as the user chose, should they have chosen: all the windows alike.
    preview::show_look(app, app.state::<Perora>().setting(|settings| settings.look));
    let in_tray = std::env::args_os().any(|argument| argument == START_IN_TRAY);
    // (A new version the user installed from the window shows it, however the old one was started.)
    if window_shown_before.unwrap_or(!in_tray || update::installed_from_window()) {
        main_window::open(app);
    }

    // Whether there is a newer version, a while from now and every few hours.
    update::keep_looking(app);
    // Who is signed in, if anyone: asked in the background, nothing waits for the network.
    let app = app.clone();
    std::thread::spawn(move || refresh_status(&app));
    Ok(())
}

/// What the icon in the notification area says: the name, and when the shortcut is not Perora's to
/// use (`registered` is `None`), that.
fn tooltip(language: &Language, registered: Option<&str>, shortcut: &str) -> String {
    match registered {
        Some(_) => "Perora".to_owned(),
        None => format!("Perora ({})", language.shortcut_unavailable(shortcut)),
    }
}

/// The name of Perora's icon in the notification area.
const IN_TRAY: &str = "perora";

/// Puts Perora's icon in the notification area, with its menu.
fn put_in_tray(app: &AppHandle, menu: &Menu<Wry>, tooltip: String) -> tauri::Result<()> {
    let mut tray = TrayIconBuilder::with_id(IN_TRAY)
        .tooltip(tooltip)
        .menu(menu)
        // The menu is the right button's. The left one is the window: there, and away again.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                main_window::toggle(tray.app_handle());
            }
        });
    // The application's icon, at the size the notification area draws it: the icon has one drawn for
    // each small size, which is better than the window's own made smaller.
    #[cfg(windows)]
    let small =
        tauri::image::Image::from_app_icon_resource(perora_core::platform::small_icon_side()).ok();
    #[cfg(not(windows))]
    let small = None;
    if let Some(icon) = small.or_else(|| app.default_window_icon().cloned()) {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}

/// Puts the icon in the notification area anew: the shell that had it is gone, and the one that
/// took its place has not been given it. (From where the windows are made: the icon has one.)
fn put_in_tray_again(app: &AppHandle) {
    let again = app.clone();
    let _ = app.run_on_main_thread(move || {
        let perora = again.state::<Perora>();
        let language = perora.language();
        let shortcut = perora.shortcut_shown();
        let registered = perora.shortcut_label();
        drop(again.remove_tray_by_id(IN_TRAY));
        if let Err(error) = put_in_tray(
            &again,
            &perora.menu.all,
            tooltip(&language, registered.as_deref(), &shortcut),
        ) {
            eprintln!("the icon could not be put in the notification area again: {error}");
        }
    });
}

/// The menu and the icon say what the shortcut is now, in the language of now.
fn relabel(app: &AppHandle) {
    let perora = app.state::<Perora>();
    let language = perora.language();
    let shortcut = perora.shortcut_shown();
    let registered = perora.shortcut_label();

    let menu = &perora.menu;
    let _ = menu
        .capture
        .set_text(language.capture(registered.as_deref()));
    let _ = menu.open.set_text(language.open());
    let _ = menu.quit.set_text(language.quit());
    if let Some(tray) = app.tray_by_id(IN_TRAY) {
        let _ = tray.set_tooltip(Some(tooltip(&language, registered.as_deref(), &shortcut)));
    }
}

/// Whether a key combination may be a global shortcut: one that typing does not need. A letter, a
/// digit or any other key of the main block needs Ctrl, Alt or the Windows key with it (with Shift
/// alone it is a capital letter); function keys and PrintScreen may stand alone.
fn is_a_shortcut(combination: &str) -> bool {
    let mut keys: Vec<&str> = combination.split('+').collect();
    let Some(key) = keys.pop() else {
        return false;
    };
    let function_key = key
        .strip_prefix('F')
        .is_some_and(|number| matches!(number.parse::<u8>(), Ok(1..=24)));
    let strong = keys
        .iter()
        .any(|modifier| matches!(*modifier, "Ctrl" | "Alt" | "Super"));
    strong || function_key || key == "PrintScreen"
}

/// The system's own combination for snipping, and the letter the shell is told to leave alone for
/// it. The shell leaves every combination with that letter alone then, the one for searching
/// (Win+S) among them: nobody has that one while this is on. (Perora does not take it to search
/// in the shell's place: what opens the system's search from outside is not what the key opens.)
const SNIPPING: &str = "Super+Shift+S";
/// As it is shown: by the name the key has on a keyboard.
const SNIPPING_SHOWN: &str = "Win+Shift+S";
const SNIPPING_LETTER: char = 'S';

/// Whether a combination is the user's own shortcut for capturing.
fn is_the_shortcut(app: &AppHandle, shortcut: &Shortcut) -> bool {
    app.state::<Perora>()
        .setting(|settings| settings.shortcut.parse::<Shortcut>())
        .is_ok_and(|main| main.id() == shortcut.id())
}

/// How the system's combination for snipping stands, for the window.
struct Snipping {
    /// The user wants it to capture with Perora.
    wanted: bool,
    /// It does.
    registered: bool,
    /// The shell has to start anew before things are as the user last chose: it was told
    /// something since it started, or it still has what Perora is to have.
    behind: bool,
    /// It is being started anew.
    restarting: bool,
}

fn snipping(app: &AppHandle) -> Snipping {
    let perora = app.state::<Perora>();
    let wanted = perora.setting(|settings| settings.take_snipping);
    let registered = perora.snipping_registered.load(Ordering::Relaxed);
    let told = perora_core::platform::hotkeys_left_alone(SNIPPING_LETTER);
    let knew = perora.shell_left_alone.load(Ordering::Relaxed);
    Snipping {
        wanted,
        registered,
        behind: told != knew || (wanted && !registered),
        restarting: perora.shell_restarting.load(Ordering::Relaxed),
    }
}

/// Asks the system for its combination for snipping when the user wants it, lets go of it when
/// not. Returns whether things are as wanted: given only once the
/// shell has let go of them. (One that is the user's own shortcut is that already, and stays.)
fn claim_snipping(app: &AppHandle) -> bool {
    let perora = app.state::<Perora>();
    let wanted = perora.setting(|settings| settings.take_snipping);
    let shortcuts = app.global_shortcut();
    let claim = |combination: &str| {
        let Ok(shortcut) = combination.parse::<Shortcut>() else {
            return false;
        };
        if is_the_shortcut(app, &shortcut) {
            return perora.shortcut_registered.load(Ordering::Relaxed);
        }
        if wanted {
            shortcuts.is_registered(shortcut) || shortcuts.register(shortcut).is_ok()
        } else {
            let _ = shortcuts.unregister(shortcut);
            false
        }
    };
    let registered = claim(SNIPPING) && wanted;
    perora
        .snipping_registered
        .store(registered, Ordering::Relaxed);

    // One shortcut at a time: while the system's is Perora's, the user's own is let go of (it
    // does nothing, and is not to be changed); it is asked for again when the system's is not
    // Perora's any more, or not yet.
    let main = perora.setting(|settings| settings.shortcut.parse::<Shortcut>());
    if let Ok(main) = main
        && SNIPPING
            .parse::<Shortcut>()
            .is_ok_and(|snipping| snipping.id() != main.id())
    {
        if registered {
            let _ = shortcuts.unregister(main);
            perora.shortcut_registered.store(false, Ordering::Relaxed);
        } else if !shortcuts.is_registered(main)
            // (Let go of on purpose while another is recorded: whatever ends that takes it back.)
            && !perora.shortcut_suspended.load(Ordering::Relaxed)
        {
            let back = shortcuts.register(main).is_ok();
            perora.shortcut_registered.store(back, Ordering::Relaxed);
        }
    }
    relabel(app);
    registered == wanted
}

/// The user wants the system's combination for snipping to capture with Perora, or no longer: the
/// shell is told to leave it alone (or to take it again), which it does when it next starts.
fn take_snipping(app: &AppHandle, on: bool) {
    if let Err(detail) = perora_core::platform::leave_hotkeys_alone(SNIPPING_LETTER, on) {
        eprintln!("the shell could not be told about its combinations: {detail}");
        return;
    }
    settings::update(app, |settings| settings.take_snipping = on);
    // (Given at once only if the shell did not have it in the first place.)
    claim_snipping(app);
}

/// How long the shell is given to start anew and take (or not take) its combinations, and how
/// often Perora asks for the one it wants meanwhile.
const SHELL_STARTS_WITHIN: Duration = Duration::from_secs(20);
const SHELL_ASKED_EVERY: Duration = Duration::from_millis(500);
/// How long after that the notification area is surely there to put an icon in.
const TRAY_READY_WITHIN: Duration = Duration::from_secs(6);

/// Starts the system's shell anew, so that it reads what it was told about its combinations; and
/// asks for the one for snipping once the shell is back, if the user wants it. Once at a time.
fn restart_shell(app: &AppHandle) {
    let perora = app.state::<Perora>();
    if perora.shell_restarting.swap(true, Ordering::AcqRel) {
        return;
    }
    main_window::changed(app);

    let app = app.clone();
    std::thread::spawn(move || {
        let perora = app.state::<Perora>();
        // What the shell reads as it starts is what it is told now.
        let told = perora_core::platform::hotkeys_left_alone(SNIPPING_LETTER);
        match perora_core::platform::restart_shell() {
            Ok(()) => {
                perora.shell_left_alone.store(told, Ordering::Relaxed);
                // (A moment for the shell to take what is its own: asked for before that, the
                // combination would be Perora's although the user wanted it back with the shell.)
                std::thread::sleep(SHELL_ASKED_EVERY * 4);
                let began = std::time::Instant::now();
                while !claim_snipping(&app) && began.elapsed() < SHELL_STARTS_WITHIN {
                    std::thread::sleep(SHELL_ASKED_EVERY);
                }
                // The notification area is the new shell's, and Perora's icon is not in it: put
                // there again, and once more a while later, should the area not have been ready.
                // (Not asked to start anew again until all of that is over.)
                put_in_tray_again(&app);
                std::thread::sleep(TRAY_READY_WITHIN);
                put_in_tray_again(&app);
            }
            Err(detail) => eprintln!("the shell could not be started anew: {detail}"),
        }
        perora.shell_restarting.store(false, Ordering::Release);
        main_window::changed(&app);
    });
}

/// Lets go of the shortcut for the moment (a new one is being recorded: pressing the old one is an
/// answer, not a capture), or takes it back.
fn suspend_shortcut(app: &AppHandle, suspended: bool) {
    let perora = app.state::<Perora>();
    let Ok(shortcut) = perora
        .setting(|settings| settings.shortcut.clone())
        .parse::<Shortcut>()
    else {
        return;
    };
    let shortcuts = app.global_shortcut();
    perora
        .shortcut_suspended
        .store(suspended, Ordering::Relaxed);
    // (Let go of already, while the system's combination for snipping is the shortcut.)
    if perora.snipping_registered.load(Ordering::Relaxed) {
        return;
    }
    if suspended {
        let _ = shortcuts.unregister(shortcut);
    } else if !shortcuts.is_registered(shortcut) {
        let registered = shortcuts.register(shortcut).is_ok();
        perora
            .shortcut_registered
            .store(registered, Ordering::Relaxed);
        relabel(app);
    }
}

/// Makes another key combination the shortcut. On refusal (not a key combination, or held by another
/// application) the one before stays, and the reason comes back.
fn set_shortcut(app: &AppHandle, wanted: &str) -> Result<(), String> {
    let perora = app.state::<Perora>();
    // (Not while the user has the system's combination for snipping capture: that is the
    // shortcut then, and the window does not offer to change the other.)
    if !is_a_shortcut(wanted) || perora.setting(|settings| settings.take_snipping) {
        return Err("invalid".to_owned());
    }
    let new: Shortcut = wanted.parse().map_err(|_| "invalid".to_owned())?;
    let old = perora.setting(|settings| settings.shortcut.clone());
    let shortcuts = app.global_shortcut();

    if let Ok(old) = old.parse::<Shortcut>() {
        let _ = shortcuts.unregister(old);
    }
    // (One Perora has already, as one of the system's that it took: the user's own from now on.)
    if !shortcuts.is_registered(new) && shortcuts.register(new).is_err() {
        // Back to the one before, which may itself be unavailable (it was, if it never registered).
        let back = old
            .parse::<Shortcut>()
            .is_ok_and(|old| shortcuts.is_registered(old) || shortcuts.register(old).is_ok());
        perora.shortcut_registered.store(back, Ordering::Relaxed);
        claim_snipping(app);
        relabel(app);
        return Err("taken".to_owned());
    }

    perora.shortcut_registered.store(true, Ordering::Relaxed);
    settings::update(app, |settings| settings.shortcut = wanted.to_owned());
    // (The one before may have been one of the system's that Perora took: taken again, as that.)
    claim_snipping(app);
    relabel(app);
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "capture" => capture(app),
        "open" => main_window::open(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

/// Captures the screen and puts up the selection screen. Pressed again while that is up: once more,
/// from the screen as it is now.
fn capture(app: &AppHandle) {
    // Without windows there is no selection screen to put up.
    if recovery::windows_lost(app) {
        return;
    }
    let perora = app.state::<Perora>();
    if perora.capturing.swap(true, Ordering::AcqRel) {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        let perora = app.state::<Perora>();
        let _done = Done(&perora.capturing);
        // The main window is not what is to be captured, nor is a notice: gone at once (the window
        // is back when this is over). A window that was just hidden is on screen for one more frame.
        let noticed = notice::put_away(&app);
        if main_window::step_aside(&app) || noticed {
            std::thread::sleep(SETTLE);
        }
        if let Err(detail) = selection::start(&app) {
            notify(
                &app,
                Lasting::Long,
                perora.language().capture_failed(&detail),
            );
        }
    });
}

/// What the user selected goes to the preview; or, when the user chose so, straight to the server.
fn deliver(app: &AppHandle, frame: Frame) {
    let perora = app.state::<Perora>();
    let language = perora.language();
    let immediately = perora.uploads() && perora.setting(|settings| settings.upload_immediately);
    // Heard the moment there is a capture, whatever becomes of it.
    notice::sound(app, notice::Occasion::Capture);

    // Without a preview, a notification says that something was captured, before the image is
    // packed and sent: that takes a while. Not when the sign-in turns out to be gone: then all of
    // this ends at once, with its own message.
    if immediately && perora.account.is_signed_in().unwrap_or(false) {
        notify(app, Lasting::UntilTheNext, language.captured());
    }

    // Without a preview either (the user chose to have none): to the clipboard, and that is all
    // that becomes of it.
    if !immediately && perora.setting(|settings| settings.after_capture) == settings::After::Copy {
        let (lasting, message) = match preview::copy_alone(app, &frame) {
            Ok(()) => (Lasting::Brief, language.copied()),
            Err(detail) => (Lasting::Long, language.not_copied(&detail)),
        };
        notify(app, lasting, message);
        // What was being previewed before this capture is still to be dealt with.
        return preview::restore(app);
    }

    // For the preview, as it was captured; straight to the server, as the user has captures
    // uploaded (the kind of file, and how large at most).
    let made = if immediately {
        frame.to_file(perora.setting(|settings| settings.output(settings::Leaves::Upload)))
    } else {
        frame.to_png().map_err(|error| error.to_string())
    };
    let image = match made {
        Ok(image) => image,
        Err(detail) => {
            return notify(app, Lasting::Long, language.capture_failed(&detail));
        }
    };
    if !immediately {
        return preview::present(app, frame, image);
    }

    let _uploading = recovery::Uploading::begin();
    let (lasting, message) = match shots::upload(&perora.account, &image) {
        Ok(shot) => {
            // The main window lists the uploads.
            main_window::changed(app);
            match app.clipboard().write_text(shot.url.as_str()) {
                Ok(()) => (Lasting::Brief, language.uploaded()),
                Err(_) => (Lasting::Long, language.uploaded_but_not_copied(&shot.url)),
            }
        }
        Err(error) => {
            if matches!(error, shots::ApiError::SignedOut) {
                show_status(app, None);
            }
            (Lasting::Long, language.upload_failed(&error))
        }
    };
    notify(app, lasting, message);
    // What was being previewed before this capture is still to be dealt with.
    preview::restore(app);
}

/// Clears a flag when dropped: however the work it guards ends.
struct Done<'a>(&'a AtomicBool);

impl Drop for Done<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Sends the browser to the identity provider and waits for it to come back.
fn sign_in(app: &AppHandle) {
    let perora = app.state::<Perora>();
    let mut pending = perora.login.lock().unwrap();

    // Asked again while the browser is awaited (the tab was closed, perhaps): the same address again.
    if let Some(url) = pending.as_deref() {
        if !url.is_empty() {
            let _ = app.opener().open_url(url, None::<&str>);
        }
        return;
    }

    // A placeholder until the address is known, so that a second click does not start a second attempt.
    *pending = Some(String::new());
    drop(pending);
    perora.login_cancelled.store(false, Ordering::Relaxed);
    main_window::changed(app);

    let app = app.clone();
    std::thread::spawn(move || {
        let perora = app.state::<Perora>();
        let outcome = perora.account.begin_login().and_then(|login| {
            let url = login.authorize_url().to_owned();
            *perora.login.lock().unwrap() = Some(url.clone());
            let _ = app.opener().open_url(url, None::<&str>);
            perora
                .account
                .finish_login(login, LOGIN_TIMEOUT, &perora.login_cancelled)
        });
        if outcome.is_ok() {
            // Before the wait is declared over: the window is never to show "not signed in" between
            // the two.
            *perora.signed_in.lock().unwrap() = Some(SignedIn::default());
        }
        *perora.login.lock().unwrap() = None;

        match outcome {
            Ok(()) => {
                let name = refresh_status(&app);
                notify(
                    &app,
                    Lasting::Brief,
                    perora.language().signed_in(name.as_deref().unwrap_or("?")),
                );
            }
            // The user gave up, or Perora is quitting: nothing to say.
            Err(AccountError::Oidc(OidcError::Cancelled)) => {
                refresh_status(&app);
            }
            Err(error) => {
                refresh_status(&app);
                notify(
                    &app,
                    Lasting::Long,
                    perora.language().account_failed(&error),
                );
            }
        }
    });
}

/// Ends the sign-in on this machine.
fn sign_out(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let perora = app.state::<Perora>();
        match perora.account.sign_out() {
            Ok(()) => show_status(&app, None),
            // The sign-in may still be there: show what is, not what was asked for.
            Err(error) => {
                refresh_status(&app);
                notify(
                    &app,
                    Lasting::Long,
                    perora.language().account_failed(&error),
                );
            }
        }
    });
}

/// Asks the server who is signed in and shows it. Returns the name, when the server gave one.
fn refresh_status(app: &AppHandle) -> Option<String> {
    let perora = app.state::<Perora>();
    match shots::session(&perora.account) {
        Ok(session) => {
            let may_upload = session
                .permissions
                .iter()
                .any(|granted| matches!(granted.as_str(), "*" | "shots.*" | "shots.upload"));
            // Asked a moment ago; the user may have signed out since.
            if !stored_sign_in(&perora.account) {
                show_status(app, None);
                return None;
            }
            show_status(
                app,
                Some(SignedIn {
                    name: Some(session.name.clone()),
                    may_upload: Some(may_upload),
                }),
            );
            Some(session.name)
        }
        // The server could not be asked. A sign-in that is stored still counts: without a name.
        Err(_) => {
            show_status(app, stored_sign_in(&perora.account).then(SignedIn::default));
            None
        }
    }
}

/// Whether a sign-in is stored. A store that cannot be read right now (the file is locked, the disk
/// is in trouble) is taken as "yes": telling a signed-in user to sign in again would be the worse
/// mistake, and an upload will say so if there really is none.
fn stored_sign_in(account: &Account<SystemTokenStore>) -> bool {
    account.is_signed_in().unwrap_or(true)
}

fn show_status(app: &AppHandle, signed_in: Option<SignedIn>) {
    *app.state::<Perora>().signed_in.lock().unwrap() = signed_in;
    main_window::changed(app);
    // A capture being previewed can be uploaded only with someone signed in.
    preview::changed(app);
}

/// Switches the language of the windows, the menu and every notification from now on, and remembers
/// the choice.
fn speak(app: &AppHandle, language: Language) {
    let perora = app.state::<Perora>();
    *perora.language.lock().unwrap() = language.clone();
    settings::update(app, |settings| {
        settings.language = Some(language.code().to_owned())
    });

    relabel(app);
    main_window::changed(app);
    preview::changed(app);
    notice::changed(app);
    selection::language_changed(app);
}

/// The `perora:` protocol, by which the windows get their pictures: `/capture/…` for the preview,
/// `/selection/…` for the selection screen. Each gives out what is on screen right now and nothing
/// else; any other address is not found.
fn serve(
    context: UriSchemeContext<'_, Wry>,
    request: Request<Vec<u8>>,
) -> Response<Cow<'static, [u8]>> {
    let perora = context.app_handle().state::<Perora>();
    // The web view's helper for such addresses escapes the slashes of what it is given.
    let path = request.uri().path().replace("%2F", "/").replace("%2f", "/");
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let image = match parts.split_first() {
        Some((&"capture", rest)) => preview::image(&perora, rest),
        Some((&"selection", rest)) => selection::image(&perora, rest),
        _ => None,
    };

    match image {
        Some((kind, image)) => Response::builder()
            .header("Content-Type", kind)
            .header("Cache-Control", "no-store")
            .body(Cow::Owned(image.to_vec())),
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Cow::Borrowed(&[][..])),
    }
    .expect("a response of constant headers")
}

/// Says something to the user that no window of Perora's is the place for: in the notice window,
/// for as long as that kind of thing is said; or as a notification of the system's when that window
/// cannot be used.
///
/// Nothing is said at all when the user chose to have no notices.
fn notify(app: &AppHandle, lasting: Lasting, message: impl Into<String>) {
    say(app, lasting, message.into(), None);
}

/// Says something that is about a file: a click on it, where it is said in Perora's own window,
/// shows that file in its folder.
fn notify_of_file(
    app: &AppHandle,
    lasting: Lasting,
    message: impl Into<String>,
    file: std::path::PathBuf,
) {
    say(
        app,
        lasting,
        message.into(),
        Some(notice::Click::Reveal(file)),
    );
}

/// Says something, and what a click on it does where it is said in Perora's own window.
fn say(app: &AppHandle, lasting: Lasting, message: String, click: Option<notice::Click>) {
    if !notice::say(app, message.clone(), lasting, click) {
        notify_by_system(app, message);
    }
}

/// Says something as a notification of the system's: what is left when Perora's own windows are
/// not to be counted on.
fn notify_by_system(app: &AppHandle, message: impl Into<String>) {
    let _ = app
        .notification()
        .builder()
        .title("Perora")
        .body(message)
        .show();
}

#[cfg(test)]
mod tests {
    use super::{is_a_shortcut, started_as};

    #[test]
    fn what_is_started_at_sign_in_is_the_program_in_quotes_with_or_without_its_window() {
        let program = r"C:\Program Files\Perora\perora.exe";
        assert_eq!(
            started_as(program, true),
            r#""C:\Program Files\Perora\perora.exe" --tray"#
        );
        assert_eq!(
            started_as(program, false),
            r#""C:\Program Files\Perora\perora.exe""#
        );
    }

    #[test]
    fn a_shortcut_is_a_combination_that_typing_does_not_need() {
        for good in [
            "Ctrl+Shift+H",
            "Alt+F9",
            "Ctrl+1",
            "Super+Shift+S",
            "F9",
            "Shift+F12",
            "PrintScreen",
            "Ctrl+Alt+PrintScreen",
        ] {
            assert!(is_a_shortcut(good), "{good}");
        }
        for bad in [
            "H",
            "Shift+H",
            "1",
            "Space",
            "Shift+Space",
            "",
            "F",
            "F0",
            "Fx",
        ] {
            assert!(!is_a_shortcut(bad), "{bad}");
        }
    }
}
