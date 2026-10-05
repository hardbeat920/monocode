//! Port of src/features/orchestration/ui/OrchestrationActions.ts, plus the
//! parts of the `orchestrator` module and orchestrationSummary.ts the
//! orchestration views read.
//!
//! The React views reached the engine through two contexts and a module
//! singleton. Here each is a trait the app implements over
//! `monocode_engine::orchestration`:
//!
//! - [`OrchestrationActions`]: the card actions context.
//! - [`OrchestrationWorkers`]: the sidebar's worker inspection context.
//! - [`OrchestrationRuns`]: the `orchestrator` calls (`snapshot`,
//!   `hydrate`, `resumeBlocker`, `resumeLeadBusy`, `cancelTask`, `start`).
//!
//! Run and summary snapshots are plain structs with the engine's JSON field
//! names, so the app can convert the engine's types through serde.

use std::rc::Rc;

use gpui::{App, Subscription, Task};
use monocode_core::orchestration::OrchestrationProposal;
use monocode_core::{Extra, HarnessId};
use serde::{Deserialize, Serialize};

/// `OrchestrationWorkerDetail`: a worker to open beside its lead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationWorkerDetail {
    pub session_id: String,
    pub lead_id: String,
    pub title: String,
    pub harness: HarnessId,
}

/// The `OrchestrationActions` context, shared by transcript cards in both
/// ordinary and split session panes.
pub trait OrchestrationActions: 'static {
    /// Save an edited card.
    fn update(&self, lead_id: &str, block_id: &str, proposal: OrchestrationProposal, cx: &mut App);
    /// Save the card, then start exactly the tasks it shows.
    fn confirm(&self, lead_id: &str, block_id: &str, cx: &mut App) -> Task<Result<(), String>>;
    /// Ask the lead for the card again.
    fn retry(&self, lead_id: &str, block_id: &str, cx: &mut App);
    /// Open a session.
    fn open(&self, session_id: &str, cx: &mut App);
    /// `openAgents` is wired: View agents opens every worker beside the lead.
    fn can_open_agents(&self) -> bool {
        false
    }
    /// `openAgents`: open every worker of a run as tabs beside the lead.
    fn open_agents(&self, _workers: Vec<OrchestrationWorkerDetail>, _cx: &mut App) {}
}

/// The `OrchestrationWorkers` context. The lead's sidebar card lists
/// workers. Approvals still go to the lead, not to the user; `open_details`
/// is the one way to watch a worker's transcript.
pub trait OrchestrationWorkers: 'static {
    /// The worker a toast revealed, if any.
    fn selected_id(&self, cx: &App) -> Option<String>;
    fn inspect(&self, session_id: Option<&str>, cx: &mut App);
    /// `openDetails` is wired. It is absent wherever the card renders
    /// without a workspace behind it.
    fn can_open_details(&self) -> bool {
        false
    }
    /// Open this worker beside its lead.
    fn open_details(&self, _worker: OrchestrationWorkerDetail, _cx: &mut App) {}
}

/// `OrchestrationRun.status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrchestrationRunStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "finished")]
    Finished,
}

impl OrchestrationRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
            Self::Finished => "finished",
        }
    }
}

/// `TaskStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrchestrationTaskStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "cancelling")]
    Cancelling,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "blocked")]
    Blocked,
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// The parts of an `OrchestrationTask` the views read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationTaskView {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub harness: HarnessId,
    pub status: OrchestrationTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The parts of an `OrchestrationRun` the views read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationRunView {
    pub lead_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    pub status: OrchestrationRunStatus,
    pub allowed_harnesses: Vec<HarnessId>,
    pub max_workers: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub tasks: Vec<OrchestrationTaskView>,
}

/// The conversation still running in a paused run's project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeBlocker {
    pub id: String,
    pub title: String,
}

/// The `orchestrator` calls the cards make.
pub trait OrchestrationRuns: 'static {
    /// `orchestrator.snapshot()`: every live run.
    fn runs(&self, cx: &App) -> Vec<OrchestrationRunView>;
    /// Calls `on_change` whenever the snapshot changes
    /// (`orchestrator.subscribe`).
    fn observe(&self, on_change: Box<dyn Fn(&mut App)>, cx: &mut App) -> Subscription;
    /// `orchestrator.hydrate(leadId)`.
    fn hydrate(&self, _lead_id: &str, _cx: &mut App) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }
    /// `orchestrator.resumeBlocker(leadId)`.
    fn resume_blocker(&self, _lead_id: &str, _cx: &App) -> Option<ResumeBlocker> {
        None
    }
    /// `orchestrator.resumeLeadBusy(leadId)`.
    fn resume_lead_busy(&self, _lead_id: &str, _cx: &App) -> bool {
        false
    }
    /// `orchestrator.cancelTask(leadId, taskId)`.
    fn cancel_task(&self, lead_id: &str, task_id: &str, cx: &mut App) -> Task<Result<(), String>>;
    /// `orchestrator.start(leadId, allowedHarnesses, maxWorkers)`.
    fn start(
        &self,
        lead_id: &str,
        allowed_harnesses: &[HarnessId],
        max_workers: i64,
        cx: &mut App,
    ) -> Task<Result<(), String>>;
}

