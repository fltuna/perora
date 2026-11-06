//! The corner of the primary monitor's work area (clear of the taskbar) that the small preview and
//! the notice appear in, and how they come into it.
//!
//! A window that was not on screen comes into its corner from the side of the screen that corner
//! is at, 24 px away, quickly and slowing down (0.2 s), and fades in as it sets out: it is seen
//! wholly a little past halfway (owner: 「本当にちょっと動いたかなーぐらいでいいよー」). One that is
//! there already (something else is said, another capture is shown) is put in its place at once.
//! Nothing moves for a user who has the system move nothing.
//!
//! Each window is opened out of sight (cloaked) and seen once what it drew first is on screen
//! (`appear_when_drawn`): never an empty window first.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, App, AsyncApp, Window};
use perora_core::geometry::Rect;
use perora_core::notice::Corner;
use perora_core::platform::{self, Monitor, ScreenCapture, WindowId};

use crate::hwnd;

/// How far from the edges of the work area, in logical pixels.
const MARGIN: f64 = 16.0;
/// The room between the small preview and a notice in the same corner, in logical pixels.
const BESIDE: f64 = 8.0;
/// How far from its place a window starts that comes into a corner, in logical pixels, and how
/// long it takes to be there; how often it is moved on its way; how far along its way it is seen
/// wholly.
const COMES_FROM: f64 = 24.0;
const COMES_IN: Duration = Duration::from_millis(200);
const COMES_BY: Duration = Duration::from_millis(8);
const SEEN_BY: f64 = 0.6;

/// The monitor the corners are of: the primary one (or the first, should none be).
pub fn monitor() -> Option<Monitor> {
    let layout = platform::screen_capture().layout().ok()?;
    layout.primary().or(layout.monitors.first()).cloned()
}

/// Where a window of `size` (logical pixels) goes in that corner, in physical pixels. With
/// `clear_of`, as far from the edge as it takes to be clear of a window that tall there (physical
/// pixels): above it in a corner at the bottom, below it at the top.
pub fn spot(monitor: &Monitor, corner: Corner, size: (f64, f64), clear_of: Option<i32>) -> Rect {
    let scale = monitor.scale;
    let (width, height) = (
        (size.0 * scale).round() as i32,
        (size.1 * scale).round() as i32,
    );
    let margin = (MARGIN * scale).round() as i32;
    let raised = clear_of.map_or(0, |tall| tall + (BESIDE * scale).round() as i32);
    let area = monitor.work_area;
    Rect::new(
        if corner.is_right() {
            area.right() - width - margin
        } else {
            area.x + margin
        },
        if corner.is_bottom() {
            area.bottom() - height - margin - raised
        } else {
            area.y + margin + raised
        },
        width.max(1) as u32,
        height.max(1) as u32,
    )
}

/// Where a window that comes into `spot` starts from: to the side of it, toward the edge of the
/// screen. `None` when the user has the system move nothing.
pub fn from(monitor: &Monitor, corner: Corner, spot: Rect) -> Option<Rect> {
    if !platform::animations_enabled() {
        return None;
    }
    let away = (COMES_FROM * monitor.scale).round() as i32;
    Some(Rect {
        x: spot.x + if corner.is_right() { away } else { -away },
        ..spot
    })
}

/// How a window that was just opened (out of sight) is to be seen: where it goes, from where it
/// comes, and whether it is still the window's latest showing (one that was shown again, or closed,
/// meanwhile is not seen from here).
pub struct Appearing {
    pub to: Rect,
    pub from: Option<Rect>,
    pub still: Rc<dyn Fn(&App) -> bool>,
}

