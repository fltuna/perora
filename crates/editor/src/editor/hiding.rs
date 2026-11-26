//! The parts made unreadable, as the editor shows them: each made apart from what is under it
//! (`hidden`), at the pixels of the image, and kept while it stays as it is. While one is being
//! drawn or moved it is made from a half small enough to be made at once (so that dragging stays
//! smooth), and at its pixels once let go.
//!
//! What is under one is all that was drawn before it, as the image that leaves has it: the
//! application draws that (`app/src/burn.rs`, `under`) after the drawing changes, and gives it here
//! (`set_under`). Until then, and where a part has left what was drawn under it, it is made from the
//! picture: what was drawn under it catches up a moment later (docs/gpui-rewrite.md, ぼかし・モザイク).
//!
//! Never the picture as it is where a part is made unreadable (docs/gpui-parity.md: 「素通しを防ぐ」).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Context, RenderImage};
use perora_core::edit::Edit;
use perora_core::editor_settings::EditorSettings;
use perora_core::settings::Hider;

use super::Editor;
use super::bars::Set;
use crate::hidden::{self, Part, Under};
use crate::paint::{Base, Hiding};
use crate::tools::Kind;

/// What a part made unreadable was made of: where, which way, how strongly, from which half, from
/// what (0: the picture; else which of what is under the parts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Made {
    part: Part,
    mosaic: bool,
    strength: u32,
    level: usize,
    from: u64,
}

/// No more pixels than this are made at once while a part is being dragged.
const QUICK: u64 = 400_000;

/// The parts made unreadable, kept: made this drawing, and the drawing before.
#[derive(Default)]
pub(super) struct Kept {
    now: HashMap<Made, Arc<RenderImage>>,
    /// Those shown in the drawing being made (`hiding_shown` keeps only these).
    shown: HashMap<Made, Arc<RenderImage>>,
    /// What is under the parts, as the application last drew it: each by a number of its own.
    unders: Vec<(u64, Rc<Under>)>,
    numbered: u64,
    /// Halves of what is under the parts (by its number, and how many times halved): made once
    /// while one is dragged, not again each time it moves.
    halves: HashMap<(u64, usize), Half>,
}

/// Pixels (blue first), how wide and how high.
type Half = (Arc<Vec<u8>>, u32, u32);

impl Kept {
    /// What is under the parts of another capture is not under these.
    pub(super) fn forget_unders(&mut self) {
        self.unders.clear();
        self.halves.clear();
    }

    /// What is under a part (its number), halved `level` times (each from the half before it).
    fn half(&mut self, number: u64, under: &Under, level: usize) -> Half {
        if level == 0 {
            return (under.pixels.clone(), under.part.width, under.part.height);
        }
        if let Some(half) = self.halves.get(&(number, level)) {
            return half.clone();
        }
        let (pixels, width, height) = self.half(number, under, level - 1);
        let (pixels, width, height) = crate::picture::halved(&pixels, width, height);
        let half = (Arc::new(pixels), width, height);
        self.halves.insert((number, level), half.clone());
        half
    }
}

/// Whether it is a part made unreadable.
fn is_hidden(edit: &Edit) -> bool {
    matches!(edit, Edit::Blur { .. } | Edit::Mosaic { .. })
}

/// What a part made unreadable is made from: the picture, or what is under it (or a half of
/// either; what is under it starts at `corner`, pixels of the image).
#[derive(Clone, Copy)]
enum Source<'a> {
    Picture(&'a crate::Picture),
    Under {
        pixels: &'a [u8],
        width: u32,
        height: u32,
        corner: (u32, u32),
    },
}

impl Editor {
    /// The way to make a part unreadable that is taken first, as the settings say: the last taken
    /// (as kept), or always the one.
    pub(super) fn first_hider(&self) -> Kind {
        match self.default_hider {
            Hider::Blur => Kind::Blur,
            Hider::Mosaic => Kind::Mosaic,
            Hider::Last => {
                if EditorSettings::load().hider == "mosaic" {
                    Kind::Mosaic
                } else {
                    Kind::Blur
                }
            }
        }
    }

    /// Which way to make a part unreadable the settings start with (`defaultHider`).
    pub fn set_default_hider(&mut self, hider: Hider) {
        self.default_hider = hider;
    }

