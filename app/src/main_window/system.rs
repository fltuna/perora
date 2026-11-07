//! What the settings page changes besides the settings file, and what it asks of the rest of
//! Perora: the windows that show what was set, the tray, the shortcut, the system (start at sign-in,
//! the shell's combination for snipping, "Open with Perora" in Explorer), sounds and samples. As the
//! Tauri version's `main_set` and the functions of its `lib.rs` it calls.
//!
//! With `PERORA_DRY_SYSTEM` set (as `examples/look_main.rs` sets it), nothing here reaches the system
//! or the screen: what would be written to the registry is kept in memory instead, no sound is
//! played, no notice or other window is shown, nothing is captured or opened. The page behaves as it
//! would otherwise, for looking at it.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gpui::{App, PathPromptOptions};
use perora_core::notice::{Corner, Sound};
use perora_core::platform;
use perora_core::settings::{Look, Settings};

use crate::arguments::{SNIPPING_LETTER, START_IN_TRAY};
use crate::notice::{self, Lasting};
use crate::shortcut::{self, Shortcuts};
use crate::state::Perora;
use crate::tray::Tray;

/// Whether nothing is to reach the system or the screen (see above).
pub fn dry() -> bool {
    std::env::var_os("PERORA_DRY_SYSTEM").is_some()
}

/// What the system would say, while nothing is to reach it: what was asked of it this run.
#[derive(Default)]
struct Pretended {
    /// What starts at sign-in (`Some(None)`: nothing).
    started: Option<Option<bool>>,
    /// What the shell is told about its combinations with the letter for snipping.
    told: Option<bool>,
}

static PRETENDED: Mutex<Pretended> = Mutex::new(Pretended {
    started: None,
    told: None,
});

fn pretended<R>(with: impl FnOnce(&mut Pretended) -> R) -> R {
    with(
        &mut PRETENDED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// Changes the settings as the page asks, and has every window show what they say now.
pub fn set(cx: &mut App, change: impl FnOnce(&mut Settings)) {
    Perora::get(cx).change(change);
    changed(cx);
}

/// Has every window of Perora's draw itself again: what the settings say, how they look, the
/// language. (The small preview offers what the settings say now, as the large window does.)
pub fn changed(cx: &mut App) {
    // (Once what is being done is done: a window cannot be told while it is the one telling, as
    // the main window is when one of its controls changes a setting.)
    cx.defer(|cx| {
        for window in cx.windows() {
            let _ = window.update(cx, |_, window, _| window.refresh());
        }
    });
}

/// The windows' look: every one of them changes at once.
pub fn set_look(look: Look, cx: &mut App) {
    Perora::get(cx).change(|settings| settings.look = look);
    super::apply_theme(cx);
    changed(cx);
}

/// The language of the windows, the tray's menu and every notice from now on.
pub fn set_language(code: &str, cx: &mut App) {
    if Perora::get(cx).languages.by_code(code).is_none() {
        return;
    }
    Perora::get(cx).change(|settings| settings.language = Some(code.to_owned()));
    relabel(cx);
    // (The editor and the small preview say what they say in it from now on too.)
    if !dry() {
        crate::large::relanguage(cx);
        crate::preview::relanguage(cx);
    }
    // (Explorer's "Open with Perora" says it in the user's language too.)
    explorer_entry(cx);
    changed(cx);
}

/// Where notices appear: one says so there at once, so that the choice is seen.
pub fn set_notice_corner(corner: Corner, cx: &mut App) {
    set(cx, |settings| settings.notice_corner = corner);
    if dry() {
        eprintln!("dry: a notice in the corner chosen");
        return;
    }
    let said = Perora::get(cx).language().notices_here();
    notice::say(said, Lasting::Brief, None, cx);
}

/// Where the small preview appears.
pub fn set_preview_corner(corner: Corner, cx: &mut App) {
    set(cx, |settings| settings.preview_corner = corner);
    // A preview on screen goes there; with none, one is shown there for a moment.
    if dry() {
        eprintln!("dry: the small preview would be shown where it appears");
        return;
    }
    crate::preview::moved(cx);
}

/// Which sound a notice or a capture comes with: heard once as it is chosen. One of the system's is
/// one only if the system has it.
pub fn set_sound(of: Occasion, sound: Sound, cx: &mut App) {
    if let Sound::System(name) = &sound
        && platform::system_sound(name).is_none()
    {
        return;
    }
    set(cx, |settings| match of {
        Occasion::Notice => settings.notice_sound = sound,
        Occasion::Capture => settings.capture_sound = sound,
    });
    play(of, cx);
}

/// What a sound is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occasion {
    Notice,
    Capture,
}

/// The sound of that occasion, as the settings have it, once.
fn play(of: Occasion, cx: &App) {
    let settings = Perora::get(cx).settings();
    let (sound, file) = match of {
        Occasion::Notice => (settings.notice_sound, settings.notice_sound_file),
        Occasion::Capture => (settings.capture_sound, settings.capture_sound_file),
    };
    let file = match sound {
        Sound::None => None,
        Sound::System(name) => platform::system_sound(&name),
        Sound::Custom => file,
    };
    let Some(file) = file else {
        return;
    };
    if dry() {
        eprintln!("dry: the sound {} would be heard", file.display());
        return;
    }
    platform::play_sound(&file);
}

/// Lets the user choose a sound file of their own for that occasion: chosen, it is that sound from
/// now on, and is heard once. (The system's dialog cannot be told to show sound files only, nor
/// where to begin: a file of another kind is not taken.)
pub fn choose_sound(of: Occasion, cx: &mut App) {
    if dry() {
        eprintln!("dry: a sound file would be chosen");
        return;
    }
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(files))) = chosen.await else {
            return;
        };
        let Some(file) = files.into_iter().next().filter(|file| is_a_sound(file)) else {
            return;
        };
        cx.update(|cx| {
            set(cx, |settings| match of {
                Occasion::Notice => {
                    settings.notice_sound_file = Some(file);
                    settings.notice_sound = Sound::Custom;
                }
                Occasion::Capture => {
                    settings.capture_sound_file = Some(file);
                    settings.capture_sound = Sound::Custom;
                }
            });
            play(of, cx);
        });
    })
    .detach();
}

