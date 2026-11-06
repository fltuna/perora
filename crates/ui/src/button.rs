//! The buttons of the main window, as the Tauri version's style sheet has them (`.page button`): a
//! plain one, and one in the accent colour for what the page is for. Pressed by the pointer, or by
//! Enter or Space while it has the keyboard; not at all while it cannot be pressed (faded).

use std::rc::Rc;
use std::time::Instant;

use gpui::prelude::FluentBuilder;
use gpui::{
    App, Div, ElementId, FocusHandle, Hsla, InteractiveElement, IntoElement, ParentElement,
    PathBuilder, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, canvas, div,
    point, px,
};

use crate::Theme;

/// What lets a control have the keyboard (and be reached with Tab): one for each control, kept as
/// long as it is drawn.
pub(crate) fn focus_of(id: &ElementId, window: &mut Window, cx: &mut App) -> FocusHandle {
    window
        .use_keyed_state((id.clone(), "focus"), cx, |_, cx| {
            cx.focus_handle().tab_stop(true)
        })
        .read(cx)
        .clone()
}

/// A glyph of Segoe Fluent Icons (the icons of Windows 11), `size` pixels high.
pub fn glyph(glyph: impl Into<SharedString>, size: f32) -> Div {
    div()
        .flex_none()
        .font_family("Segoe Fluent Icons")
        .text_size(px(size))
        .line_height(px(size))
        .child(glyph.into())
}

type OnClick = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<SharedString>,
    accent: bool,
    enabled: bool,
    /// Its work is on its way: a ring turns in it.
    turning: bool,
    on_click: Option<OnClick>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            accent: false,
            enabled: true,
            turning: false,
            on_click: None,
        }
    }

    /// A glyph of Segoe Fluent Icons before what it says.
    pub fn icon(mut self, glyph: impl Into<SharedString>) -> Self {
        self.icon = Some(glyph.into());
        self
    }

    /// In the accent colour: what the page is for.
    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// With a ring that turns before what it says, and not to be pressed meanwhile.
    pub fn turning(mut self, turning: bool) -> Self {
        self.turning = turning;
        self
    }

    pub fn on_click(mut self, on_click: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(on_click));
        self
    }
}

/// How much darker the accent is under the pointer (`filter: brightness(0.94)`).
fn dimmed(colour: Hsla) -> Hsla {
    Hsla {
        l: colour.l * 0.94,
        ..colour
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let pressable = self.enabled && !self.turning;
        let focus = focus_of(&self.id, window, cx);
        let (background, text, border) = if self.accent {
            (theme.accent, theme.accent_text, gpui::transparent_black())
        } else {
            (theme.surface_hover, theme.text, theme.border)
        };
        let on_click = self.on_click.clone();
        div()
            .id(self.id)
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(32.0))
            .px(px(12.0))
            .rounded(theme.radius_small)
            .border_1()
            .border_color(border)
            .bg(background)
            .text_color(text)
            .whitespace_nowrap()
            .when(!pressable, |button| button.opacity(0.5))
            .when(pressable, |button| {
                let hover_border = theme.muted;
                let accent = self.accent;
                button
                    .track_focus(&focus)
                    .cursor_pointer()
                    .hover(move |style| {
                        if accent {
                            style.bg(dimmed(background))
                        } else {
                            style.border_color(hover_border)
                        }
                    })
                    .focus_visible(|style| style.border_color(theme.text))
                    .when_some(on_click, |button, on_click| {
                        button.on_click(move |_, window, cx| on_click(window, cx))
                    })
            })
            .when(self.turning, |button| button.child(Ring::new(14.0)))
            .when_some(self.icon, |button, icon| button.child(glyph(icon, 16.0)))
            .child(self.label)
    }
}

/// A ring that turns: something is on its way (`.ring` in the Tauri version: a 2-pixel ring in the
/// colour of the edges, with a quarter of it in the accent colour, once round in 0.9 seconds). It
/// turns whatever the system says about motion, as the Tauri version's did: it says that the work
/// is not stuck.
#[derive(IntoElement)]
pub struct Ring {
    side: f32,
}

impl Ring {
    pub fn new(side: f32) -> Self {
        Self { side }
    }
}

/// When the rings began to turn: all of them turn together.
fn began() -> Instant {
    thread_local! {
        static BEGAN: Instant = Instant::now();
    }
    BEGAN.with(|began| *began)
}

impl RenderOnce for Ring {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx);
        let (track, accent) = (theme.border, theme.accent);
        window.request_animation_frame();
        let turned = (began().elapsed().as_secs_f32() / 0.9).fract() * std::f32::consts::TAU;
        let side = self.side;
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let width = 2.0;
                let radius = (side - width) / 2.0;
                let middle = (
                    bounds.origin.x.as_f32() + side / 2.0,
                    bounds.origin.y.as_f32() + side / 2.0,
                );
                let at = |angle: f32| {
                    point(
                        px(middle.0 + radius * angle.sin()),
                        px(middle.1 - radius * angle.cos()),
                    )
                };
                let arc = |from: f32, to: f32, colour: Hsla, window: &mut Window| {
                    // (In pieces: a path of short lines is round enough at this size.)
                    let mut path = PathBuilder::stroke(px(width));
                    let steps = 24;
                    path.move_to(at(from));
                    for step in 1..=steps {
                        path.line_to(at(from + (to - from) * step as f32 / steps as f32));
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, colour);
                    }
                };
                arc(0.0, std::f32::consts::TAU, track, window);
                arc(
                    turned - std::f32::consts::FRAC_PI_4,
                    turned + std::f32::consts::FRAC_PI_4,
                    accent,
                    window,
                );
            },
        )
        .flex_none()
        .size(px(side))
    }
}
