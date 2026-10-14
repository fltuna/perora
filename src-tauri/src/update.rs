//! Updates. Perora asks the releases on GitHub whether there is a newer version: a while after it
//! starts, every six hours after that, and whenever the user asks. What it finds is shown in a band at
//! the top of the main window, and said once in a notice when that window is out of sight. Nothing is
//! installed until the user says so; then the new version is fetched, its signature checked, and its
//! installer run, which closes Perora and starts the new one in its place.
//!
//! Where to ask, and the key the signature is checked with, are in `tauri.conf.json`
//! (`plugins.updater`): the release workflow puts `latest.json` beside each release.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::notice::Lasting;
use crate::{Perora, main_window, notice, preview, recovery, settings};

/// How long after the start the first look is: Perora has other things to do as it starts.
const FIRST_LOOK: Duration = Duration::from_secs(20);

/// How long between looks after that.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// How long the releases are given to answer: to be reached at all, and between one piece of an
/// answer and the next. (No limit on the whole: the installer takes as long as the line needs.)
const REACHED_WITHIN: Duration = Duration::from_secs(15);
const SILENT_FOR_AT_MOST: Duration = Duration::from_secs(30);

/// How often the main window hears how far the download is.
const TOLD_EVERY: Duration = Duration::from_millis(250);

/// Set for the new version by the one that installs it, when that one's main window was on screen:
/// it is shown again as the new version starts (which would otherwise start as the old one was
/// started, in the tray when started with the system).
const WINDOW_WAS_SHOWN: &str = "PERORA_UPDATED_FROM_WINDOW";

#[derive(Default)]
pub struct State {
    /// A newer version, when one was found.
    found: Mutex<Option<Update>>,
    /// One look at a time.
    looking: AtomicBool,
    /// The user asked to look while a look was under way: that one tells them what it came to.
    asked_meanwhile: AtomicBool,
    /// What the last look the user asked for came to, for the general page.
    asked: Mutex<Asked>,
    /// The version being fetched or installed, and how far that is: bytes so far, of how many.
    getting: Mutex<Option<Getting>>,
    /// The version whose band the user closed: not shown again until Perora starts again.
    closed: Mutex<Option<String>>,
    /// The version a notice was given for: one notice for each version a run finds.
    noticed: Mutex<Option<String>>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Asked {
    /// Nothing asked, or what was asked is no longer worth saying.
    #[default]
    Nothing,
    Looking,
    /// This is the newest version.
    UpToDate,
    /// The releases could not be asked.
    Failed,
}

impl Asked {
    /// What is said after a look that reached the releases (`found`: a newer version, or none).
    /// Asked for, it says so; a look of its own does not bring up what was not asked, but does
    /// put right what an asked look said before.
    fn after(self, asked: bool, found: bool) -> Self {
        match (found, asked, self) {
            (true, _, _) => Asked::Nothing,
            (false, true, _) => Asked::UpToDate,
            (false, false, Asked::Nothing) => Asked::Nothing,
            (false, false, _) => Asked::UpToDate,
        }
    }
}

#[derive(Clone)]
struct Getting {
    version: String,
    got: u64,
    of: Option<u64>,
    /// Fetched, and its signature checked: the installer is starting, and Perora about to close.
    installing: bool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether the band shows a version: not one the user skipped, or closed this run. (One being
/// fetched is shown whatever was skipped: the user asked for it.)
fn band(
    available: Option<&str>,
    skipped: Option<&str>,
    closed: Option<&str>,
    getting: bool,
) -> bool {
    getting || available.is_some_and(|version| skipped != Some(version) && closed != Some(version))
}

/// Whether a notice says that this version is out: the main window is not on screen to show its
/// band, the user did not skip it or close its band, and no notice said it before in this run.
fn notice_due(
    version: &str,
    on_screen: bool,
    skipped: Option<&str>,
    closed: Option<&str>,
    noticed: Option<&str>,
) -> bool {
    !on_screen && skipped != Some(version) && closed != Some(version) && noticed != Some(version)
}

/// What the main window shows of updates.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The newer version there is, if one was found (shown on the general page, whatever the band).
    available: Option<String>,
    /// Whether the band at the top shows it: not when the user skipped that version, or closed the
    /// band this run.
    band: bool,
    /// What the last look the user asked for came to: `nothing`, `looking`, `upToDate` or `failed`.
    asked: &'static str,
    /// Being fetched: bytes so far, and of how many when that is known; or being installed.
    got: Option<u64>,
    of: Option<u64>,
    installing: bool,
    /// Whether installing would throw away a capture that is still being looked at, or uploaded:
    /// the button waits until that is done with.
    capture_in_hand: bool,
}

pub fn view(app: &AppHandle) -> View {
    let perora = app.state::<Perora>();
    let update = &perora.update;
    // (Each taken and let go of in turn: none is held while another is taken.)
    let found = lock(&update.found)
        .as_ref()
        .map(|found| found.version.clone());
    let getting = lock(&update.getting).clone();
    let asked = *lock(&update.asked);
    let closed = lock(&update.closed).clone();
    let skipped = perora.setting(|settings| settings.skipped_version.clone());
    // (While one is being fetched, that one: whatever a look meanwhile found.)
    let available = getting
        .as_ref()
        .map(|getting| getting.version.clone())
        .or(found);
    View {
        band: band(
            available.as_deref(),
            skipped.as_deref(),
            closed.as_deref(),
            getting.is_some(),
        ),
        available,
        asked: match asked {
            Asked::Nothing => "nothing",
            Asked::Looking => "looking",
            Asked::UpToDate => "upToDate",
            Asked::Failed => "failed",
        },
        got: getting.as_ref().map(|getting| getting.got),
        of: getting.as_ref().and_then(|getting| getting.of),
        installing: getting.is_some_and(|getting| getting.installing),
        capture_in_hand: capture_in_hand(app),
    }
}

/// A capture is being looked at, or uploaded: installing would end Perora under it.
fn capture_in_hand(app: &AppHandle) -> bool {
    preview::holds_a_capture(app) || recovery::uploads_under_way()
}

/// Looks for a newer version now and then, from the start until Perora quits. Not in a build for
/// working on Perora: that is not the version the releases are of.
pub fn keep_looking(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_LOOK);
        loop {
            look(&app, false);
            std::thread::sleep(EVERY);
        }
    });
}