    /// The images of the parts made unreadable among `edits` (`quick`: the one being dragged).
    /// `after`: what is drawn before `edits` (the one being drawn comes after all that is drawn).
    pub(super) fn hiding(
        &mut self,
        edits: &[Edit],
        quick: Option<usize>,
        after: &[Edit],
    ) -> Hiding {
        let mut hiding = Hiding::new();
        let Some(capture) = self.capture.as_ref() else {
            return hiding;
        };
        let picture = capture.picture.clone();
        let (width, height) = (picture.width, picture.height);
        let mut used = HashMap::new();
        // How many parts made unreadable come before this one.
        let mut rank = after.iter().filter(|edit| is_hidden(edit)).count();
        for (index, edit) in edits.iter().enumerate() {
            let (mosaic, value) = match edit {
                Edit::Blur { radius, .. } => (false, *radius),
                Edit::Mosaic { cell, .. } => (true, *cell),
                _ => continue,
            };
            rank += 1;
            let rank = rank - 1;
            // Its place in all that is drawn.
            let place = after.len() + index;
            let area = crate::marks::box_of(edit);
            let Some(part) = Part::of(area, width, height) else {
                continue;
            };
            // From a half small enough to be made at once, while it is dragged.
            let mut level = 0;
            if quick == Some(index) {
                let pixels = part.width as u64 * part.height as u64;
                while level + 1 < picture.level_count() && (pixels >> (2 * level)) > QUICK {
                    level += 1;
                }
            }
            // What is under it, as drawn: while it is still where that was drawn, and still the
            // same one of the list (not another, after one before it was taken away).
            let under = self
                .hidden_kept
                .unders
                .iter()
                .find(|(_, under)| {
                    under.index == place && under.rank == rank && under.part.holds(part)
                })
                .map(|(number, under)| (*number, under.clone()));
            let made = Made {
                part,
                mosaic,
                strength: value.to_bits(),
                level,
                from: under.as_ref().map_or(0, |(number, _)| *number),
            };
            let image = match (self.hidden_kept.now.get(&made), &under) {
                (Some(image), _) => Some(image.clone()),
                (None, Some((number, under))) => {
                    let (pixels, width, height) = self.hidden_kept.half(*number, under, level);
                    let source = Source::Under {
                        pixels: &pixels,
                        width,
                        height,
                        corner: (under.part.x, under.part.y),
                    };
                    make(source, made, value)
                }
                (None, None) => make(Source::Picture(&picture), made, value),
            };
            if let Some(image) = image {
                used.insert(made, image.clone());
                hiding.insert(
                    index,
                    Base {
                        image,
                        area: crate::view::Area::new(
                            part.x as f32,
                            part.y as f32,
                            part.width as f32,
                            part.height as f32,
                        ),
                    },
                );
            }
        }
        self.hidden_kept.shown.extend(used);
        hiding
    }

    /// The parts made unreadable not shown in this drawing (by any `hiding` of it): let go of (at
    /// the next drawing).
    pub(super) fn hiding_shown(&mut self) {
        let kept = &mut self.hidden_kept;
        let shown = std::mem::take(&mut kept.shown);
        for (made, image) in std::mem::take(&mut kept.now) {
            if !shown.contains_key(&made) {
                self.retired_images.push(image);
            }
        }
        kept.now = shown;
    }

    /// How strongly the way to make a part unreadable that the settings are about is: what is
    /// selected, or what is drawn next.
    pub(super) fn strength_now(&self) -> u32 {
        match self.chosen_edit() {
            Some(edit @ (Edit::Blur { .. } | Edit::Mosaic { .. })) => {
                hidden::step_of(edit, self.unit())
            }
            _ => self.hiders.step(),
        }
    }

    /// How strongly: for the way in hand (kept for the next time), and for what the settings are
    /// about (owner: 「強さのスライダーなんだけど直近のモザイクにも適用するようにしたいかも」).
    pub(super) fn set_hiding(&mut self, step: u32, cx: &mut Context<Self>) {
        if self.frozen {
            return;
        }
        let unit = self.unit();
        let about = self.about();
        let kind = match about.and_then(|index| self.drawing.edits().get(index)) {
            Some(Edit::Mosaic { .. }) => Kind::Mosaic,
            Some(Edit::Blur { .. }) => Kind::Blur,
            _ => self.hiders.kind,
        };
        if kind == Kind::Mosaic {
            self.hiders.mosaic = step;
        } else {
            self.hiders.blur = step;
        }
        let (blur, mosaic) = (self.hiders.blur, self.hiders.mosaic);
        EditorSettings::update(|kept| {
            kept.blur = blur;
            kept.mosaic = mosaic;
        });
        if let Some(index) = about
            && let Some(mut next) = self.drawing.edits().get(index).cloned()
        {
            match &mut next {
                Edit::Blur { radius, .. } => *radius = hidden::radius_of(step, unit),
                Edit::Mosaic { cell, .. } => *cell = hidden::cell_of(step, unit),
                _ => {
                    cx.notify();
                    return;
                }
            }
            self.alter(index, next, Set::Step(step), true, cx);
        }
        cx.notify();
    }

    /// What is drawn, as shown (the tour's, while it plays): what is under the parts made
    /// unreadable is drawn from this.
    pub fn shown(&self) -> &[Edit] {
        &self.shown
    }

