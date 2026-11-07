//! The sign-in to i.f2a.dev, as the Tauri version had it (its `lib.rs`): the same file
//! (`%LOCALAPPDATA%\dev.f2a.perora\sign-in`, so that a sign-in made with 0.1.x is still there),
//! who is signed in as far as the server says, the browser awaited while signing in, signing out,
//! and the user's recent uploads for the account page. What signs in and talks to the server is the
//! core's (`account`, `oidc`, `shots`); here is what the windows show of it and when it is asked.
//!
//! Uploading needs someone signed in and the user's having switched it on: `uploads` says whether
//! both are so, in one place.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::{App, Global, RenderImage};
use perora_core::account::{Account, AccountError};
use perora_core::config::Config;
use perora_core::oidc::OidcError;
use perora_core::platform::{self, Frame, SystemTokenStore, token_store};
use perora_core::settings;
use perora_core::shots::{self, Shot};

use crate::notice::{self, Lasting};
use crate::state::Perora;

/// How long the browser is waited for (as the Tauri version did).
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// How long a renewal of the sign-in under way is waited for as Perora quits: a refresh token is
/// good once, and one cut off halfway would sign the user out.
const QUIT_PATIENCE: Duration = Duration::from_secs(3);
/// How many of the user's uploads the account page shows.
const RECENT: u32 = 8;
/// The size the pictures of the recent uploads are made at: twice what the page shows them at
/// (150 by 96), for screens at twice the scale.
const PICTURE: (f64, f64) = (300.0, 192.0);

/// Someone is signed in (a sign-in is stored). What the server says about them, once asked.
#[derive(Clone, Default)]
pub struct SignedIn {
    pub name: Option<String>,
    /// Whether this account may upload; `None` while the server could not be asked.
    pub may_upload: Option<bool>,
}

/// The user's recent uploads, as the account page shows them.
#[derive(Clone)]
pub enum Recent {
    /// Not asked for yet, or being asked for.
    Asking,
    /// The server could not be asked.
    Unreachable,
    Got(Vec<Upload>),
}

#[derive(Clone)]
pub struct Upload {
    pub id: String,
    /// The page to share: opened by a click, copied by the button.
    pub url: String,
    /// When it was uploaded, on the user's clock (year, month, day, hour, minute).
    pub when: Option<[u16; 5]>,
    pub picture: Option<Arc<RenderImage>>,
}

pub struct Accounts {
    account: Arc<Account<SystemTokenStore>>,
    pub signed_in: Option<SignedIn>,
    /// The browser is awaited: with the address it was sent to (empty until that is known).
    pub waiting: Option<String>,
    /// Set to end the wait for the browser: by the user, or by quitting.
    cancel: Arc<AtomicBool>,
    pub recent: Recent,
    /// The pictures of the uploads, by their ids: not fetched again each time the list is.
    pictures: HashMap<String, Arc<RenderImage>>,
    /// Which asking for the recent uploads is the last: an earlier one's answer is not shown.
    asked: u64,
    /// Counted up at each signing out: what was asked of the server before it says nothing about
    /// who is signed in after it.
    signed_out: u64,
    /// Made up for trying the windows (`pretend`): nothing is asked of the server, nothing changes.
    pretended: bool,
}

impl Global for Accounts {}

/// Makes the account known as Perora starts: signed in as far as a sign-in is stored (so that the
/// windows are right from the first), then the server is asked who it is.
pub fn start(cx: &mut App) {
    let config = Config::from_environment();
    // A sign-in at a development server is kept apart from the real one.
    let file = if config.is_production() {
        "sign-in"
    } else {
        "sign-in.development"
    };
    let folder = settings::cache_directory().unwrap_or_else(std::env::temp_dir);
    let account = Arc::new(Account::new(config, token_store(folder.join(file))));
    let signed_in = stored(&account).then(SignedIn::default);
    cx.set_global(Accounts {
        account,
        signed_in,
        waiting: None,
        cancel: Arc::new(AtomicBool::new(false)),
        recent: Recent::Asking,
        pictures: HashMap::new(),
        asked: 0,
        signed_out: 0,
        pretended: false,
    });
    refresh(cx, |_, _| {});
}

