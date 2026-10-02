//! Port of src/integrations/harness/providers/fx.

pub mod adapter;
pub mod catalog;
pub mod protocol;
pub mod tool;

// The ACP session plumbing and the test peer are shared with the Grok,
// Droid, and Hermes adapters. fx also builds without the `grok` feature, and
// then compiles the same files as its own modules.
#[cfg(feature = "grok")]
pub(crate) use crate::providers::grok::shared;
#[cfg(not(feature = "grok"))]
#[path = "../grok/shared.rs"]
pub(crate) mod shared;
#[cfg(all(test, feature = "grok"))]
pub(crate) use crate::providers::grok::test_support;
#[cfg(all(test, not(feature = "grok")))]
#[path = "../grok/test_support.rs"]
pub(crate) mod test_support;

pub use adapter::{FxAdapter, register};

#[cfg(test)]
mod live_tests;
