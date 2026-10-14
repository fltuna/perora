//! Captures every monitor into one PNG and prints how the monitors are laid out.
//!
//!     cargo run -p perora-core --example capture_all -- [output.png]

use std::time::Instant;

use perora_core::platform::{ScreenCapture, screen_capture};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "capture_all.png".into());
    let capture = screen_capture();

    let layout = capture.layout()?;
    println!("virtual screen: {:?}", layout.bounds);
    for monitor in &layout.monitors {
        println!(
            "  {}{}: {}x{} at ({}, {}), scale {:.0}%",
            monitor.name,
            if monitor.primary { " (primary)" } else { "" },
            monitor.bounds.width,
            monitor.bounds.height,
            monitor.bounds.x,
            monitor.bounds.y,
            monitor.scale * 100.0
        );
    }

    let started = Instant::now();
    let frame = capture.capture_all()?;
    let captured = started.elapsed();
    let png = frame.to_png()?;
    println!(
        "captured {}x{} in {captured:.0?}, PNG {} KB in {:.0?}",
        frame.width(),
        frame.height(),
        png.len() / 1024,
        started.elapsed() - captured
    );

    std::fs::write(&output, png)?;
    println!("wrote {output}");
    Ok(())
}
