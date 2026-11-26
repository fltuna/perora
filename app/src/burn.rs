//! The image that leaves the editor: the capture with what was drawn on it, a pixel of the image to a
//! pixel of the screen, drawn by the editor's own `draw_all` (the same drawing as on screen). A
//! hidden window (cloaked: never seen, takes no input) is drawn a tile at a time and each tile read
//! back (`render_to_image`): Windows keeps a window within about the size of a monitor, and a capture
//! may be larger than that. P0 ① of docs/gpui-rewrite.md found the way (`perora-spike-wt`,
//! `spike/gpui/src/burn.rs`): 5120 x 1440 in about 0.16 s; outside what was drawn, every pixel as it
//! was.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, AppContext, AsyncApp, Context, IntoElement, ParentElement, Render,
    RenderImage, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, canvas, div,
};
use perora_core::edit::Edit;
use perora_core::geometry::Rect;
use perora_core::platform::{self, Frame};
use perora_editor::hidden::{self, Part, Under};
use perora_editor::paint::{self, Base, Hiding, Painter};
use perora_editor::view::Area;

use crate::{display, hwnd};

/// The side asked for the hidden window, in pixels of the screen. (The system may give it less.)
const TILE: u32 = 1024;
/// How long the window is given to hear of its size before it is drawn.
const SIZED_WITHIN: Duration = Duration::from_millis(500);

/// The hidden window's view: one tile of the image.
struct Tile {
    base: Option<Base>,
    /// The tile: its corner in the image, and its size (pixels of the image).
    area: Rect,
    /// The picture (all of the image, or what it is cut down to) and what was drawn on it.
    view: Area,
    edits: Arc<Vec<Edit>>,
    /// What the parts made unreadable among them show (without it: nothing).
    hiding: Arc<Hiding>,
}

impl Render for Tile {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let base = self.base.clone();
        let area = self.area;
        let view = self.view;
        let edits = self.edits.clone();
        let hiding = self.hiding.clone();
        div().size_full().bg(gpui::black()).child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    // A pixel of the image to one of the screen, the tile's corner at the window's.
                    let painter = Painter {
                        origin: bounds.origin,
                        offset: [area.x as f32, area.y as f32],
                        scale: 1.0 / window.scale_factor(),
                    };
                    paint::draw_all(
                        window,
                        &painter,
                        view,
                        base.as_slice(),
                        &edits,
                        None,
                        &hiding,
                    );
                },
            )
            .size_full(),
        )
    }
}

/// The capture (`frame`, its pixels as taken) with `edits` drawn on it, as large as it is. In the
/// order they were drawn: a part made unreadable is made so from all that was drawn before it (drawn
/// up to it, and made unreadable here, on the pixels), and what is drawn after it is not. (P4 later:
/// cut down to what the last crop leaves.)
pub async fn burn(frame: Arc<Frame>, edits: Vec<Edit>, cx: &mut AsyncApp) -> Result<Frame, String> {
    let mut base = frame;
    let mut from = 0;
    for (index, edit) in edits.iter().enumerate() {
        if !matches!(edit, Edit::Blur { .. } | Edit::Mosaic { .. }) {
            continue;
        }
        if from < index {
            base = Arc::new(draw_over(base, edits[from..index].to_vec(), cx).await?);
        }
        base = Arc::new(hidden(&base, edit));
        from = index + 1;
    }
    if from < edits.len() {
        base = Arc::new(draw_over(base, edits[from..].to_vec(), cx).await?);
    }
    // Cut down last, to what the last crop leaves (the other edits keep the whole image's places).
    let whole = Area::new(0.0, 0.0, base.width() as f32, base.height() as f32);
    if let Some(kept) = perora_editor::editor::kept_of(&edits, whole)
        && kept != whole
    {
        base = Arc::new(cut_to(&base, kept));
    }
    Ok(Arc::try_unwrap(base).unwrap_or_else(|shared| Frame {
        area: shared.area,
        rgba: shared.rgba.clone(),
    }))
}

/// The image cut down to a part of it (whole pixels).
fn cut_to(frame: &Frame, kept: Area) -> Frame {
    let part = Part {
        x: kept.x as u32,
        y: kept.y as u32,
        width: kept.width as u32,
        height: kept.height as u32,
    };
    Frame {
        area: Rect::new(0, 0, part.width, part.height),
        rgba: hidden::cut(&frame.rgba, frame.width(), part),
    }
}

