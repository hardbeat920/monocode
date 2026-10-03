//! The calls the orchestrator makes outside itself: `OrchestrationHost` (the
//! object App.tsx bound with `orchestrator.bind`) and the control storage
//! (`control_save`, `control_load`, `control_enable`, `control_disable`,
//! `control_scopes`, `control_write_path`). Port of the `OrchestrationHost`
//! and `Storage` types in src/features/orchestration/model/orchestration.ts.

use gpui::{App, Task};
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::user_question::{UserQuestion, UserQuestionReply};
use monocode_core::{HarnessId, Session};
use serde::Serialize;

use super::state::{OrchestrationRun, OrchestrationTask, OrchestrationWorkspace};
use crate::attention::notifications::InputKind;
use crate::submit::ControlOutcome;

/// One model a worker may use.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChoiceModel {
    pub id: String,
    pub name: String,
}

/// An installed harness and the models it offers workers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HarnessChoice {
    pub harness: HarnessId,
    pub models: Vec<ChoiceModel>,
}

/// `WorkerPreparation`.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerPreparation {
    pub scratch_dir: Option<String>,
    pub workspace: OrchestrationWorkspace,
}

/// What `integrateWorker` applied to the lead checkout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkerIntegration {
    pub files: Vec<String>,
    pub already_applied: i64,
}

/// The terminal callback of a managed turn.
pub type Done = Box<dyn FnOnce(ControlOutcome, &mut App)>;

/// `OrchestrationHost`. Every method runs outside the `Orchestrator` entity,
/// so an implementation may read it.
pub trait OrchestrationHost {
    fn session(&self, id: &str, cx: &App) -> Option<Session>;
    fn sessions(&self, cx: &App) -> Vec<Session>;
    /// Installed harnesses and their models.
    fn choices(&self, cx: &App) -> Vec<HarnessChoice>;
    fn create_worker(
        &self,
        run: &OrchestrationRun,
        task: &OrchestrationTask,
        cx: &mut App,
    ) -> Task<Result<WorkerPreparation, String>>;
    fn integrate_worker(
        &self,
        run: &OrchestrationRun,
        task: &OrchestrationTask,
        cx: &mut App,
    ) -> Task<Result<WorkerIntegration, String>>;
    /// `Ok(false)` when unreviewed changes require the worktree to be kept.
    fn cleanup_worker(
        &self,
        run: &OrchestrationRun,
        task: &OrchestrationTask,
        only_if_unchanged: bool,
        cx: &mut App,
    ) -> Task<Result<bool, String>>;
    /// Start a managed turn. `done` runs exactly once when it ends or is
    /// rejected.
    fn submit(&self, id: &str, text: &str, done: Done, cx: &mut App);
    fn stop(&self, id: &str, cx: &mut App) -> Task<Result<(), String>>;
    /// Redirect a worker mid-turn, without discarding what it has already done.
    fn steer(&self, id: &str, text: &str, cx: &mut App) -> Task<Result<(), String>>;
    /// Answer on a worker's behalf; the lead, not the user, decides.
    fn respond_approval(&self, id: &str, request_id: i64, decision: ApprovalDecision, cx: &mut App);
    fn answer_question(&self, id: &str, request_id: i64, reply: UserQuestionReply, cx: &mut App);
}

/// The orchestrator's storage: `control_save` and `control_load` in the store,
/// and the control server's grants and path checks.
pub trait OrchestrationStorage {
    fn save(&self, run: &OrchestrationRun, cx: &App) -> Task<Result<(), String>>;
    /// The saved run, validated and normalized.
    fn load(&self, id: &str, cx: &App) -> Task<Result<Option<OrchestrationRun>, String>>;
    /// Grant the lead its control token. Returns the CLI path.
    fn enable(&self, id: &str, cwd: &str, cx: &App) -> Task<Result<String, String>>;
    fn disable(&self, id: &str, cx: &App) -> Task<Result<(), String>>;
    fn scopes(&self, cwd: &str, files: &[String], cx: &App) -> Task<Result<Vec<String>, String>>;
    fn resolve_path(&self, path: &str, cx: &App) -> Task<Result<String, String>>;
}

/// `pendingInput`: what a worker is blocked on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInput {
    pub kind: InputKind,
    pub request_id: i64,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub questions: Option<Vec<UserQuestion>>,
}
