//! Port of src/features/orchestration/model/orchestrationState.ts: the run,
//! task, dispatch, and workspace shapes saved with `control_save`, and the
//! migration every loaded or committed run goes through.

use std::collections::{BTreeMap, HashMap};

use monocode_core::orchestration::OrchestrationChoice;
use monocode_core::paths::path_key;
use monocode_core::{Extra, HarnessId, ModelSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `TaskStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskStatus {
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

impl TaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Cancelling => "cancelling",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Interrupted => "interrupted",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `WorkspacePolicy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkspacePolicy {
    #[serde(rename = "shared")]
    Shared,
    #[serde(rename = "isolated-child")]
    IsolatedChild,
    #[serde(rename = "isolated-top-level")]
    IsolatedTopLevel,
}

/// `OrchestrationWorkspace.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkspaceKind {
    #[serde(rename = "main")]
    Main,
    #[serde(rename = "worktree")]
    Worktree,
}

/// `OrchestrationWorkspace`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationWorkspace {
    /// Stable comparison identity; not a display path.
    pub id: String,
    /// Project identity used by recents, history and project-level settings.
    pub project_cwd: String,
    /// Concrete checkout in which this run may read and write.
    pub checkout_cwd: String,
    pub kind: WorkspaceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `DispatchState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DispatchState {
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "running")]
    Running,
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
    #[serde(rename = "start_unknown")]
    StartUnknown,
    #[serde(rename = "stop_unknown")]
    StopUnknown,
}

/// `DispatchStage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DispatchStage {
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "session_prepared")]
    SessionPrepared,
    #[serde(rename = "turn_submitted")]
    TurnSubmitted,
    #[serde(rename = "settled")]
    Settled,
    #[serde(rename = "integration_started")]
    IntegrationStarted,
    #[serde(rename = "integrated")]
    Integrated,
    #[serde(rename = "cleaned")]
    Cleaned,
}

/// `OrchestrationDispatch`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationDispatch {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub workspace: OrchestrationWorkspace,
    pub state: DispatchState,
    pub stage: DispatchStage,
    pub started_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_error: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `OrchestrationTask`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationTask {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_id: Option<String>,
    pub session_id: String,
    pub title: String,
    pub harness: HarnessId,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    pub prompt: String,
    pub files: Vec<String>,
    /// Logical scopes in the lead checkout, used for scheduling overlap.
    pub scopes: Vec<String>,
    /// The same scopes resolved inside this worker's isolated checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_scopes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scratch_dir: Option<String>,
    pub depends_on: Vec<String>,
    pub status: TaskStatus,
    pub accepted: bool,
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// A retained worker can continue safely with this recovery turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_prompt: Option<String>,
    pub delivered: bool,
    /// Workspace selection is independent from task/dependency identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_policy: Option<WorkspacePolicy>,
    /// A retry reuses its worker checkout so partial work is never orphaned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<OrchestrationWorkspace>,
    /// Only this dispatch may settle the task. A retry always gets a new id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_dispatch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_dispatch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_dispatch_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl OrchestrationTask {
    /// `activeTask`: running or cancelling.
    pub fn is_active(&self) -> bool {
        matches!(self.status, TaskStatus::Running | TaskStatus::Cancelling)
    }
}

/// `OrchestrationRun.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RunStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "finished")]
    Finished,
}

impl RunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            RunStatus::Active => "active",
            RunStatus::Paused => "paused",
            RunStatus::Stopped => "stopped",
            RunStatus::Finished => "finished",
        }
    }
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One idempotent control command: the input it carried and what it returned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestReceipt {
    pub signature: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub result: Value,
}

/// `OrchestrationRun`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationRun {
    /// Version 1 remains readable; every committed snapshot is migrated to 2.
    pub version: i64,
    pub lead_id: String,
    /// Legacy project identity. Use workspace.checkoutCwd for filesystem work.
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<OrchestrationWorkspace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_root: Option<String>,
    pub status: RunStatus,
    pub allowed_harnesses: Vec<HarnessId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_models: Option<Vec<OrchestrationChoice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    pub max_workers: i64,
    pub cli: String,
    pub tasks: Vec<OrchestrationTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatches: Option<Vec<OrchestrationDispatch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub continuations: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pause_reason: Option<String>,
    pub requests: BTreeMap<String, RequestReceipt>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl OrchestrationRun {
    /// The task with this id.
    pub fn task(&self, id: &str) -> Option<&OrchestrationTask> {
        self.tasks.iter().find(|task| task.id == id)
    }

    /// `run.dispatches ?? []`.
    pub fn dispatch_list(&self) -> &[OrchestrationDispatch] {
        self.dispatches.as_deref().unwrap_or(&[])
    }

    /// Replace the task with `id` through `patch`.
    pub fn map_task(&mut self, id: &str, patch: impl FnOnce(&mut OrchestrationTask)) {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) {
            patch(task);
        }
    }

    /// Replace the dispatch with `id` through `patch`. Writes an empty list
    /// when the run had none, as `(run.dispatches ?? []).map(...)` did.
    pub fn map_dispatch(
        &mut self,
        id: Option<&str>,
        patch: impl FnOnce(&mut OrchestrationDispatch),
    ) {
        let dispatches = self.dispatches.get_or_insert_with(Vec::new);
        if let Some(id) = id
            && let Some(dispatch) = dispatches.iter_mut().find(|dispatch| dispatch.id == id)
        {
            patch(dispatch);
        }
    }
}

