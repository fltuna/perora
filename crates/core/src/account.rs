//! Staying signed in. The refresh token is the one thing kept between runs, in the operating system's
//! protected storage; access tokens live in memory for their few minutes and are renewed as needed.
//!
//! The identity provider hands out a new refresh token with every renewal and the old one stops working.
//! So renewals happen one at a time, and the new token is stored before anything else is done with the
//! answer: a token that was spent and not replaced means signing in again.

use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::oidc::{self, Discovery, Login, OidcError, Tokens};
use crate::platform::{PlatformError, TokenStore};

/// An access token is renewed when it has less than this left, so that it does not expire on its way.
const RENEW_BEFORE_EXPIRY: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    /// Nobody is signed in, or the sign-in has ended (expired, revoked, the account disabled).
    #[error("not signed in")]
    SignedOut,

    #[error(transparent)]
    Oidc(#[from] OidcError),

    #[error("the sign-in could not be stored: {0}")]
    Store(#[from] PlatformError),
}

pub type Result<T> = std::result::Result<T, AccountError>;

pub struct Account<S> {
    config: Config,
    agent: ureq::Agent,
    store: S,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    discovery: Option<Discovery>,
    access: Option<Access>,
    /// A refresh token that was issued and could not be stored. It is the only one that still works,
    /// so it is used from here, and stored as soon as storing works again.
    unstored: Option<String>,
}

struct Access {
    token: String,
    renew_at: Instant,
}

impl<S: TokenStore> Account<S> {
    pub fn new(config: Config, store: S) -> Self {
        Self {
            agent: config.agent(),
            config,
            store,
            state: Mutex::default(),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    /// Whether a sign-in is stored. It may still turn out to have ended when it is next used.
    pub fn is_signed_in(&self) -> Result<bool> {
        Ok(self.store.load()?.is_some())
    }

    /// Starts signing in: the caller opens [`Login::authorize_url`] in the browser and passes the
    /// attempt on to [`Account::finish_login`].
    pub fn begin_login(&self) -> Result<Login> {
        let discovery = self.discovery(&mut self.lock())?;
        Ok(Login::begin(&self.config, &discovery)?)
    }

    /// Waits for the browser to come back, then stores the sign-in.
    pub fn finish_login(&self, login: Login, timeout: Duration, cancel: &AtomicBool) -> Result<()> {
        let discovery = self.discovery(&mut self.lock())?;
        // Not under the lock: this waits for a person.
        let tokens = login.finish(&self.agent, &self.config, &discovery, timeout, cancel)?;

        let mut state = self.lock();
        let Some(refresh_token) = &tokens.refresh_token else {
            return Err(OidcError::Protocol("no refresh token was issued".into()).into());
        };
        self.store.save(refresh_token)?;
        state.unstored = None;
        state.access = Some(Access::from(&tokens));
        Ok(())
    }

    /// An access token good for at least the next half minute, renewed first if need be.
    pub fn access_token(&self) -> Result<String> {
        let mut state = self.lock();
        if let Some(access) = &state.access
            && Instant::now() < access.renew_at
        {
            return Ok(access.token.clone());
        }
        state.access = None;

        let refresh_token = match &state.unstored {
            Some(unstored) => unstored.clone(),
            None => match self.store.load()? {
                Some(stored) => stored,
                None => return Err(AccountError::SignedOut),
            },
        };
        let discovery = self.discovery(&mut state)?;
        let tokens = match oidc::refresh(&self.agent, &self.config, &discovery, &refresh_token) {
            Ok(tokens) => tokens,
            // The token is no longer good and never will be again. Anything else (no network, a server
            // error) leaves the sign-in as it is, to be tried again later.
            Err(OidcError::Refused { error, .. }) if error == "invalid_grant" => {
                state.unstored = None;
                self.store.clear()?;
                return Err(AccountError::SignedOut);
            }
            Err(error) => return Err(error.into()),
        };

        // The old refresh token is spent. If the new one cannot be written down, it is kept in memory
        // and the access token is still handed out: failing here would throw away a sign-in that works.
        // Only a restart before storing works again loses it.
        if let Some(next) = &tokens.refresh_token {
            state.unstored = self.store.save(next).is_err().then(|| next.clone());
        } else if let Some(unstored) = state.unstored.take() {
            state.unstored = self.store.save(&unstored).is_err().then_some(unstored);
        }
        let access = Access::from(&tokens);
        let token = access.token.clone();
        state.access = Some(access);
        Ok(token)
    }

    /// Forgets the access token in memory, so that the next call renews it. For when the server turned
    /// one down that should still have been good.
    pub fn forget_access_token(&self) {
        self.lock().access = None;
    }

    /// Ends the sign-in on this machine and tells the identity provider that its refresh token is done.
    pub fn sign_out(&self) -> Result<()> {
        let mut state = self.lock();
        state.access = None;
        let refresh_token = match state.unstored.take() {
            Some(unstored) => Some(unstored),
            None => self.store.load()?,
        };
        self.store.clear()?;

        // After the local sign-out, which must not depend on the network.
        if let Some(refresh_token) = refresh_token
            && let Ok(discovery) = self.discovery(&mut state)
        {
            oidc::revoke(&self.agent, &self.config, &discovery, &refresh_token);
        }
        Ok(())
    }

    /// Waits, for `patience` at most, until no renewal is under way. For quitting: a renewal cut off
    /// between the answer and the storing of its refresh token would lose the sign-in.
    pub fn settle(&self, patience: Duration) {
        let deadline = Instant::now() + patience;
        while self.state.try_lock().is_err() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // The state holds nothing a panic could leave half-written.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn discovery(&self, state: &mut State) -> Result<Discovery> {
        if state.discovery.is_none() {
            state.discovery = Some(Discovery::fetch(&self.agent, &self.config)?);
        }
        Ok(state.discovery.clone().expect("fetched above"))
    }
}

impl From<&Tokens> for Access {
    fn from(tokens: &Tokens) -> Self {
        let lifetime = Duration::from_secs(tokens.expires_in.unwrap_or(0));
        Self {
            token: tokens.access_token.clone(),
            renew_at: Instant::now() + lifetime.saturating_sub(RENEW_BEFORE_EXPIRY),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::testing::{FakeServer, MemoryStore, Reply, Request};

    /// An identity provider at `base` (filled in once the port is known) that answers token requests
    /// with `token`.
    fn identity_provider(
        token: impl Fn(&Request) -> Reply + Send + 'static,
    ) -> (FakeServer, Config) {
        let base = Arc::new(Mutex::new(String::new()));
        let known = base.clone();
        let server = FakeServer::start(move |request| match request.path.as_str() {
            "/.well-known/openid-configuration" => {
                let base = known.lock().unwrap().clone();
                Reply::json(
                    200,
                    &format!(
                        r#"{{"issuer":"{base}/","authorization_endpoint":"{base}/connect/authorize","token_endpoint":"{base}/connect/token","revocation_endpoint":"{base}/connect/revoke"}}"#
                    ),
                )
            }
            "/connect/token" => token(request),
            "/connect/revoke" => Reply::status(200),
            _ => Reply::status(404),
        });
        *base.lock().unwrap() = server.url("");
        let config = Config {
            issuer: server.url("/"),
            ..Config::production()
        };
        (server, config)
    }

    fn token_requests(server: &FakeServer) -> Vec<Request> {
        server
            .requests()
            .into_iter()
            .filter(|request| request.path == "/connect/token")
            .collect()
    }

    #[test]
    fn without_a_stored_sign_in_there_is_no_token_and_nothing_is_asked() {
        let (server, config) = identity_provider(|_| Reply::status(500));
        let account = Account::new(config, MemoryStore::default());

        assert!(!account.is_signed_in().unwrap());
        assert!(matches!(
            account.access_token(),
            Err(AccountError::SignedOut)
        ));
        assert!(server.requests().is_empty());
    }

    #[test]
    fn a_renewal_stores_the_next_refresh_token_and_the_access_token_is_reused_while_good() {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let (server, config) = identity_provider(move |_| {
            let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
            Reply::json(
                200,
                &format!(r#"{{"access_token":"at{n}","expires_in":300,"refresh_token":"rt{n}"}}"#),
            )
        });
        let account = Account::new(config, MemoryStore::holding("rt0"));

        assert_eq!(account.access_token().unwrap(), "at1");
        assert_eq!(account.access_token().unwrap(), "at1");
        assert_eq!(account.store.current().as_deref(), Some("rt1"));
        assert_eq!(token_requests(&server).len(), 1);

        account.forget_access_token();
        assert_eq!(account.access_token().unwrap(), "at2");

        let sent: Vec<String> = token_requests(&server)
            .iter()
            .map(|request| request.form()["refresh_token"].clone())
            .collect();
        assert_eq!(sent, ["rt0", "rt1"], "each refresh token is used once");
        assert_eq!(*account.store.saved.lock().unwrap(), ["rt1", "rt2"]);
    }

    #[test]
    fn an_access_token_about_to_expire_is_renewed() {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let (_server, config) = identity_provider(move |_| {
            let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
            // Twenty seconds: less than the margin, so never reused.
            Reply::json(
                200,
                &format!(r#"{{"access_token":"at{n}","expires_in":20,"refresh_token":"rt{n}"}}"#),
            )
        });
        let account = Account::new(config, MemoryStore::holding("rt0"));

        assert_eq!(account.access_token().unwrap(), "at1");
        assert_eq!(account.access_token().unwrap(), "at2");
    }

    #[test]
    fn a_refresh_token_that_is_turned_down_ends_the_sign_in() {
        let (_server, config) =
            identity_provider(|_| Reply::json(400, r#"{"error":"invalid_grant"}"#));
        let account = Account::new(config, MemoryStore::holding("rt0"));

        assert!(matches!(
            account.access_token(),
            Err(AccountError::SignedOut)
        ));
        assert!(!account.is_signed_in().unwrap());
    }

    #[test]
    fn a_refresh_token_that_could_not_be_stored_is_kept_and_used_and_stored_later() {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let (server, config) = identity_provider(move |_| {
            let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
            Reply::json(
                200,
                &format!(r#"{{"access_token":"at{n}","expires_in":300,"refresh_token":"rt{n}"}}"#),
            )
        });
        let account = Account::new(config, MemoryStore::holding("rt0"));

        account.store.fail_saves.store(true, Ordering::SeqCst);
        assert_eq!(account.access_token().unwrap(), "at1");
        assert_eq!(
            account.store.current().as_deref(),
            Some("rt0"),
            "not stored"
        );

        account.forget_access_token();
        account.store.fail_saves.store(false, Ordering::SeqCst);
        assert_eq!(account.access_token().unwrap(), "at2");

        let sent: Vec<String> = token_requests(&server)
            .iter()
            .map(|request| request.form()["refresh_token"].clone())
            .collect();
        assert_eq!(sent, ["rt0", "rt1"], "the spent one is not sent again");
        assert_eq!(account.store.current().as_deref(), Some("rt2"));
    }

    #[test]
    fn an_identity_provider_in_trouble_does_not_end_the_sign_in() {
        let (_server, config) = identity_provider(|_| Reply::status(503));
        let account = Account::new(config, MemoryStore::holding("rt0"));

        assert!(matches!(
            account.access_token(),
            Err(AccountError::Oidc(OidcError::Unreachable { .. }))
        ));
        assert_eq!(account.store.current().as_deref(), Some("rt0"));
    }

    #[test]
    fn renewals_at_the_same_moment_spend_the_refresh_token_once() {
        let count = Arc::new(AtomicUsize::new(0));
        let counter = count.clone();
        let (server, config) = identity_provider(move |request| {
            // As the identity provider does: a refresh token works once.
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if request.form()["refresh_token"] != format!("rt{n}") {
                return Reply::json(400, r#"{"error":"invalid_grant"}"#);
            }
            std::thread::sleep(Duration::from_millis(50));
            Reply::json(
                200,
                &format!(
                    r#"{{"access_token":"at","expires_in":300,"refresh_token":"rt{}"}}"#,
                    n + 1
                ),
            )
        });
        let account = Arc::new(Account::new(config, MemoryStore::holding("rt0")));

        let callers: Vec<_> = (0..4)
            .map(|_| {
                let account = account.clone();
                std::thread::spawn(move || account.access_token())
            })
            .collect();

        for caller in callers {
            assert_eq!(caller.join().unwrap().unwrap(), "at");
        }
        assert_eq!(token_requests(&server).len(), 1);
    }

    #[test]
    fn signing_in_stores_the_refresh_token_and_signing_out_removes_and_revokes_it() {
        let (server, config) = identity_provider(|_| {
            Reply::json(
                200,
                r#"{"access_token":"at","expires_in":300,"refresh_token":"rt"}"#,
            )
        });
        let account = Account::new(config, MemoryStore::default());

        let login = account.begin_login().unwrap();
        let sent: std::collections::HashMap<String, String> =
            url::Url::parse(login.authorize_url())
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect();
        let back = format!("{}?code=c&state={}", sent["redirect_uri"], sent["state"]);
        let browser = std::thread::spawn(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder().build().into();
            agent.get(&back).call().unwrap().status().as_u16()
        });
        account
            .finish_login(login, Duration::from_secs(10), &AtomicBool::new(false))
            .unwrap();
        assert_eq!(browser.join().unwrap(), 200);

        assert!(account.is_signed_in().unwrap());
        assert_eq!(account.access_token().unwrap(), "at");
        assert_eq!(
            token_requests(&server).len(),
            1,
            "the token from signing in is used"
        );

        account.sign_out().unwrap();
        assert!(!account.is_signed_in().unwrap());
        assert!(matches!(
            account.access_token(),
            Err(AccountError::SignedOut)
        ));
        let revoked = server
            .requests()
            .into_iter()
            .find(|request| request.path == "/connect/revoke")
            .expect("the identity provider is told");
        assert_eq!(revoked.form()["token"], "rt");
    }
}