fn is_a_sound(file: &std::path::Path) -> bool {
    file.extension()
        .and_then(|ending| ending.to_str())
        .is_some_and(|ending| {
            platform::SOUND_FILES
                .iter()
                .any(|kind| kind.eq_ignore_ascii_case(ending))
        })
}

/// Lets the user choose the folder "save" writes to; with `reset`, Perora's own again.
pub fn choose_save_directory(reset: bool, cx: &mut App) {
    if reset {
        set(cx, |settings| settings.save_directory = None);
        return;
    }
    if dry() {
        eprintln!("dry: a folder would be chosen");
        return;
    }
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    cx.spawn(async move |cx| {
        // Closed without choosing: nothing changes.
        let Ok(Ok(Some(folders))) = chosen.await else {
            return;
        };
        let Some(folder) = folders.into_iter().next() else {
            return;
        };
        cx.update(|cx| set(cx, |settings| settings.save_directory = Some(folder)));
    })
    .detach();
}

/// The name Perora is started under when the user signs in (the Tauri version's, which 0.1.x wrote).
const STARTED_AS: &str = "Perora";

/// What the system is to run at sign-in: this very program, in the tray without its window
/// (`quietly`) or with it.
fn started_by(quietly: bool) -> Option<String> {
    let program = std::env::current_exe().ok()?;
    let program = program.display();
    Some(if quietly {
        format!("\"{program}\" {START_IN_TRAY}")
    } else {
        format!("\"{program}\"")
    })
}

/// Whether Perora starts when the user signs in, and whether it does so in the tray, without its
/// window. This very program: not one of the same name somewhere else (that one may be gone).
pub fn starts_at_sign_in() -> Option<bool> {
    if dry()
        && let Some(started) = pretended(|pretended| pretended.started)
    {
        return started;
    }
    let started = platform::started_at_sign_in(STARTED_AS)?;
    [true, false].into_iter().find(|quietly| {
        started_by(*quietly).is_some_and(|this| started.eq_ignore_ascii_case(&this))
    })
}

/// Has Perora start when the user signs in (in the tray, or with its window), or no longer. What
/// the system would not have stays as it was, and the switch shows that.
pub fn start_at_sign_in(how: Option<bool>, cx: &mut App) {
    let command = how.and_then(started_by);
    if how.is_some() && command.is_none() {
        return;
    }
    if dry() {
        eprintln!("dry: start at sign-in: {command:?}");
        pretended(|pretended| pretended.started = Some(how));
    } else if let Err(detail) = platform::start_at_sign_in(STARTED_AS, command.as_deref()) {
        eprintln!("starting at sign-in could not be set: {detail}");
    }
    changed(cx);
}

