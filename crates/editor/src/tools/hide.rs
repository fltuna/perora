//! Making a part unreadable: a box dragged from one corner to the other (with Shift, a square), the
//! part in it blurred or broken into blocks, as strongly as was chosen for that way (each way has its
//! own strength; owner: 「ぼかしとモザイクで強さを別々に持ち、最後に使った強さを覚える」).

use perora_core::edit::{Edit, Place};

use super::{Kind, Press, Tool};
use crate::hidden::{cell_of, radius_of};
use crate::marks::{straightened, tenth};

pub struct Hiders {
    /// Which way is in hand (the last taken: what the button picks up again).
    pub kind: Kind,
    /// How strongly each way, in steps.
    pub blur: u32,
    pub mosaic: u32,
    /// The part being marked out: its first corner, where the pointer is, and the unit.
    shaping: Option<(Place, Place, f32)>,
}

impl Hiders {
    pub fn new(kind: Kind, blur: u32, mosaic: u32) -> Self {
        Self {
            kind,
            blur,
            mosaic,
            shaping: None,
        }
    }

    /// How strongly the way in hand is.
    pub fn step(&self) -> u32 {
        if self.kind == Kind::Mosaic {
            self.mosaic
        } else {
            self.blur
        }
    }

    fn edit(&self, from: Place, to: Place, unit: f32) -> Edit {
        if self.kind == Kind::Mosaic {
            Edit::Mosaic {
                from,
                to,
                cell: cell_of(self.mosaic, unit),
            }
        } else {
            Edit::Blur {
                from,
                to,
                radius: radius_of(self.blur, unit),
            }
        }
    }
}

impl Tool for Hiders {
    fn begin(&mut self, press: &Press) -> bool {
        self.shaping = Some((press.at, press.at, press.unit));
        true
    }

    fn go_on(&mut self, press: &Press) {
        if let Some((from, to, _)) = self.shaping.as_mut() {
            let at = if press.shift {
                straightened(true, *from, press.at)
            } else {
                press.at
            };
            *to = [tenth(at[0]), tenth(at[1])];
        }
    }

    fn shaping(&self) -> Option<Edit> {
        let (from, to, unit) = self.shaping?;
        Some(self.edit(from, to, unit))
    }

    fn end(&mut self, _press: &Press) -> Option<Edit> {
        let (from, to, unit) = self.shaping.take()?;
        // (A press without a drag is nothing: there is no part in it.)
        (from[0] != to[0] && from[1] != to[1]).then(|| self.edit(from, to, unit))
    }

    fn give_up(&mut self) {
        self.shaping = None;
    }
}
