//! What can be done with captured pixels, whichever system they came from.

use crate::geometry::Rect;
use crate::platform::Frame;

impl Frame {
    pub fn width(&self) -> u32 {
        self.area.width
    }

    pub fn height(&self) -> u32 {
        self.area.height
    }

    /// The part of the frame that shows `area` of the virtual screen; `None` when the frame shows
    /// none of it. An area reaching beyond the frame is cut to what the frame has.
    pub fn crop(&self, area: Rect) -> Option<Frame> {
        let area = self.area.intersection(&area)?;
        let stride = self.area.width as usize * 4;
        let left = (area.x - self.area.x) as usize * 4;
        let top = (area.y - self.area.y) as usize;
        let row = area.width as usize * 4;

        let mut rgba = Vec::with_capacity(row * area.height as usize);
        for y in top..top + area.height as usize {
            let start = y * stride + left;
            rgba.extend_from_slice(&self.rgba[start..start + row]);
        }

        Some(Frame { area, rgba })
    }

    /// The frame made smaller, keeping its proportions, so that it fits `max_width` by `max_height`:
    /// for showing, not for keeping. Each pixel of the result is the average of the pixels it stands
    /// for. A frame that fits already comes back as it is.
    pub fn scaled_to_fit(&self, max_width: u32, max_height: u32) -> Frame {
        let (width, height) = (self.width(), self.height());
        if width <= max_width && height <= max_height || width == 0 || height == 0 {
            return self.clone();
        }

        // The tighter of the two limits decides; the other side follows from the proportions.
        let (new_width, new_height) = if u64::from(width) * u64::from(max_height)
            > u64::from(height) * u64::from(max_width)
        {
            let scaled = u64::from(height) * u64::from(max_width) / u64::from(width);
            (max_width.max(1), (scaled as u32).max(1))
        } else {
            let scaled = u64::from(width) * u64::from(max_height) / u64::from(height);
            ((scaled as u32).max(1), max_height.max(1))
        };

        // The source columns and rows each result pixel covers: [start, end), never empty.
        let spans = |from: u32, to: u32| -> Vec<(usize, usize)> {
            (0..to)
                .map(|index| {
                    let start = u64::from(index) * u64::from(from) / u64::from(to);
                    let end = (u64::from(index) + 1) * u64::from(from) / u64::from(to);
                    (start as usize, (end.max(start + 1)) as usize)
                })
                .collect()
        };
        let (columns, rows) = (spans(width, new_width), spans(height, new_height));

        let stride = width as usize * 4;
        let mut rgba = Vec::with_capacity(new_width as usize * new_height as usize * 4);
        for &(top, bottom) in &rows {
            for &(left, right) in &columns {
                // Colours weighted by their opacity, so that transparent pixels (no monitor there)
                // do not darken the edge of a monitor.
                let (mut red, mut green, mut blue, mut alpha) = (0u64, 0u64, 0u64, 0u64);
                for y in top..bottom {
                    let row = &self.rgba[y * stride + left * 4..y * stride + right * 4];
                    for pixel in row.chunks_exact(4) {
                        let a = u64::from(pixel[3]);
                        red += u64::from(pixel[0]) * a;
                        green += u64::from(pixel[1]) * a;
                        blue += u64::from(pixel[2]) * a;
                        alpha += a;
                    }
                }
                let count = ((bottom - top) * (right - left)) as u64;
                match alpha {
                    0 => rgba.extend_from_slice(&[0, 0, 0, 0]),
                    _ => rgba.extend_from_slice(&[
                        (red / alpha) as u8,
                        (green / alpha) as u8,
                        (blue / alpha) as u8,
                        (alpha / count) as u8,
                    ]),
                }
            }
        }

        Frame {
            area: Rect::new(self.area.x, self.area.y, new_width, new_height),
            rgba,
        }
    }

