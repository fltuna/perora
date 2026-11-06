//! The GPU only while a window is open. The driver keeps what it grew for big uploads (pictures of
//! the screen) for as long as the Direct3D device lives, and Trim does not give it back: with no
//! window, GPUI is switched to headless, which lets go of the device and the vsync thread. Back to
//! windowed before a window opens (23-36 ms). Measured in P0: 8.8 MB waiting in the tray.

use std::cell::Cell;
use std::time::Duration;

use gpui::{App, GraphicalEnvironment, WindowingRequest};

thread_local! {
    static HEADLESS: Cell<bool> = const { Cell::new(false) };
}

/// Before opening a window.
pub fn attach(cx: &mut App) {
    if !HEADLESS.with(Cell::get) {
        return;
    }
    let task = cx.request_windowing(WindowingRequest::Windowed(GraphicalEnvironment::detect()));
    HEADLESS.with(|headless| headless.set(false));
    cx.foreground_executor()
        .spawn(async move {
            if let Err(error) = task.await {
                eprintln!("back to windowed failed: {error}");
            }
        })
        .detach();
}

/// Once no window is open.
pub fn release(cx: &mut App) {
    if !cx.windows().is_empty() || HEADLESS.with(Cell::get) {
        return;
    }
    let task = cx.request_windowing(WindowingRequest::Headless);
    HEADLESS.with(|headless| headless.set(true));
    cx.foreground_executor()
        .spawn(async move {
            if let Err(error) = task.await {
                eprintln!("headless failed: {error}");
            }
        })
        .detach();
}

/// Lets the GPU go a while after the last window closes (not at once: a window often follows
/// another, and the device takes a moment to make).
pub fn release_after_windows(cx: &mut App) {
    cx.on_window_closed(|cx, _| {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            cx.update(release);
        })
        .detach();
    })
    .detach();
}
