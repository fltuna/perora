//! The pen: a line where the pointer goes with the button down, in its colour and width. The marker
//! is drawn the same way (wider and seen through, as its mark draws it). Each has a colour and a size
//! of its own (owner: 「ペンとマーカーは色とサイズを別々に覚える」).

use perora_core::colours::MARKERS;
use perora_core::edit::{Colour, Edit, Place};

use super::{Press, Tool};

/// The pen's first colour: Snipping Tool's red.
pub const RED: Colour = Colour {
    red: 0xe6,
    green: 0x1b,
    blue: 0x1b,
};

/// Widths are in steps (1 to 20) of `Press::unit`.
pub const FIRST_STEP: u32 = 4;

pub struct Pen {
    /// The marker, not the pen.
    pub marker: bool,
    pub colour: Colour,
    pub step: u32,
    /// The line taking shape, and how wide it is drawn (as it was when it was begun).
    drawing: Option<(Vec<Place>, f32)>,
}

impl Default for Pen {
    fn default() -> Self {
        Self {
            marker: false,
            colour: RED,
            step: FIRST_STEP,
            drawing: None,
        }
    }
}

impl Pen {
    /// The marker: yellow to start with.
    pub fn marker() -> Self {
        Self {
            marker: true,
            colour: MARKERS[0],
            ..Self::default()
        }
    }
}

impl Tool for Pen {
    fn begin(&mut self, press: &Press) -> bool {
        self.drawing = Some((vec![press.at], self.step as f32 * press.unit));
        true
    }

    fn go_on(&mut self, press: &Press) {
        let Some((points, _)) = self.drawing.as_mut() else {
            return;
        };
        // (A place more only when the pointer has moved a pixel of the screen from the last.)
        let last = points.last().copied().unwrap_or(press.at);
        let moved = (press.at[0] - last[0]).hypot(press.at[1] - last[1]) * press.scale;
        if moved >= 1.0 {
            points.push(press.at);
        }
    }

    fn shaping(&self) -> Option<Edit> {
        let (points, width) = self.drawing.as_ref()?;
        let (points, color, width) = (points.clone(), self.colour, *width);
        Some(if self.marker {
            Edit::Marker {
                points,
                color,
                width,
            }
        } else {
            Edit::Pen {
                points,
                color,
                width,
            }
        })
    }

    fn end(&mut self, _press: &Press) -> Option<Edit> {
        // (As it was shown last, as in the Tauri version.)
        let shaped = self.shaping();
        self.drawing = None;
        shaped
    }

    fn give_up(&mut self) {
        self.drawing = None;
    }
}