/// No live runs: what a view without an orchestrator behind it reads.
pub struct NoRuns;

impl OrchestrationRuns for NoRuns {
    fn runs(&self, _: &App) -> Vec<OrchestrationRunView> {
        Vec::new()
    }

    fn observe(&self, _: Box<dyn Fn(&mut App)>, _: &mut App) -> Subscription {
        Subscription::new(|| {})
    }

    fn cancel_task(&self, _: &str, _: &str, _: &mut App) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }

    fn start(&self, _: &str, _: &[HarnessId], _: i64, _: &mut App) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }
}

/// `Rc<dyn OrchestrationRuns>`.
pub type SharedRuns = Rc<dyn OrchestrationRuns>;

/// One task in an orchestration summary row (`OrchestrationSummaryTask`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationSummaryTask {
    pub session_id: String,
    pub title: String,
    pub harness: HarnessId,
    pub model: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_input: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl OrchestrationSummaryTask {
    pub fn needs_input(&self) -> bool {
        self.needs_input == Some(true)
    }
}

/// `OrchestrationSummary`: a lead's run as its history row shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationSummary {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<bool>,
    pub tasks: Vec<OrchestrationSummaryTask>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `orchestrationTaskLabel`.
// TODO(port): a copy of monocode_engine::orchestration::summary's version,
// because view crates may not depend on the engine. Delete it when they can.
pub fn orchestration_task_label(
    task: &OrchestrationSummaryTask,
    summary: &OrchestrationSummary,
) -> &'static str {
    if task.needs_input() {
        return "Needs input";
    }
    if summary.live != Some(true)
        && matches!(task.status.as_str(), "running" | "cancelling" | "queued")
    {
        return "Saved";
    }
    if summary.status == "paused" && task.status == "queued" {
        return "Paused";
    }
    match task.status.as_str() {
        "queued" => "Queued",
        "running" => "Working",
        "cancelling" => "Stopping",
        "completed" => "Done",
        "failed" => "Failed",
        "blocked" => "Needs review",
        "interrupted" => "Interrupted",
        "cancelled" => "Cancelled",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn summary(live: bool, status: &str) -> OrchestrationSummary {
        OrchestrationSummary {
            status: status.into(),
            live: Some(live),
            tasks: Vec::new(),
            extra: Extra::new(),
        }
    }

    fn task(status: &str) -> OrchestrationSummaryTask {
        OrchestrationSummaryTask {
            session_id: "s".into(),
            title: "t".into(),
            harness: HarnessId::Codex,
            model: "codex:one".into(),
            status: status.into(),
            needs_input: None,
            extra: Extra::new(),
        }
    }

    #[test]
    fn labels_tasks_from_live_state_and_saved_history() {
        assert_eq!(
            orchestration_task_label(&task("running"), &summary(true, "active")),
            "Working"
        );
        assert_eq!(
            orchestration_task_label(&task("running"), &summary(false, "active")),
            "Saved"
        );
        assert_eq!(
            orchestration_task_label(&task("queued"), &summary(true, "paused")),
            "Paused"
        );
        let mut asking = task("running");
        asking.needs_input = Some(true);
        assert_eq!(
            orchestration_task_label(&asking, &summary(false, "active")),
            "Needs input"
        );
        assert_eq!(
            orchestration_task_label(&task("blocked"), &summary(true, "active")),
            "Needs review"
        );
    }

    #[test]
    fn reads_the_engine_run_json() {
        let run: OrchestrationRunView = serde_json::from_value(json!({
            "version": 2,
            "leadId": "lead",
            "cwd": "/repo",
            "status": "stopped",
            "allowedHarnesses": ["codex", "cursor"],
            "proposalId": "card",
            "maxWorkers": 2,
            "cli": "monocode",
            "tasks": [{
                "id": "engine", "sessionId": "worker-a", "title": "Audit engine",
                "harness": "codex", "model": "codex:two", "prompt": "Review",
                "files": [], "scopes": [], "dependsOn": [], "status": "cancelled",
                "accepted": false, "delivered": false, "result": ""
            }],
            "continuations": 0,
            "requests": {}
        }))
        .unwrap();
        assert_eq!(run.status, OrchestrationRunStatus::Stopped);
        assert_eq!(run.tasks[0].status, OrchestrationTaskStatus::Cancelled);
        assert_eq!(run.proposal_id.as_deref(), Some("card"));
    }
}
