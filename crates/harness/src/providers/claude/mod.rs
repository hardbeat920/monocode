//! Port of src/integrations/harness/providers/claude: the Claude Code
//! provider over `claude --output-format stream-json --input-format
//! stream-json`.
//!
//! - [`protocol`]: spawn arguments, user messages, control frames, and the
//!   stream-json readers (claudeProtocol.ts).
//! - [`session`]: live sessions, turns, approvals, questions, subagents, and
//!   background tasks (claude.ts).
//! - [`text`], [`title`], [`git`]: the isolated text runner and what is built
//!   on it (claudeText.ts, claudeTitle.ts, claudeGit.ts).
//! - [`catalog`]: the bundled model list and live discovery (claudeCatalog.ts).
//! - [`adapter`]: the `HarnessAdapter` and [`register`] (claudeAdapter.ts).

pub mod adapter;
pub mod catalog;
pub mod git;
pub mod io;
pub mod protocol;
pub mod session;
pub mod shared;
pub mod text;
pub mod title;

pub use adapter::{ClaudeAdapter, ClaudeAdapterParts, ClaudeAppHooks, register, register_with};

#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod session_tests;
