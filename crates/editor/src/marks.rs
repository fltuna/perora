//! What can be drawn on a capture, one part each (`Mark`): how it is drawn, where it is, what a press
//! on it means, by what it is taken hold of to change its size. The data of each is core's `Edit`;
//! this is what the editor does with it. A new kind of mark is a part of its own here and one line in
//! `mark_of`.
//!
//! The arithmetic is `editor.tsx`'s (`boxOf`, `reach`, `isAt`, `moved`, `holdsOf`, `resized`).

use gpui::Window;
use perora_core::edit::{Edit, Place};

use crate::paint::Painter;
use crate::view::Area;

mod line;
mod marker;
mod pen;
mod shape;
pub mod stamp;
mod text;

pub use line::head;
pub use marker::{SEEN_THROUGH, WIDER};

/// Where something selected can be taken hold of to change its size: an end of a line; a corner or a
/// side of the frame around a shape (-1 the left or top one, 1 the right or bottom one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    From,
    To,
    Side { x: i8, y: i8 },
}

/// One kind of mark.
pub trait Mark: Sync {
    /// Draws it, where the painter puts places of the image.
    fn paint(&self, edit: &Edit, painter: &Painter, window: &mut Window);

    /// How wide it is drawn: the width of its line; nothing for what is not drawn with a line.
    fn reach(&self, _edit: &Edit) -> f32 {
        0.0
    }

    /// Whether a press at `at` means it; `near` is how far off still counts (pixels of the image).
    fn is_at(&self, edit: &Edit, at: Place, near: f32) -> bool;

    /// It, moved by `by` (pixels of the image).
    fn moved(&self, edit: &Edit, by: Place) -> Edit {
        shifted(edit, |place| {
            [tenth(place[0] + by[0]), tenth(place[1] + by[1])]
        })
    }

    /// Where it can be taken hold of to change its size, the frame around it `margin` from it. None:
    /// it is moved, not made larger.
    fn holds(&self, _edit: &Edit, _margin: f32) -> Vec<(Hold, Place)> {
        Vec::new()
    }

    /// It, after one of its holds was pulled along by `by`; with Shift, a line along one of eight
    /// directions, a shape in its proportions. `unit` is a step of width.
    fn resized(&self, edit: &Edit, _hold: Hold, _by: Place, _shift: bool, _unit: f32) -> Edit {
        edit.clone()
    }

    /// It, once `hold` is let go: as it is kept (a number square again, something written written
    /// anew at its size). None: it cannot be kept so (larger than what is written gets).
    fn settled(&self, edit: &Edit, _hold: Hold) -> Option<Edit> {
        Some(edit.clone())
    }
}

/// The part for a mark of this kind: none for a kind the editor cannot draw yet (P4 adds them one by
/// one).
pub fn mark_of(edit: &Edit) -> Option<&'static dyn Mark> {
    match edit {
        Edit::Pen { .. } => Some(&pen::Pen),
        Edit::Marker { .. } => Some(&marker::Marker),
        Edit::Stamp { .. } => Some(&stamp::Stamp),
        Edit::Text { .. } => Some(&text::Text),
        Edit::Rectangle { .. } | Edit::Ellipse { .. } => Some(&shape::Shape),
        Edit::Line { .. } | Edit::Arrow { .. } => Some(&line::Line),
        _ => None,
    }
}

