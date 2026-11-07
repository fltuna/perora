//! The place the picture is shown in, as the Tauri version's `.picture` has it: squares behind the
//! picture (a `repeating-conic-gradient` of the surface and the background colours, 16 by 16 a pair
//! of each, from the middle), seen where the picture is not and where it is clear; and around it a
//! line, its corners rounded, which cut what is inside them.
//!
//! The squares are a small picture of a few of them, laid side by side: as many squares one by one
//! would be tens of thousands on a large screen, drawn again at each move of the pen.

use std::cell::RefCell;
use std::sync::Arc;

use gpui::{
    Bounds, Corners, Hsla, PathBuilder, Pixels, RenderImage, Window, point, px, quad, size,
    transparent_black,
};

/// The side of a square, in logical pixels (two of them a side make the Tauri version's 16).
const SQUARE: f32 = 8.0;
/// How large, at least, the picture of squares laid side by side is (pixels of the screen).
const TILE_AT_LEAST: u32 = 128;
/// The rounding of the corners and the width of the line, as the Tauri version's.
pub const RADIUS: f32 = 6.0;
pub const LINE: f32 = 1.0;

/// What a picture of squares is made for: the side of a square and of the picture (pixels of the
/// screen), and the two colours.
type MadeFor = (u32, u32, [u8; 4], [u8; 4]);

#[derive(Default)]
pub struct Checker {
    /// The picture of squares, and what it was made for: made again when that changes.
    made: RefCell<Option<(MadeFor, Arc<RenderImage>)>>,
}

/// A colour as the picture's bytes have it (blue, green, red, alpha: what GPUI takes).
fn bgra(colour: Hsla) -> [u8; 4] {
    let rgba = colour.to_rgb();
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(rgba.b), byte(rgba.g), byte(rgba.r), byte(rgba.a)]
}

impl Checker {
    /// Squares over `bounds` (cut to them), one of each colour (`light`, `dark`) by turns, with
    /// four meeting in the middle of it: as the Tauri version laid its 16 by 16 from the middle.
    pub fn paint(&self, bounds: Bounds<Pixels>, light: Hsla, dark: Hsla, window: &mut Window) {
        let dense = window.scale_factor();
        let square = (SQUARE * dense).round().max(1.0) as u32;
        let tile = 2 * square * (TILE_AT_LEAST / (2 * square)).max(1);
        let key = (square, tile, bgra(light), bgra(dark));
        let image = {
            let mut made = self.made.borrow_mut();
            match made.as_ref() {
                Some((made_for, image)) if *made_for == key => image.clone(),
                _ => {
                    if let Some((_, old)) = made.take() {
                        let _ = window.drop_image(old);
                    }
                    let Some(image) = squares(square, tile, key.2, key.3) else {
                        return;
                    };
                    *made = Some((key, image.clone()));
                    image
                }
            }
        };
        // From the middle, in pixels of the screen: a picture of squares has an even number of
        // them a side, so one laid at a whole number of pictures from the middle is in step.
        let edge = |at: Pixels| (at.as_f32() * dense).round();
        let (left, top) = (edge(bounds.origin.x), edge(bounds.origin.y));
        let (right, bottom) = (
            edge(bounds.origin.x + bounds.size.width),
            edge(bounds.origin.y + bounds.size.height),
        );
        let middle = ((left + right) / 2.0).round();
        let centre = ((top + bottom) / 2.0).round();
        let tile = tile as f32;
        let first = |middle: f32, from: f32| middle - ((middle - from) / tile).ceil() * tile;
        let mut y = first(centre, top);
        while y < bottom {
            let mut x = first(middle, left);
            while x < right {
                let laid = Bounds {
                    origin: point(px(x / dense), px(y / dense)),
                    size: size(px(tile / dense), px(tile / dense)),
                };
                // (Cut to `bounds`: what of it is outside is not drawn.)
                let _ = window.paint_image(
                    bounds,
                    laid,
                    Corners::default(),
                    image.clone(),
                    0,
                    false,
                );
                x += tile;
            }
            y += tile;
        }
    }
}

/// The picture of squares: `tile` pixels a side, squares of `square`; the one at its corner of the
/// dark colour (as is the one up and left of the middle), and the others by turns.
fn squares(square: u32, tile: u32, light: [u8; 4], dark: [u8; 4]) -> Option<Arc<RenderImage>> {
    let mut pixels = Vec::with_capacity((tile * tile * 4) as usize);
    for y in 0..tile {
        for x in 0..tile {
            let odd = (x / square + y / square) % 2 == 1;
            pixels.extend_from_slice(if odd { &light } else { &dark });
        }
    }
    let buffer = image::RgbaImage::from_raw(tile, tile, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// The line around the place (`inner`: what is inside it), its corners rounded: what is inside the
/// corners and outside their round is covered with `ground` (the colour around the place), as the
/// Tauri version's `overflow: hidden` cut it, and the line drawn over that.
pub fn frame(inner: Bounds<Pixels>, line: Hsla, ground: Hsla, window: &mut Window) {
    let outer = inner.dilate(px(LINE));
    let (left, top) = (outer.origin.x.as_f32(), outer.origin.y.as_f32());
    let (right, bottom) = (left + outer.size.width.as_f32(), top + outer.size.height.as_f32());
    // Each corner: where it is, and which way the round goes in from it.
    for (x, y, across, down) in [
        (left, top, 1.0, 1.0),
        (right, top, -1.0, 1.0),
        (right, bottom, -1.0, -1.0),
        (left, bottom, 1.0, -1.0),
    ] {
        let (cx, cy) = (x + across * RADIUS, y + down * RADIUS);
        let mut path = PathBuilder::fill();
        path.move_to(point(px(x), px(y)));
        path.line_to(point(px(cx), px(y)));
        // (In pieces: a path of short lines is round enough at this size.)
        let steps = 8;
        for step in 1..steps {
            let turned = std::f32::consts::FRAC_PI_2 * step as f32 / steps as f32;
            path.line_to(point(
                px(cx - across * RADIUS * turned.sin()),
                px(cy - down * RADIUS * turned.cos()),
            ));
        }
        path.line_to(point(px(x), px(cy)));
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, ground);
        }
    }
    window.paint_quad(
        quad(outer, px(RADIUS), transparent_black(), px(LINE), line, Default::default()),
    );
}
