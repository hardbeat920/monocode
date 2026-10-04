//! Side threads (btw sheet), second opinion, handoff cards, live agents, and orchestration previews.
//!
//! Ports of the views in src/features/sessions/ui (BtwSheet, BtwQuestionBurst,
//! SecondOpinionButton, SecondOpinionCard, HandoffMiniCard, LiveAgentsPreview,
//! OrchestratorConstellation) and src/features/orchestration/ui
//! (OrchestrationPreview, OrchestrationSidebarAgents, OrchestrationActions).
//!
//! None of these views depends on `monocode-engine`. Each takes plain
//! snapshots and reports through a small trait or an event enum, which the
//! app implements over the engine's `side_threads` and `orchestration`
//! packages:
//!
//! - [`BtwHost`]: the side-thread flows (submit, stop, retry, delete, set
//!   model) and the btw.ts rules the engine owns.
//! - [`OrchestrationActions`], [`OrchestrationWorkers`], and
//!   [`OrchestrationRuns`]: the two React contexts of OrchestrationActions.ts
//!   and the `orchestrator` calls the cards make.
//! - [`ModelMenuSource`]: what the second opinion menu reads from the model
//!   catalog and the installer probe.

pub mod actions;
pub mod btw_burst;
pub mod btw_sheet;
pub mod constellation;
pub mod live_agents;
pub mod mini_cards;
pub mod orchestration_preview;
pub mod second_opinion;
pub mod sidebar_agents;
pub mod style;

mod parts;

pub use actions::{
    NoRuns, OrchestrationActions, OrchestrationRunStatus, OrchestrationRunView, OrchestrationRuns,
    OrchestrationSummary, OrchestrationSummaryTask, OrchestrationTaskStatus, OrchestrationTaskView,
    OrchestrationWorkerDetail, OrchestrationWorkers, ResumeBlocker, orchestration_task_label,
};
pub use btw_burst::{BtwQuestionBurst, BtwQuestionBurstEvent, BurstRect};
pub use btw_sheet::{
    BoxProbe, BtwConversation, BtwConversationProps, BtwHost, BtwRequest, BtwSheet, BtwSheetEvent,
    BtwSheetProps, BtwTab, BtwThreadBlocksInput,
};
pub use constellation::{Constellation, ConstellationNode, OrchestratorConstellation};
pub use live_agents::{LiveAgent, LiveAgentsPreview, LiveAgentsPreviewEvent, ProjectAppearance};
pub use mini_cards::{
    HandoffCard, HandoffMiniCard, SecondOpinionCard, handoff_mini_card, second_opinion_card,
};
pub use orchestration_preview::OrchestrationPreview;
pub use second_opinion::{
    CatalogMenuSource, MenuLevel, ModelMenuSource, SecondOpinionButton, SecondOpinionEvent,
    SecondOpinionProps, TriggerStyle,
};
pub use sidebar_agents::OrchestrationSidebarAgents;
