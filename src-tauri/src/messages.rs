//! What Perora says in its menu and its notifications, read from language files.
//!
//! A language file is a flat JSON object of texts by key (`lang/en.json` is the reference; `{name}` and
//! the like are filled in). English and Japanese are built in. Any `<code>.json` in the `lang` directory
//! of the user's configuration directory is offered as a language too; one named like a built-in
//! language changes the texts it gives and leaves the others as they are. A text a language does not
//! have is said in English.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use perora_core::account::AccountError;
use perora_core::oidc::OidcError;
use perora_core::shots::ApiError;

const BUILT_IN: [(&str, &str); 2] = [
    ("en", include_str!("../../lang/en.json")),
    ("ja", include_str!("../../lang/ja.json")),
];

/// The language every other one falls back to.
const REFERENCE: &str = "en";

/// A language file is text for a menu, not a place to put megabytes.
const MAX_FILE_BYTES: u64 = 256 * 1024;

struct Catalog {
    code: String,
    texts: HashMap<String, String>,
    /// The reference language, for what this one leaves out. `None` in the reference itself.
    fallback: Option<Arc<Catalog>>,
}

/// One language Perora can speak.
#[derive(Clone)]
pub struct Language(Arc<Catalog>);

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.0.code == other.0.code
    }
}

/// Every language available in this run.
pub struct Languages(Vec<Language>);

impl Languages {
    /// The built-in languages, and those found in `directory`.
    pub fn load(directory: Option<&Path>) -> Self {
        let parse = |json: &str| serde_json::from_str::<HashMap<String, String>>(json).ok();
        let reference = Arc::new(Catalog {
            code: REFERENCE.into(),
            texts: BUILT_IN
                .iter()
                .find(|(code, _)| *code == REFERENCE)
                .and_then(|(_, json)| parse(json))
                .expect("the built-in reference language is valid"),
            fallback: None,
        });

        let mut languages = vec![Language(reference.clone())];
        let mut add = |code: &str, mut texts: HashMap<String, String>| {
            let known = languages.iter_mut().find(|known| known.code() == code);
            // On top of what is known of that language already.
            if let Some(known) = &known {
                for (key, text) in &known.0.texts {
                    texts.entry(key.clone()).or_insert_with(|| text.clone());
                }
            }
            let language = Language(Arc::new(Catalog {
                code: code.into(),
                texts,
                fallback: Some(reference.clone()),
            }));
            match known {
                Some(known) => *known = language,
                None => languages.push(language),
            }
        };

        for (code, json) in BUILT_IN {
            if code != REFERENCE {
                add(code, parse(json).expect("the built-in languages are valid"));
            }
        }
        for (code, texts) in directory.map(read_directory).unwrap_or_default() {
            add(&code, texts);
        }
        Self(languages)
    }

    pub fn all(&self) -> &[Language] {
        &self.0
    }

    pub fn by_code(&self, code: &str) -> Option<Language> {
        self.0
            .iter()
            .find(|language| language.code() == code)
            .cloned()
    }

    /// The language to start in when none was chosen: the first one the system names (the language it
    /// is shown in, then the region it is set up for: many run Windows in English and read another
    /// language), English otherwise.
    pub fn of_the_system(&self) -> Language {
        self.preferred(&[
            perora_core::platform::ui_language(),
            perora_core::platform::regional_locale(),
        ])
    }

    fn preferred(&self, tags: &[Option<String>]) -> Language {
        let tags: Vec<String> = tags
            .iter()
            .flatten()
            .map(|tag| tag.to_ascii_lowercase())
            .collect();
        // A language other than the reference wins wherever it is named: the reference is what one
        // gets anyway.
        let named = |language: &&Language| {
            let code = language.code().to_ascii_lowercase();
            tags.iter()
                .any(|tag| *tag == code || tag.starts_with(&format!("{code}-")))
        };
        self.0
            .iter()
            .filter(|language| language.code() != REFERENCE)
            .find(named)
            .or_else(|| self.0.iter().find(named))
            .unwrap_or(&self.0[0])
            .clone()
    }
}

