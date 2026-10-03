//! The transcript view's own model: the turn layout AgentTranscript.tsx
//! computes, the transcript pool, chat context chips, and the small helpers
//! the component kept inline. Nothing here touches GPUI.
//!
//! The shared view model (turn grouping, activity phases, folds, find, jump,
//! selection, highlights) lives in [`monocode_core::transcript`], so the
//! engine and provider tests can use it too.

pub mod chat_context;
pub mod link;
pub mod plan;
pub mod pool;
pub mod support;
pub mod turn;

pub use monocode_core::transcript::{
    ActivityPhase, ActivityPhaseKind, ActivityWorkKind, BlockRef, ToolCallDisplay, ToolCallState,
    TurnItem, WorkFold,
};
pub use plan::{
    BlockStore, FoldLine, FoldTitle, ItemView, Placement, PlanCache, PlanOptions, PlanState, Row,
    RowKind, TurnFooter, build_plan, visible_blocks,
};
