//! The drawing on a capture as the editor keeps it: the list of what was drawn, the lists undo and
//! redo go back and forth through, and how much one drawing holds. The one place for those limits:
//! the editor lets no more be drawn, and what leaves is never more than this.

use std::time::{Duration, Instant};

use crate::edit::Edit;

/// A drawing is no longer than this: so many things drawn, so many places in all.
pub const MOST_EDITS: usize = 2_000;
pub const MOST_POINTS: usize = 200_000;
/// No more parts than this are made unreadable in one drawing.
pub const MOST_HIDDEN: usize = 64;
/// All that is written in one drawing is no longer than this many letters (as the Tauri editor
/// counted them: UTF-16 units). Before, the application took 60,000 bytes and the window 20,000
/// letters; 20,000 letters are never more than 60,000 bytes.
pub const MOST_LETTERS: usize = 20_000;
/// No line is wider than this many pixels of the image.
pub const WIDEST: f32 = 400.0;

/// Changes of the same setting this close together are one step to undo: a slider being dragged.
pub const ONE_STEP: Duration = Duration::from_millis(800);

/// Whether a list of edits is no more than a drawing holds.
pub fn fits(edits: &[Edit]) -> bool {
    let points: usize = edits
        .iter()
        .map(|edit| match edit {
            Edit::Pen { points, .. } | Edit::Marker { points, .. } => points.len(),
            _ => 2,
        })
        .sum();
    let hidden = edits
        .iter()
        .filter(|edit| matches!(edit, Edit::Blur { .. } | Edit::Mosaic { .. }))
        .count();
    let letters: usize = edits
        .iter()
        .map(|edit| match edit {
            Edit::Text { text, .. } => text.encode_utf16().count(),
            _ => 0,
        })
        .sum();
    edits.len() <= MOST_EDITS
        && points <= MOST_POINTS
        && hidden <= MOST_HIDDEN
        && letters <= MOST_LETTERS
}

/// A step of width in pixels of the image: one on an image a monitor wide, more on a larger one, so
/// that a line looks about as wide on screen whatever was captured.
pub fn unit(width: u32, height: u32) -> f32 {
    (width.max(height) as f32 / 1000.0).round().max(1.0)
}

/// A line's width as it is drawn: at least a pixel, at most `WIDEST`.
pub fn held(width: f32) -> f32 {
    if width.is_nan() {
        return 1.0;
    }
    width.clamp(1.0, WIDEST)
}

