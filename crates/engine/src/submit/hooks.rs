//! Calls the submit pipeline makes into other packages.
//!
//! `runtime::hooks` covers the workspace nudges, the dock badge, and the
//! harness children, and the pipeline uses those. It has nothing yet for
//! the calls below, so submit defines them here, grouped by the package
//! that should fill them in, with no-op defaults (see NEEDS.md). The app
//! fills them in with `Submit::set_peers`. When the runtime grows the same
//! methods, these traits can go.

use std::rc::Rc;

use gpui::{App, Task};
use monocode_core::block::PlanBuildTarget;
use monocode_core::inbox::InboxAskContext;
use monocode_core::orchestration::{
    OrchestrationProposal, OrchestrationProposalStatus, OrchestrationSettings,
};
use monocode_core::session::LinkedWorkItem;
use monocode_core::{Attachment, Extra, HarnessEvent};
use monocode_harness::core::session_title::GeneratedWorkItemHint;

use super::acceptance::ProjectLocationSync;
use super::pipeline::SubmitOptions;

/// The orchestrator (orchestration.ts, orchestrationPlan.ts,
/// orchestrationCatalog.ts).
pub trait SubmitOrchestrationHooks {
    /// `orchestrator.submissionError(sessionId, managed)`: why this session
    /// cannot take a turn now, if it cannot.
    fn submission_error(&self, _session_id: &str, _managed: bool, _cx: &App) -> Option<String> {
        None
    }

    /// `orchestrator.run(sessionId)?.status`: the run this session leads.
    fn led_run_status(&self, _session_id: &str, _cx: &App) -> Option<String> {
        None
    }

    /// `orchestrator.forSession(sessionId)?.status`: the run this session
    /// leads or works in.
    fn run_status_for_session(&self, _session_id: &str, _cx: &App) -> Option<String> {
        None
    }

    /// `orchestrator.observe`: every event of a turn the pipeline runs.
    fn observe(&self, _session_id: &str, _event: &HarnessEvent, _cx: &mut App) {}

    /// `orchestrator.prompt`: add the run's context to an outgoing prompt.
    fn prompt(&self, _session_id: &str, text: String, _cx: &App) -> String {
        text
    }

    /// `orchestrator.stopForSession`: `None` when the session has no run to
    /// stop, which lets Stop cancel the turn itself.
    fn stop_for_session(&self, _session_id: &str, _cx: &mut App) -> Option<Task<()>> {
        None
    }

    /// `discoverOrchestrationSettings`.
    fn discover_settings(&self, _cx: &mut App) -> Task<OrchestrationSettings> {
        Task::ready(OrchestrationSettings {
            choices: Vec::new(),
            max_workers: 2,
            extra: Extra::new(),
        })
    }

    /// `orchestrationPlanningPrompt`.
    fn planning_prompt(
        &self,
        prompt: &str,
        _settings: &OrchestrationSettings,
        _cwd: &str,
    ) -> String {
        prompt.to_string()
    }

    /// `orchestrationRepairPrompt`.
    fn repair_prompt(&self, _proposal: &OrchestrationProposal) -> String {
        String::new()
    }

    /// `completeOrchestrationProposal`: parse the lead's response into the
    /// draft, or mark it invalid with `error`.
    fn complete_proposal(
        &self,
        draft: &OrchestrationProposal,
        response: &str,
        error: Option<&str>,
    ) -> OrchestrationProposal {
        OrchestrationProposal {
            status: OrchestrationProposalStatus::Invalid,
            error: Some(
                error
                    .unwrap_or("Orchestration is not available.")
                    .to_string(),
            ),
            response: Some(response.to_string()),
            ..draft.clone()
        }
    }
}

/// Remote hosts (`remoteProjectFor`, `remoteSessionActions`, `buildRemotePlan`).
pub trait SubmitRemoteHooks {
    /// `remoteProjectFor(cwd)`: this project lives on a remote host.
    fn is_remote(&self, _cwd: &str, _cx: &App) -> bool {
        false
    }

    /// `remoteSessionActions(sessionId)?.submit`.
    fn submit(
        &self,
        _session_id: &str,
        _text: &str,
        _attachments: &[Attachment],
        _options: &SubmitOptions,
        _cx: &mut App,
    ) -> bool {
        false
    }

    /// `remoteSessionActions(sessionId)?.saveDraft`.
    fn save_draft(
        &self,
        _session_id: &str,
        _text: &str,
        _attachments: &[Attachment],
        _cx: &mut App,
    ) -> bool {
        false
    }

    /// `remoteSessionActions(sessionId)?.compact`.
    fn compact(&self, _session_id: &str, _cx: &mut App) -> bool {
        false
    }

    /// `remoteSessionActions(sessionId)?.stop`.
    fn stop(&self, _session_id: &str, _cx: &mut App) {}

    /// `buildRemotePlan`.
    fn build_plan(
        &self,
        _session_id: &str,
        _block_id: &str,
        _target: Option<&PlanBuildTarget>,
        _cx: &mut App,
    ) {
    }
}

/// A worktree the projects package created or renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: Option<String>,
}

