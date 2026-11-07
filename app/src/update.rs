//! Updates, as the Tauri version had them (its `update.rs`). Perora asks the releases on GitHub
//! whether there is a newer version: a while after it starts, every six hours after that, and
//! whenever the user asks (the general page). What it finds is shown in a band at the top of the
//! main window, with what changed in it, and said once in a notice when that window is out of
//! sight. Nothing is installed until the user says so; then the new version is fetched, its
//! signature checked, and its installer run, which closes Perora and starts the new one in its
//! place. What is asked, fetched and checked is the core's (`update`).
//!
//! On its own, only the installed Perora looks: a build being worked on is not of the releases (it
//! looks when asked).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Global};
use perora_core::platform;
use perora_core::update::{self, Release};

use crate::notice::{self, Click, Lasting};
use crate::state::Perora;

/// How long after the start the first look is: Perora has other things to do as it starts.
const FIRST_LOOK: Duration = Duration::from_secs(20);

/// How long between looks after that.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// How often the main window hears how far the download is.
const TOLD_EVERY: Duration = Duration::from_millis(250);

/// How long the window is given to show that the installer is starting, before Perora is held up
/// starting it.
const DRAWN_FIRST: Duration = Duration::from_millis(100);

/// What 0.1.x (the Tauri version) sets for the version it installs when its main window was on
/// screen: the new version shows it again as it starts. (This version tells the next one by its
/// arguments instead: `update::installer_parameters`.)
const WINDOW_WAS_SHOWN: &str = "PERORA_UPDATED_FROM_WINDOW";

/// This Perora's version: the one the releases are compared with.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How long after the start what 0.1.x left is cleared: started by an update from it, the WebView2
/// processes of the version this one replaced may still be closing, their files held.
const LEFT_BEHIND_AFTER: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Updates {
    /// A newer version, when one was found.
    found: Option<Release>,
    /// A look is under way.
    looking: bool,
    /// The user asked to look while a look was under way: that one tells them what it came to.
    asked_meanwhile: bool,
    /// What the last look the user asked for came to, for the general page.
    asked: Asked,
    /// The version being fetched or installed, and how far that is.
    getting: Option<Getting>,
    /// The version whose band the user closed: not shown again until Perora starts again.
    closed: Option<String>,
    /// The version a notice was given for: one notice for each version a run finds.
    noticed: Option<String>,
    /// Why the last install did not happen, as the user is told it: in the band and on the
    /// general page, until they try again.
    failure: Option<String>,
    /// The band shows what changed in the version it offers.
    changes_open: bool,
}

