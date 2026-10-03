//! Port of src/integrations/harness/providers/cursor: the Cursor CLI over ACP
//! (`cursor-agent acp`), its model catalog, its isolated text runner, and the
//! lookups into Cursor's own session store.

pub mod adapter;
pub mod catalog;
pub mod git;
pub mod json;
pub mod labels;
pub mod protocol;
pub mod session;
pub mod store;
pub mod subagents;
pub mod text;
pub mod title;

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod tests;

pub use adapter::{CursorAdapter, CursorAppHooks, register, register_with};
pub use store::{CursorStore, NoCursorStore, StoredCursorSubagentRun, StoredCursorToolCall};
pub use subagents::recover_cursor_subagents;