/// The language files of a directory, by code (the file name without `.json`). Files that are not a
/// flat object of texts are skipped.
fn read_directory(directory: &Path) -> Vec<(String, HashMap<String, String>)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let code = path.file_stem()?.to_str()?.to_owned();
            let is_code = !code.is_empty()
                && code.len() <= 16
                && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if !is_code
                || path.extension()?.to_str()? != "json"
                || entry.metadata().ok()?.len() > MAX_FILE_BYTES
            {
                return None;
            }
            let texts = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            Some((code, texts))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

impl Language {
    /// As written in the settings file, and the name of its file.
    pub fn code(&self) -> &str {
        &self.0.code
    }

    /// The name of the language in itself, as a choice in the menu.
    pub fn name(&self) -> String {
        match self.0.texts.get("language.name") {
            Some(name) => name.clone(),
            None => self.0.code.clone(),
        }
    }

    /// Every text of the language, with those it does not have taken from the reference: for the
    /// windows, which look texts up by key themselves.
    pub fn texts(&self) -> HashMap<String, String> {
        let mut texts = self.0.texts.clone();
        if let Some(fallback) = &self.0.fallback {
            for (key, text) in &fallback.texts {
                texts.entry(key.clone()).or_insert_with(|| text.clone());
            }
        }
        texts
    }

    /// The text for `key`, with each `{name}` replaced by its value.
    fn text(&self, key: &str, values: &[(&str, &str)]) -> String {
        let template = self
            .0
            .texts
            .get(key)
            .or_else(|| self.0.fallback.as_ref()?.texts.get(key))
            .map(String::as_str)
            .unwrap_or(key);
        // In one pass over the template: a value is never searched for placeholders itself.
        let mut text = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find('{') {
            text.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let value = after.find('}').and_then(|end| {
                let found = values.iter().find(|(name, _)| *name == &after[..end])?;
                Some((end, found.1))
            });
            match value {
                Some((end, value)) => {
                    text.push_str(value);
                    rest = &after[end + 1..];
                }
                None => {
                    text.push('{');
                    rest = after;
                }
            }
        }
        text.push_str(rest);
        text
    }

    pub fn capture(&self, shortcut: Option<&str>) -> String {
        let label = self.text("menu.capture", &[]);
        match shortcut {
            Some(shortcut) => format!("{label} ({shortcut})"),
            None => label,
        }
    }

    pub fn open(&self) -> String {
        self.text("menu.open", &[])
    }

    pub fn quit(&self) -> String {
        self.text("menu.quit", &[])
    }

    pub fn signed_in(&self, name: &str) -> String {
        self.text("notify.signedIn", &[("name", name)])
    }

    pub fn captured(&self) -> String {
        self.text("notify.captured", &[])
    }

    pub fn copied(&self) -> String {
        self.text("notify.copied", &[])
    }

    pub fn not_copied(&self, detail: &str) -> String {
        self.text("notify.notCopied", &[("detail", detail)])
    }

    pub fn notices_here(&self) -> String {
        self.text("notify.here", &[])
    }

    pub fn saved(&self, path: &str) -> String {
        self.text("notify.saved", &[("path", path)])
    }

    pub fn saved_several(&self, path: &str, others: usize) -> String {
        self.text(
            "notify.savedSeveral",
            &[("path", path), ("others", &others.to_string())],
        )
    }

    /// A capture saved the moment it was taken, as the user has every capture saved: as one kind of
    /// file, or as `others` more beside it.
    pub fn saved_as_taken(&self, path: &str, others: usize) -> String {
        match others {
            0 => self.text("notify.savedAsTaken", &[("path", path)]),
            others => self.text(
                "notify.savedAsTakenSeveral",
                &[("path", path), ("others", &others.to_string())],
            ),
        }
    }

    /// Put on the clipboard and saved as it was taken: all that becomes of a capture without a
    /// preview.
    pub fn copied_and_saved(&self, path: &str) -> String {
        self.text("notify.copiedAndSaved", &[("path", path)])
    }

    pub fn not_saved_as_taken(&self, detail: &str) -> String {
        self.text("notify.notSavedAsTaken", &[("detail", detail)])
    }

    pub fn saved_but_not(&self, path: &str, kind: &str) -> String {
        self.text("notify.savedButNot", &[("path", path), ("kind", kind)])
    }

