//! A number of seconds, typed (`Seconds` in the Tauri version, an `<input type="number">` with what
//! it counts beside it): set as soon as what is typed is one that can be set, parts of a second too
//! (1.5). What is not one is not set, and is gone when the box is left; Up and Down go by a tenth.
//!
//! It is not written over by what it is set to while it is typed into: what is typed is the user's
//! until they leave it.

use gpui::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, Styled, Subscription, Window,
    div, px,
};

use crate::text_field::{TextField, TextFieldEvent};

/// The most seconds that can be set, and the least other than none at all (0): less is too short to
/// see, and rounded would be taken for 0.
pub const MOST: f64 = 3600.0;
const LEAST: f64 = 0.1;

/// What the box tells the one it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SecondsEvent {
    /// The user set this many seconds.
    Set(f64),
}

pub struct Seconds {
    field: Entity<TextField>,
    /// What was last set from here, or given: what the box says again when it is left.
    set: f64,
    unit: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SecondsEvent> for Seconds {}

/// What is typed, as seconds to set: digits with at most one point among them (`1.5`, `.5`, `2.`),
/// to the thousandth, and no more than can be set. `None` for anything else (a sign, an exponent,
/// letters, nothing at all).
pub fn typed(text: &str) -> Option<f64> {
    let (whole, part) = text.split_once('.').unwrap_or((text, ""));
    let digits = |text: &str| text.chars().all(|letter| letter.is_ascii_digit());
    if whole.is_empty() && part.is_empty() || !digits(whole) || !digits(part) {
        return None;
    }
    let seconds: f64 = format!("0{whole}.{part}0").parse().ok()?;
    let seconds = (seconds * 1000.0).round() / 1000.0;
    (seconds == 0.0 || (LEAST..=MOST).contains(&seconds)).then_some(seconds)
}

/// As the box writes a number of seconds: `7`, `1.5`, `0.75`.
pub fn written(seconds: f64) -> String {
    format!("{seconds}")
}

/// A tenth more or less than `seconds`, as the arrows of a number box go: to the next tenth, never
/// below 0 nor above the most there can be.
fn stepped(seconds: f64, up: bool) -> f64 {
    let tenths = seconds * 10.0;
    let next = if up {
        (tenths + 1e-9).floor() + 1.0
    } else {
        (tenths - 1e-9).ceil() - 1.0
    };
    (next / 10.0).clamp(0.0, MOST)
}

impl Seconds {
    pub fn new(
        seconds: f64,
        unit: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx).longest(10);
            field.set_text(&written(seconds), cx);
            field
        });
        let changed = cx.subscribe(&field, |this, _, event: &TextFieldEvent, cx| {
            let TextFieldEvent::Changed(text) = event;
            if let Some(seconds) = typed(text)
                && seconds != this.set
            {
                this.set = seconds;
                cx.emit(SecondsEvent::Set(seconds));
            }
        });
        let focus = field.focus_handle(cx);
        let left = cx.on_focus_out(&focus, window, |this, _, _, cx| {
            let set = written(this.set);
            this.field.update(cx, |field, cx| field.set_text(&set, cx));
        });
        Self {
            field,
            set: seconds,
            unit: unit.into(),
            _subscriptions: vec![changed, left],
        }
    }

    /// What the setting is now, and what it counts in: written in the box unless the user is
    /// typing in it.
    pub fn show(
        &mut self,
        seconds: f64,
        unit: SharedString,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.unit != unit {
            self.unit = unit;
            cx.notify();
        }
        let typing = self.field.focus_handle(cx).is_focused(window);
        if typing || seconds == self.set {
            return;
        }
        self.set = seconds;
        let written = written(seconds);
        self.field
            .update(cx, |field, cx| field.set_text(&written, cx));
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let up = match event.keystroke.key.as_str() {
            "up" => true,
            "down" => false,
            _ => return,
        };
        cx.stop_propagation();
        let seconds = stepped(self.set, up);
        if seconds == self.set {
            return;
        }
        self.set = seconds;
        let written = written(seconds);
        self.field
            .update(cx, |field, cx| field.set_text(&written, cx));
        cx.emit(SecondsEvent::Set(seconds));
    }
}

impl Focusable for Seconds {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for Seconds {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .on_key_down(cx.listener(Self::key_down))
            .child(div().w(px(76.0)).child(self.field.clone()))
            .child(self.unit.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_typed_is_set_only_when_it_is_seconds_to_set() {
        assert_eq!(typed("7"), Some(7.0));
        assert_eq!(typed("1.5"), Some(1.5));
        assert_eq!(typed(".5"), Some(0.5));
        assert_eq!(typed("2."), Some(2.0));
        assert_eq!(typed("0"), Some(0.0));
        assert_eq!(typed("0.1234"), Some(0.123));
        assert_eq!(typed("3600"), Some(3600.0));
        assert_eq!(typed("3600.5"), None);
        assert_eq!(typed("0.05"), None);
        assert_eq!(typed(""), None);
        assert_eq!(typed("."), None);
        assert_eq!(typed("-1"), None);
        assert_eq!(typed("1e3"), None);
        assert_eq!(typed("1.2.3"), None);
        assert_eq!(typed(" 1"), None);
    }

    #[test]
    fn the_arrows_go_by_a_tenth() {
        assert_eq!(stepped(0.75, true), 0.8);
        assert_eq!(stepped(0.75, false), 0.7);
        assert_eq!(stepped(7.0, true), 7.1);
        assert_eq!(stepped(0.0, false), 0.0);
        assert_eq!(stepped(0.1, false), 0.0);
        assert_eq!(stepped(3600.0, true), 3600.0);
        assert_eq!(written(7.0), "7");
        assert_eq!(written(1.5), "1.5");
    }
}
