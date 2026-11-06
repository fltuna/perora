//! The pen: a line where the pointer goes with the button down, in its colour and width. Its colour
//! and size are its own (the marker has others).

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
    pub colour: Colour,
    pub step: u32,
    /// The line taking shape, and how wide it is drawn (as it was when it was begun).
    drawing: Option<(Vec<Place>, f32)>,
}

impl Default for Pen {
    fn default() -> Self {
        Self {
            colour: RED,
            step: FIRST_STEP,
            drawing: None,
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
        Some(Edit::Pen {
            points: points.clone(),
            color: self.colour,
            width: *width,
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
