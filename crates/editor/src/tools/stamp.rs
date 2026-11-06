//! The numbers: pressed on the picture, the next number (one more than the highest there is; numbers
//! taken away are not given out again) in a disc, which goes along with the pointer until it is let
//! go. In the colour and size chosen (red, 4 steps of 8 pixels to start with).

use perora_core::edit::{Colour, Edit};

use super::pen::{FIRST_STEP, RED};
use super::{Press, Tool};
use crate::marks::stamp::{STEP, stamped};

pub struct Stamps {
    pub colour: Colour,
    pub step: u32,
    /// The one being put down.
    shaping: Option<Edit>,
}

impl Default for Stamps {
    fn default() -> Self {
        Self {
            colour: RED,
            step: FIRST_STEP,
            shaping: None,
        }
    }
}

impl Tool for Stamps {
    fn begin(&mut self, press: &Press) -> bool {
        let across = self.step as f32 * STEP * press.unit;
        self.shaping = Some(stamped(press.at, across, press.next_number, self.colour));
        true
    }

    fn go_on(&mut self, press: &Press) {
        if let Some(Edit::Stamp {
            from,
            to,
            number,
            color,
        }) = self.shaping.as_ref()
        {
            self.shaping = Some(stamped(press.at, to[0] - from[0], *number, *color));
        }
    }

    fn shaping(&self) -> Option<Edit> {
        self.shaping.clone()
    }

    fn end(&mut self, _press: &Press) -> Option<Edit> {
        self.shaping.take()
    }

    fn give_up(&mut self) {
        self.shaping = None;
    }
}
