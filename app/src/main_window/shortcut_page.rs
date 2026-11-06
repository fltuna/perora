//! The shortcut page: the combination that captures, and how a new one is recorded (press "Change",
//! then the new combination; Esc gives up); and the system's own combination for snipping
//! (Win + Shift + S), which the shell has to be started anew for.
//!
//! While a new one is recorded the old one is not a shortcut: pressing it is an answer, not a
//! capture. Whatever ends the recording gives it back.

use std::time::Duration;

use gpui::{
    AnyElement, Context, IntoElement, KeyDownEvent, ParentElement, SharedString, Styled, Window,
    div, px,
};
use perora_core::language::Language;
use perora_ui::{Button, Reveal, Switch, Theme};

use super::MainWindow;
use super::rows::{about, card, keys, notice, section, setting};
use super::system::{self, Refusal};
use crate::state::Perora;

/// The keys a shortcut can end in, by what GPUI names them (a press that a program sent, which the
/// system has no keys held for), as the shortcut names them.
fn named(key: &str) -> Option<String> {
    let mut letters = key.chars();
    if let (Some(letter), None) = (letters.next(), letters.next())
        && letter.is_ascii_alphanumeric()
    {
        return Some(letter.to_ascii_uppercase().to_string());
    }
    if let Some(number) = key.strip_prefix('f')
        && matches!(number.parse::<u8>(), Ok(1..=24))
    {
        return Some(format!("F{number}"));
    }
    let name = match key {
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "enter" => "Enter",
        "space" => "Space",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "`" => "Backquote",
        "\\" => "Backslash",
        "[" => "BracketLeft",
        "]" => "BracketRight",
        "," => "Comma",
        "=" => "Equal",
        "-" => "Minus",
        "." => "Period",
        "'" => "Quote",
        ";" => "Semicolon",
        "/" => "Slash",
        _ => return None,
    };
    Some(name.to_owned())
}

/// The combination a key press makes, as the shortcut is written (`Ctrl+Alt+Shift+Super+H`, in that
/// order); `None` for a press that is no combination to use: a key a shortcut cannot end in, or a
/// key that typing needs (a letter, a digit, any other of the main block) without Ctrl, Alt or the
/// Windows key. Function keys and PrintScreen may stand alone.
fn combination(event: &KeyDownEvent) -> Option<String> {
    // By where the key is, as the system says it is held: what GPUI makes of a press depends on
    // Shift and the layout (Shift + 1 is `!` to it, without Shift). A press a program sent has no
    // keys held: what GPUI made of it is taken then.
    #[cfg(windows)]
    let held = perora_core::platform::held_key()
        .map(|key| (key.to_owned(), perora_core::platform::held_modifiers()));
    #[cfg(not(windows))]
    let held = None;
    let (key, [control, alt, shift, windows]) = match held {
        Some(held) => held,
        None => {
            let modifiers = &event.keystroke.modifiers;
            (
                named(&event.keystroke.key)?,
                [
                    modifiers.control,
                    modifiers.alt,
                    modifiers.shift,
                    modifiers.platform,
                ],
            )
        }
    };
    joined(&key, [control, alt, shift, windows])
}

/// A key with Ctrl, Alt, Shift and the Windows key (those held), as the shortcut is written; `None`
/// for a key that typing needs without Ctrl, Alt or the Windows key.
fn joined(key: &str, [control, alt, shift, windows]: [bool; 4]) -> Option<String> {
    let free = key
        .strip_prefix('F')
        .is_some_and(|number| matches!(number.parse::<u8>(), Ok(1..=24)))
        || key == "PrintScreen";
    if !control && !alt && !windows && !free {
        return None;
    }
    let mut parts: Vec<&str> = [
        (control, "Ctrl"),
        (alt, "Alt"),
        (shift, "Shift"),
        (windows, "Super"),
    ]
    .into_iter()
    .filter_map(|(held, name)| held.then_some(name))
    .collect();
    parts.push(key);
    Some(parts.join("+"))
}

/// How often the keys a window is never told of (PrintScreen, Pause, ScrollLock, NumLock) are
/// looked at while a shortcut is recorded.
const LOOKED_AT_EVERY: Duration = Duration::from_millis(25);

impl MainWindow {
    /// "Change": the next combination pressed is the shortcut. "Cancel": it is not.
    fn toggle_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recording {
            self.stop_recording(cx);
            return;
        }
        self.recording = true;
        system::suspend_shortcut(true, cx);
        // (The keys come to the window itself, whatever had them.)
        window.focus(&self.focus, cx);
        cx.notify();
        // The keys a window is never told were pressed: looked at for as long as it records.
        #[cfg(windows)]
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(LOOKED_AT_EVERY).await;
                let recording = this.update(cx, |this, cx| {
                    if !this.recording {
                        return false;
                    }
                    let Some((key, held)) = perora_core::platform::unheard_key() else {
                        return true;
                    };
                    match joined(key, held) {
                        Some(pressed) => {
                            this.answered(pressed, cx);
                            false
                        }
                        None => true,
                    }
                });
                if !matches!(recording, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        self.recording = false;
        system::suspend_shortcut(false, cx);
        cx.notify();
    }