/// A made-up account for trying the windows without the server (`examples/look_main.rs`): signed
/// out, waiting for the browser, or signed in (with a few uploads, none, the server unreachable, or
/// an account that may not upload). Nothing is asked of the server; the sign-in file is in `folder`.
#[allow(dead_code)] // (Only `examples/look_main.rs` makes one up.)
pub fn pretend(how: &str, folder: &std::path::Path, cx: &mut App) {
    let account = Arc::new(Account::new(
        Config::production(),
        token_store(folder.join("sign-in.pretended")),
    ));
    let named = || SignedIn {
        name: Some("tuna".into()),
        may_upload: Some(true),
    };
    let uploads = (0..5)
        .map(|n| Upload {
            id: format!("shot{n}"),
            url: format!("https://i.f2a.dev/s/shot{n}"),
            when: Some([2026, 11, 7, 15 - n as u16, 2]),
            picture: None,
        })
        .collect();
    let (signed_in, waiting, recent) = match how {
        "waiting" => (
            None,
            Some("https://auth.f2a.dev/".to_owned()),
            Recent::Asking,
        ),
        "in" => (Some(named()), None, Recent::Got(uploads)),
        "in-none" => (Some(named()), None, Recent::Got(Vec::new())),
        "in-unreachable" => (Some(SignedIn::default()), None, Recent::Unreachable),
        "in-refused" => (
            Some(SignedIn {
                may_upload: Some(false),
                ..named()
            }),
            None,
            Recent::Asking,
        ),
        _ => (None, None, Recent::Asking),
    };
    cx.set_global(Accounts {
        account,
        signed_in,
        waiting,
        cancel: Arc::new(AtomicBool::new(false)),
        recent,
        pictures: HashMap::new(),
        asked: 0,
        signed_out: 0,
        pretended: true,
    });
}

/// Whether a sign-in is stored. A store that cannot be read right now (the file is locked, the disk
/// is in trouble) is taken as "yes": telling a signed-in user to sign in again would be the worse
/// mistake, and an upload says so if there really is none.
fn stored(account: &Account<SystemTokenStore>) -> bool {
    account.is_signed_in().unwrap_or(true)
}

fn accounts_mut(cx: &mut App) -> Option<&mut Accounts> {
    cx.has_global::<Accounts>()
        .then(|| cx.global_mut::<Accounts>())
}

pub fn get(cx: &App) -> Option<&Accounts> {
    cx.try_global::<Accounts>()
}

/// The account, for what is sent to the server (an upload).
pub fn account(cx: &App) -> Option<Arc<Account<SystemTokenStore>>> {
    get(cx).map(|accounts| accounts.account.clone())
}

pub fn signed_in(cx: &App) -> bool {
    get(cx).is_some_and(|accounts| accounts.signed_in.is_some())
}

/// Whether uploading is among what Perora does: the user switched it on, and someone is signed in.
/// Without it, a capture is copied and saved, and nothing says "upload".
pub fn uploads(cx: &App) -> bool {
    signed_in(cx) && Perora::get(cx).settings().upload_enabled
}

