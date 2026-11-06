//! A line, or an arrow (a line with a head at `to`): pulled by its two ends; with Shift, along one
//! of eight directions.

use gpui::Window;
use perora_core::drawing::held;
use perora_core::edit::{Colour, Edit, Place};

use super::{Hold, Mark, on_line, straightened, tenth};
use crate::paint::{self, Painter};

pub struct Line;

/// How large the head of an arrow is: so many times the width of the line long (at least so many
/// pixels, at most the whole line), and as wide on each side as `HEAD_HALF_WIDTH` of that.
const HEAD_LENGTH: f32 = 4.5;
const HEAD_HALF_WIDTH: f32 = 2.2;
const SMALLEST_HEAD: f32 = 10.0;

fn drawn(edit: &Edit) -> Option<(bool, Place, Place, Colour, f32)> {
    match edit {
        Edit::Line {
            from,
            to,
            color,
            width,
        } => Some((false, *from, *to, *color, *width)),
        Edit::Arrow {
            from,
            to,
            color,
            width,
        } => Some((true, *from, *to, *color, *width)),
        _ => None,
    }
}

/// The head of an arrow: where its base is on the line, whether a shaft is left before it, and its
/// two back corners. None for a line too short to point anywhere.
pub fn head(from: Place, to: Place, width: f32) -> Option<(Place, bool, Place, Place)> {
    let (across, down) = (to[0] - from[0], to[1] - from[1]);
    let length = across.hypot(down);
    if length < 0.5 {
        return None;
    }
    let along = [across / length, down / length];
    let head = (width * HEAD_LENGTH).max(SMALLEST_HEAD).min(length);
    let half = head * HEAD_HALF_WIDTH / HEAD_LENGTH;
    let base = [to[0] - along[0] * head, to[1] - along[1] * head];
    Some((
        base,
        length > head,
        [base[0] - along[1] * half, base[1] + along[0] * half],
        [base[0] + along[1] * half, base[1] - along[0] * half],
    ))
}

impl Mark for Line {
    fn paint(&self, edit: &Edit, painter: &Painter, window: &mut Window) {
        let Some((arrow, from, to, color, width)) = drawn(edit) else {
            return;
        };
        let (width, colour) = (held(width), paint::colour(color));
        if !arrow {
            paint::stroke_through(window, painter, &[from, to], width, colour);
            return;
        }
        let Some((base, shaft, left, right)) = head(from, to, width) else {
            paint::stroke_through(window, painter, &[from], width, colour);
            return;
        };
        if shaft {
            paint::stroke_through(window, painter, &[from, base], width, colour);
        }
        paint::fill_polygon(window, painter, &[to, left, right], colour);
    }

    fn reach(&self, edit: &Edit) -> f32 {
        drawn(edit).map_or(0.0, |(_, _, _, _, width)| held(width))
    }

    fn is_at(&self, edit: &Edit, at: Place, near: f32) -> bool {
        let Some((_, from, to, _, _)) = drawn(edit) else {
            return false;
        };
        on_line(&[from, to], at, self.reach(edit) / 2.0 + near)
    }

    fn holds(&self, edit: &Edit, _margin: f32) -> Vec<(Hold, Place)> {
        drawn(edit).map_or_else(Vec::new, |(_, from, to, _, _)| {
            vec![(Hold::From, from), (Hold::To, to)]
        })
    }

    fn resized(&self, edit: &Edit, hold: Hold, by: Place, shift: bool, _unit: f32) -> Edit {
        let Some((_, from, to, _, _)) = drawn(edit) else {
            return edit.clone();
        };
        let (end, other) = match hold {
            Hold::From => (from, to),
            Hold::To => (to, from),
            Hold::Side { .. } => return edit.clone(),
        };
        // (As far as the pointer went, from where the end was: not to the pointer, which took hold
        // of it a little beside it.)
        let pulled = [end[0] + by[0], end[1] + by[1]];
        let end = if shift {
            straightened(false, other, pulled)
        } else {
            pulled
        };
        let end = [tenth(end[0]), tenth(end[1])];
        let mut edit = edit.clone();
        if let Edit::Line { from, to, .. } | Edit::Arrow { from, to, .. } = &mut edit {
            match hold {
                Hold::From => *from = end,
                _ => *to = end,
            }
        }
        edit
    }
}