/// `workspaceIdentity`.
pub fn workspace_identity(
    project_cwd: &str,
    checkout_cwd: &str,
    branch: Option<&str>,
) -> OrchestrationWorkspace {
    OrchestrationWorkspace {
        id: format!("checkout:{}", path_key(checkout_cwd)),
        project_cwd: project_cwd.to_string(),
        checkout_cwd: checkout_cwd.to_string(),
        kind: if path_key(project_cwd) == path_key(checkout_cwd) {
            WorkspaceKind::Main
        } else {
            WorkspaceKind::Worktree
        },
        branch: branch
            .filter(|branch| !branch.is_empty())
            .map(str::to_string),
        extra: Extra::new(),
    }
}

/// `orchestrationWorkspace`.
pub fn orchestration_workspace(run: &OrchestrationRun) -> OrchestrationWorkspace {
    run.workspace
        .clone()
        .unwrap_or_else(|| workspace_identity(&run.cwd, &run.cwd, None))
}

/// `orchestrationProjectCwd`.
pub fn orchestration_project_cwd(run: &OrchestrationRun) -> String {
    orchestration_workspace(run).project_cwd
}

/// `orchestrationCheckoutCwd`.
pub fn orchestration_checkout_cwd(run: &OrchestrationRun) -> String {
    orchestration_workspace(run).checkout_cwd
}

/// `normalizeOrchestrationRun`.
pub fn normalize_orchestration_run(run: &OrchestrationRun) -> OrchestrationRun {
    let workspace = orchestration_workspace(run);
    // Older builds implemented every worker scope violation as a global pause
    // and copied the pause reason onto every running task as a generic failure.
    // Preserve the actual offender for review, but recover the collateral tasks
    // as resumable interruptions. The exact shared error is the durable marker
    // that distinguishes these tasks from ordinary worker failures.
    let legacy_pause_error = (run.status == RunStatus::Paused)
        .then_some(run.error.as_deref())
        .flatten()
        .filter(|error| !error.is_empty());
    let escaped = legacy_pause_error.and_then(|error| {
        run.tasks
            .iter()
            .find(|task| {
                error.starts_with(&format!(
                    "{} reported a write outside its assignment:",
                    task.title
                ))
            })
            .map(|task| task.id.clone())
    });
    let tasks: Vec<OrchestrationTask> = run
        .tasks
        .iter()
        .map(|task| {
            let mut next = task.clone();
            let legacy_interrupted = legacy_pause_error.is_some()
                && task.status == TaskStatus::Failed
                && task.error.as_deref() == legacy_pause_error;
            if legacy_interrupted {
                let offender = escaped.as_deref() == Some(task.id.as_str());
                next.status = if offender {
                    TaskStatus::Blocked
                } else {
                    TaskStatus::Interrupted
                };
                next.delivered = !offender;
            }
            // Version 1 predates per-worker worktrees and must retain its original
            // shared-checkout behavior. New version 2 tasks default to isolation.
            next.workspace_policy = Some(task.workspace_policy.unwrap_or(if run.version == 1 {
                WorkspacePolicy::Shared
            } else {
                WorkspacePolicy::IsolatedChild
            }));
            next
        })
        .collect();
    let status_by_dispatch: HashMap<String, TaskStatus> = tasks
        .iter()
        .filter_map(|task| {
            let id = task.last_dispatch_id.as_ref().filter(|id| !id.is_empty())?;
            matches!(task.status, TaskStatus::Blocked | TaskStatus::Interrupted)
                .then(|| (id.clone(), task.status))
        })
        .collect();
    let dispatches = run
        .dispatch_list()
        .iter()
        .map(|dispatch| {
            let mut next = dispatch.clone();
            match status_by_dispatch.get(&dispatch.id) {
                Some(TaskStatus::Blocked) => next.state = DispatchState::Blocked,
                Some(TaskStatus::Interrupted) => next.state = DispatchState::Interrupted,
                _ => {}
            }
            next
        })
        .collect();
    OrchestrationRun {
        version: 2,
        cwd: workspace.project_cwd.clone(),
        workspace: Some(workspace),
        tasks,
        dispatches: Some(dispatches),
        ..run.clone()
    }
}

