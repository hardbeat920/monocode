//! Port of src/integrations/harness/providers/antigravity: Antigravity over
//! ACP through `agy_acp_server.par`, with its model catalog probe.

pub mod adapter;
pub mod catalog;
pub mod protocol;
pub mod session;

pub use adapter::{AntigravityAdapter, AntigravityAppHooks, register, register_with};
pub use session::AntigravityOptions;

#[cfg(test)]
mod real_tests;
#[cfg(test)]
mod tests;