impl Global for Updates {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
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
    /// Bytes so far, and of how many when the releases say: shared with the download as it goes.
    progress: Arc<Mutex<(u64, Option<u64>)>>,
    /// Fetched, and its signature checked: the installer is starting, and Perora about to close.
    installing: bool,
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
pub struct View {
    /// The newer version there is, if one was found (shown on the general page, whatever the band).
    pub available: Option<String>,
    /// Whether the band at the top shows it.
    pub band: bool,
    pub asked: Asked,
    /// Being fetched: bytes so far, and of how many when that is known; or being installed.
    pub got: Option<u64>,
    pub of: Option<u64>,
    pub installing: bool,
    /// Whether installing would throw away a capture that is still in hand: the button waits until
    /// that is done with.
    pub capture_in_hand: bool,
    pub failure: Option<String>,
    /// What changed in the version offered, in the language shown (Markdown).
    pub changes: Option<String>,
    pub changes_open: bool,
}

impl View {
    pub fn getting(&self) -> bool {
        self.got.is_some() || self.installing
    }
}

pub fn view(cx: &mut App) -> View {
    let skipped = Perora::get(cx).settings().skipped_version;
    let code = Perora::get(cx).language().code().to_owned();
    let capture_in_hand = capture_in_hand(cx);
    let updates = cx.default_global::<Updates>();
    let getting = updates.getting.clone();
    // (While one is being fetched, that one: whatever a look meanwhile found.)
    let available = getting
        .as_ref()
        .map(|getting| getting.version.clone())
        .or_else(|| updates.found.as_ref().map(|found| found.version.clone()));
    let changes = updates
        .found
        .as_ref()
        .filter(|found| Some(&found.version) == available.as_ref())
        .and_then(|found| found.changes(&code))
        .map(str::to_owned);
    let progress = getting
        .as_ref()
        .map(|getting| *getting.progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    View {
        band: band(
            available.as_deref(),
            skipped.as_deref(),
            updates.closed.as_deref(),
            getting.is_some(),
        ),
        available,
        asked: updates.asked,
        got: progress.map(|(got, _)| got),
        of: progress.and_then(|(_, of)| of),
        installing: getting.is_some_and(|getting| getting.installing),
        capture_in_hand,
        failure: updates.failure.clone(),
        changes,
        changes_open: updates.changes_open,
    }
}

/// A capture is being taken, looked at, or is on its way somewhere (to the preview, the clipboard,
/// a file, the server): installing would end Perora under it.
fn capture_in_hand(cx: &mut App) -> bool {
    crate::preview::holds_a_capture(cx)
        || crate::capture::busy(cx)
        || crate::capture::on_its_way()
}

fn changed(cx: &mut App) {
    crate::account::changed(cx);
}

/// Looks for a newer version now and then, from the start until Perora quits: only when this is
/// the installed Perora (a build being worked on is not the version the releases are of).
pub fn keep_looking(cx: &mut App) {
    if !platform::is_installed_copy() {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(FIRST_LOOK).await;
        loop {
            cx.update(|cx| look(false, cx));
            cx.background_executor().timer(EVERY).await;
        }
    })
    .detach();
}

/// The user asked to look now (the general page).
pub fn look_now(cx: &mut App) {
    look(true, cx);
}

/// Asks the releases whether there is a newer version. `asked`: the user asked, and is told what
/// it came to; a version they skipped or closed is offered again. Otherwise a look that fails says
/// nothing.
fn look(asked: bool, cx: &mut App) {
    let updates = cx.default_global::<Updates>();
    if updates.getting.is_some() {
        return;
    }
    if asked {
        updates.asked = Asked::Looking;
        updates.asked_meanwhile = true;
    }
    if updates.looking {
        // The look under way tells the user.
        changed(cx);
        return;
    }
    updates.looking = true;
    changed(cx);
    cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move { update::newest(VERSION) })
            .await;
        cx.update(|cx| looked(result, cx));
    })
    .detach();
}

fn looked(result: Result<Option<Release>, update::Error>, cx: &mut App) {
    let skipped = Perora::get(cx).settings().skipped_version;
    let on_screen = crate::main_window::on_screen_now(cx).is_some();
    let updates = cx.default_global::<Updates>();
    updates.looking = false;
    // (Why an install did not happen is old news once the releases are asked again.)
    updates.failure = None;
    // Asked while this look was under way, or before it: either way, the user hears of it.
    let asked = std::mem::take(&mut updates.asked_meanwhile);
    let found = match result {
        Ok(found) => found,
        Err(error) => {
            eprintln!("could not look for a newer version: {error}");
            if asked {
                updates.asked = Asked::Failed;
            }
            changed(cx);
            return;
        }
    };
    let version = found.as_ref().map(|found| found.version.clone());
    updates.found = found;
    updates.asked = updates.asked.after(asked, version.is_some());
    let Some(version) = version else {
        changed(cx);
        return;
    };
    // Asked for, it is shown, skipped or not, closed or not.
    let mut skipped = skipped;
    if asked {
        updates.closed = None;
        if skipped.as_ref() == Some(&version) {
            skipped = None;
            Perora::get(cx).change(|settings| settings.skipped_version = None);
        }
    }
    // Out of sight, the band is not seen: a notice says it, once for each version.
    let updates = cx.default_global::<Updates>();
    let due = notice_due(
        &version,
        on_screen,
        skipped.as_deref(),
        updates.closed.as_deref(),
        updates.noticed.as_deref(),
    );
    if due {
        updates.noticed = Some(version.clone());
    }
    changed(cx);
    if due {
        let said = Perora::get(cx)
            .language()
            .text("notify.updateFound", &[("version", &version)]);
        notice::say(said, Lasting::Long, Some(Click::OpenMain), cx);
    }
}

/// The version found, if it is the one the window showed (`version`): what the user did was about
/// that one, not one a look found meanwhile.
fn found_is(version: &str, cx: &mut App) -> bool {
    cx.default_global::<Updates>()
        .found
        .as_ref()
        .is_some_and(|found| found.version == version)
}

/// The user skipped the version found: its band is not shown again, unless they look again
/// themselves.
pub fn skip(version: &str, cx: &mut App) {
    if found_is(version, cx) {
        let version = version.to_owned();
        Perora::get(cx).change(|settings| settings.skipped_version = Some(version));
        changed(cx);
    }
}

