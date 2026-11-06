//! What is written on a capture: how it is written (`Writing`: colour, size, font, its inside and the
//! line around it, bold, slanted, struck through), how it is laid out (its lines, the room each
//! takes and the room around all of it), and how it is drawn. As editor.tsx has it (`Writing`,
//! `laidOut`, `write`, `written`, `howOf`).
//!
//! It is drawn as shapes: its letters (with the line through them when they are struck through) as
//! one shape, and the line around them as another, the ring between the letters and as far out of
//! them as the line reaches. The two do not meet, so an inside seen through is as solid everywhere,
//! and the line around is not seen through the letters (the Tauri version drew it apart and took the
//! letters out of it, for the same).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{Hsla, Window};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
use perora_core::drawing::{MOST_PIXELS, MOST_SIDE, WIDEST};
use perora_core::edit::{Colour, Edit, Place};

use crate::letters::{self, OWN};
use crate::paint::{self, Painter};

/// The room a line takes, and the room around all that is written, in sizes of its letters.
pub const LINE: f32 = 1.3;
pub const AROUND: f32 = 0.2;
/// A step of size is this many pixels, times the unit; and the size letters start at, in steps.
pub const STEP: f32 = 6.0;
pub const FIRST_STEPS: f32 = 4.0;
/// The sizes letters can be set to, in pixels of the image.
pub const SIZES: (f32, f32) = (4.0, 2000.0);
/// How far a line around letters can reach out of them.
pub const EDGES: (f32, f32) = (1.0, 40.0);
/// The line through letters that are struck through: how thick it is, and how far under the middle
/// of their line, in sizes of the letters.
const STRUCK_THICK: f32 = 1.0 / 14.0;
const STRUCK_UNDER: f32 = 0.05;
/// The name of a font is no longer than this.
pub const LONGEST_FONT: usize = 64;

/// How something is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Writing {
    pub color: Colour,
    pub size: f32,
    /// None: Perora's own letters.
    pub font: String,
    /// How solid the inside of the letters is (0: none).
    pub fill: f32,
    /// The line around the letters: its colour, and how far it reaches out of them.
    pub outline: Option<Colour>,
    pub edge: f32,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
}

/// How an edit is written: within bounds, whatever it says; and never without both an inside and a
/// line (letters that had neither would be there and not be seen).
pub fn how_of(edit: &Edit) -> Option<Writing> {
    let Edit::Text {
        color,
        size,
        font,
        fill,
        outline,
        outline_width,
        bold,
        italic,
        strike,
        ..
    } = edit
    else {
        return None;
    };
    let within = |value: f32, most: f32| {
        if value.is_finite() {
            value.clamp(0.0, most)
        } else {
            0.0
        }
    };
    let edge = if outline.is_some() {
        within(outline_width.unwrap_or(0.0), WIDEST)
    } else {
        0.0
    };
    let outline = if edge > 0.0 { *outline } else { None };
    let fill = within(*fill, 1.0);
    Some(Writing {
        color: *color,
        size: *size,
        font: font.clone(),
        fill: if fill == 0.0 && outline.is_none() {
            1.0
        } else {
            fill
        },
        outline,
        edge,
        bold: *bold,
        italic: *italic,
        strike: *strike,
    })
}

/// How far the line around letters of a size reaches out of them to start with: a twelfth of their
/// size, within what its slider offers.
pub fn first_edge(size: f32) -> f32 {
    (size / 12.0).round().clamp(EDGES.0, EDGES.1)
}

/// How far the line around the letters reaches out of them, in whole pixels: so much more room is
/// around what is written.
pub fn reach_of(how: &Writing) -> f32 {
    if how.outline.is_some() {
        how.edge.ceil()
    } else {
        0.0
    }
}

/// The fonts letters are written in, as a page's list goes: the one chosen, then Perora's own.
pub fn families(font: &str) -> Vec<&str> {
    let mut families = Vec::with_capacity(OWN.len() + 1);
    if !font.is_empty() {
        families.push(font);
    }
    families.extend(OWN);
    families
}

/// The name of a font as it can be kept: no longer than it gets, nothing that ends a name.
pub fn family(font: &str) -> String {
    font.chars()
        .filter(|letter| {
            !matches!(letter, '"' | '\'' | ';' | '\\' | '{' | '}' | '<' | '>')
                && !letter.is_control()
        })
        .take(LONGEST_FONT)
        .collect::<String>()
        .trim()
        .to_string()
}

/// How what is written is laid out, in pixels of the image.
pub struct Layout {
    pub lettered: Rc<letters::Lettered>,
    /// The room a line takes, the room around all of it, and how large that makes it.
    pub line: f32,
    pub around: f32,
    pub width: f32,
    pub height: f32,
}

