//! A number in a disc (`stamp` in editor.tsx): the disc in its colour, and the number and a thin ring
//! inside its edge in black or white, whichever shows on it (the ring sets it off from a ground of
//! its own colour). At whole pixels. Made larger or smaller by its corners, square, as a whole.

use gpui::{Bounds, Window, fill, point, size};
use perora_core::drawing::{MOST_PIXELS, MOST_SIDE};
use perora_core::edit::{Colour, Edit, Place};

use super::{Hold, Mark, box_of, shifted};
use crate::letters::{self, OWN};
use crate::paint::{self, Painter};
use crate::view::Area;

/// A step of size is this many pixels across, times the unit.
pub const STEP: f32 = 8.0;
/// The smallest a number is, across (pixels of the image).
const LEAST: f32 = 4.0;

pub struct Stamp;

/// Whether a colour is a light one: what is written on it is black then, white otherwise.
pub fn is_light(colour: Colour) -> bool {
    colour.red as f32 * 0.299 + colour.green as f32 * 0.587 + colour.blue as f32 * 0.114 > 160.0
}

/// A number in a disc around a place, so far across, at whole pixels.
pub fn stamped(middle: Place, across: f32, number: u32, color: Colour) -> Edit {
    let side = across.round().clamp(LEAST, MOST_SIDE);
    let from = [
        (middle[0] - side / 2.0).round(),
        (middle[1] - side / 2.0).round(),
    ];
    Edit::Stamp {
        from,
        to: [from[0] + side, from[1] + side],
        number,
        color,
    }
}

impl Mark for Stamp {
    fn paint(&self, edit: &Edit, painter: &Painter, window: &mut Window) {
        let Edit::Stamp { number, color, .. } = edit else {
            return;
        };
        let area = box_of(edit);
        let across = area.width.min(area.height);
        if across <= 0.0 {
            return;
        }
        let middle = [area.x + area.width / 2.0, area.y + area.height / 2.0];
        let on = if is_light(*color) {
            gpui::black()
        } else {
            gpui::white()
        };
        paint::disc(window, painter, middle, across / 2.0, paint::colour(*color));
        // The ring: a line across * 0.05 wide around a circle across * 0.43 from the middle.
        let (radius, width) = (across * 0.43, across * 0.05);
        let outer = painter.length(radius + width / 2.0);
        let centre = painter.at(middle);
        window.paint_quad(
            fill(
                Bounds::new(
                    point(centre.x - outer, centre.y - outer),
                    size(outer * 2.0, outer * 2.0),
                ),
                gpui::transparent_black(),
            )
            .corner_radii(outer)
            .border_widths(painter.length(width))
            .border_color(on),
        );
        let digits = number.to_string();
        let share = match digits.len() {
            0 | 1 => 0.56,
            2 => 0.46,
            _ => 0.34,
        };
        let lettered = letters::laid(&[&digits], &OWN, across * share, true, false);
        let Some(line) = lettered.lines.first() else {
            return;
        };
        // In the middle; a little lower than the middle of the line (digits have nothing below their
        // foot), as a canvas puts a "middle" baseline.
        let at = [
            middle[0] - line.width / 2.0,
            middle[1] + across * 0.035 + (lettered.ascent - lettered.descent) / 2.0,
        ];
        paint::triangles(window, painter, at, &line.corners, &line.indices, on);
    }

    fn is_at(&self, edit: &Edit, at: Place, near: f32) -> bool {
        let area = box_of(edit);
        at[0] >= area.x - near
            && at[0] <= area.right() + near
            && at[1] >= area.y - near
            && at[1] <= area.bottom() + near
    }

    /// (At whole pixels: what has letters is laid in pixel for pixel.)
    fn moved(&self, edit: &Edit, by: Place) -> Edit {
        let by = [by[0].round(), by[1].round()];
        shifted(edit, |place| [place[0] + by[0], place[1] + by[1]])
    }

    fn holds(&self, edit: &Edit, margin: f32) -> Vec<(Hold, Place)> {
        corners(box_of(edit), margin)
    }

