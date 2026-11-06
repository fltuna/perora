//! What the editor remembers of how it was last used, from one capture to the next and from one run
//! to the next: the font last written in, how strongly a part was last made unreadable each way, and
//! which way was last taken. Not the user's settings (`settings.json`, what they chose as their
//! defaults): `editor-settings.json` beside it (owner: 「普通にedtiro-settings.jsonとかでいい気がする」).
//! The Tauri version kept these in the WebView's storage, which is not read: from 0.1.x, they start
//! again once from what they are at first.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::settings::config_directory;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EditorSettings {
    /// The font last written in (empty: Perora's own).
    pub font: String,
    /// How strongly a part was last blurred, and broken into blocks (1 to 10).
    pub blur: u32,
    pub mosaic: u32,
    /// Which of the two was taken last: "blur" or "mosaic".
    pub hider: String,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font: String::new(),
            blur: 3,
            mosaic: 5,
            hider: "blur".into(),
        }
    }
}

/// Where they are kept. (`PERORA_EDITOR_SETTINGS` names another file: for trying the editor
/// without touching what the user's Perora remembers.)
fn path() -> Option<PathBuf> {
    if let Some(elsewhere) = std::env::var_os("PERORA_EDITOR_SETTINGS") {
        return Some(PathBuf::from(elsewhere));
    }
    Some(config_directory()?.join("editor-settings.json"))
}

impl EditorSettings {
    /// As they were last kept; as they are at first when there are none (or they cannot be read).
    pub fn load() -> Self {
        path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Kept for the next time. (Not kept, nothing is lost but this.)
    pub fn save(&self) {
        let Some(path) = path() else {
            return;
        };
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    /// Changed by `change`, and kept.
    pub fn update(change: impl FnOnce(&mut Self)) {
        let mut settings = Self::load();
        let before = settings.clone();
        change(&mut settings);
        if settings != before {
            settings.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_not_said_is_as_it_is_at_first() {
        let read: EditorSettings = serde_json::from_str(r#"{"font":"Yu Mincho"}"#).unwrap();
        assert_eq!(read.font, "Yu Mincho");
        assert_eq!(
            (read.blur, read.mosaic, read.hider.as_str()),
            (3, 5, "blur")
        );
    }
}
