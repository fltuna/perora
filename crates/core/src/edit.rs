//! What the user draws on a capture before it goes anywhere: lines, arrows, shapes, strokes of a pen;
//! what they write on it and number it with; what they make unreadable (blurred, or broken into
//! blocks); and what they cut it down to.
//!
//! The drawing is a list of edits, each in the pixels of the image (not of the screen it is shown
//! on, which scales it). The window that shows the capture draws the list over the picture, and it
//! is the window that makes the image that leaves: the capture with the list drawn into it, as it
//! shows it. Nothing is drawn here. The list is kept here for the window, which asks for it when it
//! shows the capture anew, and to tell whether an image made earlier still shows the drawing as it
//! is.

use serde::{Deserialize, Serialize};

/// A drawing is no longer than this: so many things drawn, so many places in all. The window lets
/// no more be drawn (`MOST` in `src/editor.tsx`: keep the two in step).
const MOST_EDITS: usize = 2_000;
const MOST_POINTS: usize = 200_000;

/// No more parts than this are made unreadable in one drawing. (The window lets no more be made:
/// `MOST.hidden` in `src/editor.tsx`.)
const MOST_HIDDEN: usize = 64;

/// All that is written in one drawing is no longer than this, in bytes. (`MOST.written` in
/// `src/editor.tsx`, which counts letters: no more of those than a third of this.)
const LONGEST_TEXT: usize = 60_000;

/// Wholly solid: what the inside of letters is when nothing else is said.
fn solid() -> f32 {
    1.0
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_solid(fill: &f32) -> bool {
    *fill == 1.0
}

/// Not so: what goes without saying of letters that are not bold, slanted, struck through.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_not(so: &bool) -> bool {
    !*so
}

/// The name of a font is no longer than this, in bytes. (The window cuts one off well before.)
const LONGEST_FONT: usize = 256;

/// A place in the image, in its pixels from its top-left corner: across, then down.
pub type Place = [f32; 2];

/// A colour as the window names it: `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Colour {
    fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|digit| digit.is_ascii_hexdigit()) {
            return None;
        }
        let part = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        Some(Self {
            red: part(0)?,
            green: part(2)?,
            blue: part(4)?,
        })
    }
}

impl Serialize for Colour {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!(
            "#{:02x}{:02x}{:02x}",
            self.red, self.green, self.blue
        ))
    }
}

impl<'de> Deserialize<'de> for Colour {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("a colour is written #rrggbb"))
    }
}

/// One thing drawn on the capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Edit {
    /// A stroke of the pen, through these places.
    Pen {
        points: Vec<Place>,
        color: Colour,
        width: f32,
    },
    /// A stroke of the marker: wide, and what is under it shows through.
    Marker {
        points: Vec<Place>,
        color: Colour,
        width: f32,
    },
    Line {
        from: Place,
        to: Place,
        color: Colour,
        width: f32,
    },
    /// A line with a head at `to`.
    Arrow {
        from: Place,
        to: Place,
        color: Colour,
        width: f32,
    },
    /// A rectangle between two opposite corners. `color` is its outline's; `fill` is how solid its
    /// inside is, from 0 (only the outline) to 1, and `fillColor` the colour of that (the outline's
    /// when none is named).
    Rectangle {
        from: Place,
        to: Place,
        color: Colour,
        width: f32,
        #[serde(default)]
        fill: f32,
        #[serde(default, rename = "fillColor", skip_serializing_if = "Option::is_none")]
        fill_color: Option<Colour>,
    },
    /// The ellipse that fits between two opposite corners.
    Ellipse {
        from: Place,
        to: Place,
        color: Colour,
        width: f32,
        #[serde(default)]
        fill: f32,
        #[serde(default, rename = "fillColor", skip_serializing_if = "Option::is_none")]
        fill_color: Option<Colour>,
    },
    /// Something written, between two opposite corners: what, in which colour, how large (the
    /// height of its letters, in pixels), and in which font (the name of its family; none for the
    /// one Perora writes in when none is chosen). `fill` is how solid the inside of its letters
    /// is, from 0 (only their outline) to 1, which it is when nothing is said; `outline` the colour
    /// of a line around them, when they have one, and `outlineWidth` how far that reaches out of
    /// them, in pixels. `bold`, `italic` and `strike` are said of letters that are bold, slanted,
    /// struck through, and not said of letters that are not.
    Text {
        from: Place,
        to: Place,
        text: String,
        color: Colour,
        size: f32,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        font: String,
        #[serde(default = "solid", skip_serializing_if = "is_solid")]
        fill: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outline: Option<Colour>,
        #[serde(
            default,
            rename = "outlineWidth",
            skip_serializing_if = "Option::is_none"
        )]
        outline_width: Option<f32>,
        #[serde(default, skip_serializing_if = "is_not")]
        bold: bool,
        #[serde(default, skip_serializing_if = "is_not")]
        italic: bool,
        #[serde(default, skip_serializing_if = "is_not")]
        strike: bool,
    },
    /// A number in a disc, between two opposite corners.
    Stamp {
        from: Place,
        to: Place,
        number: u32,
        color: Colour,
    },
    /// What is between two opposite corners, blurred as it is at that point of the drawing: what
    /// was drawn before it with it, what is drawn after it not. `radius` is how far each pixel is
    /// spread, in pixels.
    Blur { from: Place, to: Place, radius: f32 },
    /// What is between two opposite corners, broken into square blocks of one colour each, `cell`
    /// pixels wide.
    Mosaic { from: Place, to: Place, cell: f32 },
    /// What the image is cut down to, between two opposite corners. However many of these there
    /// are, the last counts, and it is done last: everything else is placed in the image as it was
    /// captured.
    Crop { from: Place, to: Place },
}