    fn resized(&self, edit: &Edit, hold: Hold, by: Place, _shift: bool, unit: f32) -> Edit {
        let Hold::Side { x, y } = hold else {
            return edit.clone();
        };
        let area = box_of(edit);
        if area.width <= 0.0 || area.height <= 0.0 {
            return edit.clone();
        }
        let least = STEP * unit / area.width;
        let times = scaled(area, x, y, by, least);
        let (width, height) = ((area.width * times).round(), (area.height * times).round());
        let from = [
            if x == 1 { area.x } else { area.right() - width },
            if y == 1 {
                area.y
            } else {
                area.bottom() - height
            },
        ];
        let mut edit = edit.clone();
        if let Edit::Stamp { from: was, to, .. } = &mut edit {
            *was = from;
            *to = [from[0] + width, from[1] + height];
        }
        edit
    }

    /// Square, at whole pixels, once it is let go.
    fn settled(&self, edit: &Edit) -> Edit {
        let area = box_of(edit);
        let side = area.width.min(area.height).round().max(LEAST);
        let from = [area.x.round(), area.y.round()];
        let mut edit = edit.clone();
        if let Edit::Stamp { from: was, to, .. } = &mut edit {
            *was = from;
            *to = [from[0] + side, from[1] + side];
        }
        edit
    }
}

/// The four corners of the frame around a box, `margin` from it: what has letters keeps its shape,
/// and is pulled only by its corners.
pub fn corners(area: Area, margin: f32) -> Vec<(Hold, Place)> {
    let mut holds = Vec::new();
    for x in [-1i8, 1] {
        for y in [-1i8, 1] {
            let at = [
                area.x + (x as f32 + 1.0) / 2.0 * area.width + x as f32 * margin,
                area.y + (y as f32 + 1.0) / 2.0 * area.height + y as f32 * margin,
            ];
            holds.push((Hold::Side { x, y }, at));
        }
    }
    holds
}

/// How many times larger what has letters is made by a corner pulled along by `by`, the corner
/// opposite staying: as far as the pointer went, the further of across and down; no smaller than
/// `least` (a step), no larger than a picture gets.
pub fn scaled(area: Area, x: i8, y: i8, by: Place, least: f32) -> f32 {
    let across = area.width + x as f32 * by[0];
    let down = area.height + y as f32 * by[1];
    // (A little under what a picture gets: the picture made of it at the end may come out a pixel
    // or two larger.)
    let most = 0.97
        * (MOST_SIDE / area.width)
            .min(MOST_SIDE / area.height)
            .min((MOST_PIXELS / (area.width * area.height)).sqrt());
    (least.min(1.0))
        .max(across / area.width)
        .max(down / area.height)
        .min(most)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Colour = Colour {
        red: 0xe6,
        green: 0x1b,
        blue: 0x1b,
    };

    #[test]
    fn a_number_is_square_at_whole_pixels_around_its_place() {
        let Edit::Stamp { from, to, .. } = stamped([100.3, 50.6], 32.4, 1, RED) else {
            panic!()
        };
        assert_eq!(from, [84.0, 35.0]);
        assert_eq!(to, [116.0, 67.0]);
        let Edit::Stamp { from, to, .. } = stamped([10.0, 10.0], 1.0, 1, RED) else {
            panic!()
        };
        assert_eq!(to[0] - from[0], LEAST);
    }

    #[test]
    fn white_is_written_on_red_and_black_on_yellow() {
        assert!(!is_light(RED));
        assert!(is_light(Colour {
            red: 0xff,
            green: 0xe6,
            blue: 0
        }));
    }

    #[test]
    fn a_corner_pulled_makes_it_larger_from_the_one_opposite_and_square_once_let_go() {
        let one = stamped([100.0, 100.0], 32.0, 1, RED);
        let pulled = Stamp.resized(&one, Hold::Side { x: 1, y: 1 }, [16.0, 4.0], false, 1.0);
        let area = box_of(&pulled);
        assert_eq!(
            (area.x, area.y, area.width, area.height),
            (84.0, 84.0, 48.0, 48.0)
        );
        let smaller = Stamp.resized(
            &one,
            Hold::Side { x: -1, y: -1 },
            [100.0, 100.0],
            false,
            1.0,
        );
        // No smaller than a step.
        assert_eq!(box_of(&smaller).width, 8.0);
        assert_eq!(box_of(&smaller).right(), 116.0);
        let moved = Stamp.moved(&one, [0.4, 0.6]);
        assert_eq!(box_of(&moved).x, 84.0);
        assert_eq!(box_of(&moved).y, 85.0);
    }
}
