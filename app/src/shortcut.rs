//! The global shortcut: the combination in the settings, registered with the system
//! (`RegisterHotKey` through `global-hotkey`, as the Tauri version's plugin did, with the same grammar:
//! `Ctrl+Shift+H`).

use std::str::FromStr;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use perora_core::settings::DEFAULT_SHORTCUT;

/// Whether a key combination may be a global shortcut: one that typing does not need. A letter, a
/// digit or any other key of the main block needs Ctrl, Alt or the Windows key with it (with Shift
/// alone it is a capital letter); function keys and PrintScreen may stand alone.
pub fn is_a_shortcut(combination: &str) -> bool {
    let mut keys: Vec<&str> = combination.split('+').collect();
    let Some(key) = keys.pop() else {
        return false;
    };
    let function_key = key
        .strip_prefix('F')
        .is_some_and(|number| matches!(number.parse::<u8>(), Ok(1..=24)));
    let strong = keys
        .iter()
        .any(|modifier| matches!(*modifier, "Ctrl" | "Alt" | "Super"));
    strong || function_key || key == "PrintScreen"
}

/// The combination to register: the settings', or the default when theirs means nothing (edited by
/// hand). (As in the Tauri version, only that: whether it is one typing needs is the settings
/// page's to check when it is chosen.)
pub fn chosen(setting: &str) -> String {
    if HotKey::from_str(setting).is_ok() {
        setting.to_owned()
    } else {
        DEFAULT_SHORTCUT.to_owned()
    }
}

/// The system's own combination for snipping, and the letter the shell is told to leave alone for
/// it. The shell leaves every combination with that letter alone then, the one for searching
/// (Win+S) among them: nobody has that one while this is on. (Perora does not take it to search in
/// the shell's place: what opens the system's search from outside is not what the key opens.)
pub const SNIPPING: &str = "Super+Shift+S";
/// As it is shown: by the name the key has on a keyboard.
pub const SNIPPING_SHOWN: &str = "Win+Shift+S";

pub struct Shortcuts {
    manager: GlobalHotKeyManager,
    /// The user's own combination, while it is registered.
    own: Option<HotKey>,
    /// The system's combination for snipping, while it is Perora's.
    snipping: Option<HotKey>,
    /// The user's own combination is let go of for the moment, on purpose: another is being
    /// recorded on the settings page, and pressing the old one is an answer there, not a capture.
    suspended: bool,
}

/// Which shortcuts are Perora's, after `Shortcuts::claim`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Claimed {
    /// The user's own combination is Perora's.
    pub own: bool,
    /// The system's combination for snipping is Perora's.
    pub snipping: bool,
}

impl Claimed {
    /// The combination that captures, as shown: `None` when none is Perora's.
    pub fn label(&self, own: &str) -> Option<String> {
        if self.snipping {
            Some(SNIPPING_SHOWN.to_owned())
        } else {
            self.own.then(|| own.to_owned())
        }
    }

    /// The combination that captures now, whether or not the system gave it to Perora.
    pub fn shown(&self, own: &str) -> String {
        if self.snipping {
            SNIPPING_SHOWN.to_owned()
        } else {
            own.to_owned()
        }
    }
}

impl Shortcuts {
    pub fn new() -> Option<Self> {
        GlobalHotKeyManager::new().ok().map(|manager| Self {
            manager,
            own: None,
            snipping: None,
            suspended: false,
        })
    }

