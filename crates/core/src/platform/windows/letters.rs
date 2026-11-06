//! Windows: letters as shapes. What is written on a capture is drawn as the shapes of its letters,
//! not as pictures of them: at any size it is looked at, and with a line around it (GPUI only draws
//! letters as pictures, docs/gpui-rewrite.md 「テキストの縁取り」). DirectWrite lays a line out
//! (with the system's fonts for letters the font has not) and gives the outline of each letter.
//!
//! The outlines come flattened into polygons, in pixels at the size asked for, each line from its
//! start (x 0) with its baseline at y 0.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN, D2D1_FIGURE_END, D2D1_FILL_MODE, D2D1_PATH_SEGMENT,
    ID2D1SimplifiedGeometrySink, ID2D1SimplifiedGeometrySink_Impl,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_METRICS, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_GLYPH_RUN, DWRITE_GLYPH_RUN_DESCRIPTION, DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS,
    DWRITE_MATRIX, DWRITE_MEASURING_MODE, DWRITE_STRIKETHROUGH, DWRITE_TEXT_METRICS,
    DWRITE_UNDERLINE, DWriteCreateFactory, IDWriteFactory, IDWriteInlineObject,
    IDWritePixelSnapping_Impl, IDWriteTextRenderer, IDWriteTextRenderer_Impl,
};
use windows::core::{BOOL, HSTRING, IUnknown, Ref, Result, implement};
use windows_numerics::Vector2;

/// A line laid out: how far it goes (as far as its letters advance, spaces at its end too) and the
/// outlines of its letters.
#[derive(Debug, Clone, Default)]
pub struct Line {
    pub width: f32,
    pub outlines: Vec<Vec<[f32; 2]>>,
    /// Where a caret is before each letter (by UTF-16 unit) and after the last.
    pub carets: Vec<f32>,
}

/// Lines laid out in one font, and how far the font reaches over and under the baseline (pixels at
/// the size): what puts the middle of a line where it is asked for.
#[derive(Debug, Clone, Default)]
pub struct Laid {
    pub lines: Vec<Line>,
    pub ascent: f32,
    pub descent: f32,
}

thread_local! {
    static FACTORY: Option<IDWriteFactory> =
        unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }.ok();
}

/// Lays out each of `lines` in the first of `families` the system has (or its default font), at
/// `size` pixels; curves flattened to within `flat` pixels. None when DirectWrite cannot be had.
pub fn lay_out(
    lines: &[&str],
    families: &[&str],
    size: f32,
    bold: bool,
    italic: bool,
    flat: f32,
) -> Option<Laid> {
    FACTORY.with(|factory| {
        let factory = factory.as_ref()?;
        unsafe { laid(factory, lines, families, size, bold, italic, flat) }.ok()
    })
}

#[allow(clippy::too_many_arguments)]
unsafe fn laid(
    factory: &IDWriteFactory,
    lines: &[&str],
    families: &[&str],
    size: f32,
    bold: bool,
    italic: bool,
    flat: f32,
) -> Result<Laid> {
    let weight = if bold {
        DWRITE_FONT_WEIGHT_BOLD
    } else {
        DWRITE_FONT_WEIGHT_NORMAL
    };
    let style = if italic {
        DWRITE_FONT_STYLE_ITALIC
    } else {
        DWRITE_FONT_STYLE_NORMAL
    };
    let mut collection = None;
    unsafe { factory.GetSystemFontCollection(&mut collection, false)? };
    let collection = collection.ok_or_else(windows::core::Error::empty)?;
    // The first family that is there: as a page's list of fonts goes.
    let mut family = None;
    for name in families {
        let (mut index, mut exists) = (0u32, BOOL(0));
        unsafe { collection.FindFamilyName(&HSTRING::from(*name), &mut index, &mut exists)? };
        if exists.as_bool() {
            family = Some(((*name).to_string(), index));
            break;
        }
    }
    let (name, index) = family.unwrap_or_else(|| ("Segoe UI".into(), 0));
    let mut metrics = DWRITE_FONT_METRICS::default();
    if let Ok(found) = unsafe { collection.GetFontFamily(index) }
        && let Ok(font) =
            unsafe { found.GetFirstMatchingFont(weight, DWRITE_FONT_STRETCH_NORMAL, style) }
    {
        unsafe { font.GetMetrics(&mut metrics) };
    }
    let per_em = f32::from(metrics.designUnitsPerEm.max(1));
    let format = unsafe {
        factory.CreateTextFormat(
            &HSTRING::from(name.as_str()),
            None,
            weight,
            style,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            &HSTRING::from("ja-jp"),
        )?
    };
    let shared = Rc::new(Collected {
        flat,
        ..Collected::default()
    });
    let sink: ID2D1SimplifiedGeometrySink = Sink {
        collected: shared.clone(),
    }
    .into();
    let renderer: IDWriteTextRenderer = Renderer {
        collected: shared.clone(),
        sink,
    }
    .into();
    let mut laid = Vec::with_capacity(lines.len());
    for line in lines {
        let text: Vec<u16> = line.encode_utf16().collect();
        if text.is_empty() {
            laid.push(Line {
                carets: vec![0.0],
                ..Line::default()
            });
            continue;
        }
        let layout =
            unsafe { factory.CreateTextLayout(&text, &format, f32::MAX / 4.0, f32::MAX / 4.0)? };
        let mut measured = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut measured)? };
        let mut line_metrics = [DWRITE_LINE_METRICS::default(); 1];
        let mut count = 0u32;
        // (One line: it is not broken, it is as wide as it goes.)
        let _ = unsafe { layout.GetLineMetrics(Some(&mut line_metrics), &mut count) };
        shared.baseline.set(line_metrics[0].baseline);
        shared.outlines.borrow_mut().clear();
        unsafe { layout.Draw(None, &renderer, 0.0, 0.0)? };
        let carets = (0..=text.len() as u32)
            .map(|at| {
                let (mut x, mut y) = (0.0, 0.0);
                let mut hit = DWRITE_HIT_TEST_METRICS::default();
                match unsafe { layout.HitTestTextPosition(at, false, &mut x, &mut y, &mut hit) } {
                    Ok(()) => x,
                    Err(_) => measured.widthIncludingTrailingWhitespace,
                }
            })
            .collect();
        laid.push(Line {
            width: measured.widthIncludingTrailingWhitespace,
            outlines: std::mem::take(&mut *shared.outlines.borrow_mut()),
            carets,
        });
    }
    Ok(Laid {
        lines: laid,
        ascent: f32::from(metrics.ascent) / per_em * size,
        descent: f32::from(metrics.descent) / per_em * size,
    })
}

