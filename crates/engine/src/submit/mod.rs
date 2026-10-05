//! Engine package `submit`: the turn submission pipeline and the composer
//! models around it. Port of `submitSession` and the stop, compact, model,
//! and draft actions in src/app/App.tsx, plus the composer feature models in
//! src/features/sessions/model, src/features/skills/model, and
//! src/features/settings/model (MCP).

pub mod acceptance;
pub mod attachments;
#[cfg(feature = "attention")]
pub mod attention_glue;
pub mod chat_context;
pub mod ci_repair;
pub mod commands;
pub mod draft_cache;
pub mod draft_restore;
pub mod edit_last_turn;
pub mod handoff;
pub mod handoff_turn;
pub mod hooks;
pub mod link_preview;
pub mod mcp;
pub mod mcp_picker;
pub mod mcp_settings_cache;
pub mod message_queue;
pub mod monocode_tool_call;
pub mod operator_command;
pub mod paths;
pub mod pipeline;
pub mod prefs;
pub mod prompt;
pub mod quote_draft;
pub mod second_opinion;
pub mod session_folder_command;
pub mod skills;
pub mod text;

pub use acceptance::{ControlOutcome, ControlStatus, OnSettled, SubmissionAcceptance, SubmitError};
pub use hooks::SubmitPeers;
pub use pipeline::{Submit, SubmitConfig, SubmitEvent, SubmitGlobal, SubmitOptions};