/// The user closed the band: until Perora starts again.
pub fn close(version: &str, cx: &mut App) {
    if found_is(version, cx) {
        cx.default_global::<Updates>().closed = Some(version.to_owned());
        changed(cx);
    }
}

/// The band shows what changed, or no longer does.
pub fn toggle_changes(cx: &mut App) {
    let updates = cx.default_global::<Updates>();
    updates.changes_open = !updates.changes_open;
    changed(cx);
}

/// The user asked for the new version: fetched, checked, and installed, which ends this Perora and
/// starts the new one. Not while a capture is in hand (installing would throw it away): asked
/// before fetching, and again before installing, as one may have been taken meanwhile. One install
/// at a time.
pub fn install(cx: &mut App) {
    let language = Perora::get(cx).language();
    if cx.default_global::<Updates>().getting.is_some() {
        return;
    }
    // (The band and the general page say so as long as it is so: nothing to keep saying after.)
    if capture_in_hand(cx) {
        changed(cx);
        return;
    }
    let updates = cx.default_global::<Updates>();
    let Some(found) = updates.found.clone() else {
        fail(language.text("error.updateGone", &[]), cx);
        return;
    };
    let progress = Arc::new(Mutex::new((0, None)));
    updates.failure = None;
    updates.getting = Some(Getting {
        version: found.version.clone(),
        progress: progress.clone(),
        installing: false,
    });
    changed(cx);

    // The window hears how far it is, as long as it is fetched.
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(TOLD_EVERY).await;
            let fetching = cx.update(|cx| {
                cx.default_global::<Updates>()
                    .getting
                    .as_ref()
                    .is_some_and(|getting| !getting.installing)
            });
            if !fetching {
                return;
            }
            cx.update(changed);
        }
    })
    .detach();

    cx.spawn(async move |cx| {
        let fetched = cx
            .background_executor()
            .spawn({
                let found = found.clone();
                async move {
                    update::fetch(&found, |got, of| {
                        *progress.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
                            (got, of);
                    })
                }
            })
            .await;
        cx.update(|cx| fetched_as(found, fetched, cx));
    })
    .detach();
}

/// The installer is here (or is not): run, unless a capture was taken meanwhile.
fn fetched_as(found: Release, fetched: Result<Vec<u8>, update::Error>, cx: &mut App) {
    let language = Perora::get(cx).language();
    let bytes = match fetched {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("the new version could not be fetched: {error}");
            fail(
                language.text("error.update", &[("detail", &error.to_string())]),
                cx,
            );
            return;
        }
    };
    // A capture taken meanwhile: not installed under it. (The band says why as long as it is so;
    // the user installs once it is dealt with.)
    if capture_in_hand(cx) {
        cx.default_global::<Updates>().getting = None;
        changed(cx);
        return;
    }
    if let Some(getting) = cx.default_global::<Updates>().getting.as_mut() {
        getting.installing = true;
    }
    changed(cx);
    // Drawn as installing before the installer takes over (a moment later Perora is gone): what
    // follows holds the application up to a few seconds.
    cx.spawn(async move |cx| {
        cx.background_executor().timer(DRAWN_FIRST).await;
        cx.update(|cx| run(found, bytes, cx));
    })
    .detach();
}

/// Runs the installer of a version fetched and checked, which ends this Perora.
fn run(found: Release, bytes: Vec<u8>, cx: &mut App) {
    let language = Perora::get(cx).language();
    // (Once more: one may have been taken while the window drew.)
    if capture_in_hand(cx) {
        cx.default_global::<Updates>().getting = None;
        changed(cx);
        return;
    }
    // As quitting has it: whatever is under way with the sign-in is let finish (a refresh token is
    // good once), before the installer ends this Perora.
    crate::account::settle(cx);
    // The new version comes back as this one is: with its window, if it was open (minimized or
    // not).
    let in_tray = !crate::main_window::is_open(cx);
    match platform::run_installer(&bytes, &found.version, update::installer_parameters(in_tray)) {
        // The icon, the shortcut and the claim to be the one Perora go before the installer looks
        // for a Perora still running.
        Ok(()) => crate::quit(cx),
        Err(detail) => {
            eprintln!("the new version could not be installed: {detail}");
            // Perora goes on: a sign-in in the browser may be waited for again.
            crate::account::unsettle(cx);
            fail(language.text("error.update", &[("detail", &detail)]), cx);
        }
    }
}

