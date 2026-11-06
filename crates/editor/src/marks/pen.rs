//! A stroke of the pen: a line through the places the pointer went through, round at its ends and
//! where it turns; a dot where it did not move. It is moved, not made larger.

use gpui::Window;
use perora_core::drawing::held;
use perora_core::edit::{Edit, Place};

use super::{Mark, on_line};
use crate::paint::{self, Painter};

pub struct Pen;

impl Mark for Pen {
    fn paint(&self, edit: &Edit, painter: &Painter, window: &mut Window) {
        let Edit::Pen {
            points,
            color,
            width,
        } = edit
        else {
            return;
        };
        paint::stroke_through(window, painter, points, held(*width), paint::colour(*color));
    }

    fn reach(&self, edit: &Edit) -> f32 {
        match edit {
            Edit::Pen { width, .. } => held(*width),
            _ => 0.0,
        }
    }

    fn is_at(&self, edit: &Edit, at: Place, near: f32) -> bool {
        let Edit::Pen { points, .. } = edit else {
            return false;
        };
        on_line(points, at, self.reach(edit) / 2.0 + near)
    }
}
