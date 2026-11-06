//! Making a part unreadable: blurred, or broken into blocks of one colour each (`hide` in editor.tsx).
//! Only what is in the part goes into it: a blur takes nothing from beside the part (its edges are
//! repeated beyond it), and the blocks are counted from the part's top-left corner, those at its
//! right and bottom edges only partly in it being of what is in it. Done on the pixels themselves
//! (four bytes each, in whatever order: each byte is blurred or averaged on its own), in pixels of
//! the image.

/// A part of an image, in whole pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Part {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Part {
    /// The part of an image `width` by `height` an area covers (pixels it touches), if any.
    pub fn of(area: crate::view::Area, width: u32, height: u32) -> Option<Self> {
        let left = area.x.floor().max(0.0) as u32;
        let top = area.y.floor().max(0.0) as u32;
        let right = (area.right().ceil().max(0.0) as u32).min(width);
        let bottom = (area.bottom().ceil().max(0.0) as u32).min(height);
        (right > left && bottom > top).then_some(Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// The pixels of a part of an image `stride` pixels wide (four bytes to a pixel).
pub fn cut(pixels: &[u8], stride: u32, part: Part) -> Vec<u8> {
    let mut out = Vec::with_capacity(part.width as usize * part.height as usize * 4);
    for row in part.y..part.y + part.height {
        let start = (row as usize * stride as usize + part.x as usize) * 4;
        out.extend_from_slice(&pixels[start..start + part.width as usize * 4]);
    }
    out
}

/// The pixels of a part put back into the image.
pub fn put(pixels: &mut [u8], stride: u32, part: Part, those: &[u8]) {
    let row_bytes = part.width as usize * 4;
    for (index, row) in (part.y..part.y + part.height).enumerate() {
        let start = (row as usize * stride as usize + part.x as usize) * 4;
        pixels[start..start + row_bytes]
            .copy_from_slice(&those[index * row_bytes..(index + 1) * row_bytes]);
    }
}

/// The pixels of a part (`width` by `height`) blurred as a canvas's `blur(spread)` does (a Gaussian
/// of that standard deviation, here three box blurs that come to it), its edges repeated beyond it.
pub fn blurred(pixels: &[u8], width: u32, height: u32, spread: f32) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    if width == 0 || height == 0 || spread < 0.5 {
        return pixels.to_vec();
    }
    let mut now: Vec<f32> = pixels.iter().map(|&byte| byte as f32).collect();
    let mut spare = vec![0.0f32; now.len()];
    for radius in boxes(spread) {
        across(&now, &mut spare, width, height, radius);
        down(&spare, &mut now, width, height, radius);
    }
    now.iter()
        .map(|&value| value.round().clamp(0.0, 255.0) as u8)
        .collect()
}

/// The half-widths of three box blurs that come to a Gaussian of `sigma` (as W3C's filter effects
/// have it for `feGaussianBlur`).
fn boxes(sigma: f32) -> [usize; 3] {
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let upper = lower + 2;
    let ideal_count =
        (12.0 * sigma * sigma - 3.0 * (lower * lower) as f32 - 12.0 * lower as f32 - 9.0)
            / (-4.0 * lower as f32 - 4.0);
    let count = ideal_count.round() as i64;
    let mut sizes = [0usize; 3];
    for (index, size) in sizes.iter_mut().enumerate() {
        let width = if (index as i64) < count { lower } else { upper };
        *size = ((width.max(1) - 1) / 2) as usize;
    }
    sizes
}

/// A box blur along the rows, `radius` each way, the row's ends repeated beyond it.
fn across(from: &[f32], to: &mut [f32], width: usize, height: usize, radius: usize) {
    if radius == 0 {
        to.copy_from_slice(from);
        return;
    }
    let span = (radius * 2 + 1) as f32;
    for row in 0..height {
        let base = row * width * 4;
        for channel in 0..4 {
            let at =
                |x: isize| from[base + (x.clamp(0, width as isize - 1) as usize) * 4 + channel];
            let mut sum = 0.0;
            for x in -(radius as isize)..=(radius as isize) {
                sum += at(x);
            }
            for x in 0..width {
                to[base + x * 4 + channel] = sum / span;
                sum += at(x as isize + radius as isize + 1) - at(x as isize - radius as isize);
            }
        }
    }
}

/// A box blur along the columns, as `across`.
fn down(from: &[f32], to: &mut [f32], width: usize, height: usize, radius: usize) {
    if radius == 0 {
        to.copy_from_slice(from);
        return;
    }
    let span = (radius * 2 + 1) as f32;
    for column in 0..width {
        for channel in 0..4 {
            let at = |y: isize| {
                from[(y.clamp(0, height as isize - 1) as usize * width + column) * 4 + channel]
            };
            let mut sum = 0.0;
            for y in -(radius as isize)..=(radius as isize) {
                sum += at(y);
            }
            for y in 0..height {
                to[(y * width + column) * 4 + channel] = sum / span;
                sum += at(y as isize + radius as isize + 1) - at(y as isize - radius as isize);
            }
        }
    }
}

/// The pixels of a part broken into blocks `cell` pixels wide from its top-left corner, each the
/// average of the pixels it covers (those at the right and bottom edges, of the pixels they have).
pub fn mosaicked(pixels: &[u8], width: u32, height: u32, cell: f32) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let cell = cell.max(1.0);
    let mut out = vec![0u8; pixels.len()];
    let columns = (width as f32 / cell).ceil() as usize;
    let rows = (height as f32 / cell).ceil() as usize;
    for row in 0..rows {
        let (top, bottom) = (
            (row as f32 * cell).round() as usize,
            (((row + 1) as f32 * cell).round() as usize).min(height),
        );
        for column in 0..columns {
            let (left, right) = (
                (column as f32 * cell).round() as usize,
                (((column + 1) as f32 * cell).round() as usize).min(width),
            );
            if right <= left || bottom <= top {
                continue;
            }
            let mut sums = [0u64; 4];
            for y in top..bottom {
                for x in left..right {
                    let at = (y * width + x) * 4;
                    for channel in 0..4 {
                        sums[channel] += pixels[at + channel] as u64;
                    }
                }
            }
            let count = ((bottom - top) * (right - left)) as u64;
            let average = sums.map(|sum| ((sum + count / 2) / count) as u8);
            for y in top..bottom {
                for x in left..right {
                    let at = (y * width + x) * 4;
                    out[at..at + 4].copy_from_slice(&average);
                }
            }
        }
    }
    out
}

