//! The toggle of Windows: a pill 40 by 20 with a dot that slides from one end to the other in 0.12
//! seconds (`.switch` in the Tauri version's style sheet). Off, an outline with a grey dot; on, filled
//! with the accent colour. Toggled by the pointer, or by Space or Enter while it has the keyboard.
//!
//! It says what it is told and no more: whoever it belongs to toggles what it stands for, as it is
//! then, and draws it again. (Two quick clicks are on and off again, not "on" twice.)

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::button::focus_of;
use crate::{Theme, motion};

/// How long the dot takes to slide.
const SLIDES_IN: Duration = Duration::from_millis(120);
/// Where the dot is, from the left of the pill inside its edge: off and on.
const OFF: f32 = 3.0;
const ON: f32 = 23.0;

type OnToggle = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    enabled: bool,
    on_toggle: Option<OnToggle>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            enabled: true,
            on_toggle: None,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Told when the switch is toggled.
    pub fn on_toggle(mut self, on_toggle: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(on_toggle));
        self
    }
}

/// What the switch showed when it was last drawn, and since when (while the dot slides).
struct Shown {
    on: bool,
    since: Option<Instant>,
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let focus = focus_of(&self.id, window, cx);
        let shown = window.use_keyed_state((self.id.clone(), "shown"), cx, |_, _| Shown {
            on: self.on,
            since: None,
        });
        // (As it is drawn the first time, the dot is where it is; it slides only when it changes.)
        let animates = crate::reveal::animates();
        let since = shown.update(cx, |shown, _| {
            if shown.on != self.on {
                shown.on = self.on;
                shown.since = animates.then(Instant::now);
            }
            shown.since
        });
        let along = since
            .map(|since| motion::along(since, Duration::ZERO, SLIDES_IN, window))
            .unwrap_or(1.0);
        let eased = motion::EASE.at(along);
        let (from, to) = if self.on { (OFF, ON) } else { (ON, OFF) };
        let left = from + (to - from) * eased;

        let (fill, edge, dot) = if self.on {
            (theme.accent, theme.accent, theme.accent_text)
        } else {
            (gpui::transparent_black(), theme.muted, theme.muted)
        };
        let on_toggle = self.on_toggle.clone();
        div()
            .id(self.id)
            .flex_none()
            .relative()
            .w(px(40.0))
            .h(px(20.0))
            .rounded(px(10.0))
            .border_1()
            .border_color(edge)
            .bg(fill)
            .when(!self.enabled, |switch| switch.opacity(0.4))
            .when(self.enabled, |switch| {
                switch
                    .track_focus(&focus)
                    .cursor_pointer()
                    .focus_visible(|style| style.border_color(theme.text))
                    .when_some(on_toggle, |switch, on_toggle| {
                        switch.on_click(move |_, window, cx| on_toggle(window, cx))
                    })
            })
            .child(
                div()
                    .absolute()
                    .top(px(3.0))
                    .left(px(left))
                    .size(px(12.0))
                    .rounded_full()
                    .bg(dot),
            )
    }
}