/// A value to a tenth: what is moved is kept at that.
pub fn tenth(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

/// With Shift: a line along one of eight directions; a box (`boxed`) as wide as it is tall.
pub fn straightened(boxed: bool, from: Place, to: Place) -> Place {
    let (across, down) = (to[0] - from[0], to[1] - from[1]);
    if boxed {
        let side = across.abs().max(down.abs());
        let sign = |value: f32| if value < 0.0 { -1.0 } else { 1.0 };
        return [from[0] + sign(across) * side, from[1] + sign(down) * side];
    }
    let eighth = std::f32::consts::FRAC_PI_4;
    let angle = (down.atan2(across) / eighth).round() * eighth;
    let length = across.hypot(down);
    [
        from[0] + angle.cos() * length,
        from[1] + angle.sin() * length,
    ]
}

/// The places an edit is drawn through: the points of a stroke, the two corners of the rest.
pub fn places_of(edit: &Edit) -> Vec<Place> {
    match edit {
        Edit::Pen { points, .. } | Edit::Marker { points, .. } => points.clone(),
        Edit::Line { from, to, .. }
        | Edit::Arrow { from, to, .. }
        | Edit::Rectangle { from, to, .. }
        | Edit::Ellipse { from, to, .. }
        | Edit::Text { from, to, .. }
        | Edit::Stamp { from, to, .. }
        | Edit::Blur { from, to, .. }
        | Edit::Mosaic { from, to, .. }
        | Edit::Crop { from, to } => vec![*from, *to],
    }
}

/// The edit with each of its places put elsewhere.
pub fn shifted(edit: &Edit, to: impl Fn(Place) -> Place) -> Edit {
    let mut edit = edit.clone();
    match &mut edit {
        Edit::Pen { points, .. } | Edit::Marker { points, .. } => {
            for point in points.iter_mut() {
                *point = to(*point);
            }
        }
        Edit::Line { from, to: end, .. }
        | Edit::Arrow { from, to: end, .. }
        | Edit::Rectangle { from, to: end, .. }
        | Edit::Ellipse { from, to: end, .. }
        | Edit::Text { from, to: end, .. }
        | Edit::Stamp { from, to: end, .. }
        | Edit::Blur { from, to: end, .. }
        | Edit::Mosaic { from, to: end, .. }
        | Edit::Crop { from, to: end } => {
            *from = to(*from);
            *end = to(*end);
        }
    }
    edit
}

/// The box around an edit's places (not as far as its line reaches).
pub fn box_of(edit: &Edit) -> Area {
    let places = places_of(edit);
    let mut left = f32::INFINITY;
    let mut top = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let mut bottom = f32::NEG_INFINITY;
    for place in &places {
        left = left.min(place[0]);
        top = top.min(place[1]);
        right = right.max(place[0]);
        bottom = bottom.max(place[1]);
    }
    if left > right {
        return Area::new(0.0, 0.0, 0.0, 0.0);
    }
    Area::new(left, top, right - left, bottom - top)
}

/// How wide an edit is drawn, by its part (nothing for a kind not drawn yet).
pub fn reach_of(edit: &Edit) -> f32 {
    mark_of(edit).map_or(0.0, |mark| mark.reach(edit))
}

/// The part of the image an edit covers, as far as its line reaches.
pub fn area_of(edit: &Edit) -> Area {
    let area = box_of(edit);
    let half = reach_of(edit) / 2.0;
    Area::new(
        area.x - half,
        area.y - half,
        area.width + 2.0 * half,
        area.height + 2.0 * half,
    )
}

/// How far around an edit the frame that shows it selected is, `scale` pixels of the screen to one
/// of the image.
pub fn frame_around(edit: &Edit, scale: f32) -> f32 {
    reach_of(edit) / 2.0 + 4.0 / scale
}

pub fn distance_to_segment(at: Place, a: Place, b: Place) -> f32 {
    let (across, down) = (b[0] - a[0], b[1] - a[1]);
    let length = across * across + down * down;
    let along = if length == 0.0 {
        0.0
    } else {
        (((at[0] - a[0]) * across + (at[1] - a[1]) * down) / length).clamp(0.0, 1.0)
    };
    (at[0] - (a[0] + along * across)).hypot(at[1] - (a[1] + along * down))
}

/// Whether a press at `at` is on a line through the places, `slack` from it at most.
pub fn on_line(places: &[Place], at: Place, slack: f32) -> bool {
    match places {
        [] => false,
        [one] => (at[0] - one[0]).hypot(at[1] - one[1]) <= slack,
        _ => places
            .windows(2)
            .any(|pair| distance_to_segment(at, pair[0], pair[1]) <= slack),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perora_core::edit::Colour;

    fn pen(points: &[Place]) -> Edit {
        Edit::Pen {
            points: points.to_vec(),
            color: Colour {
                red: 0,
                green: 0,
                blue: 0,
            },
            width: 4.0,
        }
    }

    #[test]
    fn a_stroke_is_pressed_on_along_its_line() {
        let stroke = pen(&[[0.0, 0.0], [100.0, 0.0]]);
        let mark = mark_of(&stroke).unwrap();
        // Half its width and the slack away: on it; further: not.
        assert!(mark.is_at(&stroke, [50.0, 5.0], 3.0));
        assert!(!mark.is_at(&stroke, [50.0, 5.1], 3.0));
        assert!(mark.is_at(&stroke, [-3.0, 0.0], 2.0));
        // A dot.
        let dot = pen(&[[10.0, 10.0]]);
        assert!(mark.is_at(&dot, [12.0, 12.0], 1.0));
        assert!(!mark.is_at(&dot, [14.0, 14.0], 1.0));
    }

    #[test]
    fn moved_it_stays_at_tenths() {
        let stroke = pen(&[[1.0, 2.0], [3.0, 4.0]]);
        let moved = mark_of(&stroke).unwrap().moved(&stroke, [0.333, -0.666]);
        assert_eq!(places_of(&moved), vec![[1.3, 1.3], [3.3, 3.3]]);
    }

    const RED: Colour = Colour {
        red: 255,
        green: 0,
        blue: 0,
    };

    fn rectangle(fill: f32) -> Edit {
        Edit::Rectangle {
            from: [100.0, 100.0],
            to: [300.0, 200.0],
            color: RED,
            width: 4.0,
            fill,
            fill_color: None,
        }
    }

    #[test]
    fn a_shape_with_no_inside_is_its_outline_alone() {
        let empty = rectangle(0.0);
        let mark = mark_of(&empty).unwrap();
        assert!(!mark.is_at(&empty, [200.0, 150.0], 6.0));
        assert!(mark.is_at(&empty, [200.0, 104.0], 6.0));
        let filled = rectangle(0.5);
        assert!(mark.is_at(&filled, [200.0, 150.0], 6.0));
        // A small one is taken hold of whole.
        let small = Edit::Rectangle {
            from: [0.0, 0.0],
            to: [10.0, 10.0],
            color: RED,
            width: 1.0,
            fill: 0.0,
            fill_color: None,
        };
        assert!(mark.is_at(&small, [5.0, 5.0], 6.0));
        let ellipse = Edit::Ellipse {
            from: [100.0, 100.0],
            to: [300.0, 200.0],
            color: RED,
            width: 4.0,
            fill: 0.0,
            fill_color: None,
        };
        assert!(!mark.is_at(&ellipse, [200.0, 150.0], 6.0));
        assert!(mark.is_at(&ellipse, [200.0, 101.0], 6.0));
        // (The corner of its box is not on an ellipse.)
        assert!(!mark.is_at(&ellipse, [102.0, 102.0], 6.0));
    }

    #[test]
    fn a_corner_pulled_with_shift_keeps_the_proportions() {
        let shape = rectangle(0.0);
        let mark = mark_of(&shape).unwrap();
        let pulled = mark.resized(&shape, Hold::Side { x: 1, y: 1 }, [100.0, 0.0], true, 1.0);
        assert_eq!(box_of(&pulled), Area::new(100.0, 100.0, 300.0, 150.0));
        // A side onto the one opposite: a pixel is left.
        let flat = mark.resized(&shape, Hold::Side { x: 0, y: 1 }, [0.0, -100.0], false, 1.0);
        assert_eq!(box_of(&flat).height, 1.0);
        assert_eq!(mark.holds(&shape, 4.0).len(), 8);
    }

    #[test]
    fn a_line_is_pulled_by_its_ends_and_with_shift_along_eight_directions() {
        let line = Edit::Line {
            from: [0.0, 0.0],
            to: [100.0, 0.0],
            color: RED,
            width: 2.0,
        };
        let mark = mark_of(&line).unwrap();
        let pulled = mark.resized(&line, Hold::To, [0.0, 90.0], true, 1.0);
        let Edit::Line { to, .. } = pulled else {
            panic!("a line");
        };
        assert!((to[0] - to[1]).abs() < 0.11, "{to:?}");
        assert_eq!(straightened(true, [0.0, 0.0], [-5.0, 3.0]), [-5.0, 5.0]);
    }

    #[test]
    fn an_arrow_head_is_as_long_as_the_line_allows() {
        let (base, shaft, _, _) = head([0.0, 0.0], [100.0, 0.0], 4.0).unwrap();
        assert_eq!(base, [82.0, 0.0]);
        assert!(shaft);
        let (_, shaft, _, _) = head([0.0, 0.0], [5.0, 0.0], 4.0).unwrap();
        assert!(!shaft);
        assert!(head([0.0, 0.0], [0.2, 0.0], 4.0).is_none());
    }

    #[test]
    fn the_frame_and_the_box_go_around_the_line() {
        let stroke = pen(&[[10.0, 20.0], [30.0, 25.0]]);
        assert_eq!(box_of(&stroke), Area::new(10.0, 20.0, 20.0, 5.0));
        assert_eq!(area_of(&stroke), Area::new(8.0, 18.0, 24.0, 9.0));
        assert_eq!(frame_around(&stroke, 2.0), 4.0);
        // A stroke has no holds: it is moved, not made larger.
        assert!(mark_of(&stroke).unwrap().holds(&stroke, 4.0).is_empty());
    }
}
