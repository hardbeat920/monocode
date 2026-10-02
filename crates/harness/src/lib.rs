//! Agent provider adapters, ported from `src/integrations/harness`.
//!
//! `core` is the provider-independent framework: framing, the adapter trait
//! and registry, child process plumbing, login, availability, and the
//! feature models the adapters share. Each module under `providers` is one
//! CLI, behind a cargo feature of the same name.
//!
//! The re-exports below are the framework half of
//! src/integrations/harness/index.ts. Provider entry points are reached
//! through the registry rather than re-exported one by one.

pub mod core;
pub mod providers;

pub use crate::core::auth::{
    HarnessLogin, harness_login_args, is_harness_auth_error, latest_turn_needs_harness_login,
    supports_harness_login,
};
pub use crate::core::availability::{HarnessAvailabilityProbe, harness_unavailable_hint};
pub use crate::core::availability_state::HarnessAvailabilityStore;
pub use crate::core::child::{BridgeLease, Children};
pub use crate::core::register::{HarnessContext, register_builtin_harnesses};
pub use crate::core::registry::{
    HARNESS_IDLE_PARK_MS, HarnessAdapter, HarnessRegistry, TextPromptInput,
};
pub use crate::core::text_harness::{
    generate_commit_message, generate_pr_content, pick_text_harness, warmup_text,
};
pub use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, HarnessEvent, SteerTurnInput,
};
pub use monocode_core::user_question::{UserQuestion, UserQuestionPrompt, UserQuestionReply};
