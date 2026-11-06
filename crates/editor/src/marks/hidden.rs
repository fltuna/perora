//! A part made unreadable (blurred, or broken into blocks): a box, taken hold of anywhere in it,
//! pulled by its corners and sides as a rectangle is. What it shows is made apart (`hidden`), from
//! what is under it, and laid in by `paint::draw_all`.

use gpui::Window;
use perora_core::edit::{Edit, Place};

use super::shape::Shape;
use super::{Hold, Mark, box_of};
use crate::paint::Painter;

pub struct Hidden;

impl Mark for Hidden {
    fn paint(&self, _edit: &Edit, _painter: &Painter, _window: &mut Window) {}

    fn is_at(&self, edit: &Edit, at: Place, near: f32) -> bool {
        let area = box_of(edit);
        at[0] >= area.x - near
            && at[0] <= area.right() + near
            && at[1] >= area.y - near
            && at[1] <= area.bottom() + near
    }

    fn holds(&self, edit: &Edit, margin: f32) -> Vec<(Hold, Place)> {
        Shape.holds(edit, margin)
    }

    fn resized(&self, edit: &Edit, hold: Hold, by: Place, shift: bool, unit: f32) -> Edit {
        Shape.resized(edit, hold, by, shift, unit)
    }
}
