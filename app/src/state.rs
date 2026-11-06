//! What the application knows while it runs: the settings (as in the file), the language, and what
//! became of the shortcut. One of it, kept as a GPUI global.

use std::path::PathBuf;
use std::sync::Mutex;

use gpui::{App, Global};
use perora_core::language::{Language, Languages, user_directory};
use perora_core::settings::{self, Settings};

pub struct Perora {
    /// Where the settings are written: `%APPDATA%\dev.f2a.perora\settings.json`, the Tauri version's.
    pub settings_path: Option<PathBuf>,
    pub settings: Mutex<Settings>,
    pub languages: Languages,
    /// Which shortcuts are Perora's.
    pub claimed: crate::shortcut::Claimed,
    /// Whether the shell left the system's combination for snipping alone when it last started:
    /// what it was told by then. (When it was told otherwise since, it has to start anew before
    /// things are as the user chose.)
    pub shell_left_alone: bool,
    /// The shell is being started anew, as the settings asked: it is not asked to again meanwhile.
    pub shell_restarting: bool,
}

impl Global for Perora {}

impl Perora {
    pub fn load() -> Self {
        let settings_path = settings::settings_path();
        let chosen = settings_path
            .as_deref()
            .map(settings::read)
            .unwrap_or_default();
        Self {
            settings_path,
            settings: Mutex::new(chosen),
            languages: Languages::load(user_directory().as_deref()),
            claimed: Default::default(),
            shell_left_alone: false,
            shell_restarting: false,
        }
    }

    pub fn get(cx: &App) -> &Perora {
        cx.global::<Perora>()
    }

    /// The language the settings name, or the system's when they name none (or one not there).
    pub fn language(&self) -> Language {
        let chosen = self.settings().language.clone();
        chosen
            .as_deref()
            .and_then(|code| self.languages.by_code(code))
            .unwrap_or_else(|| self.languages.of_the_system())
    }

    /// The combination that captures, as shown: `None` when none is Perora's.
    pub fn shortcut_label(&self) -> Option<String> {
        self.claimed.label(&self.settings().shortcut)
    }

    /// The combination that captures now, whether or not the system gave it to Perora.
    pub fn shortcut_shown(&self) -> String {
        self.claimed.shown(&self.settings().shortcut)
    }

    /// A copy of the settings as they are now.
    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Changes the settings: at once for the running application, and in the file for the next run
    /// (with whatever else the file holds kept as it is). Without a file to write, only for this
    /// run. What is changed is the settings as they are when this is called, not a copy taken
    /// earlier: two changes made one after the other both count.
    pub fn change(&self, change: impl FnOnce(&mut Settings)) {
        match &self.settings_path {
            Some(path) => settings::update(path, &self.settings, change),
            None => change(
                &mut self
                    .settings
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ),
        }
    }
}
