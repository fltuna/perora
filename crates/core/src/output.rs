//! A capture as the file that leaves: of a kind the user chose (PNG, JPEG, WebP), as good as they
//! chose, and no larger than they chose.

use crate::platform::Frame;

/// The kind of file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Lossless, and what everything takes.
    #[default]
    Png,
    /// Small, and never quite what was captured. Nothing in it is see-through.
    Jpeg,
    /// Smaller than either at the same look; lossless at its best.
    WebP,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Png, Format::Jpeg, Format::WebP];

    /// The name used in settings, language files and between the windows and the application;
    /// also how a file of the kind ends.
    pub const fn name(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::WebP => "webp",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|format| format.name() == name)
    }

    /// The kind of a file, from its first bytes; `None` for any other.
    pub fn of(file: &[u8]) -> Option<Self> {
        if file.starts_with(&[0x89, b'P', b'N', b'G']) {
            Some(Format::Png)
        } else if file.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(Format::Jpeg)
        } else if file.len() >= 12 && &file[..4] == b"RIFF" && &file[8..12] == b"WEBP" {
            Some(Format::WebP)
        } else {
            None
        }
    }
}

/// How good a file that loses something is: chosen by name, not by number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    /// As good as the kind gets: for WebP, nothing is lost at all.
    Best,
    #[default]
    High,
    Standard,
    /// Small before all else.
    Light,
}

impl Quality {
    pub const ALL: [Quality; 4] = [
        Quality::Best,
        Quality::High,
        Quality::Standard,
        Quality::Light,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Quality::Best => "best",
            Quality::High => "high",
            Quality::Standard => "standard",
            Quality::Light => "light",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|quality| quality.name() == name)
    }

    /// As the encoders are told it, of a hundred.
    const fn of_a_hundred(self) -> u8 {
        match self {
            Quality::Best => 97,
            Quality::High => 90,
            Quality::Standard => 80,
            Quality::Light => 65,
        }
    }
}

/// The sides a file can be kept within: a square of that many pixels, the picture keeping its
/// proportions inside it. (2048 is what VRChat takes.)
pub const FITS: [u32; 5] = [4096, 2048, 1920, 1280, 1024];

/// What a capture is made into as it leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Output {
    pub format: Format,
    pub quality: Quality,
    /// The square the picture is made to fit, when it does not: by so many pixels a side. `None`
    /// for as large as it was captured.
    pub fit: Option<u32>,
}

impl Output {
    /// Whether a PNG of the capture is what leaves already: nothing to make.
    pub fn keeps(&self, width: u32, height: u32) -> bool {
        self.format == Format::Png && self.fit.is_none_or(|side| width <= side && height <= side)
    }
}

/// What each pixel of a row made shorter is made of: the first pixel of the longer row it covers,
/// and how much of that one and of each after it (parts of a whole: together, one).
fn shares(from: u32, to: u32) -> Vec<(usize, Vec<f32>)> {
    let each = f64::from(from) / f64::from(to);
    (0..to)
        .map(|index| {
            let (start, end) = (f64::from(index) * each, f64::from(index + 1) * each);
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).clamp(first + 1, from as usize);
            let shares = (first..last)
                .map(|pixel| {
                    let covered = end.min((pixel + 1) as f64) - start.max(pixel as f64);
                    (covered.max(0.0) / each) as f32
                })
                .collect();
            (first, shares)
        })
        .collect()
}

impl Frame {
    /// The frame made to fit a square of `side` pixels, keeping its proportions: for keeping, not
    /// only for showing. Each pixel of the result is the average of what it covers of the frame,
    /// by how much of each pixel it covers (a pixel it covers half of counts half), so that
    /// letters stay as even as they were at any size. `None` when the frame fits already.
    pub fn fitted(&self, side: u32) -> Option<Frame> {
        let (width, height) = (self.width(), self.height());
        if (width <= side && height <= side) || width == 0 || height == 0 || side == 0 {
            return None;
        }
        let other = |long: u32, short: u32| {
            ((u64::from(short) * u64::from(side) + u64::from(long) / 2) / u64::from(long)).max(1)
                as u32
        };
        let (new_width, new_height) = if width >= height {
            (side, other(width, height))
        } else {
            (other(height, width), side)
        };

        // Across first, then down. Colours count by how opaque they are, so that what is
        // see-through beside them does not darken them.
        let (across, down) = (shares(width, new_width), shares(height, new_height));
        let mut rows = vec![0f32; new_width as usize * height as usize * 4];
        for y in 0..height as usize {
            let row = &self.rgba[y * width as usize * 4..][..width as usize * 4];
            for (x, (first, shares)) in across.iter().enumerate() {
                let mut sum = [0f32; 4];
                for (pixel, share) in row[first * 4..].chunks_exact(4).zip(shares) {
                    let opaque = f32::from(pixel[3]) * share;
                    sum[0] += f32::from(pixel[0]) * opaque;
                    sum[1] += f32::from(pixel[1]) * opaque;
                    sum[2] += f32::from(pixel[2]) * opaque;
                    sum[3] += opaque;
                }
                rows[(y * new_width as usize + x) * 4..][..4].copy_from_slice(&sum);
            }
        }
        let mut rgba = Vec::with_capacity(new_width as usize * new_height as usize * 4);
        for (first, shares) in &down {
            for x in 0..new_width as usize {
                let mut sum = [0f32; 4];
                for (offset, share) in shares.iter().enumerate() {
                    let pixel = &rows[((first + offset) * new_width as usize + x) * 4..][..4];
                    for (total, part) in sum.iter_mut().zip(pixel) {
                        *total += part * share;
                    }
                }
                let colour = |total: f32| {
                    if sum[3] > 0.0 {
                        (total / sum[3]).round().clamp(0.0, 255.0) as u8
                    } else {
                        0
                    }
                };
                rgba.extend_from_slice(&[
                    colour(sum[0]),
                    colour(sum[1]),
                    colour(sum[2]),
                    sum[3].round().clamp(0.0, 255.0) as u8,
                ]);
            }
        }
        Some(Frame {
            area: crate::geometry::Rect::new(self.area.x, self.area.y, new_width, new_height),
            rgba,
        })
    }

