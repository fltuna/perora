//! Drawing what was drawn: one way of doing it, for the screen and for the image that leaves, so that
//! the two are the same (docs/gpui-rewrite.md, 「描画と焼き込み」). The editor draws the list over the
//! part of the picture it shows, at the size it shows it; the image that leaves is drawn a tile at a
//! time, a pixel of the image to a pixel of the screen, by the same `draw_all`.

use std::sync::Arc;

use gpui::{
    Bounds, ContentMask, Corners, Hsla, PathBuilder, PathStyle, Pixels, Point, RenderImage,
    StrokeOptions, Window, fill, point, px, rgb, size,
};
use lyon::tessellation::{LineCap, LineJoin};
use perora_core::edit::{Colour, Edit, Place};

use crate::marks;
use crate::view::Area;

/// Where places of the image are drawn: the place `offset` of the image at `origin` (logical pixels
/// of the window), and `scale` logical pixels to one of the image.
#[derive(Debug, Clone, Copy)]
pub struct Painter {
    pub origin: Point<Pixels>,
    pub offset: Place,
    pub scale: f32,
}

impl Painter {
    pub fn at(&self, at: Place) -> Point<Pixels> {
        point(
            self.origin.x + px((at[0] - self.offset[0]) * self.scale),
            self.origin.y + px((at[1] - self.offset[1]) * self.scale),
        )
    }

    /// A length in the image, on the screen.
    pub fn length(&self, length: f32) -> Pixels {
        px(length * self.scale)
    }

    pub fn bounds(&self, area: Area) -> Bounds<Pixels> {
        Bounds {
            origin: self.at([area.x, area.y]),
            size: size(self.length(area.width), self.length(area.height)),
        }
    }
}

pub fn colour(colour: Colour) -> Hsla {
    rgb(((colour.red as u32) << 16) | ((colour.green as u32) << 8) | colour.blue as u32).into()
}

/// Some of the capture's pixels, under what is drawn: an image that covers that part of it.
#[derive(Clone)]
pub struct Base {
    pub image: Arc<RenderImage>,
    pub area: Area,
}

/// The picture (`view`: all of the image, or what it is cut down to) with what was drawn on it, in
/// its turn, cut to the picture. `skip`: one of the list not drawn (being written anew elsewhere).
pub fn draw_all(
    window: &mut Window,
    painter: &Painter,
    view: Area,
    base: &[Base],
    edits: &[Edit],
    skip: Option<usize>,
) {
    let mask = ContentMask {
        bounds: painter.bounds(view),
    };
    window.with_content_mask(Some(mask), |window| {
        for part in base {
            // (The whole image where it is, cut by the mask: asked for only a part of it, GPUI cuts
            // the part to whole pixels of the image, which puts it beside where it is drawn on.)
            let bounds = painter.bounds(part.area);
            let _ = window.paint_image(
                bounds,
                bounds,
                Corners::default(),
                part.image.clone(),
                0,
                false,
            );
        }
        for (index, edit) in edits.iter().enumerate() {
            if Some(index) == skip {
                continue;
            }
            if let Some(mark) = marks::mark_of(edit) {
                mark.paint(edit, painter, window);
            }
        }
    });
}

/// So many places in one path at most: GPUI counts what it draws a path with in 16 bits, and a long
/// line of the pen would come to more than that (and not be drawn at all).
const PLACES_IN_A_PATH: usize = 1000;

