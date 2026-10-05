//! Port of src/integrations/harness/providers/hermes.

pub mod adapter;
pub mod catalog;
pub mod protocol;

pub use adapter::{HermesAdapter, register};

#[cfg(test)]
mod live_tests;
