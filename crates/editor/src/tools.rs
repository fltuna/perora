//! What draws on a capture, one part each (`Tool`): what a press, the pointer moving and letting go
//! make of it. What every tool shares (selecting, moving, undoing, copying) is the editor's, not a
//! tool's. A new tool is a part of its own here and one entry in `Kind`.

use perora_core::edit::{Edit, Place};

pub mod pen;

/// A press, a move or a letting go, as a tool is told of it.
#[derive(Debug, Clone, Copy)]
pub struct Press {
    /// Where the pointer is, in pixels of the image.
    pub at: Place,
    /// How many logical pixels of the screen one of the image is shown as now.
    pub scale: f32,
    /// A step of width, in pixels of the image (`perora_core::drawing::unit`).
    pub unit: f32,
    pub shift: bool,
}

pub trait Tool {
    /// The button went down: whether the tool begins something there.
    fn begin(&mut self, press: &Press) -> bool;
    /// The pointer moved with the button down.
    fn go_on(&mut self, press: &Press);
    /// What is taking shape, drawn over the rest until it is done.
    fn shaping(&self) -> Option<Edit>;
    /// The button was let go: what was drawn, if anything.
    fn end(&mut self, press: &Press) -> Option<Edit>;
    /// What was begun is given up (Esc).
    fn give_up(&mut self);
}

/// The tools on the first bar, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Select,
    Pen,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Select, Kind::Pen];

    /// The key that takes it up.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Select => "v",
            Kind::Pen => "p",
        }
    }

    /// The name the window gives it (a key of the language files).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Select => "editor.tool.select",
            Kind::Pen => "editor.tool.pen",
        }
    }

    /// Whether a drag may begin beside the picture with it (and the pointer is a cross there too).
    pub fn begins_beside(self) -> bool {
        !matches!(self, Kind::Select)
    }
}
