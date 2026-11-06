//! What can be drawn on a capture, one part each (`Mark`): how it is drawn, and (as the editor comes
//! to need them) where it is, what a press on it means, by what it is taken hold of. The data of each
//! is core's `Edit`; this is what the editor does with it. A new kind of mark is a part of its own
//! here and one line in `mark_of`.

use gpui::Window;
use perora_core::edit::Edit;

use crate::paint::Painter;
use crate::view::Area;

mod pen;

/// One kind of mark.
pub trait Mark: Sync {
    /// Draws it, where the painter puts places of the image.
    fn paint(&self, edit: &Edit, painter: &Painter, window: &mut Window);
    /// The part of the image it covers (pixels of the image), as far as it reaches.
    fn area(&self, edit: &Edit) -> Area;
}

/// The box around places, as far as a line `reach` wide through them goes.
pub fn around(places: &[perora_core::edit::Place], reach: f32) -> Area {
    let mut left = f32::INFINITY;
    let mut top = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    for place in places {
        left = left.min(place[0]);
        top = top.min(place[1]);
        right = right.max(place[0]);
        bottom = bottom.max(place[1]);
    }
    if left > right {
        return Area::new(0.0, 0.0, 0.0, 0.0);
    }
    let half = reach / 2.0;
    Area::new(
        left - half,
        top - half,
        right - left + reach,
        bottom - top + reach,
    )
}

/// The part for a mark of this kind: none for a kind the editor cannot draw yet (P4 adds them one by
/// one).
pub fn mark_of(edit: &Edit) -> Option<&'static dyn Mark> {
    match edit {
        Edit::Pen { .. } => Some(&pen::Pen),
        _ => None,
    }
}
