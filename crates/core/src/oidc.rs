//! Signing in the way a native application does (RFC 8252): the user signs in in their own browser, the
//! identity provider sends the browser back to a port on this machine with a one-time code, and the code
//! plus a secret only this process knows (PKCE) becomes tokens. No client secret: there is no way to keep
//! one in a program anyone can download.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::config::Config;

#[derive(Debug, thiserror::Error)]
pub enum OidcError {
    /// Nothing came back: no network, the server is down, the connection broke.
    #[error("could not reach {what}: {detail}")]
    Unreachable { what: &'static str, detail: String },

    /// Something came back that an identity provider behaving as agreed would not send.
    #[error("unexpected answer from the identity provider: {0}")]
    Protocol(String),

    /// The identity provider said no: at sign-in (the user declined, the account may not use this
    /// client) or to a token request (the refresh token expired, was used already or was revoked).
    #[error("the identity provider refused: {error}{}", description.as_deref().map(|d| format!(" ({d})")).unwrap_or_default())]
    Refused {
        error: String,
        description: Option<String>,
    },

    #[error("could not listen for the browser coming back: {0}")]
    Listener(std::io::Error),

    #[error("nobody signed in in time")]
    TimedOut,

    #[error("signing in was cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, OidcError>;

/// What the identity provider publishes about itself.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
}

impl Discovery {
    pub fn fetch(agent: &ureq::Agent, config: &Config) -> Result<Self> {
        let url = format!(
            "{}/.well-known/openid-configuration",
            config.issuer.trim_end_matches('/')
        );
        let mut response = agent.get(&url).call().map_err(unreachable)?;
        if response.status().as_u16() != 200 {
            return Err(OidcError::Protocol(format!(
                "{url} answered {}",
                response.status()
            )));
        }
        let discovery: Self = response
            .body_mut()
            .read_json()
            .map_err(|error| OidcError::Protocol(error.to_string()))?;

        // A document that names another issuer is not the one that was asked for.
        if discovery.issuer.trim_end_matches('/') != config.issuer.trim_end_matches('/') {
            return Err(OidcError::Protocol(format!(
                "the issuer is {}, not {}",
                discovery.issuer, config.issuer
            )));
        }

        // The browser is sent to one of these and the tokens are posted to the others: none of them
        // may be anywhere but at the identity provider itself, over the same protocol.
        let issuer = origin(&config.issuer)?;
        let endpoints = [
            Some(&discovery.authorization_endpoint),
            Some(&discovery.token_endpoint),
            discovery.revocation_endpoint.as_ref(),
        ];
        for endpoint in endpoints.into_iter().flatten() {
            if origin(endpoint)? != issuer {
                return Err(OidcError::Protocol(format!(
                    "the endpoint {endpoint} is not at {}",
                    config.issuer
                )));
            }
        }
        Ok(discovery)
    }
}

/// Scheme, host and port of an `http(s)` address.
fn origin(address: &str) -> Result<url::Origin> {
    match Url::parse(address) {
        Ok(url) if matches!(url.scheme(), "https" | "http") => Ok(url.origin()),
        _ => Err(OidcError::Protocol(format!(
            "{address} is not a web address"
        ))),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    /// Seconds until the access token expires.
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// One attempt at signing in: the address for the browser, and the port waiting for it to come back.
pub struct Login {
    authorize_url: String,
    redirect_uri: String,
    state: String,
    verifier: String,
    listener: TcpListener,
}

impl Login {
    pub fn begin(config: &Config, discovery: &Discovery) -> Result<Self> {
        // Any free port: the identity provider accepts every port on the loopback address for a
        // native client.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(OidcError::Listener)?;
        let port = listener.local_addr().map_err(OidcError::Listener)?.port();
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");

        let verifier = random(32);
        let state = random(16);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let authorize_url = Url::parse_with_params(
            &discovery.authorization_endpoint,
            [
                ("response_type", "code"),
                ("client_id", config.client_id.as_str()),
                ("redirect_uri", redirect_uri.as_str()),
                ("scope", config.scope.as_str()),
                ("state", state.as_str()),
                ("code_challenge", challenge.as_str()),
                ("code_challenge_method", "S256"),
            ],
        )
        .map_err(|error| OidcError::Protocol(format!("authorization endpoint: {error}")))?
        .into();

        Ok(Self {
            authorize_url,
            redirect_uri,
            state,
            verifier,
            listener,
        })
    }

    /// What to open in the user's browser.
    pub fn authorize_url(&self) -> &str {
        &self.authorize_url
    }

    /// Waits for the browser to come back and turns the code it brings into tokens. Gives up after
    /// `timeout`, or as soon as `cancel` is set.
    pub fn finish(
        self,
        agent: &ureq::Agent,
        config: &Config,
        discovery: &Discovery,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<Tokens> {
        let code = self.wait_for_code(timeout, cancel)?;
        request_tokens(
            agent,
            discovery,
            &[
                ("client_id", config.client_id.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("code_verifier", self.verifier.as_str()),
            ],
        )
    }

    fn wait_for_code(&self, timeout: Duration, cancel: &AtomicBool) -> Result<String> {
        self.listener
            .set_nonblocking(true)
            .map_err(OidcError::Listener)?;
        let deadline = Instant::now() + timeout;

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(OidcError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(OidcError::TimedOut);
            }

            match self.listener.accept() {
                Ok((stream, _)) => {
                    // Anything that is not the answer to this attempt (a favicon, a stray request, a
                    // wrong state) is turned away and the wait goes on.
                    if let Some(outcome) = self.answer(stream) {
                        return outcome;
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(OidcError::Listener(error)),
            }
        }
    }

    fn answer(&self, mut stream: TcpStream) -> Option<Result<String>> {
        let (target, japanese) = read_request(&mut stream)?;
        let say = |english: &'static str, japanese_text: &'static str| {
            if japanese { japanese_text } else { english }
        };
        let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
        let parameter = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };

        if url.path() != "/callback" || parameter("state").as_deref() != Some(self.state.as_str()) {
            respond(
                &mut stream,
                "404 Not Found",
                say("Not found.", "見つかりません。"),
            );
            return None;
        }

        if let Some(code) = parameter("code") {
            respond(
                &mut stream,
                "200 OK",
                say(
                    "You can close this tab and go back to Perora.",
                    "このタブを閉じて、Perora に戻ってください。",
                ),
            );
            return Some(Ok(code));
        }

        respond(
            &mut stream,
            "200 OK",
            say(
                "Signing in did not complete. You can close this tab.",
                "ログインは完了しませんでした。このタブは閉じてかまいません。",
            ),
        );
        Some(Err(OidcError::Refused {
            error: parameter("error").unwrap_or_else(|| "no code".into()),
            description: parameter("error_description"),
        }))
    }
}

/// A new access token for a refresh token. The refresh token is spent by this: the answer carries the
/// one to use next time.
pub fn refresh(
    agent: &ureq::Agent,
    config: &Config,
    discovery: &Discovery,
    refresh_token: &str,
) -> Result<Tokens> {
    request_tokens(
        agent,
        discovery,
        &[
            ("client_id", config.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ],
    )
}

/// Tells the identity provider that a refresh token will not be used again. Best effort.
pub fn revoke(agent: &ureq::Agent, config: &Config, discovery: &Discovery, refresh_token: &str) {
    if let Some(endpoint) = &discovery.revocation_endpoint {
        let _ = agent.post(endpoint).send_form([
            ("client_id", config.client_id.as_str()),
            ("token", refresh_token),
            ("token_type_hint", "refresh_token"),
        ]);
    }
}

fn request_tokens(
    agent: &ureq::Agent,
    discovery: &Discovery,
    form: &[(&str, &str)],
) -> Result<Tokens> {
    let mut response = agent
        .post(&discovery.token_endpoint)
        .send_form(form.iter().copied())
        .map_err(unreachable)?;
    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().map_err(unreachable)?;

    match status {
        200 => serde_json::from_str(&body).map_err(|error| OidcError::Protocol(error.to_string())),
        // RFC 6749 §5.2: a refusal is a 400 (401 for the client itself) with a reason.
        400 | 401 => match serde_json::from_str::<TokenError>(&body) {
            Ok(refusal) => Err(OidcError::Refused {
                error: refusal.error,
                description: refusal.error_description,
            }),
            Err(_) => Err(OidcError::Protocol(format!(
                "the token endpoint answered {status}"
            ))),
        },
        _ => Err(OidcError::Unreachable {
            what: "the identity provider",
            detail: format!("the token endpoint answered {status}"),
        }),
    }
}

fn unreachable(error: ureq::Error) -> OidcError {
    OidcError::Unreachable {
        what: "the identity provider",
        detail: error.to_string(),
    }
}

/// `bytes` random bytes as URL-safe text.
fn random(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).expect("the operating system provides randomness");
    URL_SAFE_NO_PAD.encode(buffer)
}

/// The target of a `GET` request line (`/callback?code=…`), and whether the browser asks for Japanese
/// before anything else; `None` for anything but a `GET`.
fn read_request(stream: &mut TcpStream) -> Option<(String, bool)> {
    // Two seconds for the whole request, however slowly it trickles in: while this reads, nobody else
    // is listened to, and neither the timeout nor a cancellation of the sign-in is noticed.
    let deadline = Instant::now() + Duration::from_secs(2);
    stream.set_nonblocking(false).ok()?;

    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") && request.len() < 16 * 1024 {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        stream
            .set_read_timeout(Some(remaining.max(Duration::from_millis(1))))
            .ok()?;
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => request.extend_from_slice(&chunk[..read]),
        }
    }

    let head = String::from_utf8_lossy(&request);
    let mut lines = head.split("\r\n");
    let mut parts = lines.next()?.split(' ');
    let target = (parts.next()? == "GET").then(|| parts.next().map(str::to_owned))??;

    // The page is two sentences in the two languages Perora speaks; the first language the browser
    // lists decides.
    let japanese = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("accept-language"))
        .is_some_and(|(_, value)| value.trim_start().to_ascii_lowercase().starts_with("ja"));
    Some((target, japanese))
}

fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Perora</title>\
         <body style=\"font-family:system-ui,sans-serif;margin:3rem\"><p>{message}</p>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeServer, Reply};

    fn discovery(server: &FakeServer) -> Discovery {
        Discovery {
            issuer: server.url(""),
            authorization_endpoint: server.url("/connect/authorize"),
            token_endpoint: server.url("/connect/token"),
            revocation_endpoint: None,
        }
    }

    fn config(server: &FakeServer) -> Config {
        Config {
            issuer: server.url(""),
            ..Config::production()
        }
    }

    fn parameters(url: &str) -> std::collections::HashMap<String, String> {
        Url::parse(url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }

    /// What the browser does at the end of signing in: a request to the address it was sent back to.
    fn browser(url: String) -> std::thread::JoinHandle<u16> {
        std::thread::spawn(move || {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into();
            agent.get(&url).call().unwrap().status().as_u16()
        })
    }

    #[test]
    fn the_browser_is_sent_off_with_a_challenge_and_back_to_a_port_of_ours() {
        let server = FakeServer::start(|_| Reply::status(404));
        let login = Login::begin(&config(&server), &discovery(&server)).unwrap();

        let url = login.authorize_url();
        let sent = parameters(url);

        assert!(url.starts_with(&server.url("/connect/authorize?")));
        assert_eq!(sent["response_type"], "code");
        assert_eq!(sent["client_id"], "shots-desktop");
        assert_eq!(sent["scope"], "openid profile offline_access shots");
        assert_eq!(sent["code_challenge_method"], "S256");
        assert_eq!(
            sent["code_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(login.verifier.as_bytes()))
        );
        assert_eq!(sent["state"], login.state);
        assert!(sent["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert!(sent["redirect_uri"].ends_with("/callback"));
        assert!(
            login.verifier.len() >= 43,
            "RFC 7636: at least 43 characters"
        );
    }

    #[test]
    fn the_code_the_browser_brings_back_becomes_tokens() {
        let server = FakeServer::start(|request| {
            assert_eq!(request.path, "/connect/token");
            Reply::json(
                200,
                r#"{"access_token":"at","expires_in":300,"refresh_token":"rt"}"#,
            )
        });
        let (config, discovery) = (config(&server), discovery(&server));
        let login = Login::begin(&config, &discovery).unwrap();
        let (redirect_uri, state, verifier) = (
            login.redirect_uri.clone(),
            login.state.clone(),
            login.verifier.clone(),
        );

        // Turned away, without ending the wait: a request for something else, and one with another state.
        let stray = browser(redirect_uri.replace("/callback", "/favicon.ico"));
        let wrong = browser(format!("{redirect_uri}?code=stolen&state=other"));
        std::thread::sleep(Duration::from_millis(100));
        let right = browser(format!("{redirect_uri}?code=the-code&state={state}"));

        let tokens = login
            .finish(
                &config.agent(),
                &config,
                &discovery,
                Duration::from_secs(10),
                &AtomicBool::new(false),
            )
            .unwrap();

        assert_eq!(tokens.access_token, "at");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
        assert_eq!(right.join().unwrap(), 200);
        assert_eq!(stray.join().unwrap(), 404);
        assert_eq!(wrong.join().unwrap(), 404);

        let form = server.requests()[0].form();
        assert_eq!(form["grant_type"], "authorization_code");
        assert_eq!(form["code"], "the-code");
        assert_eq!(form["client_id"], "shots-desktop");
        assert_eq!(form["redirect_uri"], redirect_uri);
        assert_eq!(form["code_verifier"], verifier);
        assert!(!form.contains_key("client_secret"));
    }

    #[test]
    fn a_refusal_in_the_browser_ends_the_attempt_with_its_reason() {
        let server = FakeServer::start(|_| Reply::status(500));
        let (config, discovery) = (config(&server), discovery(&server));
        let login = Login::begin(&config, &discovery).unwrap();
        let back = browser(format!(
            "{}?error=access_denied&error_description=no+thanks&state={}",
            login.redirect_uri, login.state
        ));

        let outcome = login.finish(
            &config.agent(),
            &config,
            &discovery,
            Duration::from_secs(10),
            &AtomicBool::new(false),
        );

        assert!(matches!(
            outcome,
            Err(OidcError::Refused { error, description })
                if error == "access_denied" && description.as_deref() == Some("no thanks")
        ));
        assert_eq!(back.join().unwrap(), 200);
        assert!(
            server.requests().is_empty(),
            "no token request without a code"
        );
    }

    #[test]
    fn waiting_ends_when_nobody_comes_and_when_it_is_cancelled() {
        let server = FakeServer::start(|_| Reply::status(500));
        let (config, discovery) = (config(&server), discovery(&server));
        let agent = config.agent();

        let nobody = Login::begin(&config, &discovery).unwrap().finish(
            &agent,
            &config,
            &discovery,
            Duration::from_millis(120),
            &AtomicBool::new(false),
        );
        assert!(matches!(nobody, Err(OidcError::TimedOut)));

        let cancelled = Login::begin(&config, &discovery).unwrap().finish(
            &agent,
            &config,
            &discovery,
            Duration::from_secs(10),
            &AtomicBool::new(true),
        );
        assert!(matches!(cancelled, Err(OidcError::Cancelled)));
    }

    #[test]
    fn a_refused_token_request_says_why_and_a_broken_server_is_not_a_refusal() {
        let refusing = FakeServer::start(|_| {
            Reply::json(
                400,
                r#"{"error":"invalid_grant","error_description":"used"}"#,
            )
        });
        let config_a = config(&refusing);
        let outcome = refresh(&config_a.agent(), &config_a, &discovery(&refusing), "rt");
        assert!(
            matches!(outcome, Err(OidcError::Refused { error, .. }) if error == "invalid_grant")
        );
        assert_eq!(refusing.requests()[0].form()["refresh_token"], "rt");

        let broken = FakeServer::start(|_| Reply::status(502));
        let config_b = config(&broken);
        let outcome = refresh(&config_b.agent(), &config_b, &discovery(&broken), "rt");
        assert!(matches!(outcome, Err(OidcError::Unreachable { .. })));
    }

    #[test]
    fn the_page_the_browser_comes_back_to_is_in_the_browsers_language() {
        let server = FakeServer::start(|_| {
            Reply::json(200, r#"{"access_token":"at","refresh_token":"rt"}"#)
        });
        let (config, discovery) = (config(&server), discovery(&server));
        let page = |language: &'static str| {
            let login = Login::begin(&config, &discovery).unwrap();
            let url = format!("{}?code=c&state={}", login.redirect_uri, login.state);
            let browser = std::thread::spawn(move || {
                let agent: ureq::Agent = ureq::Agent::config_builder().build().into();
                let mut response = agent
                    .get(&url)
                    .header("Accept-Language", language)
                    .call()
                    .unwrap();
                response.body_mut().read_to_string().unwrap()
            });
            login
                .finish(
                    &config.agent(),
                    &config,
                    &discovery,
                    Duration::from_secs(10),
                    &AtomicBool::new(false),
                )
                .unwrap();
            browser.join().unwrap()
        };

        assert!(page("ja,en-US;q=0.9,en;q=0.8").contains("このタブを閉じて"));
        assert!(page("en-GB,ja;q=0.5").contains("You can close this tab"));
    }

    #[test]
    fn a_request_that_trickles_in_does_not_hold_up_the_wait() {
        let server = FakeServer::start(|_| Reply::status(500));
        let (config, discovery) = (config(&server), discovery(&server));
        let login = Login::begin(&config, &discovery).unwrap();
        let port = login.listener.local_addr().unwrap().port();

        // A byte every 300 ms, never a whole request: each read alone would be in time.
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            while stream.write_all(b"G").is_ok() {
                std::thread::sleep(Duration::from_millis(300));
            }
        });
        std::thread::sleep(Duration::from_millis(100));

        let started = Instant::now();
        let outcome = login.finish(
            &config.agent(),
            &config,
            &discovery,
            Duration::from_millis(500),
            &AtomicBool::new(false),
        );

        assert!(matches!(outcome, Err(OidcError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn endpoints_anywhere_but_at_the_issuer_are_not_accepted() {
        let document = |authorization: &str, token: &str, issuer: &str| {
            format!(
                r#"{{"issuer":"{issuer}","authorization_endpoint":"{authorization}","token_endpoint":"{token}"}}"#
            )
        };
        let fetch = |build: fn(&str) -> (String, String)| {
            let base = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
            let known = base.clone();
            let server = FakeServer::start(move |_| {
                let base = known.lock().unwrap().clone();
                let (authorization, token) = build(&base);
                Reply::json(200, &document(&authorization, &token, &base))
            });
            *base.lock().unwrap() = server.url("");
            let config = config(&server);
            Discovery::fetch(&config.agent(), &config)
        };

        assert!(fetch(|base| (format!("{base}/a"), format!("{base}/t"))).is_ok());
        for outcome in [
            fetch(|base| ("search-ms:query=x".into(), format!("{base}/t"))),
            fetch(|base| ("file://host/share/a".into(), format!("{base}/t"))),
            fetch(|base| (format!("{base}/a"), "https://elsewhere.example/t".into())),
            fetch(|base| {
                (
                    base.replace("127.0.0.1", "localhost") + "/a",
                    format!("{base}/t"),
                )
            }),
        ] {
            assert!(matches!(outcome, Err(OidcError::Protocol(_))));
        }
    }

    #[test]
    fn a_discovery_document_of_another_issuer_is_not_accepted() {
        let server = FakeServer::start(|request| {
            assert_eq!(request.path, "/.well-known/openid-configuration");
            Reply::json(
                200,
                r#"{"issuer":"https://elsewhere.example/","authorization_endpoint":"https://elsewhere.example/a","token_endpoint":"https://elsewhere.example/t"}"#,
            )
        });
        let config = config(&server);

        assert!(matches!(
            Discovery::fetch(&config.agent(), &config),
            Err(OidcError::Protocol(_))
        ));
    }
}
