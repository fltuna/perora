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
}

impl Part {
    const fn line(d: &'static str) -> Self {
        Self {
            d,
            ink: Ink::Line(1.3),
            colour: None,
            solid: 1.0,
            placed: None,
        }
    }

    const fn solid(d: &'static str) -> Self {
        Self {
            d,
            ink: Ink::Solid,
            colour: None,
            solid: 1.0,
            placed: None,
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

/// The parts of a tool's icon. `colour`: what it draws in, shown on its tip and in a line under it.
pub fn of_tool(kind: Kind, colour: Option<Hsla>) -> Vec<Part> {
    match kind {
        Kind::Select => vec![Part::solid("M4 2.5v10.2l2.9-2.6 1.9 4.1 1.6-.7-1.9-4h3.8z")],
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

/// The line under a tool, in the colour it draws in.
fn underline(colour: Hsla) -> Part {
    Part {
        d: "M2.5 14.6h11",
        ink: Ink::Line(2.0),
        colour: Some(colour),
        solid: 1.0,
        placed: None,
    }
}

/// An element that draws the icon at `side` logical pixels, in `colour` where a part names none.
pub fn icon(parts: Vec<Part>, side: f32, colour: Hsla) -> impl gpui::IntoElement {
    use gpui::Styled;
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint(window, bounds, &parts, colour),
    )
    .size(px(side))
    .flex_none()
}

fn paint(window: &mut Window, bounds: Bounds<Pixels>, parts: &[Part], colour: Hsla) {
    let scale = bounds.size.width.as_f32() / 16.0;
    let origin = bounds.origin;
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
                    .with_line_cap(LineCap::Round)
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
