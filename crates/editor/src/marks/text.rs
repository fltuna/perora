//! Something written: its letters where its room is, as `writing` draws them; stretched to its room
//! while it is being made larger or smaller, and written anew at its new size once let go. At whole
//! pixels. Made larger or smaller by its corners, as a whole.

use gpui::{ContentMask, Window};
use perora_core::edit::{Edit, Place};

use super::stamp::{corners, scaled};
use super::{Hold, Mark, box_of, shifted, tenth};
use crate::paint::Painter;
use crate::writing::{self, how_of, laid_out, written};

pub struct Text;

impl Mark for Text {
    fn paint(&self, edit: &Edit, painter: &Painter, window: &mut Window) {
        let (Some(how), Edit::Text { text, .. }) = (how_of(edit), edit) else {
            return;
        };
        let area = box_of(edit);
        if area.width <= 0.0 || area.height <= 0.0 {
            return;
        }
        let own = laid_out(text, &how);
        let stretch = [
            area.width / own.width.max(1.0),
            area.height / own.height.max(1.0),
        ];
        // (Nothing of it outside its room: its picture had nothing there either.)
        let mask = ContentMask {
            bounds: painter.bounds(area),
        };
        window.with_content_mask(Some(mask), |window| {
            writing::paint_written(window, painter, [area.x, area.y], stretch, text, &how);
        });
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
        let (Hold::Side { x, y }, Edit::Text { size, .. }) = (hold, edit) else {
            return edit.clone();
        };
        let area = box_of(edit);
        if area.width <= 0.0 || area.height <= 0.0 || *size <= 0.0 {
            return edit.clone();
        }
        let times = scaled(area, x, y, by, writing::STEP * unit / size);
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
        if let Edit::Text {
            from: was,
            to,
            size,
            outline_width,
            ..
        } = &mut edit
        {
            *was = from;
            *to = [from[0] + width, from[1] + height];
            *size = tenth(*size * times);
            // (The line around letters grows with them.)
            if let Some(edge) = outline_width.as_mut()
                && *edge > 0.0
            {
                *edge = tenth(*edge * times).max(0.5);
            }
        }
        edit
    }

    /// Written anew at its size, the corner that was not pulled staying where it is.
    fn settled(&self, edit: &Edit, hold: Hold) -> Option<Edit> {
        let (Some(how), Edit::Text { text, .. }) = (how_of(edit), edit) else {
            return Some(edit.clone());
        };
        let area = box_of(edit);
        let anew = written([area.x, area.y], text, &how)?;
        let anew_area = box_of(&anew);
        let (x, y) = match hold {
            Hold::Side { x, y } => (x, y),
            _ => (1, 1),
        };
        let from = [
            if x == 1 {
                area.x
            } else {
                area.right() - anew_area.width
            },
            if y == 1 {
                area.y
            } else {
                area.bottom() - anew_area.height
            },
        ];
        Some(super::shifted(&anew, |place| {
            [
                place[0] - anew_area.x + from[0],
                place[1] - anew_area.y + from[1],
            ]
        }))
    }
}