    /// The parts made unreadable that may be dragged now, by their place in what is shown: the one
    /// being dragged, or the one the settings are about (selected alone; or, the tool in hand, the
    /// one just drawn); and, a way to make a part unreadable in hand, the one to be drawn next
    /// (after all that is shown: what is under it is all of that). What is under them is drawn for
    /// all of the image (not only where each is), so that it stays seen as they are dragged.
    pub fn held_hidden(&self) -> Vec<usize> {
        let mut held = Vec::new();
        let dragged = match &self.gesture {
            Some(super::Gesture::Move { indices, .. }) if indices.len() == 1 => Some(indices[0]),
            Some(super::Gesture::Resize { index, .. }) => Some(*index),
            _ => self.about(),
        };
        if let Some(index) = dragged
            && self.shown.get(index).is_some_and(is_hidden)
        {
            held.push(index);
        }
        if matches!(self.tool, Kind::Blur | Kind::Mosaic) {
            held.push(self.shown.len());
        }
        held
    }

    /// What is under the parts made unreadable, as the application drew it from what is shown. (One
    /// as it was keeps its number: what was made of it is not made again.)
    pub fn set_under(&mut self, unders: Vec<Under>, cx: &mut Context<Self>) {
        let kept = &mut self.hidden_kept;
        let before = std::mem::take(&mut kept.unders);
        kept.unders = unders
            .into_iter()
            .map(|under| {
                if let Some((number, was)) = before.iter().find(|(_, was)| **was == under) {
                    return (*number, was.clone());
                }
                kept.numbered += 1;
                (kept.numbered, Rc::new(under))
            })
            .collect();
        let numbers: Vec<u64> = kept.unders.iter().map(|(number, _)| *number).collect();
        kept.halves
            .retain(|(number, _), _| numbers.contains(number));
        cx.notify();
    }

    /// The way taken by the user is kept as the last taken (not the tour's).
    pub(super) fn keep_hider(&self, kind: Kind) {
        let name = if kind == Kind::Mosaic {
            "mosaic"
        } else {
            "blur"
        };
        EditorSettings::update(|kept| kept.hider = name.into());
    }
}

/// The part made unreadable from the picture, or from what is under it (or a half of either: the
/// one `made.level` says).
fn make(source: Source, made: Made, value: f32) -> Option<Arc<RenderImage>> {
    // The pixels, how large they are, and the corner of the image they start at (pixels of it).
    let (pixels, width, height, corner) = match source {
        Source::Picture(picture) => {
            let (pixels, width, height) = picture.pixels(made.level)?;
            (pixels, width, height, (0, 0))
        }
        Source::Under {
            pixels,
            width,
            height,
            corner,
        } => (pixels, width, height, corner),
    };
    let shrink = (1u32 << made.level) as f32;
    let area = crate::view::Area::new(
        (made.part.x - corner.0) as f32 / shrink,
        (made.part.y - corner.1) as f32 / shrink,
        made.part.width as f32 / shrink,
        made.part.height as f32 / shrink,
    );
    let part = Part::of(area, width, height)?;
    let cut = hidden::cut(pixels, width, part);
    let done = if made.mosaic {
        hidden::mosaicked(&cut, part.width, part.height, value / shrink)
    } else {
        hidden::blurred(&cut, part.width, part.height, value / shrink)
    };
    let buffer = image::RgbaImage::from_raw(part.width, part.height, done)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn under(width: u32, height: u32) -> Under {
        let pixels = (0..width * height * 4)
            .map(|byte| (byte % 251) as u8)
            .collect();
        Under {
            index: 1,
            rank: 0,
            part: Part {
                x: 10,
                y: 20,
                width,
                height,
            },
            pixels: Arc::new(pixels),
        }
    }

    #[test]
    fn what_is_under_a_part_is_halved_once_for_each_level() {
        let mut kept = Kept::default();
        let under = under(9, 7);
        let (pixels, width, height) = kept.half(1, &under, 0);
        assert!(Arc::ptr_eq(&pixels, &under.pixels));
        assert_eq!((width, height), (9, 7));

        let (twice, width, height) = kept.half(1, &under, 2);
        let (once, once_width, once_height) = crate::picture::halved(&under.pixels, 9, 7);
        let (expected, expected_width, expected_height) =
            crate::picture::halved(&once, once_width, once_height);
        assert_eq!((width, height), (expected_width, expected_height));
        assert_eq!(*twice, expected);
        // Kept: not halved again.
        assert!(Arc::ptr_eq(&kept.half(1, &under, 2).0, &twice));
        assert_eq!(kept.halves.len(), 2);
    }

    #[test]
    fn a_part_made_from_what_is_under_it_is_cut_where_it_is() {
        let under = under(9, 7);
        let made = Made {
            part: Part {
                x: 12,
                y: 23,
                width: 4,
                height: 3,
            },
            mosaic: true,
            strength: 2f32.to_bits(),
            level: 0,
            from: 1,
        };
        let source = Source::Under {
            pixels: &under.pixels,
            width: 9,
            height: 7,
            corner: (10, 20),
        };
        let image = make(source, made, 2.0).unwrap();
        let within = Part {
            x: 2,
            y: 3,
            width: 4,
            height: 3,
        };
        let expected = hidden::mosaicked(&hidden::cut(&under.pixels, 9, within), 4, 3, 2.0);
        assert_eq!(image.as_bytes(0).unwrap(), &expected[..]);
    }
}
