//! Perora's editor on GPUI (P4 of `docs/gpui-rewrite.md`): one part per kind of mark (`marks`) and
//! per tool (`tools`), the same drawing code for the screen and for the picture that leaves
//! (`paint`), and the editor that puts them together (`editor`).

pub mod editor;
pub mod hidden;
pub mod icons;
pub mod letters;
pub mod marks;
pub mod outline;
pub mod paint;
pub mod picture;
pub mod selecting;
pub mod tools;
pub mod view;
pub mod writing;

pub use editor::{Capture, Editor, Event, Leave, Open};
pub use picture::Picture;