/// Whether the shell is told to leave the combinations with the letter for snipping alone.
fn told() -> bool {
    if dry()
        && let Some(told) = pretended(|pretended| pretended.told)
    {
        return told;
    }
    platform::hotkeys_left_alone(SNIPPING_LETTER)
}

/// How the system's combination for snipping (Win + Shift + S) stands, for the page.
pub struct Snipping {
    /// The user wants it to capture with Perora.
    pub wanted: bool,
    /// The shell has to start anew before things are as the user last chose: it was told
    /// something since it started, or it still has what Perora is to have.
    pub behind: bool,
    /// It is being started anew.
    pub restarting: bool,
}

pub fn snipping(cx: &App) -> Snipping {
    let perora = Perora::get(cx);
    let wanted = perora.settings().take_snipping;
    Snipping {
        wanted,
        behind: told() != perora.shell_left_alone || (wanted && !perora.claimed.snipping),
        restarting: perora.shell_restarting,
    }
}

/// The user wants the system's combination for snipping to capture with Perora, or no longer: the
/// shell is told to leave it alone (or to take it again), which it does when it next starts.
pub fn take_snipping(on: bool, cx: &mut App) {
    if dry() {
        eprintln!("dry: the shell would be told to leave Win + S alone: {on}");
        pretended(|pretended| pretended.told = Some(on));
    } else if let Err(detail) = platform::leave_hotkeys_alone(SNIPPING_LETTER, on) {
        eprintln!("the shell could not be told about its combinations: {detail}");
        changed(cx);
        return;
    }
    Perora::get(cx).change(|settings| settings.take_snipping = on);
    // (Perora's at once only if the shell did not have it in the first place.)
    claim(cx);
    changed(cx);
}

/// How long the shell is given to start anew and take (or let go of) its combinations, how often
/// Perora asks for the one it wants meanwhile, and how long it waits before it first asks (asked
/// before the shell took what is its own, the combination would be Perora's although the user wanted
/// it back with the shell).
const SHELL_STARTS_WITHIN: Duration = Duration::from_secs(20);
const SHELL_ASKED_EVERY: Duration = Duration::from_millis(500);
const SHELL_SETTLES: Duration = Duration::from_secs(2);

/// Starts the system's shell anew, so that it reads what it was told about its combinations, and
/// asks for the one for snipping once it is back, as the user wants. Once at a time. (Explorer's
/// coming back is the system's: Perora starts it itself only when the system does not.)
pub fn restart_shell(cx: &mut App) {
    let perora = cx.global_mut::<Perora>();
    if perora.shell_restarting {
        return;
    }
    perora.shell_restarting = true;
    changed(cx);
    cx.spawn(async move |cx| {
        // What the shell reads as it starts is what it is told now.
        let told = told();
        let restarted = cx
            .background_executor()
            .spawn(async move {
                if dry() {
                    eprintln!("dry: the shell would be started anew");
                    std::thread::sleep(Duration::from_millis(1500));
                    return Ok(());
                }
                platform::restart_shell()
            })
            .await;
        match restarted {
            Ok(()) => {
                cx.update(|cx| cx.global_mut::<Perora>().shell_left_alone = told);
                cx.background_executor().timer(SHELL_SETTLES).await;
                let began = Instant::now();
                while !cx.update(claim) && began.elapsed() < SHELL_STARTS_WITHIN {
                    cx.background_executor().timer(SHELL_ASKED_EVERY).await;
                }
                // (tray-icon puts the icon back in the new shell's notification area itself: it
                // hears the shell's "TaskbarCreated".)
            }
            Err(detail) => eprintln!("the shell could not be started anew: {detail}"),
        }
        cx.update(|cx| {
            cx.global_mut::<Perora>().shell_restarting = false;
            changed(cx);
        });
    })
    .detach();
}

/// The shortcuts, while Perora has them (not in a Perora started only to be looked at).
fn shortcuts(cx: &mut App) -> Option<&mut Shortcuts> {
    if !cx.has_global::<crate::Kept>() {
        return None;
    }
    cx.global_mut::<crate::Kept>()._shortcuts.as_mut()
}

fn tray(cx: &App) -> Option<&Tray> {
    cx.has_global::<crate::Kept>()
        .then(|| &cx.global::<crate::Kept>()._tray)
}

