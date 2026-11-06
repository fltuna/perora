//! The capture as the editor shows it. The GPU gets it as it is, and halves of it, each half the size
//! of the one before: shown smaller than it is, the GPU leaves pixels out (it does not average
//! them), so the editor shows the half nearest the size it is shown at, which the GPU makes smaller
//! by less than half. Looked at closely (`SQUARES_FROM`), its pixels are shown as the squares they are:
//! the GPU would smooth them into each other, so the part in sight is made at the size of the screen,
//! a pixel of the image to so many of the screen, here (owner, 2026-11-06: 「1がいいね」).

use std::cell::RefCell;
use std::sync::Arc;

use gpui::{RenderImage, Window};
use perora_core::platform::Frame;

use crate::paint::Base;
use crate::view::{Area, Look, SQUARES_FROM};

/// No half smaller than this on its longer side.
const SMALLEST_HALF: u32 = 128;

pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// The image (blue first, as GPUI has pictures), and its halves.
    levels: Vec<Arc<RenderImage>>,
    /// The part in sight, as squares: for which look it was made, and the image.
    squares: RefCell<Option<(Squares, Arc<RenderImage>)>>,
}

/// Which pixels of the screen the squares were made for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Squares {
    /// The part of the image the place shows, and how many pixels of the screen one of it is.
    part: (f32, f32),
    dense_scale: f32,
    /// The pixels of the screen made, from the corner of the place: across, down, how many of each.
    columns: (u32, u32),
    rows: (u32, u32),
}

impl Picture {
    /// The picture of a capture. (Some work for a large one: made beside the window, not in it.)
    pub fn new(frame: &Frame) -> Option<Self> {
        let (width, height) = (frame.area.width, frame.area.height);
        let mut pixels = frame.rgba.clone();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let mut levels = Vec::new();
        let (mut now, mut across, mut down) = (pixels, width, height);
        loop {
            let half = (across.max(down) > SMALLEST_HALF * 2).then(|| halved(&now, across, down));
            levels.push(image(now, across, down)?);
            let Some((smaller, smaller_across, smaller_down)) = half else {
                break;
            };
            (now, across, down) = (smaller, smaller_across, smaller_down);
        }
        Some(Self {
            width,
            height,
            levels,
            squares: RefCell::new(None),
        })
    }

    pub fn whole(&self) -> Area {
        Area::new(0.0, 0.0, self.width as f32, self.height as f32)
    }

    /// What is drawn under the drawing, for a place of `size` (logical pixels) on a screen of `dense`
    /// pixels to a logical one, looked at as `look` says. Lets go of squares no longer shown.
    pub fn bases(
        &self,
        look: &Look,
        size: (f32, f32),
        dense: f32,
        window: &mut Window,
    ) -> Vec<Base> {
        let on_screen = look.scale * dense;
        if on_screen <= 0.0 {
            return Vec::new();
        }
        if on_screen >= SQUARES_FROM {
            if let Some(base) = self.squares(look, size, dense, window) {
                return vec![base];
            }
        } else if let Some((_, image)) = self.squares.borrow_mut().take() {
            let _ = window.drop_image(image);
        }
        // The half that is shown made smaller by less than half: the largest no smaller than shown.
        let level = if on_screen < 1.0 {
            ((1.0 / on_screen).log2().floor() as usize).min(self.levels.len() - 1)
        } else {
            0
        };
        vec![Base {
            image: self.levels[level].clone(),
            area: self.whole(),
        }]
    }

    /// The part of the picture in sight, at the size of the screen: each pixel of the screen the
    /// pixel of the image its middle is on.
    fn squares(
        &self,
        look: &Look,
        size: (f32, f32),
        dense: f32,
        window: &mut Window,
    ) -> Option<Base> {
        let on_screen = look.scale * dense;
        let part = look.part;
        // The pixels of the screen over the picture.
        let shown = (
            (size.0 * dense).round().max(0.0),
            (size.1 * dense).round().max(0.0),
        );
        let across = |x: f32| ((x - part.x) * on_screen).clamp(0.0, shown.0);
        let down = |y: f32| ((y - part.y) * on_screen).clamp(0.0, shown.1);
        let (left, right) = (across(0.0).floor(), across(self.width as f32).ceil());
        let (top, bottom) = (down(0.0).floor(), down(self.height as f32).ceil());
        if right <= left || bottom <= top {
            return None;
        }
        let made = Squares {
            part: (part.x, part.y),
            dense_scale: on_screen,
            columns: (left as u32, (right - left) as u32),
            rows: (top as u32, (bottom - top) as u32),
        };
        let area = Area::new(
            part.x + left / on_screen,
            part.y + top / on_screen,
            (right - left) / on_screen,
            (bottom - top) / on_screen,
        );
        if let Some((was, image)) = self.squares.borrow().as_ref()
            && *was == made
        {
            return Some(Base {
                image: image.clone(),
                area,
            });
        }
        let whole = self.levels[0].as_bytes(0)?;
        let pixel_of = |at: u32, start: f32, most: u32| {
            ((start + (at as f32 + 0.5) / on_screen).floor().max(0.0) as u32).min(most - 1)
        };
        let columns: Vec<usize> = (made.columns.0..made.columns.0 + made.columns.1)
            .map(|x| pixel_of(x, part.x, self.width) as usize * 4)
            .collect();
        let stride = self.width as usize * 4;
        let mut pixels = Vec::with_capacity(columns.len() * made.rows.1 as usize * 4);
        for y in made.rows.0..made.rows.0 + made.rows.1 {
            let row = &whole[pixel_of(y, part.y, self.height) as usize * stride..][..stride];
            for &x in &columns {
                pixels.extend_from_slice(&row[x..x + 4]);
            }
        }
        let image = image(pixels, made.columns.1, made.rows.1)?;
        if let Some((_, old)) = self.squares.replace(Some((made, image.clone()))) {
            let _ = window.drop_image(old);
        }
        Some(Base { image, area })
    }

    /// The images the GPU holds of this picture, let go of as it is no longer shown.
    pub fn let_go(&self, window: &mut Window) {
        for image in self.levels.iter().cloned() {
            let _ = window.drop_image(image);
        }
        if let Some((_, image)) = self.squares.borrow_mut().take() {
            let _ = window.drop_image(image);
        }
    }
}

fn image(pixels: Vec<u8>, width: u32, height: u32) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// Half as wide and half as tall (rounded up), each pixel the average of the four it stands for (of
/// those there are, at an odd edge).
fn halved(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (half_width, half_height) = (width.div_ceil(2), height.div_ceil(2));
    let (width, height) = (width as usize, height as usize);
    let mut out = Vec::with_capacity(half_width as usize * half_height as usize * 4);
    for y in 0..half_height as usize {
        let rows = [2 * y, (2 * y + 1).min(height - 1)];
        for x in 0..half_width as usize {
            let columns = [2 * x, (2 * x + 1).min(width - 1)];
            for channel in 0..4 {
                let mut sum = 0u32;
                for row in rows {
                    for column in columns {
                        sum += pixels[(row * width + column) * 4 + channel] as u32;
                    }
                }
                out.push(((sum + 2) / 4) as u8);
            }
        }
    }
    (out, half_width, half_height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_is_the_average_of_the_four() {
        // 3 x 1: the odd pixel at the edge stands for itself.
        let pixels = [0, 0, 0, 255, 100, 100, 100, 255, 50, 60, 70, 255];
        let (half, width, height) = halved(&pixels, 3, 1);
        assert_eq!((width, height), (2, 1));
        assert_eq!(half, vec![50, 50, 50, 255, 50, 60, 70, 255]);
    }
}