/// `storage.load`'s checks: parse a saved run and refuse a shape this build
/// cannot read.
pub fn parse_saved_run(raw: &str, lead_id: &str) -> Result<OrchestrationRun, String> {
    let value: Value = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    let version = value.get("version").and_then(Value::as_i64);
    if !matches!(version, Some(1) | Some(2))
        || value.get("leadId").and_then(Value::as_str) != Some(lead_id)
        || !value.get("tasks").is_some_and(Value::is_array)
    {
        return Err("Unsupported orchestration history".into());
    }
    let run: OrchestrationRun = serde_json::from_value(value).map_err(|error| error.to_string())?;
    Ok(normalize_orchestration_run(&run))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn task(id: &str) -> OrchestrationTask {
        OrchestrationTask {
            id: id.into(),
            assignment_id: None,
            session_id: id.into(),
            title: id.into(),
            harness: HarnessId::Claude,
            model: "claude:test".into(),
            model_settings: None,
            prompt: "Work".into(),
            files: vec![id.into()],
            scopes: vec![format!("/repo/{id}")],
            write_scopes: None,
            scratch_dir: None,
            depends_on: Vec::new(),
            status: TaskStatus::Running,
            accepted: false,
            result: String::new(),
            error: None,
            recovery_prompt: None,
            delivered: false,
            workspace_policy: None,
            workspace: None,
            active_dispatch_id: None,
            last_dispatch_id: None,
            accepted_dispatch_id: None,
            extra: Extra::new(),
        }
    }

    pub(crate) fn run(tasks: Vec<OrchestrationTask>) -> OrchestrationRun {
        OrchestrationRun {
            version: 1,
            lead_id: "lead".into(),
            cwd: "/repo".into(),
            workspace: None,
            canonical_root: None,
            status: RunStatus::Active,
            allowed_harnesses: vec![HarnessId::Claude],
            allowed_models: None,
            proposal_id: None,
            max_workers: 2,
            cli: "monocode".into(),
            tasks,
            dispatches: None,
            error: None,
            continuations: 0,
            last_pause_reason: None,
            requests: BTreeMap::new(),
            extra: Extra::new(),
        }
    }

    #[test]
    fn version_one_tasks_keep_the_shared_checkout_and_version_two_isolates() {
        let legacy = normalize_orchestration_run(&run(vec![task("a")]));
        assert_eq!(legacy.version, 2);
        assert_eq!(
            legacy.tasks[0].workspace_policy,
            Some(WorkspacePolicy::Shared)
        );
        assert_eq!(legacy.workspace.unwrap().kind, WorkspaceKind::Main);
        let mut current = run(vec![task("a")]);
        current.version = 2;
        assert_eq!(
            normalize_orchestration_run(&current).tasks[0].workspace_policy,
            Some(WorkspacePolicy::IsolatedChild)
        );
    }

    #[test]
    fn identifies_worktree_checkouts_and_drops_an_empty_branch() {
        let tree = workspace_identity("/repo", "/repo-worktrees/a", Some("feature"));
        assert_eq!(tree.kind, WorkspaceKind::Worktree);
        assert_eq!(tree.id, "checkout:/repo-worktrees/a");
        assert_eq!(tree.branch.as_deref(), Some("feature"));
        assert_eq!(workspace_identity("/repo", "/repo/", Some("")).branch, None);
        assert_eq!(
            workspace_identity("/repo", "/repo/", None).kind,
            WorkspaceKind::Main
        );
    }

    #[test]
    fn round_trips_the_saved_json_shape() {
        let saved = json!({
            "version": 2,
            "leadId": "lead",
            "cwd": "/repo",
            "status": "paused",
            "allowedHarnesses": ["codex"],
            "maxWorkers": 2,
            "cli": "/bin/monocode",
            "tasks": [{
                "id": "t", "sessionId": "s", "title": "T", "harness": "codex",
                "model": "codex:test", "prompt": "P", "files": ["a"], "scopes": ["/repo/a"],
                "dependsOn": [], "status": "interrupted", "accepted": false, "result": "",
                "delivered": true, "workspacePolicy": "isolated-child", "futureField": 1
            }],
            "dispatches": [],
            "continuations": 0,
            "requests": { "r1": { "signature": "{}", "result": { "ok": 1 } } },
            "somethingNew": true
        });
        let parsed = parse_saved_run(&saved.to_string(), "lead").unwrap();
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back["tasks"][0]["futureField"], 1);
        assert_eq!(back["somethingNew"], true);
        assert_eq!(back["requests"]["r1"]["result"]["ok"], 1);
        assert_eq!(back["workspace"]["checkoutCwd"], "/repo");
        assert!(parse_saved_run(&saved.to_string(), "other").is_err());
        let mut bad = saved.clone();
        bad["version"] = json!(3);
        assert_eq!(
            parse_saved_run(&bad.to_string(), "lead").unwrap_err(),
            "Unsupported orchestration history"
        );
    }
}