/// A look under way, for as long as this lives: whichever way the look ends, it is over.
struct Looking<'a>(&'a AtomicBool);

impl Drop for Looking<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Asks the releases whether there is a newer version. `asked`: the user asked, and is told what it
/// came to; a version they skipped or closed is offered again. Otherwise a look that fails says
/// nothing. Not to be called where an asynchronous runtime runs (it waits for one of its own).
fn look(app: &AppHandle, asked: bool) {
    let perora = app.state::<Perora>();
    let update = &perora.update;
    if lock(&update.getting).is_some() {
        return;
    }
    if asked {
        *lock(&update.asked) = Asked::Looking;
        update.asked_meanwhile.store(true, Ordering::Release);
        main_window::changed(app);
    }
    if update.looking.swap(true, Ordering::AcqRel) {
        // The look under way tells the user.
        return;
    }
    let looking = Looking(&update.looking);

    // (What is found carries this with it, for when it is installed.)
    let before = app.clone();
    let result = app
        .updater_builder()
        .configure_client(|client| {
            client
                .connect_timeout(REACHED_WITHIN)
                .read_timeout(SILENT_FOR_AT_MOST)
        })
        .on_before_exit(move || before_installing(&before))
        .build()
        .and_then(|updater| tauri::async_runtime::block_on(updater.check()));
    // Asked while this look was under way, or before it: either way, the user hears of it. (Looked
    // at once this look is over: asked any later, the user's own look runs.)
    drop(looking);
    let asked = update.asked_meanwhile.swap(false, Ordering::AcqRel) || asked;

    let found = match result {
        Ok(found) => found,
        Err(error) => {
            eprintln!("could not look for a newer version: {error}");
            if asked {
                *lock(&update.asked) = Asked::Failed;
                main_window::changed(app);
            }
            return;
        }
    };
    let version = found.as_ref().map(|found| found.version.clone());
    *lock(&update.found) = found;
    {
        let mut said = lock(&update.asked);
        *said = said.after(asked, version.is_some());
    }

    // Asked for, it is shown, skipped or not, closed or not.
    if asked && let Some(version) = &version {
        *lock(&update.closed) = None;
        if perora.setting(|settings| settings.skipped_version.as_ref() == Some(version)) {
            settings::update(app, |settings| settings.skipped_version = None);
        }
    }
    main_window::changed(app);

    // Out of sight, the band is not seen: a notice says it, once for each version.
    if let Some(version) = version {
        let skipped = perora.setting(|settings| settings.skipped_version.clone());
        let closed = lock(&update.closed).clone();
        let noticed = lock(&update.noticed).clone();
        if notice_due(
            &version,
            main_window::is_on_screen(app),
            skipped.as_deref(),
            closed.as_deref(),
            noticed.as_deref(),
        ) {
            *lock(&update.noticed) = Some(version.clone());
            crate::say(
                app,
                Lasting::Long,
                perora.language().update_found(&version),
                Some(notice::Click::OpenMain),
            );
        }
    }
}

