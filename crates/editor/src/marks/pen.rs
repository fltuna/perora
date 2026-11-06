//! A stroke of the pen: a line through the places the pointer went through, round at its ends and
//! where it turns; a dot where it did not move.

use gpui::Window;
use perora_core::drawing::held;
use perora_core::edit::Edit;

use super::{Mark, around};
use crate::paint::{self, Painter};
use crate::view::Area;

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

    fn area(&self, edit: &Edit) -> Area {
        let Edit::Pen { points, width, .. } = edit else {
            return Area::new(0.0, 0.0, 0.0, 0.0);
        };
        around(points, held(*width))
    }
}
