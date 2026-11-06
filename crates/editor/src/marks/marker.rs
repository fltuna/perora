//! A stroke of the marker: as the pen's, four times as wide and seen through (40 %), as solid
//! everywhere, also where it turns or crosses itself (`outline`). It is moved, not made larger.

use gpui::Window;
use perora_core::drawing::held;
use perora_core::edit::{Edit, Place};

use super::{Mark, on_line};
use crate::paint::{self, Painter};

/// How much wider a marker is drawn than its width says, and how much of what is under it shows
/// through it (`MARKER` in editor.tsx).
pub const WIDER: f32 = 4.0;
pub const SEEN_THROUGH: f32 = 0.4;

pub struct Marker;

impl Mark for Marker {
    fn paint(&self, edit: &Edit, painter: &Painter, window: &mut Window) {
        let Edit::Marker {
            points,
            color,
            width,
        } = edit
        else {
            return;
        };
        let mut colour = paint::colour(*color);
        colour.a = SEEN_THROUGH;
        paint::seen_through(window, painter, points, held(*width * WIDER), colour);
    }

    fn reach(&self, edit: &Edit) -> f32 {
        match edit {
            Edit::Marker { width, .. } => held(*width * WIDER),
            _ => 0.0,
        }
    }

    fn is_at(&self, edit: &Edit, at: Place, near: f32) -> bool {
        let Edit::Marker { points, .. } = edit else {
            return false;
        };
        on_line(points, at, self.reach(edit) / 2.0 + near)
    }
}