/// A line through the places, as wide as `width` (pixels of the image), with round ends and corners;
/// a dot where they are all one place.
pub fn stroke_through(
    window: &mut Window,
    painter: &Painter,
    points: &[Place],
    width: f32,
    colour: Hsla,
) {
    let Some(first) = points.first() else {
        return;
    };
    if points.iter().all(|place| place == first) {
        disc(window, painter, *first, width / 2.0, colour);
        return;
    }
    let width = painter.length(width);
    let options = StrokeOptions::default()
        .with_line_width(width.as_f32())
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round);
    // (Each part begins where the one before ends: the round ends meet in one.)
    let mut start = 0;
    while start + 1 < points.len() {
        let end = (start + PLACES_IN_A_PATH).min(points.len() - 1);
        let mut path = PathBuilder::stroke(width).with_style(PathStyle::Stroke(options));
        path.move_to(painter.at(points[start]));
        for place in &points[start + 1..=end] {
            path.line_to(painter.at(*place));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, colour);
        }
        start = end;
    }
}

/// A round dot, `radius` pixels of the image.
pub fn disc(window: &mut Window, painter: &Painter, middle: Place, radius: f32, colour: Hsla) {
    let radius = painter.length(radius);
    let middle = painter.at(middle);
    let bounds = Bounds {
        origin: point(middle.x - radius, middle.y - radius),
        size: size(radius * 2.0, radius * 2.0),
    };
    window.paint_quad(fill(bounds, colour).corner_radii(radius));
}

// What shows what is selected: drawn over the picture, not cut to it (`editor.tsx`'s frames, holds
// and the part being marked out, in pixels of the screen whatever the look).

/// The dashed frame around what is selected: `area` in the image, `margin` around it.
pub fn frame(window: &mut Window, painter: &Painter, area: Area, margin: f32) {
    let around = Area::new(
        area.x - margin,
        area.y - margin,
        area.width + 2.0 * margin,
        area.height + 2.0 * margin,
    );
    let bounds = painter.bounds(around);
    // White dashes on black: as the Tauri version's white dashes with black ones between.
    rectangle_line(window, bounds, 1.5, None, gpui::black());
    rectangle_line(window, bounds, 1.5, Some((5.0, 4.0)), gpui::white());
}

/// Where the one selected can be taken hold of: a white square on its frame, or a disc on an end of
/// a line.
pub fn grip(window: &mut Window, painter: &Painter, at: Place, round: bool) {
    let middle = painter.at(at);
    let side = px(9.0);
    let bounds = Bounds {
        origin: point(middle.x - side / 2.0, middle.y - side / 2.0),
        size: size(side, side),
    };
    let quad = fill(bounds, gpui::white())
        .border_widths(px(1.0))
        .border_color(Hsla {
            a: 0.7,
            ..gpui::black()
        });
    window.paint_quad(if round {
        quad.corner_radii(side / 2.0)
    } else {
        quad
    });
}

/// The part being marked out: a pale blue inside, a white line and blue dashes around it.
pub fn marked(window: &mut Window, painter: &Painter, from: Place, to: Place) {
    let area = Area::new(
        from[0].min(to[0]),
        from[1].min(to[1]),
        (to[0] - from[0]).abs(),
        (to[1] - from[1]).abs(),
    );
    let bounds = painter.bounds(area);
    window.paint_quad(fill(
        bounds,
        gpui::Rgba {
            r: 79.0 / 255.0,
            g: 140.0 / 255.0,
            b: 1.0,
            a: 0.14,
        },
    ));
    rectangle_line(window, bounds, 1.0, None, gpui::white());
    rectangle_line(window, bounds, 1.0, Some((4.0, 3.0)), rgb(0x2f6fe0).into());
}

/// A line around a box of the screen, `width` logical pixels, dashed when `dashes` says how.
fn rectangle_line(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    width: f32,
    dashes: Option<(f32, f32)>,
    colour: Hsla,
) {
    let mut path = PathBuilder::stroke(px(width));
    if let Some((on, off)) = dashes {
        path = path.dash_array(&[px(on), px(off)]);
    }
    let (left, top) = (bounds.origin.x, bounds.origin.y);
    let (right, bottom) = (left + bounds.size.width, top + bounds.size.height);
    path.move_to(point(left, top));
    path.line_to(point(right, top));
    path.line_to(point(right, bottom));
    path.line_to(point(left, bottom));
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, colour);
    }
}
