//! The screenshot service's API, as the signed-in user: who am I there, and here is an image.

use serde::Deserialize;

use crate::account::{Account, AccountError};
use crate::platform::TokenStore;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not signed in")]
    SignedOut,

    /// Signed in, but this account may not do that (uploading needs a permission the administrator gives).
    #[error("this account is not allowed to do that")]
    Forbidden,

    #[error("the image is larger than the server accepts")]
    TooLarge,

    #[error("could not reach the server: {0}")]
    Unreachable(String),

    #[error("the server answered {status}{}", detail.as_deref().map(|d| format!(": {d}")).unwrap_or_default())]
    Rejected { status: u16, detail: Option<String> },

    #[error(transparent)]
    Account(AccountError),
}

impl From<AccountError> for ApiError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::SignedOut => Self::SignedOut,
            other => Self::Account(other),
        }
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;

/// The signed-in user as the service knows them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Session {
    pub name: String,
    pub permissions: Vec<String>,
}

/// An uploaded image.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub id: String,
    /// The page to share.
    pub url: String,
    /// The image itself.
    pub image_url: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// When it was uploaded, as the server writes it (ISO 8601, UTC).
    #[serde(default)]
    pub created_at: String,
}

#[derive(Deserialize)]
struct Page {
    items: Vec<Shot>,
}

/// What the server says about a request it turned down (RFC 9457).
#[derive(Deserialize)]
struct Problem {
    #[serde(default)]
    detail: Option<String>,
}

pub fn session<S: TokenStore>(account: &Account<S>) -> Result<Session> {
    let url = format!("{}/api/session", account.config().server);
    send(account, |agent, authorization| {
        agent
            .get(&url)
            .header("Authorization", authorization)
            .call()
    })
}

/// The user's own uploads, newest first, `count` of them at most.
pub fn recent<S: TokenStore>(account: &Account<S>, count: u32) -> Result<Vec<Shot>> {
    let url = format!("{}/api/shots?limit={count}", account.config().server);
    let page: Page = send(account, |agent, authorization| {
        agent
            .get(&url)
            .header("Authorization", authorization)
            .call()
    })?;
    Ok(page.items)
}

/// Uploads an image file (PNG, JPEG or WebP: the server decides from its bytes).
pub fn upload<S: TokenStore>(account: &Account<S>, image: &[u8]) -> Result<Shot> {
    let url = format!("{}/api/shots", account.config().server);
    send(account, |agent, authorization| {
        agent
            .post(&url)
            .header("Authorization", authorization)
            .header("Content-Type", "application/octet-stream")
            .send(image)
    })
}

