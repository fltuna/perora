//! Updates: whether the releases on GitHub have a newer Perora than this one, fetching it, and
//! checking that it is one Perora's key signed. (Running its installer is the platform's:
//! `platform::run_installer`.)
//!
//! What is read is what the Tauri version's updater read, and what the release workflow writes for
//! both: `latest.json` beside the newest release, which names the newest version and, for each kind
//! of Windows, the installer and its signature. The signature is minisign's, as `tauri signer sign`
//! makes it (in base64), and says in its trusted comment which version it was made for.

use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;

/// Where the newest release says what it is. (A prerelease, `v0.2.0-rc.1`, is never the newest:
/// GitHub's `latest` passes over it.)
pub const LATEST: &str = "https://github.com/fltuna/perora/releases/latest/download/latest.json";

/// The public half of the key the releases are signed with (minisign, key ID B21287D738A95859),
/// in base64 as `tauri signer generate` wrote it: what the Tauri version's tauri.conf.json had. The
/// secret half signs every release; with another key, the Perora already installed would refuse it.
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIyMTI4N0Q3MzhBOTU4NTkKUldSWldLazQxNGNTc29IWmVZemZoSUs4VWtNT0s3ZW8xMElQRjRLVDREOWlYTlpvb0FjVmhCSnkK";

/// How long the releases are given to be reached, and to say something once they are (between one
/// piece of an answer and the next). No limit on the whole: an installer takes as long as the line
/// needs.
pub const REACHED_WITHIN: Duration = Duration::from_secs(15);
pub const SILENT_FOR_AT_MOST: Duration = Duration::from_secs(30);

/// The kinds of Windows a release names an installer for, in the order they are looked for: the
/// NSIS one by name, then the plain one (as Tauri's updater looked for them).
const TARGETS: [&str; 2] = ["windows-x86_64-nsis", "windows-x86_64"];

/// Larger than any `latest.json`, and than any installer Perora will have (0.2.0's is under 10 MiB):
/// what is larger is not one. Counted as it is read, after any compression is undone.
const LATEST_AT_MOST: u64 = 1 << 20;
const INSTALLER_AT_MOST: usize = 64 << 20;
/// What a release may say of what changed in it, in each language: more is not said at all (it is
/// drawn as the main window is, and comes before the signature is checked).
const CHANGES_AT_MOST: usize = 64 << 10;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the releases could not be reached: {0}")]
    Unreachable(String),

    #[error("the releases answered {0}")]
    Status(u16),

    #[error("the release is not understood: {0}")]
    Unreadable(String),

    #[error("the release has no installer for this Windows")]
    NoInstaller,

    #[error("the download stopped: nothing came for {0} seconds")]
    Silent(u64),

    #[error("the download is larger than an installer")]
    TooLarge,

    #[error("the signature does not match: {0}")]
    Signature(String),

    #[error("the signature is of version {signed}, not of {announced}")]
    OtherVersion { signed: String, announced: String },

    #[error("the signature does not say which version it is of")]
    NoSignedVersion,

    #[error("what was fetched is not an installer")]
    NotAnInstaller,
}

/// A newer version the releases have: which, where its installer is, signed, and what changed in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    signature: String,
    changes: Changes,
    /// What changed in each version since the one installed, up to this one, newest first, a
    /// `## <version>` heading each (from the release's `history`).
    since: Changes,
}

impl Release {
    /// What changed since the version installed (Markdown: a `## <version>` heading for each
    /// version, newest first), or, where the release does not say that, in this version (as
    /// `CHANGELOG/v<version>_CHANGELOGS*.md` has it); in the language of that code if the release
    /// says it in that one, or else in the other; none when the release does not say.
    pub fn changes(&self, language: &str) -> Option<&str> {
        let japanese = language.starts_with("ja");
        fn in_language(changes: &Changes, japanese: bool) -> Option<&str> {
            if japanese { said(&changes.ja) } else { said(&changes.en) }
        }
        fn said(changes: &Option<String>) -> Option<&str> {
            changes
                .as_deref()
                .map(str::trim)
                .filter(|changes| !changes.is_empty())
        }
        in_language(&self.since, japanese)
            .or(in_language(&self.changes, japanese))
            .or(in_language(&self.since, !japanese))
            .or(in_language(&self.changes, !japanese))
    }
}