/// Asks for the shortcuts as the settings have them now. Whether they are as wanted: the system's
/// combination for snipping is Perora's exactly when the user wants it to be.
fn claim(cx: &mut App) -> bool {
    let settings = Perora::get(cx).settings();
    let wanted = settings.take_snipping;
    if let Some(claimed) =
        shortcuts(cx).map(|shortcuts| shortcuts.claim(&settings.shortcut, wanted))
    {
        cx.global_mut::<Perora>().claimed = claimed;
    }
    relabel(cx);
    Perora::get(cx).claimed.snipping == wanted
}

/// The tray says the language and the shortcut as they are now.
fn relabel(cx: &App) {
    let perora = Perora::get(cx);
    let language = perora.language();
    let (registered, shown) = (perora.shortcut_label(), perora.shortcut_shown());
    if let Some(tray) = tray(cx) {
        tray.relabel(&language, registered.as_deref(), &shown);
    }
}

/// Why a combination was not made the shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Another application has it.
    Taken,
    /// It is no combination to use (typing needs it), or the system's combination for snipping is
    /// the shortcut now.
    Invalid,
}

/// Lets go of the shortcut for the moment (a new one is being recorded: pressing the old one is an
/// answer, not a capture), or asks for it again.
pub fn suspend_shortcut(suspended: bool, cx: &mut App) {
    let settings = Perora::get(cx).settings();
    if let Some(claimed) = shortcuts(cx)
        .map(|shortcuts| shortcuts.suspend(suspended, &settings.shortcut, settings.take_snipping))
    {
        cx.global_mut::<Perora>().claimed = claimed;
    }
    // (Let go of to record another, it is not taken by anything: the tray says it as it was until
    // the recording is over.)
    if !suspended {
        relabel(cx);
    }
    changed(cx);
}

/// Makes another key combination the shortcut. Refused when it is no combination to use, or another
/// application has it: the shortcut stays what it was then.
pub fn set_shortcut(wanted: &str, cx: &mut App) -> Result<(), Refusal> {
    let settings = Perora::get(cx).settings();
    // (Not while the system's combination for snipping is to capture: that is the shortcut then,
    // and the page does not offer to change the other.)
    if !shortcut::is_a_shortcut(wanted)
        || settings.take_snipping
        || shortcut::chosen(wanted) != wanted
    {
        return Err(Refusal::Invalid);
    }
    let outcome =
        shortcuts(cx).map(|shortcuts| shortcuts.change(wanted, &settings.shortcut, false));
    let result = match outcome {
        Some(Ok(claimed)) => {
            cx.global_mut::<Perora>().claimed = claimed;
            Ok(())
        }
        Some(Err(claimed)) => {
            cx.global_mut::<Perora>().claimed = claimed;
            Err(Refusal::Taken)
        }
        // (No shortcuts in this run: the choice is kept for the next.)
        None => Ok(()),
    };
    if result.is_ok() {
        Perora::get(cx).change(|settings| settings.shortcut = wanted.to_owned());
    }
    relabel(cx);
    changed(cx);
    result
}

/// The user switched "Open with Perora" on or off.
pub fn open_from_explorer(on: bool, cx: &mut App) {
    set(cx, |settings| settings.open_from_explorer = on);
    explorer_entry(cx);
}

/// Has Explorer offer "Open with Perora" as the settings say, in the user's language.
fn explorer_entry(cx: &mut App) {
    if dry() {
        eprintln!("dry: Explorer's entry would be kept as the settings say");
        return;
    }
    crate::opening::keep_explorer_entry(cx);
}

/// Captures, as the shortcut does (the main window steps aside for it).
pub fn capture(cx: &mut App) {
    if dry() {
        eprintln!("dry: a capture would start");
        return;
    }
    // (Once the main window's own update is over: pressed from it, the window is being updated, and
    // could not be stepped aside for the capture.)
    cx.defer(crate::capture::start);
}

/// The editor: with the image that is previewed when there is one, otherwise empty.
pub fn open_editor(cx: &mut App) {
    if dry() {
        eprintln!("dry: the editor would open");
        return;
    }
    crate::opening::open_editor(cx);
}

/// What is on the clipboard, in the editor.
pub fn open_clipboard(cx: &mut App) {
    if dry() {
        eprintln!("dry: the clipboard would open in the editor");
        return;
    }
    crate::opening::open_clipboard(cx);
}

/// A file dropped on the window, in the editor.
pub fn open_file(path: PathBuf, cx: &mut App) {
    if dry() {
        eprintln!("dry: {} would open in the editor", path.display());
        return;
    }
    crate::opening::open_file(path, cx);
}
