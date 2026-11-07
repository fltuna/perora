//! What Perora does, without its window: capturing the screen across every monitor, signing in,
//! uploading, and what the user chose and in which language it is said. No user interface in here, so
//! that it can be tested and tried on its own.

pub mod account;
pub mod changelog;
pub mod colours;
pub mod config;
#[cfg(windows)]
pub mod copying;
pub mod drawing;
pub mod edit;
pub mod editor_settings;
pub mod frame;
pub mod geometry;
pub mod language;
pub mod notice;
pub mod oidc;
pub mod output;
pub mod platform;
pub mod preview;
pub mod saving;
pub mod selection;
pub mod settings;
pub mod shots;
pub mod update;

#[cfg(test)]
mod testing;
