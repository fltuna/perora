//! A colour as it is chosen: a disc of it, with a faint edge; ringed when it is the one in use. No
//! colour at all is an empty disc, struck through. (`.swatch` in the Tauri version's style.css.)

use gpui::prelude::FluentBuilder;
use gpui::{
    Div, Hsla, InteractiveElement, ParentElement, PathBuilder, Styled, canvas, div, point, px,
};

use crate::Theme;

/// A disc of `colour` (none: no colour), `side` across. `current`: ringed, as the one in use (the
/// ring reaches 2 pixels past it).
pub fn swatch(colour: Option<Hsla>, side: f32, current: bool, theme: &Theme) -> Div {
    let edge = Hsla {
        a: 0.3,
        ..theme.text
    };
    let text = theme.text;
    let disc = div()
        .size_full()
        .rounded_full()
        .when_some(colour, |disc, colour| disc.bg(colour))
        .map(|disc| match (colour, current) {
            (None, _) => disc.border_color(text),
            (Some(_), true) => disc.border_color(theme.background),
            (Some(_), false) => disc
                .border_color(edge)
                .hover(|style| style.border_color(text)),
        })
        .map(|disc| {
            if current {
                disc.border_2()
            } else {
                disc.border_1()
            }
        })
        .when(colour.is_none(), |disc| disc.child(struck(text)));
    div()
        .relative()
        .flex_none()
        .size(px(side))
        .child(disc)
        .when(current, |swatch| {
            swatch.child(
                div()
                    .absolute()
                    .top(px(-2.0))
                    .left(px(-2.0))
                    .size(px(side + 4.0))
                    .rounded_full()
                    .border_2()
                    .border_color(text),
            )
        })
}

/// Any colour: a disc of every hue, round from red at the top (`conic-gradient`), `side` across.
pub fn any(side: f32, theme: &Theme) -> Div {
    let edge = Hsla {
        a: 0.3,
        ..theme.text
    };
    div()
        .relative()
        .flex_none()
        .size(px(side))
        .child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, _| {
                    const PIECES: usize = 48;
                    let middle = bounds.center();
                    let reach = bounds.size.width.as_f32() / 2.0;
                    let at = |turn: f32| {
                        let angle = turn * std::f32::consts::TAU;
                        point(
                            middle.x + px(reach * angle.sin()),
                            middle.y - px(reach * angle.cos()),
                        )
                    };
                    for piece in 0..PIECES {
                        let (from, to) = (
                            piece as f32 / PIECES as f32,
                            (piece + 1) as f32 / PIECES as f32,
                        );
                        let mut wedge = PathBuilder::fill();
                        wedge.move_to(middle);
                        wedge.line_to(at(from));
                        wedge.line_to(at(to + 0.002));
                        wedge.close();
                        if let Ok(path) = wedge.build() {
                            window.paint_path(path, crate::slider::hue((from + to) * 180.0));
                        }
                    }
                },
            )
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .rounded_full()
                .border_1()
                .border_color(edge),
        )
}

/// The line through no colour: across the disc, from its bottom left to its top right.
fn struck(colour: Hsla) -> impl gpui::IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let half = bounds.size.width.as_f32() / 2.0;
            let middle = bounds.center();
            let reach = half * std::f32::consts::FRAC_1_SQRT_2;
            let mut line = PathBuilder::stroke(px(2.0));
            line.move_to(point(middle.x - px(reach), middle.y + px(reach)));
            line.line_to(point(middle.x + px(reach), middle.y - px(reach)));
            if let Ok(path) = line.build() {
                window.paint_path(path, colour);
            }
        },
    )
    .absolute()
    .size_full()
}
