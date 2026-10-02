//! Port of src/integrations/harness/providers/codex: the Codex adapter over
//! `codex app-server` JSON-RPC (without the `jsonrpc` field).
//!
//! - `protocol`: pure mapping between app-server messages and harness events.
//! - `session`: live sessions, turns, approvals, questions, MCP elicitation,
//!   child-thread subagents, and generated images.
//! - `text`, `title`, `git`: one-shot prompts on a separate app-server.
//! - `catalog`: the model list from `model/list`.

mod adapter;
pub mod catalog;
pub mod elicitation;
pub mod git;
pub mod json;
pub mod protocol;
pub mod questions;
mod rate_limits;
pub mod session;
pub mod text;
mod title;

pub use adapter::{CodexAdapter, CodexHost, register, register_with};
pub use git::{GitContexts, GitRangeContext, GitStagedContext};
pub use session::{GeneratedImageAsset, GeneratedImages, SessionOptions};

#[cfg(test)]
mod tests;