/// Whether a list of edits is one a window could have made: not longer than a drawing gets.
pub fn is_a_drawing(edits: &[Edit]) -> bool {
    let points: usize = edits
        .iter()
        .map(|edit| match edit {
            Edit::Pen { points, .. } | Edit::Marker { points, .. } => points.len(),
            _ => 2,
        })
        .sum();
    let hidden = edits
        .iter()
        .filter(|edit| matches!(edit, Edit::Blur { .. } | Edit::Mosaic { .. }))
        .count();
    let written: usize = edits
        .iter()
        .map(|edit| match edit {
            Edit::Text { text, .. } => text.len(),
            _ => 0,
        })
        .sum();
    let named = edits.iter().all(|edit| match edit {
        Edit::Text { font, .. } => font.len() <= LONGEST_FONT,
        _ => true,
    });
    edits.len() <= MOST_EDITS
        && points <= MOST_POINTS
        && hidden <= MOST_HIDDEN
        && written <= LONGEST_TEXT
        && named
}

/// How large the image is after the edits: as it was, or what the last crop among them leaves.
pub fn size_after(edits: &[Edit], width: u32, height: u32) -> (u32, u32) {
    edits
        .iter()
        .rev()
        .find_map(|edit| match edit {
            Edit::Crop { from, to } => Some(pixels_between(*from, *to, width, height)),
            _ => None,
        })
        .flatten()
        .unwrap_or((width, height))
}

