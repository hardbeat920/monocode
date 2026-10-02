//! Port of src/integrations/harness/providers/droid.

pub mod adapter;
pub mod catalog;
pub mod protocol;

pub use adapter::{DroidAdapter, register};

#[cfg(test)]
mod live_tests;
