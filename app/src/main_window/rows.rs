//! The pieces the pages are made of, as the Tauri version's style sheet has them: a part of a page
//! (its name, small, over a thin line), a setting (what it is on the left, its control on the right,
//! in a card), a notice, the keys of a shortcut.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Div, FontWeight, Hsla, IntoElement, ParentElement, SharedString, Styled, div, px,
};
use perora_ui::Theme;

/// A part of a page: its name, small and muted, over a thin line. The first one on a page sits
/// right under the page's heading.
pub fn section(title: impl Into<SharedString>, first: bool, theme: &Theme) -> Div {
    div()
        .flex_none()
        .mt(px(if first { 0.0 } else { 16.0 }))
        .mb(px(6.0))
        .px(px(2.0))
        .pb(px(6.0))
        .border_b_1()
        .border_color(theme.border)
        .text_size(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.muted)
        .child(title.into())
}

/// A card across the page: what is on its left, and what is on its right (`.card`). `under`: one
/// that goes with the one above it, set in from the left.
pub fn card(left: impl IntoElement, right: Option<AnyElement>, under: bool, theme: &Theme) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(24.0))
        .min_h(px(68.0))
        .px(px(16.0))
        .py(px(14.0))
        .when(under, |card| card.ml(px(32.0)))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .rounded(theme.radius_small)
        .child(div().flex_1().min_w_0().child(left))
        .when_some(right, |card, right| card.child(right))
}

/// What a setting is: its name, and what it does under it, small and muted (in as many lines as
/// it is written in).
pub fn about(title: impl Into<SharedString>, hint: Option<SharedString>, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .child(div().text_size(px(14.0)).child(title.into()))
        .when_some(hint, |about, hint| {
            about.child(hint_text(hint, theme).mt(px(2.0)))
        })
}

/// A hint: small and muted.
pub fn hint_text(hint: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(px(12.0))
        .text_color(theme.muted)
        .child(hint.into())
}

/// One setting: what it is on the left, its control on the right.
pub fn setting(
    title: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    control: impl IntoElement,
    under: bool,
    theme: &Theme,
) -> Div {
    card(
        about(title, Some(hint.into()), theme),
        Some(control.into_any_element()),
        under,
        theme,
    )
}

/// Something said under a card: plain, or a warning (`.notice`, `.notice.warning`).
pub fn notice(said: impl Into<SharedString>, warning: bool, theme: &Theme) -> Div {
    let (edge, ground) = if warning {
        (
            mix(theme.warning, theme.border, 0.45),
            mix(theme.warning, theme.surface, 0.12),
        )
    } else {
        (theme.border, theme.surface)
    };
    div()
        .flex_none()
        .mt(px(8.0))
        .px(px(14.0))
        .py(px(10.0))
        .bg(ground)
        .border_1()
        .border_color(edge)
        .rounded(theme.radius_small)
        .child(said.into())
}

/// The keys of a shortcut, as a key cap (`.keys`): in the accent colour while a new one is being
/// recorded.
pub fn keys(said: impl Into<SharedString>, recording: bool, theme: &Theme) -> Div {
    let colour = if recording {
        theme.accent
    } else {
        theme.border
    };
    div()
        .flex_none()
        .flex()
        .items_center()
        .h(px(32.0))
        .px(px(12.0))
        .bg(theme.background)
        .border_1()
        .border_b_2()
        .border_color(colour)
        .rounded(theme.radius_small)
        .whitespace_nowrap()
        .when(recording, |keys| keys.text_color(theme.accent))
        .child(said.into())
}

/// `color-mix(in srgb, a share, b)`: the two colours mixed, `share` of the first.
pub fn mix(a: Hsla, b: Hsla, share: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let channel = |a: f32, b: f32| a * share + b * (1.0 - share);
    gpui::Rgba {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: channel(a.a, b.a),
    }
    .into()
}
