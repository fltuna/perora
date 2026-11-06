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
    RenderImage, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, canvas, div,
};
use perora_core::edit::Edit;
use perora_core::geometry::Rect;
use perora_core::platform::{self, Frame};
use perora_editor::paint::{self, Base, Painter};
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
}

impl Render for Tile {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let base = self.base.clone();
        let area = self.area;
        let view = self.view;
        let edits = self.edits.clone();
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
                        &Default::default(),
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
    Ok(Arc::try_unwrap(base).unwrap_or_else(|shared| Frame {
        area: shared.area,
        rgba: shared.rgba.clone(),
    }))
}

/// A part of the image made unreadable, on its pixels as they are drawn so far.
fn hidden(frame: &Frame, edit: &Edit) -> Frame {
    use perora_editor::hidden::{self, Part};
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
    let view = Area::new(0.0, 0.0, width as f32, height as f32);
    let edits = Arc::new(edits);

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
                    edits: edits.clone(),
                })
            })
            .map_err(|error| error.to_string())?;
        let handle = handle.ok_or("the window has no handle")?;
        Ok::<_, String>((window, handle, monitor))
    })?;
    let (window, handle, monitor) = opened;
    let any: AnyWindowHandle = window.into();
    let result = tiles(&frame, &window, any, handle, monitor, cx).await;
    let _ = any.update(cx, |_, window, _| window.remove_window());
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

async fn tiles(
    frame: &Frame,
    window: &gpui::WindowHandle<Tile>,
    any: AnyWindowHandle,
    handle: platform::WindowId,
    monitor: perora_core::platform::Monitor,
    cx: &mut AsyncApp,
) -> Result<Frame, String> {
    let (width, height) = (frame.width(), frame.height());
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
            break;
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

    let mut out = vec![0u8; width as usize * height as usize * 4];
    for top in (0..height).step_by(tile_height as usize) {
        for left in (0..width).step_by(tile_width as usize) {
            let area = Rect::new(
                left as i32,
                top as i32,
                tile_width.min(width - left),
                tile_height.min(height - top),
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
            window
                .update(cx, |tile, _, cx| {
                    tile.base = Some(base);
                    tile.area = area;
                    cx.notify();
                })
                .map_err(|error| error.to_string())?;
            let drawn = any
                .update(cx, |_, window, cx| {
                    window.draw(cx).clear(cx);
                    let drawn = window.render_to_image();
                    let _ = window.drop_image(image);
                    drawn
                })
                .map_err(|error| error.to_string())?;
            let drawn = drawn.map_err(|error| error.to_string())?;
            copy(&drawn, area, &mut out, width);
            // (The window's own messages, between tiles.)
            cx.background_executor()
                .timer(Duration::from_millis(1))
                .await;
        }
    }
    Ok(Frame {
        area: Rect::new(0, 0, width, height),
        rgba: out,
    })
}

/// A tile of the capture, blue first, as GPUI has pictures.
fn part_of(frame: &Frame, area: Rect) -> Option<Arc<RenderImage>> {
    let stride = frame.width() as usize * 4;
    let mut pixels = Vec::with_capacity(area.width as usize * area.height as usize * 4);
    for y in area.y as usize..area.y as usize + area.height as usize {
        let start = y * stride + area.x as usize * 4;
        pixels.extend_from_slice(&frame.rgba[start..start + area.width as usize * 4]);
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(area.width, area.height, pixels)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

/// A tile read back (red first), into its place in the image.
fn copy(drawn: &image::RgbaImage, area: Rect, out: &mut [u8], width: u32) {
    let row = area.width.min(drawn.width()) as usize * 4;
    let stride = drawn.width() as usize * 4;
    let raw = drawn.as_raw();
    for y in 0..area.height.min(drawn.height()) as usize {
        let from = y * stride;
        let to = ((area.y as usize + y) * width as usize + area.x as usize) * 4;
        out[to..to + row].copy_from_slice(&raw[from..from + row]);
    }
}
