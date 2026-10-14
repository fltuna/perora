//! Where Perora signs in and where it uploads. Both are fixed: Perora is the client of one service.
//! Debug builds can be pointed at a local identity provider and a local server through the environment;
//! release builds ignore it.

use std::time::Duration;

const SERVER: &str = "https://i.f2a.dev";
const ISSUER: &str = "https://auth.f2a.dev/";
const CLIENT_ID: &str = "shots-desktop";
const SCOPE: &str = "openid profile offline_access shots";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The screenshot service, without a trailing slash.
    pub server: String,
    /// The identity provider, exactly as it names itself in its tokens.
    pub issuer: String,
    pub client_id: String,
    pub scope: String,
    /// Accepts any TLS certificate. For local development certificates only; never set in a release build.
    pub accept_invalid_certs: bool,
}

impl Config {
    pub fn production() -> Self {
        Self {
            server: SERVER.into(),
            issuer: ISSUER.into(),
            client_id: CLIENT_ID.into(),
            scope: SCOPE.into(),
            accept_invalid_certs: false,
        }
    }

    /// The production configuration; in a debug build, with whatever `PERORA_SERVER`, `PERORA_ISSUER`
    /// and `PERORA_INSECURE_TLS=1` replace of it.
    pub fn from_environment() -> Self {
        let mut config = Self::production();
        if cfg!(debug_assertions) {
            if let Some(server) = variable("PERORA_SERVER") {
                config.server = server.trim_end_matches('/').into();
            }
            if let Some(issuer) = variable("PERORA_ISSUER") {
                config.issuer = issuer;
            }
            config.accept_invalid_certs = variable("PERORA_INSECURE_TLS").as_deref() == Some("1");
        }
        config
    }

    pub fn is_production(&self) -> bool {
        *self == Self::production()
    }

    /// The HTTP client every request goes through. Statuses are left to the caller: a refused token and
    /// an unreachable server are different things to tell the user.
    pub(crate) fn agent(&self) -> ureq::Agent {
        let tls = ureq::tls::TlsConfig::builder()
            .disable_verification(self.accept_invalid_certs)
            .build();
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .tls_config(tls)
            .user_agent(concat!("Perora/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .new_agent()
    }
}

fn variable(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}
