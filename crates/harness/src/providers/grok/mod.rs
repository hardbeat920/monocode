//! Port of src/integrations/harness/providers/grok.

pub mod adapter;
pub mod catalog;
pub mod git;
pub mod protocol;
pub(crate) mod shared;
pub mod text;
pub mod title;

pub use adapter::{GrokAdapter, GrokHost, register, register_with};

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod live_tests;
