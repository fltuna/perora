//! Perora's controls on GPUI. The browser gave the Tauri version its buttons, switches, menus and text
//! boxes; here they are made by hand, in the look of Windows (Fluent) and with the same colours.

pub mod motion;
pub mod slider;
pub mod swatch;
pub mod theme;
pub mod tip;

pub use slider::Slider;
pub use theme::{Mode, Theme};
pub use tip::Tip;
