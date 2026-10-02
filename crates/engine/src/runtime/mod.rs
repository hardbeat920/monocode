//! Engine package `runtime`: the session runtime, harness event batching,
//! persistence, boot restore, and the quit flow. Always on; the other
//! engine packages build on it and reach each other through `hooks`.
//!
//! Start with `Engine::init`, then read and change sessions through the
//! `Sessions` entity (`Engine::sessions(cx)`).

pub mod backend;
pub mod checkpoint;
pub mod edits;
pub mod engine;
pub mod harness_flush;
pub mod hooks;
pub mod in_flight;
pub mod lifecycle;
pub mod queue;
pub mod reducer;
pub mod session_cache;
pub mod session_done;
pub mod session_history;
pub mod session_store;
pub mod sessions;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
pub mod util;
pub mod window_transfer;

#[cfg(test)]
mod tests;

pub use backend::{CheckpointBackend, SessionBackend, StoreBackend, StoreFuture};
pub use checkpoint::{
    Checkpoints, ReviewChanged, ReviewChanges, begin_session_turn, notify_review_changed,
};
pub use engine::{Engine, EngineConfig};
pub use hooks::{
    AttentionHooks, EngineHooks, HarnessHooks, NoopHooks, OrchestrationHooks, RecoveredSession,
    RemoteHooks, SideThreadHooks, SubmitHooks, WorkspaceHooks,
};
pub use in_flight::ResumedWorkspace;
pub use lifecycle::{BootWorkspace, Lifecycle, LifecycleEvent, QuitMode};
pub use session_store::{InFlightRef, SessionSummary, SessionWriter};
pub use sessions::{Sessions, SessionsEvent};
