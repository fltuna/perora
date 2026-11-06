//! The colours offered to draw in, and a colour as it is mixed by hand. The offered ones are Snipping
//! Tool's thirty (greys, strong colours, pale ones; six to a row) and a marker's six. A mixed colour
//! is its hue, how much of the hue there is (saturation) and how light it is (value): as the Tauri
//! version's `toHex` / `toHsv` in editor.tsx, to the same rounding.

use crate::edit::Colour;

const fn rgb(hex: u32) -> Colour {
    Colour {
        red: (hex >> 16) as u8,
        green: (hex >> 8) as u8,
        blue: hex as u8,
    }
}

/// The colours offered, six to a row: greys, strong colours, and pale ones.
pub const PALETTE: [Colour; 30] = [
    rgb(0x000000),
    rgb(0xffffff),
    rgb(0xd1d3d4),
    rgb(0xa7a9ac),
    rgb(0x808285),
    rgb(0x58595b),
    rgb(0xb31564),
    rgb(0xe61b1b),
    rgb(0xff5500),
    rgb(0xffaa00),
    rgb(0xffce00),
    rgb(0xffe600),
    rgb(0xa2e61b),
    rgb(0x26e600),
    rgb(0x008055),
    rgb(0x00aacc),
    rgb(0x004de6),
    rgb(0x3d00b8),
    rgb(0x6600cc),
    rgb(0x600080),
    rgb(0xf7d7c4),
    rgb(0xbb9167),
    rgb(0x8e562e),
    rgb(0x613d30),
    rgb(0xff80ff),
    rgb(0xffc680),
    rgb(0xffff80),
    rgb(0x80ff9e),
    rgb(0x80d6ff),
    rgb(0xbcb3ff),
];

/// Where "none" is offered (the inside of a shape), it takes the place of the darkest grey: the rows
/// stay six wide.
pub const NONE_TAKES: usize = 5;

/// The colours of a marker.
pub const MARKERS: [Colour; 6] = [
    rgb(0xffe600),
    rgb(0x26e600),
    rgb(0x44c8f5),
    rgb(0xec008c),
    rgb(0xff5500),
    rgb(0x6600cc),
];

/// A colour as it is mixed: its hue (0 to 360), saturation and value (0 to 1 each).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

impl Hsv {
    /// The hue, saturation and value of a colour. A grey has no hue of its own: 0 (red).
    pub fn of(colour: Colour) -> Self {
        let [red, green, blue] =
            [colour.red, colour.green, colour.blue].map(|part| part as f32 / 255.0);
        let most = red.max(green).max(blue);
        let least = red.min(green).min(blue);
        let spread = most - least;
        let hue = if spread == 0.0 {
            0.0
        } else if most == red {
            ((green - blue) / spread + 6.0) % 6.0
        } else if most == green {
            (blue - red) / spread + 2.0
        } else {
            (red - green) / spread + 4.0
        };
        Self {
            hue: hue * 60.0,
            saturation: if most == 0.0 { 0.0 } else { spread / most },
            value: most,
        }
    }

    /// The colour it mixes to.
    pub fn colour(self) -> Colour {
        let part = |n: f32| {
            let k = (n + self.hue / 60.0) % 6.0;
            let level = self.value - self.value * self.saturation * k.min(4.0 - k).clamp(0.0, 1.0);
            (level * 255.0).round().clamp(0.0, 255.0) as u8
        };
        Colour {
            red: part(5.0),
            green: part(3.0),
            blue: part(1.0),
        }
    }

    /// The hue alone, as strong and light as it is: what the field of its shades runs to.
    pub fn pure(self) -> Colour {
        Hsv {
            hue: self.hue,
            saturation: 1.0,
            value: 1.0,
        }
        .colour()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colour_offered_is_mixed_back_to_itself() {
        for colour in PALETTE.iter().chain(MARKERS.iter()) {
            assert_eq!(Hsv::of(*colour).colour(), *colour, "{}", colour.written());
        }
    }

    #[test]
    fn the_hues_go_round_as_in_the_tauri_version() {
        let mixed = |hue: f32| {
            Hsv {
                hue,
                saturation: 1.0,
                value: 1.0,
            }
            .colour()
            .written()
        };
        assert_eq!(mixed(0.0), "#ff0000");
        assert_eq!(mixed(60.0), "#ffff00");
        assert_eq!(mixed(120.0), "#00ff00");
        assert_eq!(mixed(240.0), "#0000ff");
        assert_eq!(mixed(300.0), "#ff00ff");
        // (359 is not quite red again.)
        assert_eq!(mixed(359.0), "#ff0004");
        let half = Hsv {
            hue: 200.0,
            saturation: 0.5,
            value: 0.5,
        };
        assert_eq!(half.colour().written(), "#406a80");
    }

    #[test]
    fn a_grey_has_no_hue_and_black_no_saturation() {
        let grey = Hsv::of(rgb(0x808285));
        assert!(grey.saturation > 0.0);
        let white = Hsv::of(rgb(0xffffff));
        assert_eq!((white.hue, white.saturation, white.value), (0.0, 0.0, 1.0));
        let black = Hsv::of(rgb(0x000000));
        assert_eq!((black.hue, black.saturation, black.value), (0.0, 0.0, 0.0));
        assert_eq!(Hsv::of(rgb(0x3d00b8)).pure().written(), "#5500ff");
    }

    #[test]
    fn a_colour_is_written_and_read_as_rrggbb() {
        assert_eq!(rgb(0xe61b1b).written(), "#e61b1b");
        assert_eq!(Colour::parse("#E61B1B"), Some(rgb(0xe61b1b)));
        assert_eq!(Colour::parse("e61b1b"), None);
        assert_eq!(Colour::parse("#e61b1"), None);
        assert_eq!(Colour::parse("#e61b1g"), None);
    }
}