    /// Registers the user's combination (`own`) and, when `wanted`, the system's for snipping. One
    /// shortcut at a time: while the system's is Perora's, the user's own is let go of (it does
    /// nothing, and is not to be changed); it is asked for again when the system's is not Perora's
    /// any more, or not yet. The system's is Perora's only once the shell has let go of it (it has
    /// started since it was told to): until then registering it fails, and the user's stays.
    pub fn claim(&mut self, own: &str, wanted: bool) -> Claimed {
        let snipping = HotKey::from_str(SNIPPING).ok();
        let own_key = HotKey::from_str(own).ok();
        // The user chose the system's combination as their own: one registration serves both.
        let same = own_key.is_some() && own_key.map(|key| key.id()) == snipping.map(|key| key.id());

        if wanted {
            if self.snipping.is_none()
                && let Some(key) = snipping
            {
                if same && self.own.is_some() {
                    self.snipping = self.own;
                } else if self.manager.register(key).is_ok() {
                    self.snipping = Some(key);
                }
            }
        } else if let Some(key) = self.snipping.take()
            && !same
        {
            let _ = self.manager.unregister(key);
        }

        if self.snipping.is_some() && !same {
            if let Some(key) = self.own.take() {
                let _ = self.manager.unregister(key);
            }
        } else if self.suspended && !(same && self.snipping.is_some()) {
            // (Let go of while another is recorded: whatever ends that asks for it again.)
            if let Some(key) = self.own.take() {
                let _ = self.manager.unregister(key);
            }
        } else if self.own.map(|key| key.id()) != own_key.map(|key| key.id()) {
            if let Some(key) = self.own.take() {
                let _ = self.manager.unregister(key);
            }
            // (When it is the system's combination, already registered for snipping, it is
            // Perora's without registering it twice.)
            if let Some(key) = own_key
                && ((same && self.snipping.is_some()) || self.manager.register(key).is_ok())
            {
                self.own = Some(key);
            }
        }
        Claimed {
            own: self.own.is_some(),
            snipping: self.snipping.is_some(),
        }
    }

    /// Lets go of the user's own combination for the moment (`suspended`: another is being
    /// recorded), or asks for it again; as `claim` otherwise.
    pub fn suspend(&mut self, suspended: bool, own: &str, wanted: bool) -> Claimed {
        self.suspended = suspended;
        self.claim(own, wanted)
    }

    /// Makes `new` the user's own combination in place of `old`: refused (`Err`) when the system
    /// would not give it (another application has it), and `old` is asked for again then. Either
    /// way, which shortcuts are Perora's afterwards.
    pub fn change(&mut self, new: &str, old: &str, wanted: bool) -> Result<Claimed, Claimed> {
        // The one before goes first: the new one may be the same combination.
        let suspended = std::mem::replace(&mut self.suspended, true);
        self.claim(old, wanted);
        let given = HotKey::from_str(new).ok().filter(|key| {
            // (One Perora has already, as the system's that it took, is the user's own as well.)
            self.snipping.map(|snipping| snipping.id()) == Some(key.id())
                || self.manager.register(*key).is_ok()
        });
        self.suspended = suspended;
        match given {
            Some(key) => {
                self.own = Some(key);
                Ok(self.claim(new, wanted))
            }
            None => Err(self.claim(old, wanted)),
        }
    }

    /// Calls `pressed` whenever a registered combination is pressed (not when held: the system
    /// repeats nothing, `MOD_NOREPEAT`).
    pub fn on_pressed(pressed: impl Fn() + Send + Sync + 'static) {
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed {
                pressed();
            }
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_is_one_typing_does_not_need() {
        assert!(is_a_shortcut("Ctrl+Shift+H"));
        assert!(is_a_shortcut("Alt+F9"));
        assert!(is_a_shortcut("F9"));
        assert!(is_a_shortcut("PrintScreen"));
        assert!(is_a_shortcut("Super+Shift+S"));
        assert!(!is_a_shortcut("H"));
        assert!(!is_a_shortcut("Shift+H"));
        assert!(!is_a_shortcut("F25"));
    }

    #[test]
    fn a_setting_that_means_nothing_gives_the_default() {
        assert_eq!(chosen("Ctrl+Shift+H"), "Ctrl+Shift+H");
        assert_eq!(chosen("Alt+F9"), "Alt+F9");
        assert_eq!(chosen("nonsense"), DEFAULT_SHORTCUT);
        assert_eq!(chosen(""), DEFAULT_SHORTCUT);
        // Parsed, so kept, as the Tauri version did: a hand edit is the user's to make.
        assert_eq!(chosen("Shift+H"), "Shift+H");
    }
}