/// How strongly, in steps (1 to 10), as pixels of the image: a blur's spread and a block's width,
/// times the unit (`hiddenBy` in editor.tsx; the weakest a light one: owner, 「弱い側を削らない」).
pub const STRENGTHS: (u32, u32) = (1, 10);
const BLUR: (f32, f32) = (1.0, 7.0);
const MOSAIC: (f32, f32) = (2.0, 10.0);

fn per_step(way: (f32, f32)) -> f32 {
    (way.1 - way.0) / (STRENGTHS.1 - STRENGTHS.0) as f32
}

/// A blur's spread at a strength.
pub fn radius_of(step: u32, unit: f32) -> f32 {
    crate::marks::tenth((BLUR.0 + (step.max(1) - 1) as f32 * per_step(BLUR)) * unit)
}

/// A block's width at a strength.
pub fn cell_of(step: u32, unit: f32) -> f32 {
    crate::marks::tenth((MOSAIC.0 + (step.max(1) - 1) as f32 * per_step(MOSAIC)) * unit)
}

/// The strength a blur's spread or a block's width is (the nearest step).
pub fn step_of(edit: &perora_core::edit::Edit, unit: f32) -> u32 {
    let (value, way) = match edit {
        perora_core::edit::Edit::Blur { radius, .. } => (*radius, BLUR),
        perora_core::edit::Edit::Mosaic { cell, .. } => (*cell, MOSAIC),
        _ => return STRENGTHS.0,
    };
    let step = 1.0 + (value / unit.max(f32::EPSILON) - way.0) / per_step(way);
    (step.round() as u32).clamp(STRENGTHS.0, STRENGTHS.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blur_keeps_what_is_even_and_spreads_what_is_not() {
        let even = vec![100u8; 10 * 10 * 4];
        assert_eq!(blurred(&even, 10, 10, 3.0), even);
        // A white column in black: spread, the middle still the brightest.
        let mut line = vec![0u8; 21 * 3 * 4];
        for y in 0..3 {
            line[(y * 21 + 10) * 4] = 255;
        }
        let spread = blurred(&line, 21, 3, 2.0);
        assert!(spread[10 * 4] < 255 && spread[10 * 4] > spread[12 * 4]);
        assert!(spread[12 * 4] > 0);
    }

    #[test]
    fn blocks_are_the_average_of_what_they_cover_from_the_corner() {
        // 3 by 1: a block of 2 (0 and 100) and the half block at the edge (200).
        let pixels = [0u8, 0, 0, 0, 100, 100, 100, 100, 200, 200, 200, 200];
        let out = mosaicked(&pixels, 3, 1, 2.0);
        assert_eq!(&out[0..4], &[50, 50, 50, 50]);
        assert_eq!(&out[4..8], &[50, 50, 50, 50]);
        assert_eq!(&out[8..12], &[200, 200, 200, 200]);
    }

    #[test]
    fn the_strengths_go_from_light_to_strong() {
        assert_eq!(radius_of(1, 1.0), 1.0);
        assert_eq!(radius_of(10, 1.0), 7.0);
        assert_eq!(cell_of(1, 2.0), 4.0);
        assert_eq!(cell_of(10, 2.0), 20.0);
        let edit = perora_core::edit::Edit::Mosaic {
            from: [0.0, 0.0],
            to: [1.0, 1.0],
            cell: cell_of(5, 2.0),
        };
        assert_eq!(step_of(&edit, 2.0), 5);
    }

    #[test]
    fn a_part_is_cut_out_and_put_back() {
        let mut pixels: Vec<u8> = (0..4 * 4 * 4).map(|n| n as u8).collect();
        let part = Part {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        };
        let those = cut(&pixels, 4, part);
        assert_eq!(&those[0..4], &[20, 21, 22, 23]);
        let before = pixels.clone();
        put(&mut pixels, 4, part, &those);
        assert_eq!(pixels, before);
    }
}
