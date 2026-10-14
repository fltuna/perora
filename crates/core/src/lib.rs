//! What Perora does, without its window: capturing the screen across every monitor, signing in, and
//! uploading. No user interface in here, so that it can be tested and tried on its own.

pub mod account;
pub mod config;
pub mod edit;
pub mod frame;
pub mod geometry;
pub mod oidc;
pub mod output;
pub mod platform;
pub mod selection;
pub mod shots;

#[cfg(test)]
mod testing;