/// From the window's first drawing: once what it drew is on screen, the window is seen. (The first
/// frame is drawn as the window opens and put on screen at the next; what is asked for at the next
/// comes before that is done, so the window is seen at the one after.) Seen whatever happened
/// meanwhile: only the way in is left out for a window that was shown again since (put where it is
/// to be already) or closed.
pub fn appear_when_drawn(appearing: Appearing, window: &mut Window) {
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |window, cx| {
            let Some(handle) = hwnd::of(window) else {
                return;
            };
            let window = window.window_handle();
            // Moved and shown from a task of its own: the system tells GPUI at once of a window
            // that moves, which it cannot take in the middle of drawing.
            cx.spawn(async move |cx| {
                let still = cx.update(|cx| (appearing.still)(cx));
                match appearing.from {
                    Some(from) if still => {
                        platform::set_opacity(handle, Some(0));
                        platform::set_cloaked(handle, false);
                        come_in(window, handle, from, appearing.to, appearing.still, cx).await;
                    }
                    _ => {
                        if still {
                            hwnd::place(handle, appearing.to);
                        }
                        platform::set_cloaked(handle, false);
                    }
                }
            })
            .detach();
        });
    });
}

/// Moves a window that is on screen into its place, quickly and slowing down, and fades it in on
/// the way. Stops where it is once it is not the window's latest showing: what showed it again put
/// it where it is to be.
async fn come_in(
    window: AnyWindowHandle,
    handle: WindowId,
    from: Rect,
    to: Rect,
    still: Rc<dyn Fn(&App) -> bool>,
    cx: &mut AsyncApp,
) {
    let began = Instant::now();
    loop {
        if !cx.update(|cx| still(cx)) {
            return;
        }
        let done = (began.elapsed().as_secs_f64() / COMES_IN.as_secs_f64()).min(1.0);
        let left = (1.0 - done).powi(3);
        let x = to.x + (f64::from(from.x - to.x) * left).round() as i32;
        hwnd::move_to(handle, x, to.y);
        // Seen more and more on the way, and wholly, as a window like any other, at its end.
        let seen = (done / SEEN_BY).min(1.0);
        let seen = (done < 1.0).then(|| ((1.0 - (1.0 - seen).powi(2)) * 255.0).round() as u8);
        platform::set_opacity(handle, seen);
        if done >= 1.0 {
            // (No longer a window that is seen through: what it shows is drawn again, or it shows
            // nothing.)
            cx.update(|cx| redraw(window, cx));
            return;
        }
        cx.background_executor().timer(COMES_BY).await;
    }
}

/// A window that is on screen already, put in its place at once, at its size, and wholly seen
/// (should it have been on its way in, it is there now). From a task of its own, as
/// `appear_when_drawn` has it. (One that has not drawn yet stays out of sight: that is seen once
/// it has, by `appear_when_drawn`, where this put it.)
pub fn put(window: &Window, to: Rect, cx: &mut App) {
    let Some(handle) = hwnd::of(window) else {
        return;
    };
    let window = window.window_handle();
    cx.spawn(async move |cx| {
        hwnd::place(handle, to);
        platform::set_opacity(handle, None);
        cx.update(|cx| redraw(window, cx));
    })
    .detach();
}

/// Has the window draw what it shows again: a window that stops being seen through shows nothing
/// until it does (what it drew went to the system as the frame of a window seen through).
fn redraw(window: AnyWindowHandle, cx: &mut App) {
    let _ = window.update(cx, |_, window, _| window.refresh());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(scale: f64) -> Monitor {
        Monitor {
            name: "test".into(),
            bounds: Rect::new(0, 0, 2560, 1440),
            work_area: Rect::new(0, 0, 2560, 1392),
            scale,
            primary: true,
        }
    }

    #[test]
    fn a_corner_is_of_the_work_area_and_clear_of_what_is_there() {
        let at_100 = monitor(1.0);
        assert_eq!(
            spot(&at_100, Corner::BottomRight, (360.0, 76.0), None),
            Rect::new(2560 - 360 - 16, 1392 - 76 - 16, 360, 76)
        );
        assert_eq!(
            spot(&at_100, Corner::TopLeft, (360.0, 76.0), Some(278)),
            Rect::new(16, 16 + 278 + 8, 360, 76)
        );
        assert_eq!(
            spot(&at_100, Corner::BottomLeft, (360.0, 76.0), Some(278)).y,
            1392 - 76 - 16 - 278 - 8
        );
        let at_150 = monitor(1.5);
        assert_eq!(
            spot(&at_150, Corner::TopRight, (360.0, 76.0), None),
            Rect::new(2560 - 540 - 24, 24, 540, 114)
        );
    }
}
