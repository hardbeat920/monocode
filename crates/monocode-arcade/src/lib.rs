//! Native game engines and optional GPUI presentation. App services stay outside.
pub mod controller;
pub mod pacman;
pub use monocode_mascots::random;
pub mod registry;
pub mod snake;
pub mod types;

#[cfg(feature = "gpui")]
pub mod gpui;
