//! GPUI's displays against the system's monitors. GPUI places a window in the logical pixels of its
//! display: the display's physical corner divided by its own scale.

use std::rc::Rc;

use gpui::{App, Bounds, PlatformDisplay, Pixels, point, px, size};
use perora_core::geometry::Rect;
use perora_core::platform::Monitor;

/// GPUI's display for the system's monitor.
pub fn of(monitor: &Monitor, cx: &App) -> Option<Rc<dyn PlatformDisplay>> {
    cx.displays().into_iter().find(|display| {
        let origin = display.bounds().origin;
        (f64::from(origin.x.as_f32()) * monitor.scale).round() as i32 == monitor.bounds.x
            && (f64::from(origin.y.as_f32()) * monitor.scale).round() as i32 == monitor.bounds.y
    })
}

/// A rectangle on that monitor (physical pixels of the virtual screen), as GPUI places windows.
pub fn bounds_on(monitor: &Monitor, area: Rect) -> Bounds<Pixels> {
    let scale = monitor.scale as f32;
    Bounds {
        origin: point(px(area.x as f32 / scale), px(area.y as f32 / scale)),
        size: size(px(area.width as f32 / scale), px(area.height as f32 / scale)),
    }
}
