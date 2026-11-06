//! Perora's controls on GPUI. The browser gave the Tauri version its buttons, switches, menus and text
//! boxes; here they are made by hand, in the look of Windows (Fluent) and with the same colours.

pub mod button;
pub mod check;
pub mod hand;
pub mod motion;
pub mod reveal;
pub mod seconds;
pub mod select;
pub mod shades;
pub mod slider;
pub mod swatch;
pub mod switch;
pub mod text_field;
pub mod theme;
pub mod tip;
pub mod tour;

pub use button::{Button, Ring, glyph};
pub use check::Check;
pub use reveal::Reveal;
pub use seconds::{Seconds, SecondsEvent};
pub use select::{Item, Select};
pub use shades::Shades;
pub use slider::Slider;
pub use switch::Switch;
pub use text_field::{TextField, TextFieldEvent};
pub use theme::{Mode, Theme};
pub use tip::Tip;
