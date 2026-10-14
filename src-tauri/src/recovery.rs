//! Getting back up when the web views are gone.
//!
//! All of Perora's windows are drawn by one process of the system's web view. When that process ends
//! on its own (other software that hooks into every process can take it down, a moment after it
//! started), every window is left an empty frame and nothing says so. Perora then starts itself
//! again: a start is quick, and of what it was doing only an upload goes on without its windows (that
//! is waited for). A process that keeps ending is not chased forever: after a few starts in a row
//! Perora stays as it is, without windows, and tells the user; asked to capture or to show its window
//! it says so again.
//!
//! One window's page going the same way (its own process ended) is loaded again, a few times.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::{Perora, main_window};

/// How a start that is a recovery knows it is one: the starts in a row so far, and whether the main
/// window was on screen (`2,1`). In the environment, which the next start inherits.
const VARIABLE: &str = "PERORA_RECOVERY";

/// How many starts in a row are tried.
const MOST_IN_A_ROW: u32 = 2;

/// Up this long, a start counts as having worked: the next failure is the first again.
const SETTLED_AFTER: Duration = Duration::from_secs(60);

/// How often a page whose process ended is loaded again, within that time.
const MOST_RELOADS: usize = 3;

/// How long an upload under way is waited for before Perora starts again.
const UPLOAD_PATIENCE: Duration = Duration::from_secs(30);

/// The windows are gone and are not coming back in this run.
static LOST: AtomicBool = AtomicBool::new(false);

/// The uploads under way.
static UPLOADS: AtomicUsize = AtomicUsize::new(0);

/// An upload, for as long as this lives: Perora does not start again in the middle of one.
pub struct Uploading(());

impl Uploading {
    pub fn begin() -> Self {
        UPLOADS.fetch_add(1, Ordering::AcqRel);
        Self(())
    }
}

impl Drop for Uploading {
    fn drop(&mut self) {
        UPLOADS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Whether an upload is under way.
pub fn uploads_under_way() -> bool {
    UPLOADS.load(Ordering::Acquire) > 0
}

/// What this start was told of the start before it is not passed on to whatever starts next (a new
/// version, started by its installer): that one is no recovery.
pub fn forget_for_the_next_start() {
    // The system keeps the environment of a process in step by itself: other threads may read it.
    unsafe { std::env::remove_var(VARIABLE) };
}

/// Perora is about to start again, because its windows are gone.
static RECOVERING: AtomicBool = AtomicBool::new(false);

/// Whether Perora's windows are not to be counted on: it has given up on them, or is about to start
/// again for their sake. What is to be said then is said another way.
pub fn windows_in_doubt() -> bool {
    LOST.load(Ordering::Acquire) || RECOVERING.load(Ordering::Acquire)
}

/// Whether Perora has given up on its windows. Then nothing that needs one is to be started: the
/// user is told again instead, and this returns true.
pub fn windows_lost(app: &AppHandle) -> bool {
    let lost = LOST.load(Ordering::Acquire);
    if lost {
        crate::notify_by_system(app, app.state::<Perora>().language().windows_lost());
    }
    lost
}

/// No more tries: every window out of sight (an empty frame over the screen cannot be dismissed),
/// and the user told.
fn give_up(app: &AppHandle) {
    if LOST.swap(true, Ordering::AcqRel) {
        return;
    }
    for window in app.webview_windows().values() {
        let _ = window.hide();
    }
    crate::notify_by_system(app, app.state::<Perora>().language().windows_lost());
}

/// What this start was told by the one before it, and when it began.
struct Start {
    began: Instant,
    in_a_row: u32,
    window_shown: Option<bool>,
}

fn start() -> &'static Start {
    static START: OnceLock<Start> = OnceLock::new();
    START.get_or_init(|| {
        let told = std::env::var(VARIABLE).ok();
        let told = told.as_deref().and_then(told_by_the_start_before);
        Start {
            began: Instant::now(),
            in_a_row: told.map_or(0, |(in_a_row, _)| in_a_row),
            window_shown: told.map(|(_, shown)| shown),
        }
    })
}

fn told_by_the_start_before(value: &str) -> Option<(u32, bool)> {
    let (in_a_row, shown) = value.split_once(',')?;
    Some((in_a_row.parse().ok()?, shown == "1"))
}

/// Whether the main window is to be shown at this start, when the start is a recovery: as it was
/// before. `None` for a start by the user or the system. Also marks when this start began: to be
/// called early.
pub fn window_shown_before() -> Option<bool> {
    start().window_shown
}

/// Has Perora start again when the web view process behind this window ends on its own, and the page
/// loaded again when only its own process does.
#[cfg(windows)]
pub fn watch(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    };
    use webview2_com::ProcessFailedEventHandler;

    let app = window.app_handle().clone();
    let _ = window.with_webview(move |webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else {
            return;
        };
        let mut reloads: Vec<Instant> = Vec::new();
        let handler = ProcessFailedEventHandler::create(Box::new(move |view, failure| {
            let Some(failure) = failure else {
                return Ok(());
            };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            failure.ProcessFailedKind(&mut kind)?;
            if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                recover(&app);
            } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                && let Some(view) = view
            {
                // A page that ends its process every time it is loaded is not loaded forever.
                reloads.retain(|at| at.elapsed() < SETTLED_AFTER);
                if reloads.len() >= MOST_RELOADS {
                    give_up(&app);
                } else if !LOST.load(Ordering::Acquire) {
                    reloads.push(Instant::now());
                    let _ = view.Reload();
                }
            }
            Ok(())
        }));
        let mut registration = 0i64;
        let _ = core.add_ProcessFailed(&handler, &mut registration);
    });
}

#[cfg(not(windows))]
pub fn watch(_: &WebviewWindow) {}

/// Starts Perora again, once: every window reports the same failure.
#[cfg_attr(not(windows), allow(dead_code))]
fn recover(app: &AppHandle) {
    if RECOVERING.swap(true, Ordering::AcqRel) {
        return;
    }

    let start = start();
    let in_a_row = if start.began.elapsed() < SETTLED_AFTER {
        start.in_a_row
    } else {
        0
    };
    if in_a_row >= MOST_IN_A_ROW || LOST.load(Ordering::Acquire) {
        return give_up(app);
    }

    let shown = u8::from(main_window::is_wanted(app));
    // The system keeps the environment of a process in step by itself: other threads may read it.
    unsafe { std::env::set_var(VARIABLE, format!("{},{shown}", in_a_row + 1)) };
    // Not from here: this is the web view calling, on the thread the windows live on.
    let app = app.clone();
    std::thread::spawn(move || {
        // An upload needs no window: it ends in its notification first, with its link copied.
        let asked = Instant::now();
        while UPLOADS.load(Ordering::Acquire) > 0 && asked.elapsed() < UPLOAD_PATIENCE {
            std::thread::sleep(Duration::from_millis(100));
        }
        app.restart()
    });
}

#[cfg(test)]
mod tests {
    use super::told_by_the_start_before;

    #[test]
    fn a_recovery_is_told_how_many_came_before_and_whether_the_window_was_shown() {
        assert_eq!(told_by_the_start_before("1,1"), Some((1, true)));
        assert_eq!(told_by_the_start_before("2,0"), Some((2, false)));
        assert_eq!(told_by_the_start_before(""), None);
        assert_eq!(told_by_the_start_before("soon"), None);
        assert_eq!(told_by_the_start_before("x,1"), None);
    }
}
