//! The GPU only while a window is open. The driver keeps what it grew for big uploads (pictures of
//! the screen) for as long as the Direct3D device lives, and Trim does not give it back: with no
//! window, GPUI is switched to headless, which lets go of the device and the vsync thread. Back to
//! windowed before a window opens (23-36 ms). Measured in P0: 8.8 MB waiting in the tray.

use std::time::Duration;

use gpui::{App, GraphicalEnvironment, WindowingRequest};

/// How the application starts: with no GPU, as it waits in the tray. (No device is made only to be
/// let go of, and a GPU that cannot be used does not stop the start: the tray is there anyway.)
pub const AT_START: WindowingRequest = WindowingRequest::Headless;

/// Before opening a window. Whether the GPU is there is the platform's to say (asked every time):
/// a device that could not be made leaves it headless, and the next window tries again.
pub fn attach(cx: &mut App) {
    if cx.graphical_environment().is_some() {
        return;
    }
    let task = cx.request_windowing(WindowingRequest::Windowed(GraphicalEnvironment::detect()));
    cx.foreground_executor()
        .spawn(async move {
            if let Err(error) = task.await {
                eprintln!("back to windowed failed: {error}");
            }
        })
        .detach();
}

/// Once no window is open (and none is about to be: not while the screen is being taken for the
/// selection screen).
pub fn release(cx: &mut App) {
    if !cx.windows().is_empty()
        || cx.graphical_environment().is_none()
        || crate::capture::busy(cx)
    {
        return;
    }
    eprintln!("gpu: let go (no window for a while)");
    let task = cx.request_windowing(WindowingRequest::Headless);
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
    cx.on_window_closed(|cx, _| release_soon(cx)).detach();
}

/// Lets the GPU go in a while, when no window is open by then: also after it was made for a window
/// that did not open.
pub fn release_soon(cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(3)).await;
        cx.update(release);
    })
    .detach();
}