    /// The frame as the file that leaves: made smaller first when it is to fit a square it does
    /// not fit, then written as the kind of file asked for.
    pub fn to_file(&self, how: Output) -> Result<Vec<u8>, String> {
        let fitted = how.fit.and_then(|side| self.fitted(side));
        let frame = fitted.as_ref().unwrap_or(self);
        match how.format {
            Format::Png => frame.to_png().map_err(|error| error.to_string()),
            Format::Jpeg => frame.to_jpeg(how.quality),
            Format::WebP => frame.to_webp(how.quality),
        }
    }

    /// As a JPEG file. What is see-through (where no monitor is) is white in it: the kind has
    /// nothing see-through.
    fn to_jpeg(&self, quality: Quality) -> Result<Vec<u8>, String> {
        let (width, height) = (
            u16::try_from(self.width()).map_err(|_| "too wide for a JPEG".to_owned())?,
            u16::try_from(self.height()).map_err(|_| "too tall for a JPEG".to_owned())?,
        );
        let rgb: Vec<u8> = self
            .rgba
            .chunks_exact(4)
            .flat_map(|pixel| {
                let opaque = u32::from(pixel[3]);
                let over_white = |colour: u8| {
                    ((u32::from(colour) * opaque + 255 * (255 - opaque) + 127) / 255) as u8
                };
                [
                    over_white(pixel[0]),
                    over_white(pixel[1]),
                    over_white(pixel[2]),
                ]
            })
            .collect();
        let mut file = Vec::new();
        jpeg_encoder::Encoder::new(&mut file, quality.of_a_hundred())
            .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
            .map_err(|error| error.to_string())?;
        Ok(file)
    }