    /// Makes everything outside `visible` transparent (black, alpha 0): the parts of the virtual screen
    /// that belong to no monitor.
    pub fn clear_outside(&mut self, visible: &[Rect]) {
        let width = self.area.width as usize;
        for y in 0..self.area.height as usize {
            let screen_y = self.area.y + y as i32;
            // The spans of this row that some monitor shows, as columns of the frame.
            let mut spans: Vec<(usize, usize)> = visible
                .iter()
                .filter(|rect| screen_y >= rect.y && screen_y < rect.bottom())
                .filter_map(|rect| {
                    let start = (rect.x - self.area.x).clamp(0, width as i32) as usize;
                    let end = (rect.right() - self.area.x).clamp(0, width as i32) as usize;
                    (end > start).then_some((start, end))
                })
                .collect();
            spans.sort_unstable();

            let row = &mut self.rgba[y * width * 4..(y + 1) * width * 4];
            let mut cleared_to = 0;
            for (start, end) in spans {
                if start > cleared_to {
                    row[cleared_to * 4..start * 4].fill(0);
                }
                cleared_to = cleared_to.max(end);
            }
            row[cleared_to * 4..].fill(0);
        }
    }

    /// The frame as a BMP file: nothing to compute, so it is there at once, at four bytes a pixel.
    /// For handing a picture to something on this machine that will only show it; transparency is not
    /// kept (viewers ignore it in this format).
    pub fn to_bmp(&self) -> Vec<u8> {
        const HEADERS: u32 = 14 + 40;
        let pixels = self.rgba.len() as u32;
        let row = self.area.width as usize * 4;

        let mut file = Vec::with_capacity(HEADERS as usize + self.rgba.len());
        file.extend_from_slice(b"BM");
        file.extend_from_slice(&(HEADERS + pixels).to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&HEADERS.to_le_bytes());
        file.extend_from_slice(&40u32.to_le_bytes());
        file.extend_from_slice(&(self.area.width as i32).to_le_bytes());
        // Positive: rows from the bottom, the form every viewer reads.
        file.extend_from_slice(&(self.area.height as i32).to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&[0; 4]); // not compressed
        file.extend_from_slice(&pixels.to_le_bytes());
        file.extend_from_slice(&[0; 16]);

        for line in self.rgba.chunks_exact(row.max(1)).rev() {
            let start = file.len();
            file.extend_from_slice(line);
            for pixel in file[start..].chunks_exact_mut(4) {
                pixel.swap(0, 2); // blue first
            }
        }
        file
    }

