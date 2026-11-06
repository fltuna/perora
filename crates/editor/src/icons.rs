//! The editor's own icons: the Tauri version's SVG drawings (`ToolIcon` in `editor.tsx`, 16 x 16,
//! lines 1.3 wide with round ends), read with lyon's parser and drawn as GPUI paths, so that they look
//! as they did and can take the colour a tool draws in.

use gpui::{Bounds, Hsla, PathBuilder, Pixels, Window, canvas, px};
use lyon::extra::parser::{ParserOptions, PathParser, Source};
use lyon::math::Transform;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};

use crate::tools::Kind;

/// How a part of an icon is drawn.
#[derive(Debug, Clone, Copy)]
pub enum Ink {
    /// A line this wide (in the icon's sixteenths).
    Line(f32),
    Solid,
}

/// One part of an icon: its SVG path, how it is drawn, in which colour (none: the icon's own), how
/// solid, and moved and made smaller (across, down, how large) within the icon.
#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub d: &'static str,
    pub ink: Ink,
    pub colour: Option<Hsla>,
    pub solid: f32,
    pub placed: Option<(f32, f32, f32)>,
    /// A line ends square where it ends (a marker's), not round.
    pub butt: bool,
}

impl Part {
    const fn line(d: &'static str) -> Self {
        Self {
            d,
            ink: Ink::Line(1.3),
            colour: None,
            solid: 1.0,
            placed: None,
            butt: false,
        }
    }

    const fn solid(d: &'static str) -> Self {
        Self {
            d,
            ink: Ink::Solid,
            colour: None,
            solid: 1.0,
            placed: None,
            butt: false,
        }
    }

    fn placed(self, placed: (f32, f32, f32)) -> Self {
        Self {
            placed: Some(placed),
            ..self
        }
    }

    fn in_colour(self, colour: Hsla) -> Self {
        Self {
            colour: Some(colour),
            ..self
        }
    }
}

/// The pen drawn smaller, to leave the line under it its room (`withLine`).
const PEN_PLACED: (f32, f32, f32) = (1.5, 0.0, 0.82);

/// The parts of a tool's icon. `colour`: what it draws in, shown on its tip and in a line under it
/// (or as the outline of a shape); `inside`: a shape's inside, its colour and how solid it is.
pub fn of_tool(kind: Kind, colour: Option<Hsla>, inside: Option<(Hsla, f32)>) -> Vec<Part> {
    let mut parts = shaped(kind, colour);
    if let (Some((fill, solid)), Kind::Rectangle | Kind::Ellipse) = (inside, kind) {
        let outline = parts[0];
        parts.insert(
            0,
            Part {
                solid,
                ..Part::solid(outline.d).in_colour(fill)
            },
        );
    }
    parts
}

fn shaped(kind: Kind, colour: Option<Hsla>) -> Vec<Part> {
    match kind {
        Kind::Select => vec![Part::solid("M4 2.5v10.2l2.9-2.6 1.9 4.1 1.6-.7-1.9-4h3.8z")],
        Kind::Line => vec![tinted(Part::line("M3 13L13 3"), colour)],
        Kind::Arrow => vec![tinted(Part::line("M3 13L12.5 3.5M7 3.2h5.8V9"), colour)],
        Kind::Rectangle => vec![tinted(
            Part::line(
                "M3 3.5h10a.5.5 0 0 1 .5.5v8a.5.5 0 0 1-.5.5H3a.5.5 0 0 1-.5-.5V4a.5.5 0 0 1 .5-.5z",
            ),
            colour,
        )],
        Kind::Ellipse => vec![tinted(
            Part::line("M2.5 8a5.5 4.5 0 1 0 11 0a5.5 4.5 0 1 0-11 0z"),
            colour,
        )],
        Kind::Stamp => match colour {
            // The disc in the colour, the 1 in black or white on it.
            Some(colour) => vec![
                Part::solid("M2 8a6 6 0 1 0 12 0a6 6 0 1 0-12 0z").in_colour(colour),
                Part::line("M6.9 6.3L8.4 5.2v5.6").in_colour(on(colour)),
            ],
            None => vec![
                Part::line("M2 8a6 6 0 1 0 12 0a6 6 0 1 0-12 0z"),
                Part::line("M6.9 6.3L8.4 5.2v5.6"),
            ],
        },
        Kind::Marker => {
            let mut parts = vec![
                Part::line("M5.2 9.2l5.5-6 2.6 2.4-5.5 6zM5.2 9.2l-1.4 3 3.9-.7")
                    .placed(PEN_PLACED),
            ];
            if let Some(colour) = colour {
                parts.push(
                    Part::solid("M5.2 9.2l-1.4 3 3.9-.7z")
                        .placed(PEN_PLACED)
                        .in_colour(colour),
                );
                parts.push(underline(colour));
            }
            parts
        }
        Kind::Pen => {
            let mut parts = vec![
                Part::line(
                    "M2.5 13.5l.8-3.2 7.6-7.6a1.4 1.4 0 0 1 2 0l.4.4a1.4 1.4 0 0 1 0 2l-7.6 7.6zM9.8 3.8l2.4 2.4",
                )
                .placed(PEN_PLACED),
            ];
            if let Some(colour) = colour {
                parts.push(
                    Part::solid("M2.5 13.5l.8-3.2 2.4 2.4z")
                        .placed(PEN_PLACED)
                        .in_colour(colour),
                );
                parts.push(underline(colour));
            }
            parts
        }
    }
}

/// Black on a light colour, white on a dark one (`isLight` in editor.tsx).
fn on(colour: Hsla) -> Hsla {
    let rgb = colour.to_rgb();
    if rgb.r * 255.0 * 0.299 + rgb.g * 255.0 * 0.587 + rgb.b * 255.0 * 0.114 > 160.0 {
        gpui::black()
    } else {
        gpui::white()
    }
}

