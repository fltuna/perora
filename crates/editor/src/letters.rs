//! What is written, as the shapes of its letters (core's `platform::lay_out`, from DirectWrite), cut
//! into triangles to be drawn as GPUI paths: at any size it is looked at, the same on the screen and
//! in the image that leaves. Kept for what was written lately.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};

/// Perora's own letters, as a page's list of fonts goes (`FONT` in editor.tsx).
pub const OWN: [&str; 4] = [
    "Segoe UI Variable Text",
    "Segoe UI",
    "Yu Gothic UI",
    "Meiryo",
];

/// How far a curve may be from the letter, in pixels of the image.
const FLAT: f32 = 0.05;
/// How many are kept.
const KEPT: usize = 128;

/// A line of letters: how far it goes, and its letters' triangles (from its start, its baseline at
/// 0; three corners to a triangle).
#[derive(Debug, Default)]
pub struct Line {
    pub width: f32,
    pub corners: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// Lines of letters in one font, and how far the font reaches over and under the baseline.
#[derive(Debug, Default)]
pub struct Lettered {
    pub lines: Vec<Line>,
    pub ascent: f32,
    pub descent: f32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    lines: Vec<String>,
    families: Vec<String>,
    size: u32,
    bold: bool,
    italic: bool,
}

/// What was laid out lately, and when each was last used.
type Kept = (HashMap<Key, (Rc<Lettered>, u64)>, u64);

thread_local! {
    static MADE: RefCell<Kept> = RefCell::new((HashMap::new(), 0));
}

/// `lines` written in the first of `families` there is, `size` pixels high.
pub fn laid(
    lines: &[&str],
    families: &[&str],
    size: f32,
    bold: bool,
    italic: bool,
) -> Rc<Lettered> {
    let key = Key {
        lines: lines.iter().map(|line| line.to_string()).collect(),
        families: families.iter().map(|family| family.to_string()).collect(),
        size: size.to_bits(),
        bold,
        italic,
    };
    MADE.with(|made| {
        let (shapes, used) = &mut *made.borrow_mut();
        *used += 1;
        let now = *used;
        if let Some((lettered, last)) = shapes.get_mut(&key) {
            *last = now;
            return lettered.clone();
        }
        let lettered = Rc::new(make(lines, families, size, bold, italic));
        if shapes.len() >= KEPT {
            let mut ages: Vec<u64> = shapes.values().map(|(_, last)| *last).collect();
            ages.sort_unstable();
            let before = ages[ages.len() / 2];
            shapes.retain(|_, (_, last)| *last > before);
        }
        shapes.insert(key, (lettered.clone(), now));
        lettered
    })
}

#[cfg(windows)]
fn make(lines: &[&str], families: &[&str], size: f32, bold: bool, italic: bool) -> Lettered {
    let Some(laid) = perora_core::platform::lay_out(lines, families, size, bold, italic, FLAT)
    else {
        return Lettered::default();
    };
    Lettered {
        lines: laid
            .lines
            .into_iter()
            .map(|line| {
                let (corners, indices) = triangles(&line.outlines);
                Line {
                    width: line.width,
                    corners,
                    indices,
                }
            })
            .collect(),
        ascent: laid.ascent,
        descent: laid.descent,
    }
}

#[cfg(not(windows))]
fn make(_: &[&str], _: &[&str], _: f32, _: bool, _: bool) -> Lettered {
    Lettered::default()
}

/// The inside of outlines (as letters have it: a hole is what winds the other way) as triangles.
pub fn triangles(outlines: &[Vec<[f32; 2]>]) -> (Vec<[f32; 2]>, Vec<u32>) {
    let mut path = Path::builder();
    for outline in outlines {
        let [start, rest @ ..] = outline.as_slice() else {
            continue;
        };
        path.begin(point(start[0], start[1]));
        for at in rest {
            path.line_to(point(at[0], at[1]));
        }
        path.end(true);
    }
    let path = path.build();
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let made = FillTessellator::new().tessellate_path(
        &path,
        &FillOptions::default()
            .with_fill_rule(FillRule::NonZero)
            .with_tolerance(FLAT),
        &mut BuffersBuilder::new(&mut buffers, |vertex: FillVertex| {
            vertex.position().to_array()
        }),
    );
    if made.is_err() {
        return (Vec::new(), Vec::new());
    }
    (buffers.vertices, buffers.indices)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn digits_are_laid_out_and_kept() {
        let one = laid(&["12"], &OWN, 40.0, true, false);
        assert_eq!(one.lines.len(), 1);
        assert!(one.lines[0].width > 10.0);
        assert!(!one.lines[0].indices.is_empty());
        let again = laid(&["12"], &OWN, 40.0, true, false);
        assert!(Rc::ptr_eq(&one, &again));
    }
}