/// What changed, in Japanese and in English. In a field of its own (`changes`) beside `notes`,
/// which the Tauri version's updater reads as a text: anything else there, and it could read none
/// of the release, and would update no more.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
struct Changes {
    ja: Option<String>,
    en: Option<String>,
}

#[derive(Deserialize)]
struct Latest {
    version: String,
    platforms: std::collections::HashMap<String, Platform>,
    /// What changed in this version (all 0.2.0 reads).
    #[serde(default)]
    changes: Changes,
    /// What changed in the last versions up to this one (`CHANGELOG/CHANGELOGS*.md`, cut down):
    /// from it, what changed since the version installed, when more than one version was passed.
    /// (From 0.2.1 on; 0.2.0 passes over it.)
    #[serde(default)]
    history: Changes,
}

#[derive(Deserialize)]
struct Platform {
    url: String,
    signature: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        // (Both answers are redirections: to where GitHub keeps what a release has.)
        .max_redirects(5)
        .https_only(true)
        .user_agent(concat!("Perora/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(REACHED_WITHIN))
        .timeout_recv_response(Some(SILENT_FOR_AT_MOST))
        .build()
        .new_agent()
}

fn unreachable(error: ureq::Error) -> Error {
    Error::Unreachable(error.to_string())
}

/// The newer version the releases have than `current`, if they have one.
pub fn newest(current: &str) -> Result<Option<Release>, Error> {
    let mut response = agent()
        .get(LATEST)
        .header("Accept", "application/json")
        .config()
        // (Small: all of it within the time a piece of a larger answer gets.)
        .timeout_recv_body(Some(SILENT_FOR_AT_MOST))
        .build()
        .call()
        .map_err(unreachable)?;
    let status = response.status().as_u16();
    // (What Tauri's updater took for "nothing newer".)
    if status == 204 {
        return Ok(None);
    }
    if !(200..300).contains(&status) {
        return Err(Error::Status(status));
    }
    // (The limit of the body's own reader counts what came over the line, before it is unzipped:
    // this counts what it is unzipped into.)
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(LATEST_AT_MOST + 1)
        .read_to_end(&mut body)
        .map_err(|error| Error::Unreachable(error.to_string()))?;
    if body.len() as u64 > LATEST_AT_MOST {
        return Err(Error::Unreadable("larger than a release says".into()));
    }
    newer(&body, current)
}

/// What a `latest.json` says, if it is of a newer version than `current`. (What `newest` reads;
/// public for the release workflow, which reads what it wrote before publishing it.)
pub fn newer(latest: &[u8], current: &str) -> Result<Option<Release>, Error> {
    let latest: Latest =
        serde_json::from_slice(latest).map_err(|error| Error::Unreadable(error.to_string()))?;
    let version = parse_version(&latest.version)?;
    let current = parse_version(current)?;
    if version <= current {
        return Ok(None);
    }
    let platform = TARGETS
        .iter()
        .find_map(|target| latest.platforms.get(*target))
        .ok_or(Error::NoInstaller)?;
    let url = url::Url::parse(&platform.url).map_err(|error| Error::Unreadable(error.to_string()))?;
    if url.scheme() != "https" {
        return Err(Error::Unreadable(format!("not an address to fetch from: {url}")));
    }
    let within = |changes: Option<String>| changes.filter(|changes| changes.len() <= CHANGES_AT_MOST);
    let since = |history: Option<String>| {
        within(history).and_then(|history| changes_since(&history, &current, &version))
    };
    Ok(Some(Release {
        version: version.to_string(),
        url: url.into(),
        signature: platform.signature.clone(),
        changes: Changes {
            ja: within(latest.changes.ja),
            en: within(latest.changes.en),
        },
        since: Changes {
            ja: since(latest.history.ja),
            en: since(latest.history.en),
        },
    }))
}

/// Of a changelog of several versions, those after `current` up to `newest`, newest first, a
/// `## <version>` heading each; none if it has none of them. (A heading that is not a version is
/// passed over.)
fn changes_since(
    history: &str,
    current: &semver::Version,
    newest: &semver::Version,
) -> Option<String> {
    let mut versions: Vec<(semver::Version, &str, &str)> = crate::changelog::by_version(history)
        .into_iter()
        .filter_map(|(title, body)| {
            let version = parse_version(title.split_whitespace().next()?).ok()?;
            (&version > current && &version <= newest).then_some((version, title, body))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.dedup_by(|a, b| a.0 == b.0);
    (!versions.is_empty()).then(|| {
        versions
            .iter()
            .map(|(_, title, body)| format!("## {title}\n\n{body}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    })
}

/// A version as the releases and the signatures write it: `0.2.0`, or `v0.2.0`.
fn parse_version(version: &str) -> Result<semver::Version, Error> {
    semver::Version::parse(version.trim().trim_start_matches('v'))
        .map_err(|error| Error::Unreadable(format!("{version}: {error}")))
}

/// Fetches the installer of a release, and checks that Perora's key signed it, for that version.
/// `told` hears how far it is: bytes so far, and of how many when the releases say.
pub fn fetch(release: &Release, mut told: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>, Error> {
    let response = agent()
        .get(&release.url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(unreachable)?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(Error::Status(status));
    }
    let of: Option<u64> = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());

    // Read on a thread of its own: a read the line leaves hanging is given up on here, after
    // `SILENT_FOR_AT_MOST` without a byte. (The thread then ends when the read does, as no one
    // takes what it sends any more.)
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(INSTALLER_AT_MOST as u64 + 1)
        .reader();
    let (send, receive) = mpsc::sync_channel::<std::io::Result<Vec<u8>>>(8);
    std::thread::spawn(move || {
        let mut piece = vec![0; 64 * 1024];
        loop {
            let read = reader.read(&mut piece);
            let end = !matches!(read, Ok(read) if read > 0);
            if send.send(read.map(|read| piece[..read].to_vec())).is_err() || end {
                return;
            }
        }
    });
    let mut bytes = Vec::with_capacity(of.unwrap_or(0).min(INSTALLER_AT_MOST as u64) as usize);
    loop {
        match receive.recv_timeout(SILENT_FOR_AT_MOST) {
            Ok(Ok(piece)) if piece.is_empty() => break,
            Ok(Ok(piece)) => {
                bytes.extend_from_slice(&piece);
                if bytes.len() > INSTALLER_AT_MOST {
                    return Err(Error::TooLarge);
                }
                told(bytes.len() as u64, of);
            }
            Ok(Err(error)) => return Err(Error::Unreachable(error.to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(Error::Silent(SILENT_FOR_AT_MOST.as_secs()));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Error::Unreachable("the download ended early".into()));
            }
        }
    }
    verify(&bytes, &release.signature, PUBLIC_KEY, &release.version)?;
    // (Signed, and still checked: the installer is run as a program.)
    if !bytes.starts_with(b"MZ") {
        return Err(Error::NotAnInstaller);
    }
    Ok(bytes)
}

/// Whether `signature` (base64, as the releases give it) is the signature of `bytes` by the key
/// `public_key` (base64, as `tauri signer generate` writes it), made for `version`. A signature
/// that does not say its version is refused: every release Perora's workflow signs says it, and
/// one that does not could be an older release's under a newer number.
pub fn verify(bytes: &[u8], signature: &str, public_key: &str, version: &str) -> Result<(), Error> {
    let text = |base64: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(base64.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| Error::Signature("not in base64".into()))
    };
    let key = minisign_verify::PublicKey::decode(&text(public_key)?)
        .map_err(|error| Error::Signature(error.to_string()))?;
    let signature = minisign_verify::Signature::decode(&text(signature)?)
        .map_err(|error| Error::Signature(error.to_string()))?;
    // (Only the prehashed kind, which is what `tauri signer` makes.)
    key.verify(bytes, &signature, false)
        .map_err(|error| Error::Signature(error.to_string()))?;
    // The trusted comment is covered by the signature, so it can be believed only now.
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .ok_or(Error::NoSignedVersion)?;
    if parse_version(signed)? != parse_version(version)? {
        return Err(Error::OtherVersion {
            signed: signed.into(),
            announced: version.into(),
        });
    }
    Ok(())
}

/// What the installer is run with, as the Tauri version's updater ran it: without asking anything
/// but with its progress shown (`/P`), as an update (`/UPDATE`: the old version is not uninstalled
/// first, and what it registered stays), and starting Perora again when it is done (`/R`, with
/// what follows `/ARGS`). `in_tray`: the new version starts in the notification area, its window
/// not shown (`--tray`); otherwise it starts as Perora started without a word does, with it.
pub fn installer_parameters(in_tray: bool) -> &'static str {
    if in_tray {
        "/P /UPDATE /R /ARGS --tray"
    } else {
        "/P /UPDATE /R /ARGS"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made for these tests with a throwaway key (`tauri signer generate`, then `tauri signer sign
    /// --app-version 0.2.1` over `SIGNED`): the format the release workflow's signatures come in.
    const TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDE4MEQ0QUI1QTUxODI5NzMKUldSektSaWx0VW9OR0hBL3hYUlptMUZMSDVZVEFZaHpWR09OYXRJc1Q5cktaeEM3VHJkODdKR3kK";
    const SIGNED: &[u8] = b"Perora updater test vector\n";
    const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSektSaWx0VW9OR09jSklPZ2dHeWV0d0VoYUEwVXgvZmNrT0o2TlFENUhUb0RaQzdINVJ5NHllQjljK2pIV09Zb3RrR1RUWjdRT1c4ZTVYZUZEQVVxamZzdHhHbWFXN2dFPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0MDM2OTI3CWZpbGU6c21hbGwuYmluCXZlcnNpb246MC4yLjEKRmhKZnpGMXpUbFZpYUpVYmMrcDNCcE5rU2IrMFhscDRuL3RobXlQR0NPV0JNaUtSd3hpVXVFRXlDNnVnL3pHdUFQRTVlNC9lanoxNG5NdmptR2QyQmc9PQo=";

    /// The latest.json of 0.1.7, as tauri-action wrote it.
    const LATEST_0_1_7: &str = r#"{
      "version": "0.1.7",
      "notes": "Download and run Perora_*_x64-setup.exe. Perora already installed updates itself.",
      "pub_date": "2026-11-02T15:12:06.915Z",
      "platforms": {
        "windows-x86_64": {
          "signature": "plain",
          "url": "https://github.com/fltuna/perora/releases/download/v0.1.7/Perora_0.1.7_x64-setup.exe"
        },
        "windows-x86_64-nsis": {
          "signature": "nsis",
          "url": "https://github.com/fltuna/perora/releases/download/v0.1.7/Perora_0.1.7_x64-setup.exe"
        }
      }
    }"#;

    #[test]
    fn a_signature_of_the_workflow_s_kind_is_checked() {
        assert!(verify(SIGNED, SIGNATURE, TEST_KEY, "0.2.1").is_ok());
        assert!(verify(SIGNED, SIGNATURE, TEST_KEY, "v0.2.1").is_ok());
        let mut changed = SIGNED.to_vec();
        changed[0] ^= 1;
        assert!(matches!(
            verify(&changed, SIGNATURE, TEST_KEY, "0.2.1"),
            Err(Error::Signature(_))
        ));
        // Signed for 0.2.1: not to be installed as another version.
        assert!(matches!(
            verify(SIGNED, SIGNATURE, TEST_KEY, "0.2.2"),
            Err(Error::OtherVersion { .. })
        ));
        // Not by Perora's key.
        assert!(matches!(
            verify(SIGNED, SIGNATURE, PUBLIC_KEY, "0.2.1"),
            Err(Error::Signature(_))
        ));
        assert!(verify(SIGNED, "not base64!", TEST_KEY, "0.2.1").is_err());
    }

    #[test]
    fn the_key_is_perora_s() {
        let text = base64::engine::general_purpose::STANDARD
            .decode(PUBLIC_KEY)
            .unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains("minisign public key: B21287D738A95859"));
        assert!(minisign_verify::PublicKey::decode(&text).is_ok());
    }

    #[test]
    fn only_a_newer_version_is_offered() {
        assert_eq!(newer(LATEST_0_1_7.as_bytes(), "0.1.7").unwrap(), None);
        assert_eq!(newer(LATEST_0_1_7.as_bytes(), "0.2.0").unwrap(), None);
        let found = newer(LATEST_0_1_7.as_bytes(), "0.1.6").unwrap().unwrap();
        assert_eq!(found.version, "0.1.7");
        // The NSIS one by name first.
        assert_eq!(found.signature, "nsis");
        assert_eq!(
            found.url,
            "https://github.com/fltuna/perora/releases/download/v0.1.7/Perora_0.1.7_x64-setup.exe"
        );
        // A prerelease of a version is older than it.
        assert!(newer(LATEST_0_1_7.as_bytes(), "0.1.7-rc.1").unwrap().is_some());
    }

    #[test]
    fn a_release_written_otherwise_is_taken_or_refused() {
        let plain_only = r#"{"version": "v0.3.0", "platforms": {"windows-x86_64": {"url": "https://example.com/a.exe", "signature": "s"}}}"#;
        let found = newer(plain_only.as_bytes(), "0.2.0").unwrap().unwrap();
        assert_eq!((found.version.as_str(), found.signature.as_str()), ("0.3.0", "s"));

        let other_system = r#"{"version": "0.3.0", "platforms": {"darwin-aarch64": {"url": "https://example.com/a", "signature": "s"}}}"#;
        assert!(matches!(
            newer(other_system.as_bytes(), "0.2.0"),
            Err(Error::NoInstaller)
        ));
        let not_https = r#"{"version": "0.3.0", "platforms": {"windows-x86_64": {"url": "http://example.com/a.exe", "signature": "s"}}}"#;
        assert!(matches!(
            newer(not_https.as_bytes(), "0.2.0"),
            Err(Error::Unreadable(_))
        ));
        assert!(newer(b"not json", "0.2.0").is_err());
        assert!(newer(br#"{"version": "soon", "platforms": {}}"#, "0.2.0").is_err());
    }

    #[test]
    fn what_changed_since_the_version_installed_comes_from_the_history() {
        let latest = r###"{"version": "0.2.3",
            "changes": {"ja": "## v0.2.3\n- 三", "en": "## v0.2.3\n- Three"},
            "history": {
                "ja": "# 変更履歴\n\n## v0.2.3\n\n- 三\n\n## v0.2.2\n\n- 二\n\n## v0.2.1\n\n- 一\n\n## v0.2.0\n\n- 作り直し\n",
                "en": "# Changelog\n\n## v0.2.3\n\n- Three\n\n## v0.2.1\n\n- One\n"
            },
            "platforms": {"windows-x86_64": {"url": "https://example.com/a.exe", "signature": "s"}}}"###;
        // Two versions passed over: all three, newest first; not the one installed.
        let found = newer(latest.as_bytes(), "0.2.0").unwrap().unwrap();
        assert_eq!(
            found.changes("ja"),
            Some("## v0.2.3\n\n- 三\n\n## v0.2.2\n\n- 二\n\n## v0.2.1\n\n- 一")
        );
        // (What the history has: no 0.2.2 in English.)
        assert_eq!(
            found.changes("en"),
            Some("## v0.2.3\n\n- Three\n\n## v0.2.1\n\n- One")
        );
        // The version before: that one alone.
        let found = newer(latest.as_bytes(), "0.2.2").unwrap().unwrap();
        assert_eq!(found.changes("ja"), Some("## v0.2.3\n\n- 三"));
        // Older than all the history has: what it has.
        let found = newer(latest.as_bytes(), "0.1.7").unwrap().unwrap();
        assert!(found.changes("ja").unwrap().ends_with("## v0.2.0\n\n- 作り直し"));

        // No history (0.2.0's release): what changed in this version.
        let without = r###"{"version": "0.2.3", "changes": {"ja": "## v0.2.3\n- 三"},
            "platforms": {"windows-x86_64": {"url": "https://example.com/a.exe", "signature": "s"}}}"###;
        let found = newer(without.as_bytes(), "0.2.0").unwrap().unwrap();
        assert_eq!(found.changes("ja"), Some("## v0.2.3\n- 三"));

        // A history with nothing after the version installed, or too much of it: the same.
        let unhelpful = format!(
            r###"{{"version": "0.2.3", "changes": {{"ja": "## v0.2.3\n- 三"}},
            "history": {{"ja": "## v0.2.0\n- 作り直し", "en": "{}"}},
            "platforms": {{"windows-x86_64": {{"url": "https://example.com/a.exe", "signature": "s"}}}}}}"###,
            "a".repeat(CHANGES_AT_MOST + 1)
        );
        let found = newer(unhelpful.as_bytes(), "0.2.0").unwrap().unwrap();
        assert_eq!(found.changes("ja"), Some("## v0.2.3\n- 三"));
        assert_eq!(found.changes("en"), Some("## v0.2.3\n- 三"));
    }

    #[test]
    fn what_changed_is_said_in_the_language_asked_or_else_in_the_other() {
        let both = r###"{"version": "0.3.0", "notes": "Download and run it.",
            "changes": {"ja": "## v0.3.0\n- 直した", "en": "## v0.3.0\n- Fixed"},
            "platforms": {"windows-x86_64": {"url": "https://example.com/a.exe", "signature": "s"}}}"###;
        let found = newer(both.as_bytes(), "0.2.0").unwrap().unwrap();
        assert_eq!(found.changes("ja"), Some("## v0.3.0\n- 直した"));
        assert_eq!(found.changes("en"), Some("## v0.3.0\n- Fixed"));
        // A language the release does not say it in.
        assert_eq!(found.changes("fr"), Some("## v0.3.0\n- Fixed"));

        let japanese_only = r#"{"version": "0.3.0", "changes": {"ja": "- 直した", "en": "  "},
            "platforms": {"windows-x86_64": {"url": "https://example.com/a.exe", "signature": "s"}}}"#;
        let found = newer(japanese_only.as_bytes(), "0.2.0").unwrap().unwrap();
        assert_eq!(found.changes("en"), Some("- 直した"));
        assert_eq!(found.changes("ja"), Some("- 直した"));

        // 0.1.x's releases say nothing of it.
        let found = newer(LATEST_0_1_7.as_bytes(), "0.1.6").unwrap().unwrap();
        assert_eq!(found.changes("ja"), None);

        // Too much to say is not said.
        let long = format!(
            r#"{{"version": "0.3.0", "changes": {{"ja": "{}", "en": "- Fixed"}},
            "platforms": {{"windows-x86_64": {{"url": "https://example.com/a.exe", "signature": "s"}}}}}}"#,
            "a".repeat(CHANGES_AT_MOST + 1)
        );
        let found = newer(long.as_bytes(), "0.2.0").unwrap().unwrap();
        assert_eq!(found.changes("ja"), Some("- Fixed"));
    }

    #[test]
    fn the_installer_is_run_as_an_update_that_starts_perora_again() {
        assert_eq!(installer_parameters(true), "/P /UPDATE /R /ARGS --tray");
        assert_eq!(installer_parameters(false), "/P /UPDATE /R /ARGS");
    }
}