/// The user asked to look now, from the general page. (On a thread of its own: a look waits for the
/// network, and `look` is not to be called where an asynchronous runtime is already running.)
#[tauri::command]
pub fn main_update_look(app: AppHandle) {
    std::thread::spawn(move || look(&app, true));
}

/// The version found, if it is the one the window showed (`version`): what the user did was about
/// that one, not one a look found meanwhile.
fn found_is(app: &AppHandle, version: &str) -> bool {
    lock(&app.state::<Perora>().update.found)
        .as_ref()
        .is_some_and(|found| found.version == version)
}

/// The user skipped the version found: its band is not shown again, unless they look again
/// themselves.
#[tauri::command]
pub fn main_update_skip(app: AppHandle, version: String) {
    if found_is(&app, &version) {
        settings::update(&app, |settings| settings.skipped_version = Some(version));
        main_window::changed(&app);
    }
}

/// The user closed the band: until Perora starts again.
#[tauri::command]
pub fn main_update_close(app: AppHandle, version: String) {
    if found_is(&app, &version) {
        *lock(&app.state::<Perora>().update.closed) = Some(version);
        main_window::changed(&app);
    }
}

/// The user asked for the new version: fetched, checked, and installed, which ends this Perora and
/// starts the new one. Not while a capture is in hand (installing would throw it away): asked
/// before fetching, and again before installing, as one may have been taken meanwhile. One install
/// at a time.
#[tauri::command]
pub async fn main_update_install(app: AppHandle) -> Result<(), String> {
    let perora = app.state::<Perora>();
    let update = &perora.update;
    if capture_in_hand(&app) {
        return Err(perora.language().update_capture_in_hand());
    }
    let Some(found) = lock(&update.found).clone() else {
        return Err(perora.language().update_gone());
    };
    {
        let mut getting = lock(&update.getting);
        if getting.is_some() {
            return Ok(());
        }
        *getting = Some(Getting {
            version: found.version.clone(),
            got: 0,
            of: None,
            installing: false,
        });
    }
    main_window::changed(&app);

    let mut told = std::time::Instant::now();
    let fetched = found
        .download(
            |chunk, of| {
                if let Some(getting) = lock(&update.getting).as_mut() {
                    getting.got += chunk as u64;
                    getting.of = of;
                }
                if told.elapsed() >= TOLD_EVERY {
                    told = std::time::Instant::now();
                    main_window::changed(&app);
                }
            },
            || {},
        )
        .await;
    let result = match fetched {
        Err(error) => Err(perora.language().update_failed(&error.to_string())),
        Ok(_) if capture_in_hand(&app) => Err(perora.language().update_capture_in_hand()),
        Ok(bytes) => {
            if let Some(getting) = lock(&update.getting).as_mut() {
                getting.installing = true;
            }
            main_window::changed(&app);
            // On Windows this ends Perora once the installer is running; it returns only when the
            // installer could not be run.
            let installed = found.install(bytes);
            put_back_after_failing(&app);
            installed.map_err(|error| perora.language().update_failed(&error.to_string()))
        }
    };
    *lock(&update.getting) = None;
    main_window::changed(&app);
    if let Err(error) = &result {
        eprintln!("the new version could not be installed: {error}");
    }
    result
}