    /// A key pressed anywhere in the window, before whatever has the keyboard hears it: while a
    /// shortcut is recorded, it is an answer, and nothing else hears it.
    pub(super) fn recorded(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.recording {
            return;
        }
        cx.stop_propagation();
        if event.keystroke.key == "escape" {
            self.stop_recording(cx);
            return;
        }
        // Not one to use: as if nothing was pressed, and the recording goes on.
        if let Some(pressed) = combination(event) {
            self.answered(pressed, cx);
        }
    }

    /// A combination to use was pressed: it is the shortcut, unless the system would not have it.
    fn answered(&mut self, pressed: String, cx: &mut Context<Self>) {
        self.stop_recording(cx);
        self.refusal = system::set_shortcut(&pressed, cx)
            .err()
            .map(|refusal| (refusal, pressed));
        cx.notify();
    }
}

pub fn page(
    this: &MainWindow,
    language: &Language,
    theme: &Theme,
    cx: &mut Context<MainWindow>,
) -> Vec<AnyElement> {
    let t = |key: &str| -> SharedString { language.text(key, &[]).into() };
    let perora = Perora::get(cx);
    let shown = perora.shortcut_shown();
    let registered = perora.shortcut_label().is_some();
    let snipping = system::snipping(cx);
    let recording = this.recording;
    let window_of = cx.entity();

    let mut page = vec![
        section(t("main.shortcut.section.keyboard"), true, theme).into_any_element(),
        card(
            about(
                t("main.shortcut.capture"),
                Some(t(if recording {
                    "main.shortcut.recording"
                } else {
                    "main.shortcut.captureHint"
                })),
                theme,
            ),
            Some(
                div()
                    .flex_none()
                    .flex()
                    .gap(px(8.0))
                    .child(keys(
                        if recording {
                            "…".to_owned()
                        } else {
                            shown.clone()
                        },
                        recording,
                        theme,
                    ))
                    // Not while the system's combination for snipping is to capture: that is the
                    // shortcut then, and this one is not asked about.
                    .child(
                        Button::new(
                            "change",
                            t(if recording {
                                "main.account.cancel"
                            } else {
                                "main.shortcut.change"
                            }),
                        )
                        .enabled(!snipping.wanted || recording)
                        .on_click(move |window, cx| {
                            window_of.update(cx, |this, cx| this.toggle_recording(window, cx));
                        }),
                    )
                    .into_any_element(),
            ),
            false,
            theme,
        )
        .into_any_element(),
    ];
    match &this.refusal {
        Some((refusal, pressed)) => page.push(
            notice(
                language.text(
                    match refusal {
                        Refusal::Taken => "main.shortcut.taken",
                        Refusal::Invalid => "main.shortcut.invalid",
                    },
                    &[("shortcut", pressed)],
                ),
                true,
                theme,
            )
            .into_any_element(),
        ),
        None if !registered => page.push(
            notice(
                language.text("main.home.shortcutTaken", &[("shortcut", &shown)]),
                true,
                theme,
            )
            .into_any_element(),
        ),
        None => {}
    }
    page.extend([
        section(t("main.shortcut.section.advanced"), false, theme).into_any_element(),
        setting(
            t("main.shortcut.snipping"),
            t("main.shortcut.snippingHint"),
            Switch::new("takeSnipping", snipping.wanted)
                .enabled(!snipping.restarting)
                .on_toggle(|_, cx| {
                    let on = !Perora::get(cx).settings().take_snipping;
                    system::take_snipping(on, cx);
                }),
            false,
            theme,
        )
        .into_any_element(),
        Reveal::new("takeSnipping-more", snipping.behind)
            .child(card(
                about(
                    t(if snipping.wanted {
                        "main.shortcut.snippingWaits"
                    } else {
                        "main.shortcut.snippingGivenBack"
                    }),
                    Some(t("main.shortcut.restartShellHint")),
                    theme,
                ),
                // While the shell starts anew the button turns, and nothing here is to be pressed.
                Some(
                    Button::new("restartShell", t("main.shortcut.restartShell"))
                        .turning(snipping.restarting)
                        .on_click(|_, cx| system::restart_shell(cx))
                        .into_any_element(),
                ),
                true,
                theme,
            ))
            .into_any_element(),
    ]);
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named_as_the_shortcut_names_them() {
        assert_eq!(named("h").as_deref(), Some("H"));
        assert_eq!(named("1").as_deref(), Some("1"));
        assert_eq!(named("f5").as_deref(), Some("F5"));
        assert_eq!(named("f24").as_deref(), Some("F24"));
        assert_eq!(named("f25"), None);
        assert_eq!(named("pageup").as_deref(), Some("PageUp"));
        assert_eq!(named("`").as_deref(), Some("Backquote"));
        assert_eq!(named("menu"), None);
        assert_eq!(named("escape"), None);
        assert_eq!(named("!"), None);
    }

    #[test]
    fn a_combination_is_one_typing_does_not_need() {
        let none = [false; 4];
        let control_shift = [true, false, true, false];
        assert_eq!(joined("H", control_shift).as_deref(), Some("Ctrl+Shift+H"));
        assert_eq!(joined("H", [false, false, true, false]), None);
        assert_eq!(joined("F5", none).as_deref(), Some("F5"));
        assert_eq!(joined("PrintScreen", none).as_deref(), Some("PrintScreen"));
        assert_eq!(joined("Pause", none), None);
        assert_eq!(
            joined("1", [true, true, true, true]).as_deref(),
            Some("Ctrl+Alt+Shift+Super+1")
        );
    }
}