/// A part of the image made unreadable, on its pixels as they are drawn so far.
fn hidden(frame: &Frame, edit: &Edit) -> Frame {
    let (width, height) = (frame.width(), frame.height());
    let mut rgba = frame.rgba.clone();
    let area = perora_editor::marks::box_of(edit);
    if let Some(part) = Part::of(area, width, height) {
        let cut = hidden::cut(&rgba, width, part);
        let done = match edit {
            Edit::Blur { radius, .. } => hidden::blurred(&cut, part.width, part.height, *radius),
            Edit::Mosaic { cell, .. } => hidden::mosaicked(&cut, part.width, part.height, *cell),
            _ => cut,
        };
        hidden::put(&mut rgba, width, part, &done);
    }
    Frame {
        area: frame.area,
        rgba,
    }
}

/// `edits` drawn over the picture (`frame`), a tile at a time.
async fn draw_over(frame: Arc<Frame>, edits: Vec<Edit>, cx: &mut AsyncApp) -> Result<Frame, String> {
    let started = Instant::now();
    let (width, height) = (frame.width(), frame.height());
    let drawer = Drawer::open(Area::new(0.0, 0.0, width as f32, height as f32), cx).await?;
    let whole = Rect::new(0, 0, width, height);
    let result = drawer
        .draw(&frame, Arc::new(edits), Arc::default(), whole, cx)
        .await;
    drawer.close(Vec::new(), cx);
    if let Ok(frame) = &result {
        eprintln!(
            "burn: {}x{} drawn in {:.0} ms",
            frame.width(),
            frame.height(),
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    result
}

/// Whether anything is under a part made unreadable among `edits` but the picture: something is
/// drawn before one, or before the one to be drawn next when it is `held` (a crop draws nothing).
pub fn needs_under(edits: &[Edit], held: &[usize]) -> bool {
    let before_one = edits.iter().enumerate().any(|(index, edit)| {
        matches!(edit, Edit::Blur { .. } | Edit::Mosaic { .. }) && drawn_before(&edits[..index])
    });
    before_one || (held.contains(&edits.len()) && drawn_before(edits))
}

/// Whether any of these draws something (a crop draws nothing).
fn drawn_before(edits: &[Edit]) -> bool {
    edits
        .iter()
        .any(|before| !matches!(before, Edit::Crop { .. }))
}

/// What was drawn under a part made unreadable: given again, not drawn again, while all that is
/// drawn before it and where it is drawn stay as they are (of the same capture).
pub struct Drawn {
    before: Vec<Edit>,
    at: Part,
    /// Blue first.
    pixels: Arc<Vec<u8>>,
}

/// What is under each part made unreadable among `edits` in the editor, as the image that leaves has
/// it there: all drawn before it, over the capture (`frame`), parts made unreadable before it made
/// so; drawn as `burn` draws, only where the part is. Those `held` (by their place; one after all of
/// `edits`: the one to be drawn next, under which is all of them), for all of the image. For those
/// with something drawn before them (under the rest, the picture is what there is). What was
/// `drawn` before is given again where it is as it was; what is drawn now comes back with them.
pub async fn under(
    frame: Arc<Frame>,
    edits: Vec<Edit>,
    held: Vec<usize>,
    drawn: Vec<Drawn>,
    cx: &mut AsyncApp,
) -> Result<(Vec<Under>, Vec<Drawn>), String> {
    let started = Instant::now();
    let (width, height) = (frame.width(), frame.height());
    let whole = Part {
        x: 0,
        y: 0,
        width,
        height,
    };
    let next = edits.len();
    let mut parts: Vec<Option<Part>> = edits
        .iter()
        .map(|edit| match edit {
            Edit::Blur { .. } | Edit::Mosaic { .. } => {
                Part::of(perora_editor::marks::box_of(edit), width, height)
            }
            _ => None,
        })
        .collect();
    if held.contains(&next) {
        parts.push(Some(whole));
    }
    // Where each is drawn: where the part is, or all of the image for one held.
    let ats: Vec<Option<Part>> = parts
        .iter()
        .enumerate()
        .map(|(index, part)| part.map(|part| if held.contains(&index) { whole } else { part }))
        .collect();
    // Those to draw now: something is under them, and it was not drawn so before.
    let to_draw: Vec<bool> = ats
        .iter()
        .enumerate()
        .map(|(index, at)| {
            let before = &edits[..index];
            at.is_some_and(|at| {
                drawn_before(before)
                    && !drawn.iter().any(|was| was.at == at && was.before == before)
            })
        })
        .collect();
    let mut hiding = Hiding::new();
    let mut unders = Vec::new();
    let mut kept = Vec::new();
    let mut drawer: Option<Drawer> = None;
    let mut failed = None;
    let mut drawn_now = 0;
    for index in 0..parts.len() {
        let (Some(part), Some(at)) = (parts[index], ats[index]) else {
            continue;
        };
        let before = &edits[..index];
        let pixels = if !drawn_before(before) {
            None
        } else if !to_draw[index] {
            drawn
                .iter()
                .find(|was| was.at == at && was.before == before)
                .map(|was| was.pixels.clone())
        } else {
            if drawer.is_none() {
                match Drawer::open(Area::new(0.0, 0.0, width as f32, height as f32), cx).await {
                    Ok(opened) => drawer = Some(opened),
                    Err(error) => {
                        failed = Some(error);
                        break;
                    }
                }
            }
            let Some(drawing) = drawer.as_ref() else {
                break;
            };
            let region = Rect::new(at.x as i32, at.y as i32, at.width, at.height);
            let edits = Arc::new(before.to_vec());
            match drawing
                .draw(&frame, edits, Arc::new(hiding.clone()), region, cx)
                .await
            {
                Ok(drawn) => {
                    drawn_now += 1;
                    let rgba = drawn.rgba;
                    let pixels = cx
                        .background_executor()
                        .spawn(async move { blue_first(rgba) })
                        .await;
                    Some(Arc::new(pixels))
                }
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            }
        };
        if let Some(pixels) = &pixels {
            kept.push(Drawn {
                before: before.to_vec(),
                at,
                pixels: pixels.clone(),
            });
            // (As the editor counts them: each part made unreadable, in the image or not.)
            let rank = before
                .iter()
                .filter(|edit| matches!(edit, Edit::Blur { .. } | Edit::Mosaic { .. }))
                .count();
            unders.push(Under {
                index,
                rank,
                part: at,
                pixels: pixels.clone(),
            });
        }
        // As it is made, for one drawn now after it, where that is drawn. (Made beside the window.)
        let under_later = (index + 1..parts.len())
            .any(|later| to_draw[later] && ats[later].is_some_and(|at| at.meets(part)));
        if !under_later {
            continue;
        }
        let edit = edits[index].clone();
        let frame = frame.clone();
        let made = cx
            .background_executor()
            .spawn(async move {
                // Blue first: as drawn, or the picture (red first) turned. (Made unreadable a byte
                // at a time: the order does not matter to it.)
                let mine = match pixels {
                    Some(pixels) if at == part => (*pixels).clone(),
                    Some(pixels) => {
                        let within = Part {
                            x: part.x - at.x,
                            y: part.y - at.y,
                            ..part
                        };
                        hidden::cut(&pixels, at.width, within)
                    }
                    None => blue_first(hidden::cut(&frame.rgba, width, part)),
                };
                match edit {
                    Edit::Mosaic { cell, .. } => {
                        hidden::mosaicked(&mine, part.width, part.height, cell)
                    }
                    Edit::Blur { radius, .. } => {
                        hidden::blurred(&mine, part.width, part.height, radius)
                    }
                    _ => mine,
                }
            })
            .await;
        if let Some(image) = image_of(made, part.width, part.height) {
            let area = Area::new(
                part.x as f32,
                part.y as f32,
                part.width as f32,
                part.height as f32,
            );
            hiding.insert(index, Base { image, area });
        }
    }
    let images = hiding.into_values().map(|base| base.image).collect();
    if let Some(drawer) = drawer {
        drawer.close(images, cx);
    }
    match failed {
        Some(error) => Err(error),
        None => {
            if drawn_now > 0 {
                eprintln!(
                    "under: {drawn_now} of {} part(s) drawn in {:.0} ms (all of the image under: {held:?})",
                    unders.len(),
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
            Ok((unders, kept))
        }
    }
}

/// A window nobody sees (cloaked: never seen, takes no input), to draw in a tile at a time.
struct Drawer {
    window: WindowHandle<Tile>,
    any: AnyWindowHandle,
    /// How large a tile is: what the system gave the window (pixels of the screen).
    tile: (u32, u32),
}

impl Drawer {
    /// The window, open and of its size. `view`: all of the image, or what it is cut down to.
    async fn open(view: Area, cx: &mut AsyncApp) -> Result<Self, String> {
        let opened = cx.update(|cx| {
            crate::gpu::attach(cx);
            let monitor = crate::corner::monitor().ok_or("no monitor to draw on")?;
            let screen = display::of(&monitor, cx).ok_or("no monitor to draw on")?;
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(display::bounds_on(
                    &monitor,
                    Rect::new(monitor.bounds.x, monitor.bounds.y, TILE, TILE),
                ))),
                titlebar: None,
                focus: false,
                show: false,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                display_id: Some(screen.id()),
                window_background: WindowBackgroundAppearance::Opaque,
                ..Default::default()
            };
            let mut handle = None;
            let window = cx
                .open_window(options, |window, cx| {
                    handle = hwnd::of(window);
                    if let Some(handle) = handle {
                        // Never seen: a cloaked window is drawn by nobody and takes no input.
                        platform::set_cloaked(handle, true);
                        platform::without_transitions(handle);
                    }
                    cx.new(|_| Tile {
                        base: None,
                        area: Rect::new(0, 0, 1, 1),
                        view,
                        edits: Arc::default(),
                        hiding: Arc::default(),
                    })
                })
                .map_err(|error| error.to_string())?;
            let handle = handle.ok_or("the window has no handle")?;
            Ok::<_, String>((window, handle, monitor))
        })?;
        let (window, handle, monitor) = opened;
        let any: AnyWindowHandle = window.into();
        match sized(any, handle, monitor, cx).await {
            Ok(tile) => Ok(Self { window, any, tile }),
            Err(error) => {
                let _ = any.update(cx, |_, window, _| window.remove_window());
                Err(error)
            }
        }
    }

    /// `edits` drawn over the picture (`frame`) in a part of it (`region`, pixels of the image), a
    /// tile at a time; the parts made unreadable among them show what `hiding` has of them.
    async fn draw(
        &self,
        frame: &Frame,
        edits: Arc<Vec<Edit>>,
        hiding: Arc<Hiding>,
        region: Rect,
        cx: &mut AsyncApp,
    ) -> Result<Frame, String> {
        self.window
            .update(cx, |tile, _, _| {
                tile.edits = edits;
                tile.hiding = hiding;
            })
            .map_err(|error| error.to_string())?;
        let (tile_width, tile_height) = self.tile;
        let (left_edge, top_edge) = (region.x as u32, region.y as u32);
        let (right_edge, bottom_edge) = (left_edge + region.width, top_edge + region.height);
        let mut out = vec![0u8; region.width as usize * region.height as usize * 4];
        for top in (top_edge..bottom_edge).step_by(tile_height as usize) {
            for left in (left_edge..right_edge).step_by(tile_width as usize) {
                let area = Rect::new(
                    left as i32,
                    top as i32,
                    tile_width.min(right_edge - left),
                    tile_height.min(bottom_edge - top),
                );
                let base = Base {
                    image: part_of(frame, area).ok_or("the picture could not be cut into tiles")?,
                    area: Area::new(
                        area.x as f32,
                        area.y as f32,
                        area.width as f32,
                        area.height as f32,
                    ),
                };
                let image = base.image.clone();
                self.window
                    .update(cx, |tile, _, cx| {
                        tile.base = Some(base);
                        tile.area = area;
                        cx.notify();
                    })
                    .map_err(|error| error.to_string())?;
                let drawn = self
                    .any
                    .update(cx, |_, window, cx| {
                        window.draw(cx).clear(cx);
                        let drawn = window.render_to_image();
                        let _ = window.drop_image(image);
                        drawn
                    })
                    .map_err(|error| error.to_string())?;
                let drawn = drawn.map_err(|error| error.to_string())?;
                copy(&drawn, area, region, &mut out);
                // (The window's own messages, between tiles.)
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
            }
        }
        Ok(Frame {
            area: Rect::new(0, 0, region.width, region.height),
            rgba: out,
        })
    }

    /// The window closed, the GPU let go of `images` (drawn in it) first.
    fn close(self, images: Vec<Arc<RenderImage>>, cx: &mut AsyncApp) {
        let _ = self.any.update(cx, |_, window, _| {
            for image in images {
                let _ = window.drop_image(image);
            }
            window.remove_window();
        });
    }
}

/// The window, once it has heard of its size: the size of a tile (pixels of the screen).
async fn sized(
    any: AnyWindowHandle,
    handle: platform::WindowId,
    monitor: perora_core::platform::Monitor,
    cx: &mut AsyncApp,
) -> Result<(u32, u32), String> {
    // Sized here, outside any update: the size message it brings reaches GPUI through the
    // application, which an update in progress would keep busy (GPUI then draws a window of 1 x 1).
    let (tile_width, tile_height) = hwnd::behind_everything(
        handle,
        Rect::new(monitor.bounds.x, monitor.bounds.y, TILE, TILE),
    );
    let waited = Instant::now();
    loop {
        let seen = any
            .update(cx, |_, window, _| {
                let viewport = window.viewport_size();
                let scale = window.scale_factor();
                (
                    (viewport.width.as_f32() * scale).round() as u32,
                    (viewport.height.as_f32() * scale).round() as u32,
                )
            })
            .map_err(|error| error.to_string())?;
        if seen.0 >= tile_width && seen.1 >= tile_height {
            return Ok((tile_width, tile_height));
        }
        if waited.elapsed() > SIZED_WITHIN {
            return Err(format!(
                "the window to draw in did not get its size ({}x{} of {tile_width}x{tile_height})",
                seen.0, seen.1
            ));
        }
        cx.background_executor()
            .timer(Duration::from_millis(8))
            .await;
    }
}

/// Pixels red first made blue first (as GPUI has pictures), or back.
fn blue_first(mut pixels: Vec<u8>) -> Vec<u8> {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    pixels
}

/// A picture for GPUI, of pixels blue first.
fn image_of(pixels: Vec<u8>, width: u32, height: u32) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// A tile of the capture, blue first, as GPUI has pictures.
fn part_of(frame: &Frame, area: Rect) -> Option<Arc<RenderImage>> {
    let stride = frame.width() as usize * 4;
    let mut pixels = Vec::with_capacity(area.width as usize * area.height as usize * 4);
    for y in area.y as usize..area.y as usize + area.height as usize {
        let start = y * stride + area.x as usize * 4;
        pixels.extend_from_slice(&frame.rgba[start..start + area.width as usize * 4]);
    }
    image_of(blue_first(pixels), area.width, area.height)
}

/// A tile read back (red first), into its place in what is drawn (`region` of the image).
fn copy(drawn: &image::RgbaImage, area: Rect, region: Rect, out: &mut [u8]) {
    let row = area.width.min(drawn.width()) as usize * 4;
    let stride = drawn.width() as usize * 4;
    let raw = drawn.as_raw();
    let (left, top) = ((area.x - region.x) as usize, (area.y - region.y) as usize);
    for y in 0..area.height.min(drawn.height()) as usize {
        let from = y * stride;
        let to = ((top + y) * region.width as usize + left) * 4;
        out[to..to + row].copy_from_slice(&raw[from..from + row]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blur() -> Edit {
        Edit::Blur {
            from: [0.0, 0.0],
            to: [10.0, 10.0],
            radius: 4.0,
        }
    }

    #[test]
    fn under_a_part_is_drawn_only_when_something_is_drawn_before_it() {
        let crop = Edit::Crop {
            from: [0.0, 0.0],
            to: [5.0, 5.0],
        };
        assert!(!needs_under(&[], &[]));
        assert!(!needs_under(&[blur()], &[]));
        // A crop draws nothing.
        assert!(!needs_under(&[crop.clone(), blur()], &[]));
        // A part made unreadable before another is under it.
        assert!(needs_under(&[blur(), blur()], &[]));
        let pen = Edit::Pen {
            points: vec![[1.0, 1.0]],
            color: perora_core::edit::Colour {
                red: 255,
                green: 0,
                blue: 0,
            },
            width: 3.0,
        };
        assert!(needs_under(&[crop.clone(), pen.clone(), blur()], &[]));
        // The one to be drawn next, held: all that is drawn is under it.
        assert!(needs_under(&[pen.clone()], &[1]));
        assert!(!needs_under(&[pen], &[]));
        assert!(!needs_under(&[crop], &[1]));
    }

    #[test]
    fn a_tile_goes_to_its_place_in_the_region() {
        let region = Rect::new(10, 20, 4, 3);
        let mut out = vec![0u8; 4 * 3 * 4];
        let drawn = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 4]));
        copy(&drawn, Rect::new(12, 21, 2, 2), region, &mut out);
        let at = |x: usize, y: usize| &out[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(at(2, 1), &[1, 2, 3, 4]);
        assert_eq!(at(3, 2), &[1, 2, 3, 4]);
        assert_eq!(at(1, 1), &[0, 0, 0, 0]);
        assert_eq!(at(2, 0), &[0, 0, 0, 0]);
    }
}