/// What the renderer and the sink gather: the outlines so far, the one being made, and where the run
/// being outlined begins (its baseline at the line's baseline).
#[derive(Default)]
struct Collected {
    flat: f32,
    baseline: Cell<f32>,
    origin: Cell<(f32, f32)>,
    outlines: RefCell<Vec<Vec<[f32; 2]>>>,
    figure: RefCell<Vec<[f32; 2]>>,
}

impl Collected {
    fn at(&self, point: &Vector2) -> [f32; 2] {
        let (x, y) = self.origin.get();
        [x + point.X, y + point.Y]
    }
}

#[implement(IDWriteTextRenderer)]
struct Renderer {
    collected: Rc<Collected>,
    sink: ID2D1SimplifiedGeometrySink,
}

impl IDWritePixelSnapping_Impl for Renderer_Impl {
    fn IsPixelSnappingDisabled(&self, _: *const core::ffi::c_void) -> Result<BOOL> {
        Ok(BOOL(1))
    }

    fn GetCurrentTransform(
        &self,
        _: *const core::ffi::c_void,
        transform: *mut DWRITE_MATRIX,
    ) -> Result<()> {
        if !transform.is_null() {
            unsafe {
                *transform = DWRITE_MATRIX {
                    m11: 1.0,
                    m12: 0.0,
                    m21: 0.0,
                    m22: 1.0,
                    dx: 0.0,
                    dy: 0.0,
                };
            }
        }
        Ok(())
    }

    fn GetPixelsPerDip(&self, _: *const core::ffi::c_void) -> Result<f32> {
        Ok(1.0)
    }
}

impl IDWriteTextRenderer_Impl for Renderer_Impl {
    fn DrawGlyphRun(
        &self,
        _: *const core::ffi::c_void,
        x: f32,
        y: f32,
        _: DWRITE_MEASURING_MODE,
        run: *const DWRITE_GLYPH_RUN,
        _: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        let Some(run) = (unsafe { run.as_ref() }) else {
            return Ok(());
        };
        let Some(face) = run.fontFace.as_ref() else {
            return Ok(());
        };
        self.collected
            .origin
            .set((x, y - self.collected.baseline.get()));
        unsafe {
            face.GetGlyphRunOutline(
                run.fontEmSize,
                run.glyphIndices,
                (!run.glyphAdvances.is_null()).then_some(run.glyphAdvances),
                (!run.glyphOffsets.is_null()).then_some(run.glyphOffsets),
                run.glyphCount,
                run.isSideways.as_bool(),
                run.bidiLevel % 2 == 1,
                &self.sink,
            )
        }
    }

    fn DrawUnderline(
        &self,
        _: *const core::ffi::c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_UNDERLINE,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _: *const core::ffi::c_void,
        _: f32,
        _: f32,
        _: *const DWRITE_STRIKETHROUGH,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        Ok(())
    }

