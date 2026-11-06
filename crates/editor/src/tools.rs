//! What draws on a capture, one part each (`Tool`): what a press, the pointer moving and letting go
//! make of it. What every tool shares (selecting, moving, undoing, copying) is the editor's, not a
//! tool's. A new tool is a part of its own here and one entry in `Kind`.

use perora_core::edit::{Edit, Place};

pub mod figure;
pub mod pen;
pub mod stamp;

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
    /// The number the next stamp gets.
    pub next_number: u32,
}

pub trait Tool {
    /// The button went down: whether the tool begins something there.
    fn begin(&mut self, press: &Press) -> bool;
    /// The pointer moved with the button down.
    fn go_on(&mut self, press: &Press);
    /// What is taking shape, drawn over the rest until it is done.
    fn shaping(&self) -> Option<Edit>;
    /// The button was let go: what was drawn, if anything (as it was shown last).
    fn end(&mut self, press: &Press) -> Option<Edit>;
    /// What was begun is given up (Esc).
    fn give_up(&mut self);
}

/// The tools, as the keys take them up. (The first bar has one button for the four shapes and
/// lines: the last of them taken.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Select,
    Pen,
    Marker,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Text,
    Stamp,
}

impl Kind {
    pub const ALL: [Kind; 9] = [
        Kind::Select,
        Kind::Pen,
        Kind::Marker,
        Kind::Rectangle,
        Kind::Ellipse,
        Kind::Line,
        Kind::Arrow,
        Kind::Text,
        Kind::Stamp,
    ];

    /// The key that takes it up.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Select => "v",
            Kind::Pen => "p",
            Kind::Marker => "m",
            Kind::Rectangle => "r",
            Kind::Ellipse => "e",
            Kind::Line => "l",
            Kind::Arrow => "a",
            Kind::Text => "t",
            Kind::Stamp => "n",
        }
    }

    /// The name the window gives it (a key of the language files).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Select => "editor.tool.select",
            Kind::Pen => "editor.tool.pen",
            Kind::Marker => "editor.tool.marker",
            Kind::Rectangle => "editor.tool.rectangle",
            Kind::Ellipse => "editor.tool.ellipse",
            Kind::Line => "editor.tool.line",
            Kind::Arrow => "editor.tool.arrow",
            Kind::Text => "editor.tool.text",
            Kind::Stamp => "editor.tool.stamp",
        }
    }

    /// The pen or the marker: a tool with a panel of its own under its button (no second bar).
    pub fn is_stroke(self) -> bool {
        matches!(self, Kind::Pen | Kind::Marker)
    }

    /// One of the shapes and lines (one button on the first bar, one bar of their own).
    pub fn is_figure(self) -> bool {
        matches!(
            self,
            Kind::Rectangle | Kind::Ellipse | Kind::Line | Kind::Arrow
        )
    }

    /// Whether a drag may begin beside the picture with it (and the pointer is a cross there too).
    pub fn begins_beside(self) -> bool {
        !matches!(self, Kind::Select | Kind::Text | Kind::Stamp)
    }
}
