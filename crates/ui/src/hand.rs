//! The pointer and a mouse beside it, drawn by the tours where they show what is done with the mouse:
//! its buttons held or pressed twice, its wheel turning one way or the other (with arrows in its round
//! under the buttons going that way).

use std::time::{Duration, Instant};

use gpui::{
    BorderStyle, Bounds, Corners, Hsla, IntoElement, PathBuilder, Pixels, Point, Styled, Window,
    canvas, point, px, quad, size,
};

use crate::tour::BLUE;

/// What is done with a button of the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// The left button held down.
    Left,
    /// The other (right) button held down.
    Right,
    /// The other button pressed twice.
    Twice,
}

/// Which way the wheel turns: away from the hand (closer, the tool before), or towards it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wheel {
    Up,
    Down,
}

/// A press twice: shown blue, clear, blue, clear (0.9 s, in fifths).
const TWICE: Duration = Duration::from_millis(900);
/// The ridges of the wheel go round once in this long.
const TURNS: Duration = Duration::from_millis(250);

/// The pointer with its tip at `at`, and the mouse beside it, below to the right. `since`: when what
/// it shows began (a press twice is shown once from then; the wheel turns from then).
pub fn hand(
    at: Point<Pixels>,
    held: Option<Held>,
    wheel: Option<Wheel>,
    since: Instant,
    motion: bool,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            pointer(at, window);
            mouse(
                at + point(px(22.0), px(16.0)),
                held,
                wheel,
                since,
                motion,
                window,
            );
        },
    )
    .absolute()
    .inset_0()
}

fn white() -> Hsla {
    gpui::rgb(0xffffff).into()
}

fn black(alpha: f32) -> Hsla {
    gpui::hsla(0.0, 0.0, 0.0, alpha)
}

/// The system's arrow, its tip at `at` (20 by 20), with a little shadow under it.
pub fn pointer(at: Point<Pixels>, window: &mut Window) {
    let shape = [
        (2.0, 1.5),
        (2.0, 16.7),
        (6.1, 12.8),
        (8.7, 18.7),
        (11.3, 17.6),
        (8.7, 11.8),
        (14.5, 11.8),
    ];
    let points = |by: f32| -> Vec<Point<Pixels>> {
        shape
            .iter()
            .map(|&(x, y)| at + point(px(x - 2.0), px(y - 1.5 + by)))
            .collect()
    };
    for (by, colour, stroke) in [(1.0, black(0.35), None), (0.0, white(), Some(black(1.0)))] {
        let mut fill = PathBuilder::fill();
        fill.add_polygon(&points(by), true);
        if let Ok(path) = fill.build() {
            window.paint_path(path, colour);
        }
        if let Some(stroke) = stroke {
            let mut line = PathBuilder::stroke(px(1.2));
            line.add_polygon(&points(by), true);
            if let Ok(path) = line.build() {
                window.paint_path(path, stroke);
            }
        }
    }
}

/// The mouse (44 by 66), its corner at `at`.
fn mouse(
    at: Point<Pixels>,
    held: Option<Held>,
    wheel: Option<Wheel>,
    since: Instant,
    motion: bool,
    window: &mut Window,
) {
    let place = |x: f32, y: f32, width: f32, height: f32| {
        Bounds::new(at + point(px(x), px(y)), size(px(width), px(height)))
    };
    let blue: Hsla = gpui::rgb(BLUE).into();
    let dark: Hsla = gpui::rgb(0x222222).into();
    // (A shadow, then the body.)
    window.paint_quad(quad(
        place(1.0, 3.0, 42.0, 64.0),
        Corners::all(px(21.0)),
        black(0.25),
        px(0.0),
        gpui::transparent_black(),
        BorderStyle::Solid,
    ));
    window.paint_quad(quad(
        place(1.0, 1.0, 42.0, 64.0),
        Corners::all(px(21.0)),
        gpui::rgb(0xf4f4f4),
        px(1.6),
        dark,
        BorderStyle::Solid,
    ));
    // The button held: blue (pressed twice: blue, clear, blue, clear, then clear).
    let lit = match held {
        Some(Held::Twice) => {
            let fifth = (since.elapsed().as_secs_f32() / (TWICE.as_secs_f32() / 5.0)) as u32;
            if motion && fifth < 5 {
                window.request_animation_frame();
            }
            !motion || matches!(fifth, 0 | 2)
        }
        Some(_) => true,
        None => false,
    };
    if lit {
        let left = held == Some(Held::Left);
        let corners = if left {
            Corners {
                top_left: px(20.2),
                ..Corners::all(px(0.0))
            }
        } else {
            Corners {
                top_right: px(20.2),
                ..Corners::all(px(0.0))
            }
        };
        window.paint_quad(quad(
            place(if left { 1.8 } else { 22.0 }, 1.8, 20.2, 32.2),
            corners,
            blue,
            px(0.0),
            gpui::transparent_black(),
            BorderStyle::Solid,
        ));
    }
    // Between the buttons, and under them.
    window.paint_quad(gpui::fill(place(1.0, 33.3, 42.0, 1.4), dark));
    window.paint_quad(gpui::fill(place(21.3, 1.0, 1.4, 33.0), dark));
    // The wheel, its ridges going round as it turns.
    let wheel_at = place(17.0, 8.0, 10.0, 20.0);
    window.paint_quad(quad(
        wheel_at,
        Corners::all(px(5.0)),
        gpui::rgb(0x2f3338),
        px(0.0),
        gpui::transparent_black(),
        BorderStyle::Solid,
    ));
    let turned = match wheel {
        Some(way) if motion => {
            window.request_animation_frame();
            let share = (since.elapsed().as_secs_f32() / TURNS.as_secs_f32()).fract();
            (if way == Wheel::Up { -4.0 } else { 4.0 }) * share
        }
        _ => 0.0,
    };
    window.with_content_mask(Some(gpui::ContentMask { bounds: wheel_at }), |window| {
        for ridge in [-8.0, -4.0, 0.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0] {
            window.paint_quad(gpui::fill(
                place(17.0, 8.0 + ridge + turned, 10.0, 2.0),
                gpui::rgba(0xffffff8c),
            ));
        }
    });
    // Which way it turns, in the round under the buttons: arrows going that way, one after another.
    if let Some(way) = wheel {
        let order: [f32; 3] = if way == Wheel::Up {
            [55.0, 48.0, 41.0]
        } else {
            [41.0, 48.0, 55.0]
        };
        let lit = if motion {
            (since.elapsed().as_secs_f32() / 0.25) as usize % 3
        } else {
            3
        };
        for (index, middle) in order.into_iter().enumerate() {
            let mut line = PathBuilder::stroke(px(2.2));
            let (tip, ends) = if way == Wheel::Up {
                (middle - 3.0, middle + 3.0)
            } else {
                (middle + 3.0, middle - 3.0)
            };
            line.move_to(at + point(px(16.0), px(ends)));
            line.line_to(at + point(px(22.0), px(tip)));
            line.line_to(at + point(px(28.0), px(ends)));
            let alpha = if index == lit || lit == 3 { 1.0 } else { 0.25 };
            if let Ok(path) = line.build() {
                window.paint_path(path, blue.opacity(alpha));
            }
        }
    }
}