/// How many pixels two opposite corners span across and down, as far as they are in an image of
/// this size: every pixel the area touches. `None` when that is none (no place named, or all of it
/// outside).
fn pixels_between(from: Place, to: Place, width: u32, height: u32) -> Option<(u32, u32)> {
    if !from.iter().chain(&to).all(|part| part.is_finite()) {
        return None;
    }
    let within = |value: f32, most: u32| value.clamp(0.0, most as f32) as u32;
    let left = within(from[0].min(to[0]).floor(), width);
    let top = within(from[1].min(to[1]).floor(), height);
    let right = within(from[0].max(to[0]).ceil(), width);
    let bottom = within(from[1].max(to[1]).ceil(), height);
    (right > left && bottom > top).then_some((right - left, bottom - top))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Colour = Colour {
        red: 255,
        green: 0,
        blue: 0,
    };

    #[test]
    fn a_drawing_comes_from_the_window_as_a_list_of_what_was_drawn() {
        let edits: Vec<Edit> = serde_json::from_str(
            r##"[
                {"kind": "pen", "points": [[1, 2], [3.5, 4]], "color": "#ff0000", "width": 3},
                {"kind": "marker", "points": [[1, 2]], "color": "#00ff00", "width": 3},
                {"kind": "line", "from": [0, 0], "to": [9, 9], "color": "#0000ff", "width": 2},
                {"kind": "arrow", "from": [0, 0], "to": [9, 9], "color": "#0000ff", "width": 2},
                {"kind": "rectangle", "from": [0, 0], "to": [9, 9], "color": "#0a0b0c", "width": 2, "fill": 0.5},
                {"kind": "ellipse", "from": [0, 0], "to": [9, 9], "color": "#0A0B0C", "width": 2},
                {"kind": "rectangle", "from": [0, 0], "to": [9, 9], "color": "#ff0000", "width": 2,
                 "fill": 1, "fillColor": "#0000ff"},
                {"kind": "text", "from": [10, 10], "to": [14, 12], "text": "あ\nb", "color": "#ff0000", "size": 24},
                {"kind": "stamp", "from": [1, 1], "to": [5, 3], "number": 3, "color": "#ff0000"},
                {"kind": "blur", "from": [1, 2], "to": [30, 40], "radius": 12},
                {"kind": "mosaic", "from": [1, 2], "to": [30, 40], "cell": 16},
                {"kind": "crop", "from": [0, 0], "to": [20.5, 10]}
            ]"##,
        )
        .unwrap();

        assert_eq!(edits.len(), 12);

        // What is written names its font when one was chosen, and says nothing of it otherwise:
        // read and written back the same way.
        let with: Vec<Edit> = serde_json::from_str(
            r##"[{"kind": "text", "from": [1, 1], "to": [9, 9], "text": "a", "color": "#ff0000",
                  "size": 30.5, "font": "Yu Mincho"}]"##,
        )
        .unwrap();
        assert!(
            matches!(&with[0], Edit::Text { font, size, .. } if font == "Yu Mincho" && *size == 30.5)
        );
        assert!(
            serde_json::to_string(&with)
                .unwrap()
                .contains(r#""font":"Yu Mincho""#)
        );
        assert!(matches!(&edits[7], Edit::Text { font, .. } if font.is_empty()));
        assert!(!serde_json::to_string(&edits[7]).unwrap().contains("font"));

        // The same for how solid its letters are and the line around them: solid and without one
        // when nothing is said, and then nothing is said of it on the way back.
        assert!(matches!(
            &edits[7],
            Edit::Text { fill, outline: None, outline_width: None, .. } if *fill == 1.0
        ));
        let plain = serde_json::to_string(&edits[7]).unwrap();
        assert!(!plain.contains("fill") && !plain.contains("outline"));
        let edged: Vec<Edit> = serde_json::from_str(
            r##"[{"kind": "text", "from": [1, 1], "to": [9, 9], "text": "a", "color": "#ff0000",
                  "size": 30, "fill": 0, "outline": "#00ff00", "outlineWidth": 2.5}]"##,
        )
        .unwrap();
        assert!(matches!(
            &edged[0],
            Edit::Text { fill, outline: Some(_), outline_width: Some(width), .. }
                if *fill == 0.0 && *width == 2.5
        ));
        let back = serde_json::to_string(&edged).unwrap();
        assert!(
            back.contains(r##""outline":"#00ff00""##) && back.contains(r#""outlineWidth":2.5"#)
        );
        assert!(back.contains(r#""fill":0.0"#));

        // And for letters that are bold, slanted, struck through: none of it when nothing is
        // said, and each said on the way back only when it is so.
        assert!(matches!(
            &edits[7],
            Edit::Text {
                bold: false,
                italic: false,
                strike: false,
                ..
            }
        ));
        assert!(!plain.contains("bold") && !plain.contains("italic") && !plain.contains("strike"));
        let shaped: Vec<Edit> = serde_json::from_str(
            r##"[{"kind": "text", "from": [1, 1], "to": [9, 9], "text": "a", "color": "#ff0000",
                  "size": 30, "bold": true, "strike": true}]"##,
        )
        .unwrap();
        assert!(matches!(
            &shaped[0],
            Edit::Text {
                bold: true,
                italic: false,
                strike: true,
                ..
            }
        ));
        let back = serde_json::to_string(&shaped).unwrap();
        assert!(back.contains(r#""bold":true"#) && back.contains(r#""strike":true"#));
        assert!(!back.contains("italic"));
        assert_eq!(
            edits[0],
            Edit::Pen {
                points: vec![[1.0, 2.0], [3.5, 4.0]],
                color: RED,
                width: 3.0
            }
        );
        assert!(matches!(edits[4], Edit::Rectangle { fill, fill_color: None, .. } if fill == 0.5));
        assert!(matches!(edits[5], Edit::Ellipse { fill, .. } if fill == 0.0));
        assert!(matches!(
            edits[6],
            Edit::Rectangle {
                fill_color: Some(Colour {
                    red: 0,
                    green: 0,
                    blue: 255
                }),
                ..
            }
        ));
        assert!(
            matches!(&edits[7], Edit::Text { text, size, .. } if text == "あ\nb" && *size == 24.0)
        );
        assert!(matches!(&edits[8], Edit::Stamp { number: 3, .. }));
        assert_eq!(
            edits[9..],
            [
                Edit::Blur {
                    from: [1.0, 2.0],
                    to: [30.0, 40.0],
                    radius: 12.0
                },
                Edit::Mosaic {
                    from: [1.0, 2.0],
                    to: [30.0, 40.0],
                    cell: 16.0
                },
                Edit::Crop {
                    from: [0.0, 0.0],
                    to: [20.5, 10.0]
                },
            ]
        );
        assert!(is_a_drawing(&edits));
        // And goes back as it came: the window is given what it drew when it asks.
        let again: Vec<Edit> =
            serde_json::from_str(&serde_json::to_string(&edits).unwrap()).unwrap();
        assert_eq!(again, edits);

        for not_a_colour in ["red", "#ff00", "#gg0000", "ff0000", "#ff000000"] {
            let text = format!(
                r##"[{{"kind": "line", "from": [0, 0], "to": [1, 1], "color": "{not_a_colour}", "width": 1}}]"##
            );
            assert!(
                serde_json::from_str::<Vec<Edit>>(&text).is_err(),
                "{not_a_colour}"
            );
        }
    }

    #[test]
    fn a_drawing_is_not_longer_than_drawings_get() {
        let line = Edit::Line {
            from: [0.0, 0.0],
            to: [1.0, 1.0],
            color: RED,
            width: 1.0,
        };
        assert!(is_a_drawing(&vec![line.clone(); MOST_EDITS]));
        assert!(!is_a_drawing(&vec![line; MOST_EDITS + 1]));
        assert!(!is_a_drawing(&[Edit::Pen {
            points: vec![[0.0, 0.0]; MOST_POINTS + 1],
            color: RED,
            width: 1.0,
        }]));

        let part = Edit::Blur {
            from: [0.0, 0.0],
            to: [4.0, 4.0],
            radius: 6.0,
        };
        assert!(is_a_drawing(&vec![part.clone(); MOST_HIDDEN]));
        assert!(!is_a_drawing(&vec![part; MOST_HIDDEN + 1]));

        let written = |length: usize| Edit::Text {
            from: [0.0, 0.0],
            to: [4.0, 4.0],
            text: "a".repeat(length),
            color: RED,
            size: 12.0,
            font: String::new(),
            fill: 1.0,
            outline: None,
            outline_width: None,
            bold: false,
            italic: false,
            strike: false,
        };
        assert!(is_a_drawing(&[written(LONGEST_TEXT)]));
        assert!(!is_a_drawing(&[written(LONGEST_TEXT), written(1)]));

        let named = |length: usize| Edit::Text {
            from: [0.0, 0.0],
            to: [4.0, 4.0],
            text: "a".into(),
            color: RED,
            size: 12.0,
            font: "f".repeat(length),
            fill: 1.0,
            outline: None,
            outline_width: None,
            bold: false,
            italic: false,
            strike: false,
        };
        assert!(is_a_drawing(&[named(LONGEST_FONT)]));
        assert!(!is_a_drawing(&[named(LONGEST_FONT + 1)]));
    }

    #[test]
    fn the_size_after_the_edits_is_what_the_last_crop_leaves() {
        let crop = |to: Place| Edit::Crop {
            from: [10.2, 5.0],
            to,
        };
        assert_eq!(size_after(&[], 40, 30), (40, 30));
        // Every pixel the area touches: columns 10 to 29, rows 5 to 20.
        assert_eq!(size_after(&[crop([30.0, 20.5])], 40, 30), (20, 16));
        assert_eq!(
            size_after(&[crop([30.0, 20.5]), crop([90.0, 90.0])], 40, 30),
            (30, 25)
        );
        // A crop that leaves nothing, or names no place, leaves the image as it was.
        assert_eq!(size_after(&[crop([10.2, 5.0])], 40, 30), (40, 30));
        assert_eq!(size_after(&[crop([f32::NAN, 9.0])], 40, 30), (40, 30));
    }
}
