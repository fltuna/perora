//! Lists the windows on the desktop, front to back, as the selection screen would offer them, and says
//! for each whether it gives a picture of itself. With a directory, saves those pictures there.
//!
//!     cargo run -p perora-core --example windows -- [directory]

use std::time::Instant;

use perora_core::platform::{Desktop, desktop};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::env::args().nth(1);
    let desktop = desktop();
    println!("pointer at {:?}", desktop.pointer()?);

    for (index, window) in desktop.windows()?.iter().enumerate() {
        let started = Instant::now();
        let picture = desktop.capture_window(window.id);
        let outcome = match &picture {
            Ok(frame) => format!(
                "{}x{} in {:.0?}",
                frame.width(),
                frame.height(),
                started.elapsed()
            ),
            Err(error) => format!("no picture: {error}"),
        };
        println!(
            "{index:2} {}x{} at ({}, {})  {:?}  [{outcome}]",
            window.bounds.width,
            window.bounds.height,
            window.bounds.x,
            window.bounds.y,
            window.title.chars().take(40).collect::<String>(),
        );
        if let (Some(directory), Ok(frame)) = (&directory, picture) {
            std::fs::write(
                format!("{directory}/window-{index:02}.png"),
                frame.to_png()?,
            )?;
        }
    }
    Ok(())
}
