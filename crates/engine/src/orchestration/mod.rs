//! Engine package `orchestration`: the orchestrator (a lead agent that
//! delegates to worker agents), the control request executor behind the
//! `control` and `app` CLIs, and the `/operator` agent app API.
//!
//! Ports src/features/orchestration/model, src/features/agent-app/model, and
//! the orchestration and control parts of src/app/App.tsx. Start with
//! `Orchestration::init` after `Engine::init` and `Submit::init`, and serve
//! the control server with `start_control_server`.

pub mod agent_app;
pub mod catalog;
pub mod engine_host;
pub mod executor;
pub mod host;
pub mod orchestrator;
pub mod package;
pub mod peers;
pub mod plan;
pub mod session_conversation;
pub mod state;
pub mod storage;
pub mod summary;
pub mod support;
pub mod workspace;

#[cfg(test)]
mod agent_app_tests;
#[cfg(test)]
mod executor_tests;
#[cfg(test)]
mod orchestrator_tests;
#[cfg(test)]
pub(crate) mod testing;

pub use agent_app::{
    AgentAppHost, AppLaunch, AppSessionListing, AppSessionPlacement, handle_agent_app,
};
pub use executor::{ControlExecutor, EngineAppHost, serve_control_requests, start_control_server};
pub use host::{
    ChoiceModel, Done, HarnessChoice, OrchestrationHost, OrchestrationStorage, PendingInput,
    WorkerIntegration, WorkerPreparation,
};
pub use orchestrator::{ApprovedStart, Orchestrator};
pub use package::{
    Orchestration, OrchestrationConfig, confirm_orchestration_card, retry_orchestration_card,
    update_orchestration_card,
};
pub use peers::{NoPeers, OrchestrationPeers};
pub use state::{
    DispatchStage, DispatchState, OrchestrationDispatch, OrchestrationRun, OrchestrationTask,
    OrchestrationWorkspace, RunStatus, TaskStatus, WorkspaceKind, WorkspacePolicy,
};