/// A part in the colour the tool draws in, when there is one.
fn tinted(part: Part, colour: Option<Hsla>) -> Part {
    match colour {
        Some(colour) => part.in_colour(colour),
        None => part,
    }
}

/// The shapes and lines, as one button of the first bar (`ShapesIcon`): a circle and a square, in
/// their outline colour, with their inside (its colour and how solid it is) when they have one.
pub fn shapes(outline: Hsla, inside: Option<(Hsla, f32)>) -> Vec<Part> {
    const CIRCLE: &str = "M2.3 6.2a3.9 3.9 0 1 0 7.8 0a3.9 3.9 0 1 0-7.8 0z";
    const SQUARE: &str =
        "M8.2 7.2h4.6a1 1 0 0 1 1 1v4.6a1 1 0 0 1-1 1H8.2a1 1 0 0 1-1-1V8.2a1 1 0 0 1 1-1z";
    let mut parts = Vec::new();
    for d in [CIRCLE, SQUARE] {
        if let Some((colour, solid)) = inside {
            parts.push(Part {
                solid,
                ..Part::solid(d).in_colour(colour)
            });
        }
        parts.push(Part {
            ink: Ink::Line(1.5),
            ..Part::line(d).in_colour(outline)
        });
    }
    parts
}

/// The line under a tool, in the colour it draws in.
fn underline(colour: Hsla) -> Part {
    Part {
        d: "M2.5 14.6h11",
        ink: Ink::Line(2.0),
        colour: Some(colour),
        solid: 1.0,
        placed: None,
        butt: false,
    }
}

/// An element that draws the icon at `side` logical pixels, in `colour` where a part names none.
pub fn icon(parts: Vec<Part>, side: f32, colour: Hsla) -> impl gpui::IntoElement {
    use gpui::Styled;
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint(window, bounds, (16.0, 16.0), &parts, colour),
    )
    .size(px(side))
    .flex_none()
}

/// An element that draws parts made for a box `view` across and down (an SVG's `viewBox`), as large
/// as it fits in the element and in its middle (as an SVG does by default). Its size is the caller's.
pub fn drawing(parts: Vec<Part>, view: (f32, f32), colour: Hsla) -> gpui::Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint(window, bounds, view, &parts, colour),
    )
}

/// A stroke as wide as it would be drawn, to show a size by (`Size`): `d` in a box 240 by 64, as
/// wide as `width` there, in `colour` as solid as `solid`.
pub fn sample(d: &'static str, width: f32, colour: Hsla, solid: f32, butt: bool) -> Part {
    Part {
        ink: Ink::Line(width),
        colour: Some(colour),
        solid,
        butt,
        ..Part::line(d)
    }
}

/// The mark that there is more: a small V (`Caret`), 8 by 5.
pub fn caret(colour: Hsla) -> impl gpui::IntoElement {
    caret_turned(colour, false)
}

/// The caret, turned over (`up`) when what it opens is open.
pub fn caret_turned(colour: Hsla, up: bool) -> impl gpui::IntoElement {
    use gpui::Styled;
    let part = Part {
        ink: Ink::Line(1.2),
        ..Part::line(if up { "M1 4l3-3 3 3" } else { "M1 1l3 3 3-3" })
    };
    drawing(vec![part], (8.0, 5.0), colour)
        .w(px(8.0))
        .h(px(5.0))
        .flex_none()
}

fn paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    view: (f32, f32),
    parts: &[Part],
    colour: Hsla,
) {
    let (wide, tall) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
    let scale = (wide / view.0).min(tall / view.1);
    let origin = gpui::point(
        bounds.origin.x + px((wide - view.0 * scale) / 2.0),
        bounds.origin.y + px((tall - view.1 * scale) / 2.0),
    );
    for part in parts {
        let Some(path) = read(part.d) else {
            continue;
        };
        let mut transform = Transform::identity();
        if let Some((across, down, by)) = part.placed {
            transform = transform
                .then_scale(by, by)
                .then_translate((across, down).into());
        }
        let transform = transform
            .then_scale(scale, scale)
            .then_translate((origin.x.as_f32(), origin.y.as_f32()).into());
        let path = path.transformed(&transform);
        let mut buffers: VertexBuffers<lyon::math::Point, u16> = VertexBuffers::new();
        let tessellated = match part.ink {
            Ink::Line(width) => StrokeTessellator::new().tessellate_path(
                &path,
                &StrokeOptions::default()
                    .with_line_width(width * scale)
                    .with_line_cap(if part.butt {
                        LineCap::Butt
                    } else {
                        LineCap::Round
                    })
                    .with_line_join(LineJoin::Round)
                    .with_tolerance(0.02),
                &mut BuffersBuilder::new(&mut buffers, |vertex: StrokeVertex| vertex.position()),
            ),
            Ink::Solid => FillTessellator::new().tessellate_path(
                &path,
                &FillOptions::default().with_tolerance(0.02),
                &mut BuffersBuilder::new(&mut buffers, |vertex: FillVertex| vertex.position()),
            ),
        };
        if tessellated.is_err() || buffers.vertices.is_empty() {
            continue;
        }
        let ink = part.colour.unwrap_or(colour);
        window.paint_path(
            PathBuilder::build_path(buffers),
            Hsla {
                a: ink.a * part.solid,
                ..ink
            },
        );
    }
}

fn read(d: &str) -> Option<Path> {
    let mut builder = Path::builder();
    let mut source = Source::new(d.chars());
    PathParser::new()
        .parse(&ParserOptions::DEFAULT, &mut source, &mut builder)
        .ok()?;
    Some(builder.build())
}
