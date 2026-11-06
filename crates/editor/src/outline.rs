//! A stroke as one shape: what a marker is drawn as. GPUI draws a path by laying its triangles over
//! one another, and a line seen through is darker where they meet (where it turns, where it crosses
//! itself); a canvas strokes it as solid everywhere. So the stroke is first made one shape, the
//! places it covers and no part twice (i_overlay's stroke: round ends and corners), and that shape is
//! cut into triangles that do not meet (lyon's fill). docs/gpui-rewrite.md, 「マーカーの半透明」.
//!
//! The shape is made in pixels of the image from the stroke's first place, once for each stroke
//! (kept for those drawn lately): moved, a stroke keeps its shape.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use i_overlay::mesh::float::stroke::offset::StrokeOffset;
use i_overlay::mesh::float::style::{LineCap, LineJoin, StrokeStyle};
use i_overlay::mesh::math::MathMode;
use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};
use perora_core::edit::Place;

/// The triangles of a stroke's shape: corners in pixels of the image from its first place, three
/// to a triangle.
pub struct Triangles {
    pub corners: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// How far the round parts may be from round, in pixels of the image.
const ROUND_WITHIN: f32 = 0.05;
/// How many shapes are kept.
const KEPT: usize = 256;

thread_local! {
    static MADE: RefCell<Made> = RefCell::new(Made::default());
}

#[derive(Default)]
struct Made {
    shapes: HashMap<u64, (Rc<Triangles>, u64)>,
    used: u64,
}

/// The shape of a stroke through `points`, `width` wide: its triangles, from the first place.
pub fn of(points: &[Place], width: f32) -> Rc<Triangles> {
    let first = points.first().copied().unwrap_or([0.0, 0.0]);
    let key = {
        let mut hasher = DefaultHasher::new();
        width.to_bits().hash(&mut hasher);
        for place in points {
            // (To a tenth, as places are kept: moved, the differences are the same tenths, not the
            // same bits.)
            (((place[0] - first[0]) * 10.0).round() as i64).hash(&mut hasher);
            (((place[1] - first[1]) * 10.0).round() as i64).hash(&mut hasher);
        }
        hasher.finish()
    };
    MADE.with(|made| {
        let mut made = made.borrow_mut();
        made.used += 1;
        let used = made.used;
        if let Some((triangles, last)) = made.shapes.get_mut(&key) {
            *last = used;
            return triangles.clone();
        }
        let triangles = Rc::new(make(points, first, width));
        if made.shapes.len() >= KEPT {
            // The half used longest ago go.
            let mut ages: Vec<u64> = made.shapes.values().map(|(_, last)| *last).collect();
            ages.sort_unstable();
            let before = ages[ages.len() / 2];
            made.shapes.retain(|_, (_, last)| *last > before);
        }
        made.shapes.insert(key, (triangles.clone(), used));
        triangles
    })
}

fn make(points: &[Place], first: Place, width: f32) -> Triangles {
    let mut path: Vec<[f32; 2]> = Vec::with_capacity(points.len());
    for place in points {
        let at = [place[0] - first[0], place[1] - first[1]];
        if path.last() != Some(&at) {
            path.push(at);
        }
    }
    let radius = (width / 2.0).max(0.01);
    if path.len() < 2 {
        return disc(radius);
    }
    // The step of a round part, as many as it takes to be round within ROUND_WITHIN.
    let step = (2.0 * (1.0 - (ROUND_WITHIN / radius).min(1.0)).acos())
        .clamp(0.01 * std::f32::consts::PI, 0.25 * std::f32::consts::PI);
    let style = StrokeStyle::new(width)
        .line_join(LineJoin::Round(step))
        .start_cap(LineCap::Round(step))
        .end_cap(LineCap::Round(step))
        .math(MathMode::Float);
    let shapes = path.stroke(style, false);
    let mut outline = Path::builder();
    for shape in &shapes {
        for contour in shape {
            let [start, rest @ ..] = contour.as_slice() else {
                continue;
            };
            outline.begin(point(start[0], start[1]));
            for at in rest {
                outline.line_to(point(at[0], at[1]));
            }
            outline.end(true);
        }
    }
    let outline = outline.build();
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let tessellated = FillTessellator::new().tessellate_path(
        &outline,
        &FillOptions::default()
            .with_fill_rule(FillRule::NonZero)
            .with_tolerance(ROUND_WITHIN),
        &mut BuffersBuilder::new(&mut buffers, |vertex: FillVertex| {
            vertex.position().to_array()
        }),
    );
    if tessellated.is_err() {
        return Triangles {
            corners: Vec::new(),
            indices: Vec::new(),
        };
    }
    Triangles {
        corners: buffers.vertices,
        indices: buffers.indices,
    }
}

/// A dot: a stroke that did not move.
fn disc(radius: f32) -> Triangles {
    let count = ((std::f32::consts::TAU * radius / 1.5).ceil() as usize).clamp(12, 360);
    let mut corners = vec![[0.0, 0.0]];
    for step in 0..count {
        let angle = step as f32 / count as f32 * std::f32::consts::TAU;
        corners.push([radius * angle.cos(), radius * angle.sin()]);
    }
    let indices = (0..count as u32)
        .flat_map(|step| [0, step + 1, (step + 1) % count as u32 + 1])
        .collect();
    Triangles { corners, indices }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How much of the plane the triangles cover, counted once for each triangle over a place.
    fn area(triangles: &Triangles) -> f32 {
        triangles
            .indices
            .chunks_exact(3)
            .map(|three| {
                let [a, b, c] = [0, 1, 2].map(|at| triangles.corners[three[at] as usize]);
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn a_straight_stroke_is_its_box_and_two_half_discs() {
        let triangles = of(&[[10.0, 10.0], [110.0, 10.0]], 20.0);
        let expected = 100.0 * 20.0 + std::f32::consts::PI * 10.0 * 10.0;
        assert!(
            (area(&triangles) - expected).abs() / expected < 0.01,
            "{}",
            area(&triangles)
        );
    }

    #[test]
    fn a_stroke_over_itself_covers_each_place_once() {
        // Back and forth over the same line: as much as once.
        let there_and_back = of(&[[0.0, 0.0], [100.0, 0.0], [0.0, 0.0]], 20.0);
        let once = of(&[[0.0, 0.0], [100.0, 0.0]], 20.0);
        assert!((area(&there_and_back) - area(&once)).abs() / area(&once) < 0.01);
        // Crossing itself: less than its parts laid over one another.
        let crossing = of(
            &[[0.0, 0.0], [100.0, 100.0], [100.0, 0.0], [0.0, 100.0]],
            10.0,
        );
        let parts = 100.0 * std::f32::consts::SQRT_2 * 10.0 * 2.0 + 100.0 * 10.0;
        assert!(area(&crossing) < parts - 50.0, "{}", area(&crossing));
    }

    #[test]
    fn a_dot_is_a_disc_and_a_moved_stroke_keeps_its_shape() {
        let dot = of(&[[5.0, 5.0], [5.0, 5.0]], 10.0);
        let expected = std::f32::consts::PI * 25.0;
        assert!((area(&dot) - expected).abs() / expected < 0.02);
        let here = of(&[[0.0, 0.0], [30.0, 40.0]], 6.0);
        let there = of(&[[100.0, 100.0], [130.0, 140.0]], 6.0);
        assert!(Rc::ptr_eq(&here, &there));
    }
}