    /// The frame as a PNG file: lossless, with its transparency.
    pub fn to_png(&self) -> Result<Vec<u8>, png::EncodingError> {
        let mut file = Vec::new();
        let mut encoder = png::Encoder::new(&mut file, self.width(), self.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;
        writer.finish()?;
        Ok(file)
    }

    /// How large a PNG file says it is, from its first bytes alone: nothing is made of it. `None`
    /// when it does not begin as a PNG does.
    pub fn png_size(file: &[u8]) -> Option<(u32, u32)> {
        // The signature of a PNG, and the chunk every one begins with.
        let start = file.get(..24)?;
        if start[..8] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
            || &start[12..16] != b"IHDR"
        {
            return None;
        }
        let width = u32::from_be_bytes(start[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(start[20..24].try_into().ok()?);
        (width > 0 && height > 0).then_some((width, height))
    }

    /// The pixels of a PNG file, as a frame with its top-left corner at the origin. `None` when it
    /// is not one, or says it is larger than `most` pixels (nothing that large is made room for).
    pub fn from_png(file: &[u8], most: u64) -> Option<Frame> {
        let (width, height) = Self::png_size(file)?;
        if u64::from(width) * u64::from(height) > most {
            return None;
        }
        let mut decoder = png::Decoder::new(std::io::Cursor::new(file));
        // Room for its pixels and for what it is read through, and no more.
        decoder.set_limits(png::Limits {
            bytes: usize::try_from(most.saturating_mul(8))
                .unwrap_or(usize::MAX)
                .saturating_add(file.len())
                .saturating_add(1 << 20),
        });
        // Whatever kind of PNG it is, eight bits of each of its colours, and of its opacity.
        decoder.set_transformations(
            png::Transformations::EXPAND
                | png::Transformations::STRIP_16
                | png::Transformations::ALPHA,
        );
        let mut reader = decoder.read_info().ok()?;
        // (What it says of its size further in is what it said at its start.)
        if (reader.info().width, reader.info().height) != (width, height) {
            return None;
        }
        let mut read = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut read).ok()?;
        read.truncate(info.buffer_size());
        if info.bit_depth != png::BitDepth::Eight {
            return None;
        }
        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => read,
            png::ColorType::GrayscaleAlpha => read
                .chunks_exact(2)
                .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
                .collect(),
            _ => return None,
        };
        (rgba.len() as u64 == u64::from(width) * u64::from(height) * 4).then_some(Frame {
            area: Rect::new(0, 0, width, height),
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame whose pixel at (x, y) of the virtual screen is (x as u8, y as u8, 0, 255).
    fn frame(area: Rect) -> Frame {
        let mut rgba = Vec::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                rgba.extend_from_slice(&[x as u8, y as u8, 0, 255]);
            }
        }
        Frame { area, rgba }
    }

    fn pixel(frame: &Frame, x: i32, y: i32) -> [u8; 4] {
        let index = ((y - frame.area.y) as usize * frame.area.width as usize
            + (x - frame.area.x) as usize)
            * 4;
        frame.rgba[index..index + 4].try_into().unwrap()
    }

    #[test]
    fn a_crop_keeps_the_pixels_of_its_area_in_screen_coordinates() {
        let whole = frame(Rect::new(-8, -4, 16, 8));

        let part = whole.crop(Rect::new(-2, 1, 5, 2)).unwrap();

        assert_eq!(part.area, Rect::new(-2, 1, 5, 2));
        assert_eq!(part.rgba.len(), 5 * 2 * 4);
        assert_eq!(pixel(&part, -2, 1), [(-2i32) as u8, 1, 0, 255]);
        assert_eq!(pixel(&part, 2, 2), [2, 2, 0, 255]);
        assert_eq!(whole.crop(whole.area).unwrap(), whole);
    }

    #[test]
    fn a_crop_reaching_beyond_the_frame_is_cut_and_one_outside_it_is_nothing() {
        let whole = frame(Rect::new(0, 0, 10, 10));

        assert_eq!(
            whole.crop(Rect::new(-5, 7, 8, 20)).unwrap().area,
            Rect::new(0, 7, 3, 3)
        );
        assert!(whole.crop(Rect::new(10, 0, 5, 5)).is_none());
        assert!(whole.crop(Rect::new(3, 3, 0, 5)).is_none());
    }

    #[test]
    fn scaling_down_keeps_the_proportions_and_averages_the_pixels() {
        // Left half white, right half black, 8 by 4.
        let mut whole = frame(Rect::new(0, 0, 8, 4));
        for (index, pixel) in whole.rgba.chunks_exact_mut(4).enumerate() {
            let value = if index % 8 < 4 { 255 } else { 0 };
            pixel.copy_from_slice(&[value, value, value, 255]);
        }

        let small = whole.scaled_to_fit(4, 100);
        assert_eq!((small.width(), small.height()), (4, 2));
        assert_eq!(small.rgba.len(), 4 * 2 * 4);
        assert_eq!(
            &small.rgba[..16],
            &[
                255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255
            ]
        );

        // The height is the tighter limit here; one pixel stands for the whole image.
        let tiny = whole.scaled_to_fit(100, 1);
        assert_eq!((tiny.width(), tiny.height()), (2, 1));
        let dot = whole.scaled_to_fit(1, 1);
        assert_eq!((dot.width(), dot.height()), (1, 1));
        assert_eq!(dot.rgba, [127, 127, 127, 255]);

        assert_eq!(whole.scaled_to_fit(8, 4), whole);
        assert_eq!(whole.scaled_to_fit(4000, 4000), whole);
    }

    #[test]
    fn transparent_pixels_do_not_darken_what_is_scaled_down_beside_them() {
        // Two pixels: one red and opaque, one transparent (black, as cleared).
        let pair = Frame {
            area: Rect::new(0, 0, 2, 1),
            rgba: vec![200, 10, 10, 255, 0, 0, 0, 0],
        };

        assert_eq!(pair.scaled_to_fit(1, 1).rgba, [200, 10, 10, 127]);
    }

    #[test]
    fn a_bmp_has_its_rows_from_the_bottom_and_blue_first() {
        let two_rows = Frame {
            area: Rect::new(0, 0, 1, 2),
            rgba: vec![10, 20, 30, 255, 40, 50, 60, 255],
        };

        let file = two_rows.to_bmp();

        assert_eq!(&file[..2], b"BM");
        assert_eq!(file.len(), 54 + 8);
        assert_eq!(u32::from_le_bytes(file[2..6].try_into().unwrap()), 62);
        assert_eq!(i32::from_le_bytes(file[18..22].try_into().unwrap()), 1);
        assert_eq!(i32::from_le_bytes(file[22..26].try_into().unwrap()), 2);
        assert_eq!(&file[54..], &[60, 50, 40, 255, 30, 20, 10, 255]);
    }

    #[test]
    fn what_no_monitor_shows_becomes_transparent() {
        // A tall monitor on the left, a short one on the right, lower: two corners belong to neither.
        let left = Rect::new(-4, 0, 4, 6);
        let right = Rect::new(0, 2, 4, 3);
        let mut whole = frame(Rect::new(-4, 0, 8, 6));

        whole.clear_outside(&[left, right]);

        assert_eq!(pixel(&whole, -4, 0), [(-4i32) as u8, 0, 0, 255]);
        assert_eq!(pixel(&whole, -1, 5), [(-1i32) as u8, 5, 0, 255]);
        assert_eq!(pixel(&whole, 0, 2), [0, 2, 0, 255]);
        assert_eq!(pixel(&whole, 3, 4), [3, 4, 0, 255]);
        assert_eq!(pixel(&whole, 0, 1), [0, 0, 0, 0]); // above the right monitor
        assert_eq!(pixel(&whole, 3, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&whole, 0, 5), [0, 0, 0, 0]); // below it
        assert_eq!(pixel(&whole, 3, 5), [0, 0, 0, 0]);
    }

    #[test]
    fn overlapping_and_unsorted_monitors_do_not_clear_each_other() {
        let mut whole = frame(Rect::new(0, 0, 10, 1));

        whole.clear_outside(&[
            Rect::new(6, 0, 2, 1),
            Rect::new(1, 0, 4, 1),
            Rect::new(3, 0, 2, 1),
        ]);

        let alpha: Vec<u8> = (0..10).map(|x| pixel(&whole, x, 0)[3]).collect();
        assert_eq!(alpha, [0, 255, 255, 255, 255, 0, 255, 255, 0, 0]);
    }

    #[test]
    fn a_png_round_trips() {
        let original = frame(Rect::new(-3, 2, 7, 5));

        let file = original.to_png().unwrap();

        let mut reader = png::Decoder::new(std::io::Cursor::new(&file))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(
            (info.width, info.height, info.color_type),
            (7, 5, png::ColorType::Rgba)
        );
        assert_eq!(&pixels[..info.buffer_size()], &original.rgba[..]);
    }

    #[test]
    fn a_png_is_read_back_as_the_pixels_it_was_made_of() {
        let original = frame(Rect::new(-3, 2, 7, 5));
        let file = original.to_png().unwrap();

        assert_eq!(Frame::png_size(&file), Some((7, 5)));
        let read = Frame::from_png(&file, 35).unwrap();
        assert_eq!(read.area, Rect::new(0, 0, 7, 5));
        assert_eq!(read.rgba, original.rgba);

        // Larger than there is room for; cut short; its start and nothing more; no PNG at all.
        assert!(Frame::from_png(&file, 34).is_none());
        assert!(Frame::from_png(&file[..file.len() - 20], 35).is_none());
        assert_eq!(Frame::png_size(&file[..24]), Some((7, 5)));
        assert!(Frame::from_png(&file[..24], 35).is_none());
        assert_eq!(Frame::png_size(&file[..23]), None);
        assert_eq!(
            Frame::png_size(b"not a picture, whatever it says it is"),
            None
        );
        assert!(Frame::from_png(b"not a picture, whatever it says it is", 35).is_none());
        // One without opacity is all opaque.
        let mut plain = Vec::new();
        let mut encoder = png::Encoder::new(&mut plain, 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[1, 2, 3, 4, 5, 6]).unwrap();
        writer.finish().unwrap();
        assert_eq!(
            Frame::from_png(&plain, 2).unwrap().rgba,
            [1, 2, 3, 255, 4, 5, 6, 255]
        );
        // One that says it is of no size.
        let mut none = file[..24].to_vec();
        none[16..20].fill(0);
        assert_eq!(Frame::png_size(&none), None);
    }
}
