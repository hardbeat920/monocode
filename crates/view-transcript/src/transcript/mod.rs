//! The transcript: a session's turns, with the agent's work folded behind
//! one line per turn, tool rows, subagent runs, approvals, plans, task lists,
//! and the changes card.
//!
//! Port of src/features/sessions/ui/AgentTranscript.tsx and TranscriptPool.tsx.
//! The shared view model (src/features/sessions/model/transcript*.ts) lives
//! in `monocode_core::transcript`; the layout and other view-only logic is
//! in [`model`]; the GPUI views are in [`view`].

pub mod model;
pub mod view;

pub use view::{
    ApprovalDecision, ChangedFile, TranscriptConfig, TranscriptEvent, TranscriptView, init,
};