    /// Why saving stopped, said where "could not be saved" is said: the kind that was not written,
    /// why, and the file that is there all the same.
    pub fn not_all_saved(&self, kind: &str, detail: &str, name: &str) -> String {
        self.text(
            "save.notAll",
            &[("kind", kind), ("detail", detail), ("name", name)],
        )
    }

    /// Asked before files the user did not name are written over.
    pub fn there_already(&self, names: &str) -> String {
        self.text("save.thereAlready", &[("names", names)])
    }

    pub fn replace(&self) -> String {
        self.text("save.replace", &[])
    }

    pub fn keep(&self) -> String {
        self.text("save.keep", &[])
    }

    pub fn uploaded(&self) -> String {
        self.text("notify.uploaded", &[])
    }

    pub fn uploaded_but_not_copied(&self, url: &str) -> String {
        self.text("notify.uploadedNotCopied", &[("url", url)])
    }

    pub fn not_signed_in(&self) -> String {
        self.text("notify.notSignedIn", &[])
    }

    pub fn shortcut_unavailable(&self, shortcut: &str) -> String {
        self.text("notify.shortcutUnavailable", &[("shortcut", shortcut)])
    }

    pub fn capture_failed(&self, detail: &str) -> String {
        self.text("error.capture", &[("detail", detail)])
    }

    pub fn windows_lost(&self) -> String {
        self.text("error.windowsLost", &[])
    }

    pub fn upload_failed(&self, error: &ApiError) -> String {
        match error {
            ApiError::SignedOut => self.not_signed_in(),
            ApiError::Forbidden => self.text("error.forbidden", &[]),
            ApiError::TooLarge => self.text("error.tooLarge", &[]),
            ApiError::Unreachable(_) => self.text("error.unreachable", &[]),
            ApiError::Account(error) => self.account_failed(error),
            ApiError::Rejected { .. } => {
                self.text("error.upload", &[("detail", &error.to_string())])
            }
        }
    }

    pub fn update_found(&self, version: &str) -> String {
        self.text("notify.updateFound", &[("version", version)])
    }

    pub fn update_capture_in_hand(&self) -> String {
        self.text("error.updateCaptureInHand", &[])
    }

    pub fn update_gone(&self) -> String {
        self.text("error.updateGone", &[])
    }

    pub fn update_failed(&self, detail: &str) -> String {
        self.text("error.update", &[("detail", detail)])
    }

