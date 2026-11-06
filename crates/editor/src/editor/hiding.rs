//! The parts made unreadable, as the editor shows them: each made apart from the picture under it
//! (`hidden`), at the pixels of the image, and kept while it stays as it is. While one is being
//! drawn or moved it is made from a half of the picture small enough to be made at once (so that
//! dragging stays smooth), and at its pixels once let go. What leaves is made from all that was
//! drawn before each (`app/src/burn.rs`); on the screen, from the picture: a stroke drawn before
//! a part made unreadable is not seen in it here (the Tauri version blurred it as drawn).
//!
//! Never the picture as it is where a part is made unreadable (docs/gpui-parity.md: 「素通しを防ぐ」).

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Context, RenderImage};
use perora_core::edit::Edit;
use perora_core::editor_settings::EditorSettings;
use perora_core::settings::Hider;

use super::Editor;
use super::bars::Set;
use crate::hidden::{self, Part};
use crate::paint::{Base, Hiding};
use crate::tools::Kind;

/// What a part made unreadable was made of: where, which way, how strongly, from which half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Made {
    part: Part,
    mosaic: bool,
    strength: u32,
    level: usize,
}

/// No more pixels than this are made at once while a part is being dragged.
const QUICK: u64 = 400_000;

/// The parts made unreadable, kept: made this drawing, and the drawing before.
#[derive(Default)]
pub(super) struct Kept {
    now: HashMap<Made, Arc<RenderImage>>,
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
    pub(super) fn hiding(&mut self, edits: &[Edit], quick: Option<usize>) -> Hiding {
        let mut hiding = Hiding::new();
        let Some(capture) = self.capture.as_ref() else {
            return hiding;
        };
        let picture = capture.picture.clone();
        let (width, height) = (picture.width, picture.height);
        let unit = self.unit();
        let mut used = HashMap::new();
        for (index, edit) in edits.iter().enumerate() {
            let (mosaic, value) = match edit {
                Edit::Blur { radius, .. } => (false, *radius),
                Edit::Mosaic { cell, .. } => (true, *cell),
                _ => continue,
            };
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
            let made = Made {
                part,
                mosaic,
                strength: value.to_bits(),
                level,
            };
            let image = match self.hidden_kept.now.get(&made) {
                Some(image) => Some(image.clone()),
                None => make(&picture, made, value, unit),
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
        // Those no longer shown: let go of (at the next drawing).
        let gone: Vec<Made> = self
            .hidden_kept
            .now
            .keys()
            .filter(|made| !used.contains_key(made))
            .copied()
            .collect();
        for made in gone {
            if let Some(image) = self.hidden_kept.now.remove(&made) {
                self.retired_images.push(image);
            }
        }
        self.hidden_kept.now.extend(used);
        hiding
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

/// The part made unreadable from the picture (or a half of it).
fn make(picture: &crate::Picture, made: Made, value: f32, unit: f32) -> Option<Arc<RenderImage>> {
    let _ = unit;
    let (pixels, width, height) = picture.pixels(made.level)?;
    let shrink = (1u32 << made.level) as f32;
    let area = crate::view::Area::new(
        made.part.x as f32 / shrink,
        made.part.y as f32 / shrink,
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
