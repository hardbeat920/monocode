//! The transcript's pure view model, shared by the transcript view, the
//! engine, and provider tests.
//!
//! - [`activity`]: port of src/features/sessions/model/transcriptActivity.ts.
//! - [`find`], [`jump`], [`selection`], [`highlights`]: ports of
//!   transcriptFind.ts, transcriptJump.ts, transcriptSelection.ts, and the
//!   matching half of transcriptHighlights.ts.
//! - [`monocode_call`]: port of monocodeToolCall.ts, which activity summaries use.
//! - [`paths`]: `resolveWorkspacePath` and friends from src/shared/lib/paths.ts.
//! - [`fixtures`]: synthetic blocks for tests and galleries.

pub mod activity;
pub mod find;
pub mod fixtures;
pub mod highlights;
pub mod jump;
pub mod monocode_call;
pub mod paths;
pub mod selection;

pub use activity::{
    ActivityPhase, ActivityPhaseKind, ActivityWorkKind, BlockRef, ScrollMetrics, ToolCallDisplay,
    ToolCallState, TurnItem, WorkFold,
};

/// `INTERRUPT_MESSAGE` from src/features/sessions/model/inFlight.ts: the note
/// a quit leaves on a turn it cut short.
pub const INTERRUPT_MESSAGE: &str = "Turn interrupted when MonoCode quit.";