impl Layout {
    /// Where the middle of a line is, from the top of what is written.
    pub fn middle_of(&self, line: usize) -> f32 {
        self.around + self.line * (line as f32 + 0.5)
    }

    /// Where the baseline of a line is, from the top: under its middle as a canvas puts a "middle"
    /// baseline.
    pub fn baseline_of(&self, line: usize) -> f32 {
        self.middle_of(line) + (self.lettered.ascent - self.lettered.descent) / 2.0
    }
}

/// `text` laid out as `how` writes it.
pub fn laid_out(text: &str, how: &Writing) -> Layout {
    let lines: Vec<&str> = text.split('\n').collect();
    let lettered = letters::laid(&lines, &families(&how.font), how.size, how.bold, how.italic);
    let line = (how.size * LINE).round();
    let around = (how.size * AROUND).ceil() + reach_of(how);
    let widest = lettered
        .lines
        .iter()
        .map(|line| line.width)
        .fold(0.0, f32::max);
    Layout {
        line,
        around,
        width: widest.ceil() + around * 2.0,
        height: lines.len() as f32 * line + around * 2.0,
        lettered,
    }
}

/// Something written with its top-left corner at a place, at whole pixels (where its letters are
/// sharpest). Nothing when it would be larger than what is written gets.
pub fn written(at: Place, text: &str, how: &Writing) -> Option<Edit> {
    let from = [at[0].round(), at[1].round()];
    let layout = laid_out(text, how);
    let (width, height) = (layout.width, layout.height);
    if width > MOST_SIDE || height > MOST_SIDE || width * height > MOST_PIXELS {
        return None;
    }
    let lined = how.outline.is_some() && how.edge > 0.0;
    Some(Edit::Text {
        from,
        to: [from[0] + width, from[1] + height],
        text: text.to_string(),
        color: how.color,
        size: how.size,
        font: family(&how.font),
        fill: how.fill,
        outline: if lined { how.outline } else { None },
        outline_width: lined.then_some(how.edge),
        bold: how.bold,
        italic: how.italic,
        strike: how.strike,
    })
}

/// What is written as shapes, from the top-left corner of its room: its letters (and the line
/// through them), and the line around them; three corners to a triangle.
pub struct Shapes {
    pub letters: (Vec<[f32; 2]>, Vec<u32>),
    pub around: (Vec<[f32; 2]>, Vec<u32>),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    text: String,
    font: String,
    size: u32,
    edge: u32,
    bold: bool,
    italic: bool,
    strike: bool,
}

/// What was made lately, and when each was last used.
type Kept = (HashMap<Key, (Rc<Shapes>, u64)>, u64);

thread_local! {
    static MADE: RefCell<Kept> = RefCell::new((HashMap::new(), 0));
}

/// How many are kept.
const KEPT: usize = 64;

/// The shapes of `text` written as `how` writes it.
pub fn shapes(text: &str, how: &Writing) -> Rc<Shapes> {
    let key = Key {
        text: text.to_string(),
        font: how.font.clone(),
        size: how.size.to_bits(),
        edge: if how.outline.is_some() {
            how.edge.to_bits()
        } else {
            0
        },
        bold: how.bold,
        italic: how.italic,
        strike: how.strike,
    };
    MADE.with(|made| {
        let (shapes, used) = &mut *made.borrow_mut();
        *used += 1;
        let now = *used;
        if let Some((made, last)) = shapes.get_mut(&key) {
            *last = now;
            return made.clone();
        }
        let made = Rc::new(make(text, how));
        if shapes.len() >= KEPT {
            let mut ages: Vec<u64> = shapes.values().map(|(_, last)| *last).collect();
            ages.sort_unstable();
            let before = ages[ages.len() / 2];
            shapes.retain(|_, (_, last)| *last > before);
        }
        shapes.insert(key, (made.clone(), now));
        made
    })
}

fn make(text: &str, how: &Writing) -> Shapes {
    let layout = laid_out(text, how);
    let thick = (how.size * STRUCK_THICK).max(1.0);
    // Each line's letters where they go, and the line through them.
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    for (index, line) in layout.lettered.lines.iter().enumerate() {
        let (x, y) = (layout.around, layout.baseline_of(index));
        for outline in &line.outlines {
            contours.push(outline.iter().map(|at| [x + at[0], y + at[1]]).collect());
        }
        if how.strike && line.width > 0.0 {
            let top = layout.middle_of(index) + how.size * STRUCK_UNDER - thick / 2.0;
            let (left, right) = (x, x + line.width);
            contours.push(vec![
                [left, top],
                [right, top],
                [right, top + thick],
                [left, top + thick],
            ]);
        }
    }
    if contours.is_empty() {
        return Shapes {
            letters: (Vec::new(), Vec::new()),
            around: (Vec::new(), Vec::new()),
        };
    }
    let body = contours.simplify_shape(FillRule::NonZero);
    let letters = letters::triangles(&flat(&body));
    let around = match how.outline {
        Some(_) if how.edge > 0.0 => {
            let step = (2.0 * (1.0 - (0.05 / how.edge).min(1.0)).acos())
                .clamp(0.01 * std::f32::consts::PI, 0.25 * std::f32::consts::PI);
            let outer = body.outline(&OutlineStyle::new(how.edge).line_join(LineJoin::Round(step)));
            let ring = outer.overlay(&body, OverlayRule::Difference, FillRule::NonZero);
            letters::triangles(&flat(&ring))
        }
        _ => (Vec::new(), Vec::new()),
    };
    Shapes { letters, around }
}

