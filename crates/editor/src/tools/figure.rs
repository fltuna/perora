//! The shapes and lines: a rectangle, an ellipse, a line, an arrow, dragged from one corner (or end)
//! to the other; with Shift, a square, a circle, a line along one of eight directions. They share an
//! outline (a colour and a size), and a shape has an inside (none at first).

use perora_core::edit::{Colour, Edit, Place};

use super::pen::{FIRST_STEP, RED};
use super::{Kind, Press, Tool};
use crate::marks::{straightened, tenth};

pub struct Figures {
    /// Which of them is in hand (the last taken: what the button for them picks up again).
    pub figure: Kind,
    pub colour: Colour,
    pub step: u32,
    /// How solid the inside of a shape is (0: none), and its colour.
    pub fill: f32,
    pub fill_colour: Colour,
    /// The one taking shape: its first corner, where the pointer is, how wide it is drawn.
    shaping: Option<(Place, Place, f32)>,
}

impl Default for Figures {
    fn default() -> Self {
        Self {
            figure: Kind::Rectangle,
            colour: RED,
            step: FIRST_STEP,
            fill: 0.0,
            fill_colour: RED,
            shaping: None,
        }
    }
}

impl Figures {
    fn edit(&self, from: Place, to: Place, width: f32) -> Edit {
        let color = self.colour;
        match self.figure {
            Kind::Ellipse => Edit::Ellipse {
                from,
                to,
                color,
                width,
                fill: self.fill,
                fill_color: Some(self.fill_colour),
            },
            Kind::Line => Edit::Line {
                from,
                to,
                color,
                width,
            },
            Kind::Arrow => Edit::Arrow {
                from,
                to,
                color,
                width,
            },
            _ => Edit::Rectangle {
                from,
                to,
                color,
                width,
                fill: self.fill,
                fill_color: Some(self.fill_colour),
            },
        }
    }
}

impl Tool for Figures {
    fn begin(&mut self, press: &Press) -> bool {
        self.shaping = Some((press.at, press.at, self.step as f32 * press.unit));
        true
    }

    fn go_on(&mut self, press: &Press) {
        let boxed = matches!(self.figure, Kind::Rectangle | Kind::Ellipse);
        if let Some((from, to, _)) = self.shaping.as_mut() {
            let at = if press.shift {
                straightened(boxed, *from, press.at)
            } else {
                press.at
            };
            *to = [tenth(at[0]), tenth(at[1])];
        }
    }

    fn shaping(&self) -> Option<Edit> {
        let (from, to, width) = self.shaping?;
        Some(self.edit(from, to, width))
    }

    fn end(&mut self, press: &Press) -> Option<Edit> {
        self.go_on(press);
        let shaped = self.shaping.take()?;
        // (A press without a drag is nothing: there is no shape in it.)
        (shaped.0 != shaped.1).then(|| self.edit(shaped.0, shaped.1, shaped.2))
    }

    fn give_up(&mut self) {
        self.shaping = None;
    }
}