/// The number the next stamp gets: one more than the highest there is (numbers taken away are not
/// given out again).
pub fn next_number(edits: &[Edit]) -> u32 {
    edits
        .iter()
        .filter_map(|edit| match edit {
            Edit::Stamp { number, .. } => Some(*number),
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// A change the drawing does not take: it would hold more than a drawing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

/// The list of what was drawn on one capture, and the lists it was before (for undo) and after (for
/// redo, until something else is changed).
#[derive(Debug, Default, Clone)]
pub struct Drawing {
    edits: Vec<Edit>,
    past: Vec<Vec<Edit>>,
    future: Vec<Vec<Edit>>,
    /// The setting last changed as one being dragged, and when: a change of it again soon after is
    /// part of the same step.
    dragged: Option<(String, Instant)>,
}

impl Drawing {
    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// The list, changed: one step to undo. Whether it changed (the same list again is no step).
    pub fn change(&mut self, edits: Vec<Edit>) -> Result<bool, Full> {
        self.dragged = None;
        self.take(edits, true)
    }

    /// The list, changed by a setting being dragged (`what` names it: which edit, which setting):
    /// one step with the change of the same thing before it, when that was less than `ONE_STEP` ago.
    pub fn change_dragged(
        &mut self,
        edits: Vec<Edit>,
        what: &str,
        now: Instant,
    ) -> Result<bool, Full> {
        let goes_on = self.dragged.as_ref().is_some_and(|(last, at)| {
            last == what && now.saturating_duration_since(*at) < ONE_STEP
        });
        let changed = self.take(edits, !goes_on)?;
        if changed {
            self.dragged = Some((what.to_owned(), now));
        }
        Ok(changed)
    }

    fn take(&mut self, edits: Vec<Edit>, new_step: bool) -> Result<bool, Full> {
        if edits == self.edits {
            return Ok(false);
        }
        if !fits(&edits) {
            return Err(Full);
        }
        let before = std::mem::replace(&mut self.edits, edits);
        if new_step || self.past.is_empty() {
            self.past.push(before);
        }
        self.future.clear();
        Ok(true)
    }

    /// Back to the list before the last change. Whether there was one.
    pub fn undo(&mut self) -> bool {
        self.dragged = None;
        let Some(before) = self.past.pop() else {
            return false;
        };
        self.future.push(std::mem::replace(&mut self.edits, before));
        true
    }

    /// Forward to the list the last undo went back from. Whether there was one.
    pub fn redo(&mut self) -> bool {
        self.dragged = None;
        let Some(after) = self.future.pop() else {
            return false;
        };
        self.past.push(std::mem::replace(&mut self.edits, after));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Colour;

    const RED: Colour = Colour {
        red: 255,
        green: 0,
        blue: 0,
    };

    fn pen(points: usize) -> Edit {
        Edit::Pen {
            points: (0..points).map(|at| [at as f32, 0.0]).collect(),
            color: RED,
            width: 4.0,
        }
    }

    fn text(letters: &str) -> Edit {
        Edit::Text {
            from: [0.0, 0.0],
            to: [10.0, 10.0],
            text: letters.to_owned(),
            color: RED,
            size: 24.0,
            font: String::new(),
            fill: 1.0,
            outline: None,
            outline_width: None,
            bold: true,
            italic: false,
            strike: false,
        }
    }

    #[test]
    fn a_change_is_a_step_to_undo_and_redo() {
        let mut drawing = Drawing::default();
        assert!(!drawing.can_undo());
        assert_eq!(drawing.change(vec![pen(2)]), Ok(true));
        assert_eq!(drawing.change(vec![pen(2), pen(3)]), Ok(true));
        assert_eq!(drawing.edits().len(), 2);

        assert!(drawing.undo());
        assert_eq!(drawing.edits(), &[pen(2)]);
        assert!(drawing.can_redo());
        assert!(drawing.redo());
        assert_eq!(drawing.edits().len(), 2);
        assert!(!drawing.redo());

        assert!(drawing.undo());
        assert!(drawing.undo());
        assert!(drawing.is_empty());
        assert!(!drawing.undo());

        // Something else changed: what undo went back from is not to be redone any more.
        assert!(drawing.redo());
        assert_eq!(drawing.change(vec![pen(5)]), Ok(true));
        assert!(!drawing.can_redo());
    }

    #[test]
    fn the_same_list_again_is_no_step() {
        let mut drawing = Drawing::default();
        drawing.change(vec![pen(2)]).unwrap();
        assert_eq!(drawing.change(vec![pen(2)]), Ok(false));
        assert!(drawing.undo());
        assert!(!drawing.can_undo());
    }

    #[test]
    fn a_setting_dragged_is_one_step_while_it_goes_on() {
        let mut drawing = Drawing::default();
        drawing.change(vec![pen(2)]).unwrap();
        let start = Instant::now();
        let widths = [5.0, 6.0, 7.0];
        for (at, width) in widths.iter().enumerate() {
            let edits = vec![Edit::Pen {
                points: vec![[0.0, 0.0], [1.0, 0.0]],
                color: RED,
                width: *width,
            }];
            let now = start + Duration::from_millis(300 * at as u64);
            drawing.change_dragged(edits, "0:width", now).unwrap();
        }
        // One step back is before the dragging.
        assert!(drawing.undo());
        assert_eq!(drawing.edits(), &[pen(2)]);

        // Dragged again after a while, or another setting: a step of its own.
        assert!(drawing.redo());
        let later = start + Duration::from_millis(600 + 900);
        drawing
            .change_dragged(vec![pen(4)], "0:width", later)
            .unwrap();
        drawing
            .change_dragged(vec![pen(6)], "0:colour", later)
            .unwrap();
        assert!(drawing.undo());
        assert_eq!(drawing.edits(), &[pen(4)]);
        assert!(drawing.undo());
        assert!(matches!(&drawing.edits()[0], Edit::Pen { width, .. } if *width == 7.0));
    }

    #[test]
    fn no_more_than_a_drawing_holds() {
        let mut drawing = Drawing::default();
        let many = vec![pen(2); MOST_EDITS];
        assert_eq!(drawing.change(many.clone()), Ok(true));
        let mut more = many;
        more.push(pen(2));
        assert_eq!(drawing.change(more), Err(Full));
        assert_eq!(drawing.edits().len(), MOST_EDITS);

        assert!(fits(&[pen(MOST_POINTS)]));
        assert!(!fits(&[pen(MOST_POINTS + 1)]));

        let hiding = |count: usize| {
            (0..count)
                .map(|_| Edit::Blur {
                    from: [0.0, 0.0],
                    to: [1.0, 1.0],
                    radius: 3.0,
                })
                .collect::<Vec<_>>()
        };
        assert!(fits(&hiding(MOST_HIDDEN)));
        assert!(!fits(&hiding(MOST_HIDDEN + 1)));

        // Letters, not bytes: Japanese is three bytes a letter, and as many letters fit.
        assert!(fits(&[text(&"あ".repeat(MOST_LETTERS))]));
        assert!(!fits(&[text(&"あ".repeat(MOST_LETTERS + 1))]));
    }

    #[test]
    fn a_step_of_width_grows_with_the_image() {
        assert_eq!(unit(1920, 1080), 2.0);
        assert_eq!(unit(800, 600), 1.0);
        assert_eq!(unit(1, 1), 1.0);
        assert_eq!(unit(5120, 1440), 5.0);
        assert_eq!(held(0.2), 1.0);
        assert_eq!(held(1000.0), WIDEST);
        assert_eq!(held(f32::NAN), 1.0);
    }

    #[test]
    fn stamps_are_numbered_on_from_the_highest() {
        let stamp = |number| Edit::Stamp {
            from: [0.0, 0.0],
            to: [1.0, 1.0],
            number,
            color: RED,
        };
        assert_eq!(next_number(&[]), 1);
        assert_eq!(next_number(&[stamp(1), stamp(4), pen(2)]), 5);
    }
}