/// Projects and worktrees (projectLocation.ts, worktrees.ts, and App.tsx's
/// `removingWorktreePaths` and `applyProjectLocationChange`).
pub trait SubmitProjectsHooks {
    /// `synchronizeProjectLocation`: `Ok(None)` when the folder is gone.
    fn synchronize_project_location(
        &self,
        cwd: &str,
        _cx: &mut App,
    ) -> Task<Result<Option<ProjectLocationSync>, String>> {
        Task::ready(Ok(Some(ProjectLocationSync {
            path: cwd.to_string(),
            identity: String::new(),
            moved: false,
        })))
    }

    /// `applyProjectLocationChange`: rebase sessions and state onto the
    /// project's new folder.
    fn apply_project_location_change(
        &self,
        _from: &str,
        _to: &str,
        _cx: &mut App,
    ) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }

    /// `removingWorktreePaths`: worktrees being deleted right now.
    fn removing_worktree_paths(&self, _cx: &App) -> Vec<String> {
        Vec::new()
    }

    /// `createWorktree`.
    fn create_worktree(
        &self,
        _cwd: &str,
        _branch: &str,
        _base: &str,
        _existing: bool,
        _cx: &mut App,
    ) -> Task<Result<WorktreeInfo, String>> {
        Task::ready(Err("Worktrees are not available.".into()))
    }

    /// `renameWorktreeBranch`.
    fn rename_worktree_branch(
        &self,
        _cwd: &str,
        _path: &str,
        _branch: &str,
        _cx: &mut App,
    ) -> Task<Result<WorktreeInfo, String>> {
        Task::ready(Err("Worktrees are not available.".into()))
    }
}

/// Notifications.
pub trait SubmitAttentionHooks {
    /// `dismissNoticesForContinuedSession`: dismiss the session's due
    /// reminders and mark its linked work item update seen.
    fn dismiss_notices_for_continued_session(&self, _session_id: &str, _cx: &mut App) {}

    /// `announceSessionFinished` on the next tick, so the banner quotes the
    /// reply's final text: `Notifier::announce_finished_later`. The notifier
    /// decides whether the session is visible.
    fn announce_finished_later(&self, _session_id: &str, _cx: &mut App) {}
}

/// Inbox Asks and linked work items.
pub trait SubmitInboxHooks {
    /// `inboxAskPrompt`: wrap a message for a temporary Inbox conversation.
    fn ask_prompt(&self, _context: Option<&InboxAskContext>, text: String) -> String {
        text
    }

    /// `resolveLinkedWorkItem(message, cwd, hint)`.
    fn resolve_linked_work_item(
        &self,
        _message: &str,
        _cwd: &str,
        _hint: Option<GeneratedWorkItemHint>,
        _cx: &mut App,
    ) -> Task<Option<LinkedWorkItem>> {
        Task::ready(None)
    }
}

/// Prompt expansion owned by the files and notes features.
pub trait SubmitPromptHooks {
    /// `applyFileMentionsToTurn(text, cwd)`.
    fn apply_file_mentions(&self, text: String, _cwd: &str, _cx: &mut App) -> Task<String> {
        Task::ready(text)
    }

    /// `applyNotesToTurn(text)`.
    fn apply_notes(&self, text: String, _cx: &mut App) -> Task<String> {
        Task::ready(text)
    }
}

/// Session history lists.
pub trait SubmitHistoryHooks {
    /// A draft-only session left storage (`onRemoveDraft`): drop it from the
    /// history and linked-session lists.
    fn draft_session_discarded(&self, _session_id: &str, _cx: &mut App) {}
}

/// The default for every submit hook trait: does nothing.
pub struct NoopSubmitPeers;

impl SubmitOrchestrationHooks for NoopSubmitPeers {}
impl SubmitRemoteHooks for NoopSubmitPeers {}
impl SubmitProjectsHooks for NoopSubmitPeers {}
impl SubmitAttentionHooks for NoopSubmitPeers {}
impl SubmitInboxHooks for NoopSubmitPeers {}
impl SubmitPromptHooks for NoopSubmitPeers {}
impl SubmitHistoryHooks for NoopSubmitPeers {}

/// Every hook the pipeline calls, one trait object per owning package.
#[derive(Clone)]
pub struct SubmitPeers {
    pub orchestration: Rc<dyn SubmitOrchestrationHooks>,
    pub remote: Rc<dyn SubmitRemoteHooks>,
    pub projects: Rc<dyn SubmitProjectsHooks>,
    pub attention: Rc<dyn SubmitAttentionHooks>,
    pub inbox: Rc<dyn SubmitInboxHooks>,
    pub prompt: Rc<dyn SubmitPromptHooks>,
    pub history: Rc<dyn SubmitHistoryHooks>,
}

impl Default for SubmitPeers {
    fn default() -> Self {
        let noop = Rc::new(NoopSubmitPeers);
        Self {
            orchestration: noop.clone(),
            remote: noop.clone(),
            projects: noop.clone(),
            attention: noop.clone(),
            inbox: noop.clone(),
            prompt: noop.clone(),
            history: noop,
        }
    }
}
