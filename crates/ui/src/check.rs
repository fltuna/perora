//! A kind that is ticked, among several that can be (`.check` in the Tauri version's style sheet: a
//! box with what it is beside it, in a frame as tall as a button). Ticked and unticked by the pointer,
//! or by Space or Enter while it has the keyboard.
//!
//! Like the switch, it says what it is told: whoever it belongs to ticks or unticks what it stands
//! for, as it is then.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::Theme;
use crate::button::{focus_of, glyph};

/// Segoe Fluent Icons' tick.
const TICK: &str = "\u{E73E}";

type OnToggle = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Check {
    id: ElementId,
    label: SharedString,
    ticked: bool,
    on_toggle: Option<OnToggle>,
}

impl Check {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, ticked: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            ticked,
            on_toggle: None,
        }
    }

    pub fn on_toggle(mut self, on_toggle: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(on_toggle));
        self
    }
}

impl RenderOnce for Check {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let focus = focus_of(&self.id, window, cx);
        let on_toggle = self.on_toggle.clone();
        let hover = theme.muted;
        let box_ = div()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .size(px(14.0))
            .rounded(px(3.0))
            .border_1()
            .map(|box_| {
                if self.ticked {
                    box_.bg(theme.accent)
                        .border_color(theme.accent)
                        .text_color(theme.accent_text)
                        .child(glyph(TICK, 10.0))
                } else {
                    box_.border_color(theme.muted)
                }
            });
        div()
            .id(self.id)
            .track_focus(&focus)
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(px(32.0))
            .px(px(10.0))
            .rounded(theme.radius_small)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface_hover)
            .text_color(theme.text)
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(move |style| style.border_color(hover))
            .focus_visible(|style| style.border_color(theme.text))
            .when_some(on_toggle, |check, on_toggle| {
                check.on_click(move |_, window, cx| on_toggle(window, cx))
            })
            .child(box_)
            .child(self.label)
    }
}