/// Whether an address is one of the server's (`https://host/…`): nothing else is shown, copied or
/// opened on the server's word.
pub fn is_at_server(url: &str, cx: &App) -> bool {
    get(cx).is_some_and(|accounts| {
        url.strip_prefix(accounts.account.config().server.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Has every window draw itself again: the main window (the account page, the switch for
/// uploading), and the previews (whether they offer to upload).
pub fn changed(cx: &mut App) {
    cx.defer(|cx| {
        for window in cx.windows() {
            let _ = window.update(cx, |_, window, _| window.refresh());
        }
    });
}

/// What the thread that signs in says, as it goes.
enum Signing {
    /// The browser is sent there.
    Sent(String),
    Done(Result<(), AccountError>),
}

/// Sends the browser to the identity provider and waits for it to come back (five minutes at
/// most). Asked again while it is awaited (the tab was closed, perhaps): the same address again.
pub fn sign_in(cx: &mut App) {
    let Some(accounts) = accounts_mut(cx).filter(|accounts| !accounts.pretended) else {
        return;
    };
    if let Some(url) = accounts.waiting.as_deref() {
        if !url.is_empty()
            && let Err(detail) = platform::open_in_browser(url)
        {
            eprintln!("sign-in: the browser could not be opened: {detail}");
        }
        return;
    }
    // (Empty until the address is known: a second click meanwhile does not start a second one.)
    accounts.waiting = Some(String::new());
    accounts.cancel.store(false, Ordering::Relaxed);
    let account = accounts.account.clone();
    let cancel = accounts.cancel.clone();
    changed(cx);

    let (said, heard) = async_channel::unbounded();
    // On a thread of its own: it waits for a person, for minutes.
    let started = std::thread::Builder::new()
        .name("sign-in".into())
        .spawn(move || {
            let outcome = account.begin_login().and_then(|login| {
                let url = login.authorize_url().to_owned();
                let _ = said.try_send(Signing::Sent(url.clone()));
                if let Err(detail) = platform::open_in_browser(&url) {
                    eprintln!("sign-in: the browser could not be opened: {detail}");
                }
                account.finish_login(login, LOGIN_TIMEOUT, &cancel)
            });
            let _ = said.try_send(Signing::Done(outcome));
        });
    if let Err(error) = started {
        eprintln!("sign-in: could not be started: {error}");
        if let Some(accounts) = accounts_mut(cx) {
            accounts.waiting = None;
        }
        changed(cx);
        return;
    }
    cx.spawn(async move |cx| {
        while let Ok(signing) = heard.recv().await {
            cx.update(|cx| match signing {
                Signing::Sent(url) => {
                    if let Some(accounts) = accounts_mut(cx) {
                        accounts.waiting = Some(url);
                    }
                    changed(cx);
                }
                Signing::Done(outcome) => signed(outcome, cx),
            });
        }
    })
    .detach();
}

/// The browser came back, or did not.
fn signed(outcome: Result<(), AccountError>, cx: &mut App) {
    let Some(accounts) = accounts_mut(cx) else {
        return;
    };
    if outcome.is_ok() {
        // Before the wait is over: the window never shows "not signed in" between the two.
        accounts.signed_in = Some(SignedIn::default());
        forget_uploads(accounts);
    }
    accounts.waiting = None;
    changed(cx);
    match outcome {
        Ok(()) => refresh(cx, |name, cx| {
            let said = Perora::get(cx)
                .language()
                .signed_in(name.as_deref().unwrap_or("?"));
            notice::say(said, Lasting::Brief, None, cx);
            // (The account page is likely open: it was signed in from there.)
            ask_recent(cx);
        }),
        // The user gave up, or Perora is quitting: nothing to say.
        Err(AccountError::Oidc(OidcError::Cancelled)) => refresh(cx, |_, _| {}),
        Err(error) => {
            let said = Perora::get(cx).language().account_failed(&error);
            refresh(cx, |_, _| {});
            notice::say(said, Lasting::Long, None, cx);
        }
    }
}

/// Ends waiting for the browser.
pub fn cancel_sign_in(cx: &mut App) {
    if let Some(accounts) = get(cx) {
        accounts.cancel.store(true, Ordering::Relaxed);
    }
}

/// Ends the sign-in on this machine. When that does not work, what is so is shown, not what was
/// asked for.
pub fn sign_out(cx: &mut App) {
    let Some(accounts) = accounts_mut(cx).filter(|accounts| !accounts.pretended) else {
        return;
    };
    accounts.signed_out += 1;
    let account = accounts.account.clone();
    cx.spawn(async move |cx| {
        let outcome = cx
            .background_executor()
            .spawn(async move { account.sign_out() })
            .await;
        cx.update(|cx| match outcome {
            Ok(()) => {
                if let Some(accounts) = accounts_mut(cx) {
                    accounts.signed_in = None;
                    forget_uploads(accounts);
                }
                changed(cx);
            }
            Err(error) => {
                let said = Perora::get(cx).language().account_failed(&error);
                refresh(cx, |_, _| {});
                notice::say(said, Lasting::Long, None, cx);
            }
        });
    })
    .detach();
}

/// Asks the server who is signed in, and shows it; then `then` is told the name, when the server
/// gave one. A sign-in that is stored counts while the server cannot be asked: without a name.
pub fn refresh(cx: &mut App, then: impl FnOnce(Option<String>, &mut App) + 'static) {
    let Some(accounts) = get(cx).filter(|accounts| !accounts.pretended) else {
        return;
    };
    let account = accounts.account.clone();
    let signed_out = accounts.signed_out;
    cx.spawn(async move |cx| {
        let (session, stored) = cx
            .background_executor()
            .spawn(async move { (shots::session(&account), stored(&account)) })
            .await;
        cx.update(|cx| {
            let Some(accounts) = accounts_mut(cx) else {
                return;
            };
            // Signed out since it was asked: what it says is of before.
            if accounts.signed_out != signed_out {
                return;
            }
            let name = match session {
                // (Asked a moment ago: the user may have signed out since, elsewhere.)
                Ok(session) if stored => {
                    let may_upload = session.permissions.iter().any(|granted| {
                        matches!(granted.as_str(), "*" | "shots.*" | "shots.upload")
                    });
                    accounts.signed_in = Some(SignedIn {
                        name: Some(session.name.clone()),
                        may_upload: Some(may_upload),
                    });
                    Some(session.name)
                }
                Ok(_) => {
                    accounts.signed_in = None;
                    None
                }
                Err(shots::ApiError::SignedOut) => {
                    accounts.signed_in = None;
                    None
                }
                Err(_) => {
                    accounts.signed_in =
                        stored.then(|| accounts.signed_in.clone().unwrap_or_default());
                    None
                }
            };
            changed(cx);
            then(name, cx);
        });
    })
    .detach();
}

/// The server said that no one is signed in (an upload was refused so): shown so.
pub fn signed_out_by_server(cx: &mut App) {
    if let Some(accounts) = accounts_mut(cx) {
        accounts.signed_in = None;
        forget_uploads(accounts);
    }
    changed(cx);
}

/// Who is signed in changed: the uploads of whoever was are forgotten, and what was being asked
/// for them comes to nothing.
fn forget_uploads(accounts: &mut Accounts) {
    accounts.recent = Recent::Asking;
    accounts.pictures.clear();
    accounts.asked += 1;
}

/// An upload got there: the account page lists it, when it is on screen (otherwise it asks as it
/// appears: the pictures are not fetched for a page nobody looks at).
pub fn uploaded(cx: &mut App) {
    if crate::main_window::on_account_page(cx) {
        ask_recent(cx);
    }
}

/// Asks the server for the user's recent uploads (when the account page appears, the main window
/// comes back to the front, an upload is done), and for the pictures of those not seen before.
pub fn ask_recent(cx: &mut App) {
    let Some(accounts) = accounts_mut(cx) else {
        return;
    };
    if accounts.signed_in.is_none() || accounts.pretended {
        return;
    }
    accounts.asked += 1;
    let asked = accounts.asked;
    let account = accounts.account.clone();
    let known: HashMap<String, Arc<RenderImage>> = accounts.pictures.clone();
    let server = account.config().server.clone();
    cx.spawn(async move |cx| {
        let listed = cx
            .background_executor()
            .spawn({
                let account = account.clone();
                async move { shots::recent(&account, RECENT) }
            })
            .await;
        let shots: Vec<Shot> = match listed {
            Ok(shots) => shots
                .into_iter()
                // Only what is the server's own: these addresses are opened and copied.
                .filter(|shot| at(&server, &shot.url) && at(&server, &shot.image_url))
                .collect(),
            Err(error) => {
                cx.update(|cx| {
                    // (An answer to an asking since outdone, a sign-in since ended, says nothing.)
                    if get(cx).is_none_or(|accounts| accounts.asked != asked) {
                        return;
                    }
                    if matches!(error, shots::ApiError::SignedOut) {
                        signed_out_by_server(cx);
                        return;
                    }
                    if let Some(accounts) = accounts_mut(cx) {
                        // (What was there stays, should there be something.)
                        if matches!(accounts.recent, Recent::Asking) {
                            accounts.recent = Recent::Unreachable;
                        }
                    }
                    changed(cx);
                });
                return;
            }
        };
        // The list first, with the pictures already seen; then each new picture as it comes.
        let shown = |shots: &[Shot], pictures: &HashMap<String, Arc<RenderImage>>| {
            shots
                .iter()
                .map(|shot| Upload {
                    id: shot.id.clone(),
                    url: shot.url.clone(),
                    when: shot.uploaded_at().and_then(platform::to_local_time),
                    picture: pictures.get(&shot.id).cloned(),
                })
                .collect::<Vec<_>>()
        };
        let mut pictures = known;
        let first = shown(&shots, &pictures);
        let still = cx.update(|cx| {
            let Some(accounts) = accounts_mut(cx) else {
                return false;
            };
            if accounts.asked != asked || accounts.signed_in.is_none() {
                return false;
            }
            accounts.recent = Recent::Got(first);
            changed(cx);
            true
        });
        if !still {
            return;
        }
        let unseen: Vec<Shot> = shots
            .iter()
            .filter(|shot| !pictures.contains_key(&shot.id))
            .cloned()
            .collect();
        for shot in &unseen {
            let fetched = cx
                .background_executor()
                .spawn({
                    let account = account.clone();
                    let url = shot.image_url.clone();
                    async move { picture(&account, &url) }
                })
                .await;
            let Some(fetched) = fetched else {
                continue;
            };
            pictures.insert(shot.id.clone(), fetched);
            let now = shown(&shots, &pictures);
            let still = cx.update(|cx| {
                let Some(accounts) = accounts_mut(cx) else {
                    return false;
                };
                if accounts.asked != asked || accounts.signed_in.is_none() {
                    return false;
                }
                accounts.pictures = pictures.clone();
                accounts.recent = Recent::Got(now);
                changed(cx);
                true
            });
            if !still {
                return;
            }
        }
    })
    .detach();
}

fn at(server: &str, url: &str) -> bool {
    url.strip_prefix(server)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// The picture of an upload, as the page shows it (covering its place: as small as it can be while
/// filling it, or as it is when it is smaller). `None` when it could not be had.
fn picture(account: &Account<SystemTokenStore>, url: &str) -> Option<Arc<RenderImage>> {
    let bytes = shots::image(account, url)
        .map_err(|error| eprintln!("recent uploads: a picture was not fetched: {error}"))
        .ok()?;
    let frame: Frame = platform::read_image_bytes(&bytes, 64_000_000, 16_384)
        .map_err(|error| eprintln!("recent uploads: a picture was not read: {error:?}"))
        .ok()?;
    let (width, height) = (frame.width() as f64, frame.height() as f64);
    let scale = (PICTURE.0 / width).max(PICTURE.1 / height).min(1.0);
    let fitted = frame.scaled_to_fit(
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    );
    crate::selection::drawable(fitted)
}

/// Puts the link of an upload on the clipboard (only one of the server's).
pub fn copy_link(url: &str, cx: &mut App) -> bool {
    if !is_at_server(url, cx) {
        return false;
    }
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(url.to_owned())) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("clipboard: the link was not put there: {error}");
            false
        }
    }
}

/// Opens the page of an upload in the browser (only one of the server's).
pub fn open_page(url: &str, cx: &mut App) {
    if !is_at_server(url, cx) {
        return;
    }
    if let Err(detail) = platform::open_in_browser(url) {
        eprintln!("recent uploads: the page could not be opened: {detail}");
    }
}

/// Perora is quitting: the browser is not waited for any more, and a renewal of the sign-in under
/// way is let finish (three seconds at most).
pub fn settle(cx: &mut App) {
    if let Some(accounts) = get(cx) {
        accounts.cancel.store(true, Ordering::Relaxed);
        accounts.account.settle(QUIT_PATIENCE);
    }
}

/// Perora was to quit for an update's installer, which could not be run: it goes on, and the
/// browser may be waited for again.
pub fn unsettle(cx: &mut App) {
    if let Some(accounts) = get(cx) {
        accounts.cancel.store(false, Ordering::Relaxed);
    }
}