    /// As a WebP file: lossless at its best, otherwise as good as asked.
    fn to_webp(&self, quality: Quality) -> Result<Vec<u8>, String> {
        // (The kind holds no more than this a side.)
        const MOST: u32 = 16383;
        if self.width() > MOST || self.height() > MOST || self.rgba.is_empty() {
            return Err("too large for a WebP".to_owned());
        }
        // (Asked so that a picture it cannot write is a reason, not the end of whoever asked.)
        let encoder = webp::Encoder::from_rgba(&self.rgba, self.width(), self.height());
        let file = match quality {
            // (For a file that loses nothing, the number is how hard it is tried to make it
            // small: as hard as the library does when it is not told.)
            Quality::Best => encoder.encode_simple(true, 75.0),
            lossy => encoder.encode_simple(false, f32::from(lossy.of_a_hundred())),
        };
        file.map(|file| file.to_vec())
            .map_err(|error| format!("a WebP could not be made of it: {error:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{Format, Output, Quality};
    use crate::geometry::Rect;
    use crate::platform::Frame;

    /// A picture with something in it to lose: a slope of colours, and a corner that is
    /// see-through.
    fn frame(width: u32, height: u32) -> Frame {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let see_through = x < width / 4 && y < height / 4;
                rgba.extend_from_slice(&[
                    (x * 255 / width.max(1)) as u8,
                    (y * 255 / height.max(1)) as u8,
                    ((x * 7 + y * 13) % 256) as u8,
                    if see_through { 0 } else { 255 },
                ]);
            }
        }
        Frame {
            area: Rect::new(0, 0, width, height),
            rgba,
        }
    }

    #[test]
    fn a_file_is_of_the_kind_asked_for_and_says_so_in_its_first_bytes() {
        let frame = frame(64, 40);
        for format in Format::ALL {
            let file = frame
                .to_file(Output {
                    format,
                    ..Output::default()
                })
                .unwrap();
            assert_eq!(Format::of(&file), Some(format), "{format:?}");
            assert_eq!(Format::from_name(format.name()), Some(format));
        }
        assert_eq!(Format::of(b"GIF89a, which is none of them"), None);
        assert_eq!(Format::from_name("gif"), None);
    }

    #[test]
    fn a_png_and_the_best_webp_lose_nothing() {
        let frame = frame(48, 32);
        let png = frame.to_file(Output::default()).unwrap();
        assert_eq!(Frame::from_png(&png, 1 << 20).unwrap().rgba, frame.rgba);

        let webp = frame
            .to_file(Output {
                format: Format::WebP,
                quality: Quality::Best,
                fit: None,
            })
            .unwrap();
        let read = webp::Decoder::new(&webp).decode().unwrap();
        assert_eq!((read.width(), read.height()), (48, 32));
        // (Where it is see-through, the colour under it is nobody's business.)
        for (kept, was) in read.chunks_exact(4).zip(frame.rgba.chunks_exact(4)) {
            assert_eq!(kept[3], was[3]);
            if was[3] != 0 {
                assert_eq!(kept, was);
            }
        }
    }

    #[test]
    fn what_loses_something_is_smaller_the_less_good_it_is() {
        let frame = frame(320, 200);
        for format in [Format::Jpeg, Format::WebP] {
            let sizes: Vec<usize> = Quality::ALL
                .into_iter()
                .map(|quality| {
                    frame
                        .to_file(Output {
                            format,
                            quality,
                            fit: None,
                        })
                        .unwrap()
                        .len()
                })
                .collect();
            // (The best WebP loses nothing, which is another way of writing it altogether: of a
            // picture like this one, as of most of what is on a screen, the smallest of them.)
            let losing = &sizes[usize::from(format == Format::WebP)..];
            assert!(
                losing.windows(2).all(|pair| pair[0] > pair[1]),
                "{format:?}: {sizes:?}"
            );
        }
        for quality in Quality::ALL {
            assert_eq!(Quality::from_name(quality.name()), Some(quality));
        }
    }

    #[test]
    fn a_picture_that_does_not_fit_is_made_to_keeping_its_proportions() {
        let wide = frame(400, 100);
        let fitted = wide
            .to_file(Output {
                fit: Some(200),
                ..Output::default()
            })
            .unwrap();
        assert_eq!(Frame::png_size(&fitted), Some((200, 50)));

        // One that fits is left as large as it is, and its PNG is what leaves already.
        let small = Output {
            fit: Some(2048),
            ..Output::default()
        };
        assert_eq!(
            Frame::png_size(&wide.to_file(small).unwrap()),
            Some((400, 100))
        );
        assert!(small.keeps(400, 100));
        assert!(!small.keeps(4000, 100));
        assert!(Output::default().keeps(9000, 9000));
        assert!(
            !Output {
                format: Format::Jpeg,
                ..Output::default()
            }
            .keeps(10, 10)
        );
    }

    #[test]
    fn made_smaller_every_pixel_counts_by_how_much_of_it_is_covered() {
        // Five across made into four: each of the four covers one and a quarter of the five. A
        // row of 0, 100, 200, 100, 0 becomes 20, 140, 140, 20: the same at both ends, which
        // counting whole pixels alone would not give.
        let row = [0u8, 100, 200, 100, 0];
        let frame = Frame {
            area: Rect::new(0, 0, 5, 1),
            rgba: row
                .iter()
                .flat_map(|grey| [*grey, *grey, *grey, 255])
                .collect(),
        };
        let fitted = frame.fitted(4).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (4, 1));
        let greys: Vec<u8> = fitted.rgba.chunks_exact(4).map(|pixel| pixel[0]).collect();
        assert_eq!(greys, [20, 140, 140, 20]);
        assert!(fitted.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));

        // What is see-through beside a colour does not darken it; and what fits is left alone.
        let half = Frame {
            area: Rect::new(0, 0, 2, 1),
            rgba: vec![200, 100, 50, 255, 0, 0, 0, 0],
        };
        assert_eq!(half.fitted(1).unwrap().rgba, [200, 100, 50, 128]);
        assert!(frame.fitted(5).is_none());
        // Tall, it is the height that is the side.
        let tall = Frame {
            area: Rect::new(0, 0, 10, 40),
            rgba: [9, 9, 9, 255].repeat(400),
        };
        let fitted = tall.fitted(20).unwrap();
        assert_eq!((fitted.width(), fitted.height()), (5, 20));
        assert!(
            fitted
                .rgba
                .chunks_exact(4)
                .all(|pixel| pixel == [9, 9, 9, 255])
        );
    }

    #[test]
    fn what_is_see_through_is_white_in_a_jpeg() {
        let clear = Frame {
            area: Rect::new(0, 0, 16, 16),
            rgba: [0, 0, 0, 0].repeat(16 * 16),
        };
        let file = clear
            .to_file(Output {
                format: Format::Jpeg,
                quality: Quality::Best,
                fit: None,
            })
            .unwrap();
        // (Read back by the one library here that reads pictures of more than one kind.)
        assert_eq!(Format::of(&file), Some(Format::Jpeg));
        assert!(file.len() > 100);
    }
}