/// The install did not happen: why, in the band and on the general page.
fn fail(why: String, cx: &mut App) {
    let updates = cx.default_global::<Updates>();
    updates.getting = None;
    updates.failure = Some(why);
    changed(cx);
}

/// What 0.1.x (the Tauri version) kept and this version has no use for: WebView2's data,
/// `%LOCALAPPDATA%\dev.f2a.perora\EBWebView` (its caches, and the editor's last strengths, kind of
/// hiding and font, which this version keeps in editor-settings.json; 170 MB or so). Cleared a while
/// after a start that finds it: an update from 0.1.x leaves it behind (owner, 2026-11-07: 「過去
/// バージョンからの起動を想定して消す機能入れておこうか」). What cannot be removed yet (a file still
/// held) is tried again at the next start.
pub fn clear_what_0_1_left(cx: &mut App) {
    let Some(folder) =
        perora_core::settings::cache_directory().map(|cache| cache.join("EBWebView"))
    else {
        return;
    };
    if !folder.is_dir() {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(LEFT_BEHIND_AFTER).await;
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = std::fs::remove_dir_all(&folder) {
                    eprintln!("what 0.1.x left could not all be removed: {error}");
                }
            })
            .await;
    })
    .detach();
}

/// Whether this start is of a new version the user installed from the main window of 0.1.x, which
/// is shown again then. (Not handed on to what this Perora starts.)
pub fn installed_from_window() -> bool {
    let set = std::env::var_os(WINDOW_WAS_SHOWN).is_some_and(|value| value == "1");
    // SAFETY: as Perora starts, before any other thread reads the environment.
    unsafe { std::env::remove_var(WINDOW_WAS_SHOWN) };
    set
}

/// Made up for trying the windows (`examples/look_main.rs`): a version found (`found`), and where
/// it is (`downloading`, `installing`, `failed`, `open`: what changed shown in the band).
#[allow(dead_code)] // (Only `examples/look_main.rs` makes one up.)
pub fn pretend(how: &str, cx: &mut App) {
    // (`+skipped`: a version was passed over, and what changed in both is shown.)
    let skipped = how.ends_with("+skipped");
    let how = how.trim_end_matches("+skipped");
    let latest = r###"{"version": "9.9.9",
        "changes": {
            "ja": "## v9.9.9\n\n見た目を確かめるためのバージョンです。\n\n### 変わったこと\n\n- **編集画面**の切り抜きが速くなりました\n  - 大きい画像で約 2 倍\n- アップロードの失敗を `Ctrl + Enter` でやり直せます\n\n### 直したこと\n\n- 通知が二重に出ることがあった",
            "en": "## v9.9.9\n\nA version made up to try the looks.\n\n### Changes\n\n- Cropping in the **editor** is faster\n  - About twice as fast on large images\n- A failed upload can be tried again with `Ctrl + Enter`\n\n### Fixes\n\n- A notice could be shown twice"
        },
        "platforms": {"windows-x86_64": {"url": "https://example.com/a.exe", "signature": "s"}}}"###;
    let history = r###"{"ja": "## v9.9.9\n\n### 変わったこと\n\n- **編集画面**の切り抜きが速くなりました\n\n## v9.9.8\n\n### 直したこと\n\n- 通知が二重に出ることがあった",
        "en": "## v9.9.9\n\n### Changes\n\n- Cropping in the **editor** is faster\n\n## v9.9.8\n\n### Fixes\n\n- A notice could be shown twice"}"###;
    let latest = if skipped {
        latest.replacen("\"platforms\"", &format!("\"history\": {history},\n\"platforms\""), 1)
    } else {
        latest.to_owned()
    };
    let found = update::newer(latest.as_bytes(), VERSION).ok().flatten();
    let failure = Perora::get(cx).language().text(
        "error.update",
        &[("detail", "the download stopped: nothing came for 30 seconds")],
    );
    let updates = cx.default_global::<Updates>();
    updates.found = found;
    let getting = |installing: bool, got: u64| Getting {
        version: "9.9.9".into(),
        progress: Arc::new(Mutex::new((got, Some(8_000_000)))),
        installing,
    };
    match how {
        "downloading" => updates.getting = Some(getting(false, 3_000_000)),
        "installing" => updates.getting = Some(getting(true, 8_000_000)),
        "failed" => updates.failure = Some(failure),
        "open" => updates.changes_open = true,
        _ => {}
    }
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