/// Makes a request with the access token; when the server does not take the token, once more with a
/// renewed one (the server and this machine may disagree about the time by a little).
fn send<S: TokenStore, T: serde::de::DeserializeOwned>(
    account: &Account<S>,
    request: impl Fn(
        &ureq::Agent,
        &str,
    ) -> std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<T> {
    let mut renewed = false;
    loop {
        let authorization = format!("Bearer {}", account.access_token()?);
        let mut response = request(account.agent(), &authorization)
            .map_err(|error| ApiError::Unreachable(error.to_string()))?;

        let status = response.status().as_u16();
        return match status {
            200..=299 => response
                .body_mut()
                .read_json()
                .map_err(|error| ApiError::Rejected {
                    status,
                    detail: Some(error.to_string()),
                }),
            401 if !renewed => {
                renewed = true;
                account.forget_access_token();
                continue;
            }
            403 => Err(ApiError::Forbidden),
            413 => Err(ApiError::TooLarge),
            _ => Err(ApiError::Rejected {
                status,
                detail: response
                    .body_mut()
                    .read_json::<Problem>()
                    .ok()
                    .and_then(|problem| problem.detail),
            }),
        };
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::config::Config;
    use crate::testing::{FakeServer, MemoryStore, Reply, Request};

    /// The identity provider and the service on one port. Token requests are answered with `at1`,
    /// `at2`, …; `api` answers the rest.
    fn service(
        api: impl Fn(&Request) -> Reply + Send + 'static,
    ) -> (FakeServer, Account<MemoryStore>) {
        let base = Arc::new(Mutex::new(String::new()));
        let known = base.clone();
        let renewals = AtomicUsize::new(0);
        let server = FakeServer::start(move |request| match request.path.as_str() {
            "/.well-known/openid-configuration" => {
                let base = known.lock().unwrap().clone();
                Reply::json(
                    200,
                    &format!(
                        r#"{{"issuer":"{base}/","authorization_endpoint":"{base}/a","token_endpoint":"{base}/token"}}"#
                    ),
                )
            }
            "/token" => {
                let n = renewals.fetch_add(1, Ordering::SeqCst) + 1;
                Reply::json(
                    200,
                    &format!(
                        r#"{{"access_token":"at{n}","expires_in":300,"refresh_token":"rt{n}"}}"#
                    ),
                )
            }
            _ => api(request),
        });
        *base.lock().unwrap() = server.url("");
        let config = Config {
            server: server.url(""),
            issuer: server.url("/"),
            ..Config::production()
        };
        (server, Account::new(config, MemoryStore::holding("rt0")))
    }

    fn api_requests(server: &FakeServer) -> Vec<Request> {
        server
            .requests()
            .into_iter()
            .filter(|request| request.path.starts_with("/api/"))
            .collect()
    }

    const SHOT: &str = r#"{"id":"abc","url":"https://i.example/s/abc","imageUrl":"https://i.example/f/abc","contentType":"image/png","size":3,"width":1,"height":1,"createdAt":"2026-01-01T00:00:00Z"}"#;

    #[test]
    fn an_upload_is_the_image_itself_with_the_access_token_and_nothing_else() {
        let (server, account) = service(|_| Reply::json(201, SHOT));

        let shot = upload(&account, b"\x89PNG").unwrap();

        assert_eq!(shot.url, "https://i.example/s/abc");
        assert_eq!(shot.image_url, "https://i.example/f/abc");
        let request = &api_requests(&server)[0];
        assert_eq!(
            (request.method.as_str(), request.path.as_str()),
            ("POST", "/api/shots")
        );
        assert_eq!(request.body, b"\x89PNG");
        assert_eq!(request.header("authorization"), Some("Bearer at1"));
        assert_eq!(request.header("cookie"), None);
    }

    #[test]
    fn the_recent_uploads_are_the_first_page_of_the_users_own() {
        let (server, account) = service(|request| {
            assert_eq!(request.path, "/api/shots?limit=8");
            Reply::json(200, &format!(r#"{{"items":[{SHOT}],"next":"cursor"}}"#))
        });

        let shots = recent(&account, 8).unwrap();

        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].id, "abc");
        assert_eq!((shots[0].width, shots[0].height), (1, 1));
        assert_eq!(shots[0].created_at, "2026-01-01T00:00:00Z");
        assert_eq!(api_requests(&server)[0].method, "GET");
    }

    #[test]
    fn the_session_tells_who_is_signed_in() {
        let (_server, account) = service(|request| {
            assert_eq!(request.path, "/api/session");
            Reply::json(200, r#"{"name":"someone","permissions":["shots.upload"]}"#)
        });

        assert_eq!(
            session(&account).unwrap(),
            Session {
                name: "someone".into(),
                permissions: vec!["shots.upload".into()]
            }
        );
    }

    #[test]
    fn a_token_the_server_does_not_take_is_renewed_once() {
        let (server, account) = service(|request| {
            if request.header("authorization") == Some("Bearer at1") {
                Reply::status(401)
            } else {
                Reply::json(201, SHOT)
            }
        });

        assert!(upload(&account, b"x").is_ok());
        let sent: Vec<_> = api_requests(&server)
            .iter()
            .map(|request| request.header("authorization").unwrap().to_owned())
            .collect();
        assert_eq!(sent, ["Bearer at1", "Bearer at2"]);

        let (server, account) = service(|_| Reply::status(401));
        assert!(matches!(
            upload(&account, b"x"),
            Err(ApiError::Rejected { status: 401, .. })
        ));
        assert_eq!(api_requests(&server).len(), 2, "not a third time");
    }

    #[test]
    fn what_the_server_turns_down_keeps_its_reason() {
        let (_server, account) = service(|_| Reply::status(403));
        assert!(matches!(upload(&account, b"x"), Err(ApiError::Forbidden)));

        let (_server, account) = service(|_| Reply::status(413));
        assert!(matches!(upload(&account, b"x"), Err(ApiError::TooLarge)));

        let (_server, account) = service(|_| {
            Reply::json(
                415,
                r#"{"status":415,"detail":"Only PNG, JPEG and WebP images are accepted."}"#,
            )
        });
        assert!(matches!(
            upload(&account, b"x"),
            Err(ApiError::Rejected { status: 415, detail: Some(detail) }) if detail.starts_with("Only PNG")
        ));
    }

    #[test]
    fn without_a_sign_in_nothing_is_sent() {
        let server = FakeServer::start(|_| Reply::status(500));
        let config = Config {
            server: server.url(""),
            issuer: server.url("/"),
            ..Config::production()
        };
        let account = Account::new(config, MemoryStore::default());

        assert!(matches!(upload(&account, b"x"), Err(ApiError::SignedOut)));
        assert!(server.requests().is_empty());
    }
}
