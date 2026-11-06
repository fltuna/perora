//! A rectangle, or the ellipse that fits in one: between two opposite corners, with an outline and
//! an inside as solid as `fill` says (0: none). Pulled by the corners and the sides of its frame;
//! with Shift by a corner, it keeps its proportions.

use gpui::{Hsla, Window};
use perora_core::drawing::held;
use perora_core::edit::{Colour, Edit, Place};

use super::{Hold, Mark, box_of, tenth};
use crate::paint::{self, Painter};

pub struct Shape;

/// What a rectangle or an ellipse is: whether it is the ellipse, its outline colour and width, how
/// solid its inside is and in which colour.
struct Drawn {
    round: bool,
    color: Colour,
    width: f32,
    fill: f32,
    fill_color: Option<Colour>,
}

fn drawn(edit: &Edit) -> Option<Drawn> {
    match edit {
        Edit::Rectangle {
            color,
            width,
            fill,
            fill_color,
            ..
        } => Some(Drawn {
            round: false,
            color: *color,
            width: *width,
            fill: *fill,
            fill_color: *fill_color,
        }),
        Edit::Ellipse {
            color,
            width,
            fill,
            fill_color,
            ..
        } => Some(Drawn {
            round: true,
            color: *color,
            width: *width,
            fill: *fill,
            fill_color: *fill_color,
        }),
        _ => None,
    }
}

impl Mark for Shape {
    fn paint(&self, edit: &Edit, painter: &Painter, window: &mut Window) {
        let Some(shape) = drawn(edit) else {
            return;
        };
        let area = box_of(edit);
        if area.width <= 0.0 || area.height <= 0.0 {
            return;
        }
        let outline = if shape.round {
            paint::ellipse_places(painter, area)
        } else {
            vec![
                [area.x, area.y],
                [area.right(), area.y],
                [area.right(), area.bottom()],
                [area.x, area.bottom()],
            ]
        };
        if shape.fill > 0.0 {
            let inside: Hsla = paint::colour(shape.fill_color.unwrap_or(shape.color));
            paint::fill_polygon(
                window,
                painter,
                &outline,
                Hsla {
                    a: shape.fill.clamp(0.0, 1.0),
                    ..inside
                },
            );
        }
        paint::outline(
            window,
            painter,
            &outline,
            held(shape.width),
            !shape.round,
            paint::colour(shape.color),
        );
    }

    fn reach(&self, edit: &Edit) -> f32 {
        drawn(edit).map_or(0.0, |shape| held(shape.width))
    }

    fn is_at(&self, edit: &Edit, at: Place, near: f32) -> bool {
        let Some(shape) = drawn(edit) else {
            return false;
        };
        let slack = self.reach(edit) / 2.0 + near;
        let area = box_of(edit);
        let within = at[0] >= area.x - slack
            && at[0] <= area.right() + slack
            && at[1] >= area.y - slack
            && at[1] <= area.bottom() + slack;
        if !within || shape.fill > 0.0 {
            return within;
        }
        // With no inside, it is its outline alone: a press inside it is for what is seen there,
        // behind it. (Not a small one, which is little more than its outline: taken hold of whole.)
        if near <= 0.0 || area.width.max(area.height) < near * 4.0 {
            return true;
        }
        if !shape.round {
            return !(at[0] > area.x + slack
                && at[0] < area.right() - slack
                && at[1] > area.y + slack
                && at[1] < area.bottom() - slack);
        }
        // (Of an ellipse: between the one that much larger and the one that much smaller.)
        let (across, down) = (area.width / 2.0, area.height / 2.0);
        let (x, y) = (at[0] - (area.x + across), at[1] - (area.y + down));
        let inside = |by: f32| {
            across + by > 0.0
                && down + by > 0.0
                && (x / (across + by)).powi(2) + (y / (down + by)).powi(2) <= 1.0
        };
        inside(slack) && !inside(-slack)
    }

    fn holds(&self, edit: &Edit, margin: f32) -> Vec<(Hold, Place)> {
        let area = box_of(edit);
        let mut holds = Vec::new();
        for x in [-1i8, 0, 1] {
            for y in [-1i8, 0, 1] {
                if x == 0 && y == 0 {
                    continue;
                }
                let place = [
                    area.x + (x as f32 + 1.0) / 2.0 * area.width + x as f32 * margin,
                    area.y + (y as f32 + 1.0) / 2.0 * area.height + y as f32 * margin,
                ];
                holds.push((Hold::Side { x, y }, place));
            }
        }
        holds
    }

    fn resized(&self, edit: &Edit, hold: Hold, by: Place, shift: bool, _unit: f32) -> Edit {
        let Hold::Side { x, y } = hold else {
            return edit.clone();
        };
        let area = box_of(edit);
        let (mut left, mut top, mut right, mut bottom) =
            (area.x, area.y, area.right(), area.bottom());
        if x == -1 {
            left += by[0];
        }
        if x == 1 {
            right += by[0];
        }
        if y == -1 {
            top += by[1];
        }
        if y == 1 {
            bottom += by[1];
        }
        if shift && x != 0 && y != 0 && area.width > 0.0 && area.height > 0.0 {
            // As many times larger both ways, from the corner opposite.
            let times = ((right - left).abs() / area.width).max((bottom - top).abs() / area.height);
            let sign = |value: f32| if value < 0.0 { -1.0 } else { 1.0 };
            let wide = area.width * times * sign(right - left);
            let tall = area.height * times * sign(bottom - top);
            if x == 1 {
                right = left + wide;
            } else {
                left = right - wide;
            }
            if y == 1 {
                bottom = top + tall;
            } else {
                top = bottom - tall;
            }
        }
        // (A side pulled onto the one opposite: it still has an inside, a pixel of it.)
        if (right - left).abs() < 1.0 {
            (left, right) = if x == -1 {
                (right - 1.0, right)
            } else {
                (left, left + 1.0)
            };
        }
        if (bottom - top).abs() < 1.0 {
            (top, bottom) = if y == -1 {
                (bottom - 1.0, bottom)
            } else {
                (top, top + 1.0)
            };
        }
        let mut edit = edit.clone();
        if let Edit::Rectangle { from, to, .. } | Edit::Ellipse { from, to, .. } = &mut edit {
            *from = [tenth(left), tenth(top)];
            *to = [tenth(right), tenth(bottom)];
        }
        edit
    }
}