    fn DrawInlineObject(
        &self,
        _: *const core::ffi::c_void,
        _: f32,
        _: f32,
        _: Ref<IDWriteInlineObject>,
        _: BOOL,
        _: BOOL,
        _: Ref<IUnknown>,
    ) -> Result<()> {
        Ok(())
    }
}

#[implement(ID2D1SimplifiedGeometrySink)]
struct Sink {
    collected: Rc<Collected>,
}

impl ID2D1SimplifiedGeometrySink_Impl for Sink_Impl {
    fn SetFillMode(&self, _: D2D1_FILL_MODE) {}

    fn SetSegmentFlags(&self, _: D2D1_PATH_SEGMENT) {}

    fn BeginFigure(&self, start: &Vector2, _: D2D1_FIGURE_BEGIN) {
        let at = self.collected.at(start);
        *self.collected.figure.borrow_mut() = vec![at];
    }

    fn AddLines(&self, points: *const Vector2, count: u32) {
        if points.is_null() {
            return;
        }
        let points = unsafe { std::slice::from_raw_parts(points, count as usize) };
        let mut figure = self.collected.figure.borrow_mut();
        figure.extend(points.iter().map(|point| self.collected.at(point)));
    }

    fn AddBeziers(&self, beziers: *const D2D1_BEZIER_SEGMENT, count: u32) {
        if beziers.is_null() {
            return;
        }
        let beziers = unsafe { std::slice::from_raw_parts(beziers, count as usize) };
        let mut figure = self.collected.figure.borrow_mut();
        for bezier in beziers {
            let from = figure.last().copied().unwrap_or([0.0, 0.0]);
            let (one, two, to) = (
                self.collected.at(&bezier.point1),
                self.collected.at(&bezier.point2),
                self.collected.at(&bezier.point3),
            );
            flatten(from, one, two, to, self.collected.flat, &mut figure);
        }
    }

    fn EndFigure(&self, _: D2D1_FIGURE_END) {
        let figure = std::mem::take(&mut *self.collected.figure.borrow_mut());
        if figure.len() >= 3 {
            self.collected.outlines.borrow_mut().push(figure);
        }
    }

    fn Close(&self) -> Result<()> {
        Ok(())
    }
}

/// A cubic Bézier curve as straight pieces within `flat` of it, after `from` (which is there
/// already): as many as its control points' distance from the chord asks for.
fn flatten(
    from: [f32; 2],
    one: [f32; 2],
    two: [f32; 2],
    to: [f32; 2],
    flat: f32,
    out: &mut Vec<[f32; 2]>,
) {
    let away = |point: [f32; 2]| {
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let length = dx.hypot(dy);
        if length < f32::EPSILON {
            return (point[0] - from[0]).hypot(point[1] - from[1]);
        }
        ((point[0] - from[0]) * dy - (point[1] - from[1]) * dx).abs() / length
    };
    let bulge = away(one).max(away(two));
    let pieces = ((bulge / flat.max(1e-3)).sqrt().ceil() as usize).clamp(1, 256);
    for step in 1..=pieces {
        let t = step as f32 / pieces as f32;
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push([
            a * from[0] + b * one[0] + c * two[0] + d * to[0],
            a * from[1] + b * one[1] + c * two[1] + d * to[1],
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_laid_out_with_its_letters_outlined() {
        let Some(laid) = lay_out(
            &["A1", "", "あ"],
            &["No Such Font", "Segoe UI"],
            100.0,
            true,
            false,
            0.1,
        ) else {
            return;
        };
        assert_eq!(laid.lines.len(), 3);
        assert!(laid.ascent > 50.0 && laid.descent > 10.0, "{laid:?}");
        let first = &laid.lines[0];
        assert!(first.width > 80.0 && first.width < 200.0, "{}", first.width);
        assert!(!first.outlines.is_empty());
        // Over the baseline (letters go up from it) and from the start of the line.
        let (top, bottom) = first
            .outlines
            .iter()
            .flatten()
            .fold((f32::MAX, f32::MIN), |(top, bottom), at| {
                (top.min(at[1]), bottom.max(at[1]))
            });
        assert!(top < -50.0 && bottom <= 1.0, "{top} {bottom}");
        assert!(laid.lines[1].outlines.is_empty() && laid.lines[1].width == 0.0);
        // A caret before each letter and after the last, going right.
        assert_eq!(first.carets.len(), 3);
        assert!(first.carets[0] == 0.0 && first.carets[1] > 0.0 && first.carets[2] > first.carets[1]);
        assert_eq!(laid.lines[1].carets, vec![0.0]);
        // A letter the font has not is had from another.
        assert!(!laid.lines[2].outlines.is_empty());
    }
}