/// The contours of shapes, one after another (each shape's outer one and its holes).
fn flat(shapes: &[Vec<Vec<[f32; 2]>>]) -> Vec<Vec<[f32; 2]>> {
    shapes.iter().flatten().cloned().collect()
}

/// Draws what is written with the top-left corner of its room at `at` (pixels of the image),
/// stretched by `stretch` (while it is being made larger or smaller: its room is not its own size).
pub fn paint_written(
    window: &mut Window,
    painter: &Painter,
    at: Place,
    stretch: [f32; 2],
    text: &str,
    how: &Writing,
) {
    let made = shapes(text, how);
    if let Some(outline) = how.outline {
        paint::triangles_stretched(
            window,
            painter,
            at,
            stretch,
            &made.around.0,
            &made.around.1,
            paint::colour(outline),
        );
    }
    if how.fill > 0.0 {
        let mut colour: Hsla = paint::colour(how.color);
        colour.a = how.fill;
        paint::triangles_stretched(
            window,
            painter,
            at,
            stretch,
            &made.letters.0,
            &made.letters.1,
            colour,
        );
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn how() -> Writing {
        Writing {
            color: Colour {
                red: 0xe6,
                green: 0x1b,
                blue: 0x1b,
            },
            size: 40.0,
            font: String::new(),
            fill: 1.0,
            outline: None,
            edge: 0.0,
            bold: true,
            italic: false,
            strike: false,
        }
    }

    #[test]
    fn what_is_written_is_laid_out_with_room_around_it() {
        let layout = laid_out("ab\ncd", &how());
        assert_eq!(layout.line, 52.0);
        assert_eq!(layout.around, 8.0);
        assert_eq!(layout.height, 2.0 * 52.0 + 16.0);
        assert!(layout.width > 16.0);
        // A line around the letters: that much more room around them.
        let lined = Writing {
            outline: Some(how().color),
            edge: 3.4,
            ..how()
        };
        assert_eq!(laid_out("ab", &lined).around, 12.0);
    }

    #[test]
    fn written_is_at_whole_pixels_and_says_what_is_not_as_it_goes_without_saying() {
        let Some(Edit::Text {
            from,
            to,
            outline,
            outline_width,
            ..
        }) = written([10.4, 20.6], "ab", &how())
        else {
            panic!()
        };
        assert_eq!(from, [10.0, 21.0]);
        assert_eq!(to[1] - from[1], 52.0 + 16.0);
        assert_eq!((outline, outline_width), (None, None));
        let edit = written([0.0, 0.0], "ab", &how()).unwrap();
        assert_eq!(how_of(&edit), Some(how()));
    }

    #[test]
    fn letters_never_have_neither_an_inside_nor_a_line() {
        let mut edit = written([0.0, 0.0], "ab", &how()).unwrap();
        if let Edit::Text { fill, .. } = &mut edit {
            *fill = 0.0;
        }
        assert_eq!(how_of(&edit).unwrap().fill, 1.0);
    }

    #[test]
    fn the_line_around_the_letters_is_apart_from_them() {
        let lined = Writing {
            outline: Some(how().color),
            edge: 3.0,
            strike: true,
            ..how()
        };
        let made = shapes("ab", &lined);
        assert!(!made.letters.1.is_empty() && !made.around.1.is_empty());
        let area = |(corners, indices): &(Vec<[f32; 2]>, Vec<u32>)| -> f32 {
            indices
                .chunks_exact(3)
                .map(|three| {
                    let [a, b, c] = [0, 1, 2].map(|at| corners[three[at] as usize]);
                    ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
                })
                .sum()
        };
        // The ring is no more than the letters grown by the line, less the letters.
        assert!(area(&made.around) > 0.0);
        assert!(area(&made.letters) > 0.0);
    }

    #[test]
    fn a_font_name_is_kept_short_and_without_what_would_end_it() {
        assert_eq!(family(" Yu \"Gothic\"; "), "Yu Gothic");
        assert_eq!(family(&"a".repeat(100)).len(), LONGEST_FONT);
    }
}