    pub fn account_failed(&self, error: &AccountError) -> String {
        match error {
            AccountError::SignedOut => self.not_signed_in(),
            AccountError::Oidc(OidcError::TimedOut) => self.text("error.signInTimedOut", &[]),
            AccountError::Oidc(OidcError::Unreachable { .. }) => {
                self.text("error.signInUnreachable", &[])
            }
            AccountError::Oidc(OidcError::Refused { .. }) => self.text("error.signInRefused", &[]),
            other => self.text("error.signIn", &[("detail", &other.to_string())]),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn placeholders(text: &str) -> BTreeSet<String> {
        text.split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}'))
            .map(|(name, _)| name.to_owned())
            .collect()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("perora-lang-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn the_built_in_languages_have_the_same_texts_with_the_same_blanks() {
        let languages = Languages::load(None);
        let reference = &languages.by_code("en").unwrap().0.texts;
        assert_eq!(languages.all().len(), BUILT_IN.len());

        for language in languages.all() {
            let texts = &language.0.texts;
            let keys = |texts: &HashMap<String, String>| -> BTreeSet<String> {
                texts.keys().cloned().collect()
            };
            assert_eq!(keys(texts), keys(reference), "{}", language.code());
            for (key, text) in texts {
                assert_eq!(
                    placeholders(text),
                    placeholders(&reference[key]),
                    "{}: {key}",
                    language.code()
                );
            }
        }
    }

    #[test]
    fn every_text_the_code_asks_for_is_in_the_reference() {
        let english = Languages::load(None).by_code("en").unwrap();
        let said = [
            english.capture(None),
            english.open(),
            english.quit(),
            english.signed_in("n"),
            english.captured(),
            english.uploaded(),
            english.saved("p"),
            english.saved_several("p", 2),
            english.saved_but_not("p", "k"),
            english.not_all_saved("k", "d", "n"),
            english.there_already("n"),
            english.replace(),
            english.keep(),
            english.notices_here(),
            english.copied(),
            english.not_copied("d"),
            english.uploaded_but_not_copied("u"),
            english.not_signed_in(),
            english.shortcut_unavailable("s"),
            english.capture_failed("d"),
            english.windows_lost(),
            english.update_found("v"),
            english.update_capture_in_hand(),
            english.update_gone(),
            english.update_failed("d"),
            english.upload_failed(&ApiError::Forbidden),
            english.upload_failed(&ApiError::TooLarge),
            english.upload_failed(&ApiError::Unreachable("d".into())),
            english.upload_failed(&ApiError::Rejected {
                status: 500,
                detail: None,
            }),
            english.account_failed(&AccountError::Oidc(OidcError::TimedOut)),
            english.account_failed(&AccountError::Oidc(OidcError::Unreachable {
                what: "w",
                detail: "d".into(),
            })),
            english.account_failed(&AccountError::Oidc(OidcError::Refused {
                error: "e".into(),
                description: None,
            })),
            english.account_failed(&AccountError::Oidc(OidcError::Cancelled)),
        ];

        // A missing text comes out as its key, and no blank is left unfilled.
        for text in said {
            let looks_like_a_key = !text.contains(' ') && text.trim_end_matches('.').contains('.');
            assert!(!looks_like_a_key, "{text}");
            assert!(!text.contains('{'), "{text}");
        }
        assert_eq!(english.name(), "English");
        assert_eq!(english.signed_in("tuna"), "Signed in as tuna.");
    }

    #[test]
    fn a_value_is_put_in_as_it_is() {
        let english = Languages::load(None).by_code("en").unwrap();

        assert_eq!(
            english.signed_in("{name} {url}"),
            "Signed in as {name} {url}."
        );
        assert_eq!(english.text("no.such.key", &[]), "no.such.key");
    }

    #[test]
    fn a_file_in_the_directory_is_a_language_and_says_the_rest_in_english() {
        let directory = scratch("extra");
        std::fs::write(
            directory.join("fr.json"),
            r#"{"language.name": "Français", "menu.quit": "Quitter Perora"}"#,
        )
        .unwrap();
        std::fs::write(directory.join("ja.json"), r#"{"menu.quit": "おわる"}"#).unwrap();
        std::fs::write(directory.join("broken.json"), "{").unwrap();
        std::fs::write(directory.join("nested.json"), r#"{"menu": {"quit": "x"}}"#).unwrap();
        std::fs::write(directory.join("notes.txt"), "{}").unwrap();
        std::fs::write(directory.join("not a code!.json"), "{}").unwrap();

        let languages = Languages::load(Some(&directory));
        let codes: Vec<&str> = languages.all().iter().map(Language::code).collect();
        assert_eq!(codes, ["en", "ja", "fr"]);

        let french = languages.by_code("fr").unwrap();
        assert_eq!(french.name(), "Français");
        assert_eq!(french.quit(), "Quitter Perora");
        assert_eq!(french.open(), "Open Perora");
        assert_eq!(french.texts()["menu.quit"], "Quitter Perora");
        assert_eq!(french.texts()["menu.open"], "Open Perora");

        // A file named like a built-in language changes what it gives, and nothing else.
        let japanese = languages.by_code("ja").unwrap();
        assert_eq!(japanese.quit(), "おわる");
        assert_eq!(japanese.open(), "Perora を開く");
        assert_eq!(japanese.name(), "日本語");
    }

    #[test]
    fn the_starting_language_is_the_first_the_system_names() {
        let languages = Languages::load(None);
        let start = |tags: &[&str]| {
            let tags: Vec<_> = tags.iter().map(|tag| Some(tag.to_string())).collect();
            languages.preferred(&tags).code().to_owned()
        };

        assert_eq!(start(&["ja-JP"]), "ja");
        assert_eq!(
            start(&["en-US", "ja-JP"]),
            "ja",
            "shown in English, set up for Japan"
        );
        assert_eq!(start(&["en-US", "en-GB"]), "en");
        assert_eq!(start(&["de-DE"]), "en");
        assert_eq!(start(&[]), "en");
    }
}