/// What has to happen before the installer ends this Perora, as quitting would have it: whatever is
/// under way with the sign-in is let finish, and the icon is taken out of the notification area
/// (the process ends without it otherwise, and the icon stays until the pointer passes over it).
/// The new version is told whether the main window was on screen, and not that it starts after
/// windows that were lost (it does not: it starts as a new version).
fn before_installing(app: &AppHandle) {
    let perora = app.state::<Perora>();
    perora.login_cancelled.store(true, Ordering::Relaxed);
    perora.account.settle(crate::QUIT_PATIENCE);
    let shown = main_window::is_wanted(app);
    // The system keeps the environment of a process in step by itself: other threads may read it.
    unsafe {
        if shown {
            std::env::set_var(WINDOW_WAS_SHOWN, "1");
        } else {
            std::env::remove_var(WINDOW_WAS_SHOWN);
        }
    }
    recovery::forget_for_the_next_start();
    app.cleanup_before_exit();
}

/// The installer could not be run after all, and this Perora goes on: as it was before it was to
/// end.
fn put_back_after_failing(app: &AppHandle) {
    app.state::<Perora>()
        .login_cancelled
        .store(false, Ordering::Relaxed);
    crate::put_in_tray_again(app);
}

/// Whether this start is of a new version the user installed from the main window, which is shown
/// again then.
pub fn installed_from_window() -> bool {
    std::env::var_os(WINDOW_WAS_SHOWN).is_some_and(|value| value == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_shows_a_version_not_skipped_or_closed() {
        assert!(!band(None, None, None, false));
        assert!(band(Some("0.2.0"), None, None, false));
        assert!(!band(Some("0.2.0"), Some("0.2.0"), None, false));
        assert!(!band(Some("0.2.0"), None, Some("0.2.0"), false));
        // A newer one than was skipped or closed is shown.
        assert!(band(Some("0.3.0"), Some("0.2.0"), Some("0.2.0"), false));
        // One being fetched is shown, whatever was said of it.
        assert!(band(Some("0.2.0"), Some("0.2.0"), Some("0.2.0"), true));
    }

    #[test]
    fn a_notice_is_given_once_for_a_version_and_only_out_of_sight() {
        assert!(notice_due("0.2.0", false, None, None, None));
        assert!(!notice_due("0.2.0", true, None, None, None));
        assert!(!notice_due("0.2.0", false, None, None, Some("0.2.0")));
        assert!(!notice_due("0.2.0", false, Some("0.2.0"), None, None));
        assert!(!notice_due("0.2.0", false, None, Some("0.2.0"), None));
        assert!(notice_due(
            "0.3.0",
            false,
            Some("0.2.0"),
            Some("0.2.0"),
            Some("0.2.0")
        ));
    }

    #[test]
    fn what_is_said_follows_the_last_look_that_reached_the_releases() {
        // A version found: nothing to say beyond it.
        assert_eq!(Asked::Failed.after(false, true), Asked::Nothing);
        assert_eq!(Asked::Looking.after(true, true), Asked::Nothing);
        // None, asked for: up to date.
        assert_eq!(Asked::Looking.after(true, false), Asked::UpToDate);
        // None, not asked for: what was said is put right, and nothing is brought up.
        assert_eq!(Asked::Failed.after(false, false), Asked::UpToDate);
        assert_eq!(Asked::UpToDate.after(false, false), Asked::UpToDate);
        assert_eq!(Asked::Nothing.after(false, false), Asked::Nothing);
    }
}
