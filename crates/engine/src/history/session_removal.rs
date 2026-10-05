//! Port of src/features/sessions/model/sessionRemoval.ts: the two-phase
//! archive and delete transaction. Callers supply state and I/O through a
//! `RemovalAdapter`; the flow reads state again after each wait.
//!
//! The store calls go through the runtime (`Engine::writer`,
//! `Engine::checkpoints`, and the harness hooks). The orchestrator and the
//! handoff helpers are not in the runtime yet, so the adapter supplies them
//! (see NEEDS.md).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{App, AsyncApp, Task};
use monocode_core::block::BlockRole;
use monocode_core::session::MessageQueueStatus;
use monocode_core::{HarnessId, ModelSettings, RuntimeMode, Session};
use monocode_layout::layout::{WorkspaceTab, is_filesystem_tab};
use monocode_layout::workspace_tab_groups::WorkspaceTabCloseScope;

use super::session_workspace_lifecycle::{
    RemoveSessionFromWorkspace, SessionWorkspaceRemoval, remove_session_from_workspace,
};
use crate::runtime::Engine;
use crate::runtime::reducer::{now_ms, session_child_harnesses, stop_streaming};
use crate::runtime::session_store::{SessionSummary, should_persist_session};

/// `SessionRemovalMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionRemovalMode {
    Archive,
    Delete,
}

impl SessionRemovalMode {
    /// The verb in dialogs: "archive" or "delete".
    pub fn verb(self) -> &'static str {
        match self {
            SessionRemovalMode::Archive => "archive",
            SessionRemovalMode::Delete => "delete",
        }
    }
}

/// `ReplacementSeed`: what a replacement chat copies.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplacementSeed {
    pub harness: Option<HarnessId>,
    pub cwd: String,
    pub model: Option<String>,
    pub runtime_mode: Option<RuntimeMode>,
    pub model_settings: Option<ModelSettings>,
}

impl ReplacementSeed {
    /// The seed an open session gives (`latest ?? options.replacement`).
    pub fn from_session(session: &Session) -> Self {
        Self {
            harness: Some(session.harness),
            cwd: session.cwd.clone(),
            model: Some(session.model.clone()),
            runtime_mode: Some(session.runtime_mode),
            model_settings: Some(session.model_settings.clone()),
        }
    }
}

/// `Workspace`: the state removal reads.
#[derive(Debug, Clone, Default)]
pub struct RemovalWorkspace {
    pub tabs: Vec<WorkspaceTab>,
    pub sessions: Vec<Session>,
    pub active_tab_id: String,
    pub dirty_files: HashSet<String>,
}

/// `WorkspaceChange`: what removal asks the owner of the state to apply.
#[derive(Debug, Clone)]
pub enum WorkspaceChange {
    /// The turn stopped; replace the open session with this copy.
    Stopped(Session),
    /// The deleted session led an orchestration; release its workers.
    OrchestrationReleased { lead_id: String },
    /// Commit the final workspace.
    Removed {
        mode: SessionRemovalMode,
        removal: SessionWorkspaceRemoval,
        session: Option<Session>,
        saved_summary: Option<SummaryBox>,
    },
}

/// A boxed history row, to keep `WorkspaceChange` small.
pub type SummaryBox = Box<SessionSummary>;

/// The delete `orchestrator.deleteSession` runs once its runs are stopped.
pub type DeleteSession = Box<dyn FnOnce(&mut App) -> Task<Result<(), String>>>;

/// State and I/O for one removal (`SessionRemovalOptions` minus the mode).
pub trait RemovalAdapter {
    /// `workspace.snapshot()`.
    fn snapshot(&self, cx: &App) -> RemovalWorkspace;
    /// `workspace.apply(change)`.
    fn apply(&self, change: WorkspaceChange, cx: &mut App);
    /// `confirm(tabs, mode)`: unsaved files and running terminals in the tabs
    /// that would close.
    fn confirm(
        &self,
        tabs: Vec<WorkspaceTab>,
        mode: SessionRemovalMode,
        cx: &mut App,
    ) -> Task<bool>;
    /// `stop(sessionId)`: `stopSessionForRemoval`.
    fn stop(&self, session_id: &str, cx: &mut App) -> Task<()>;
    /// `newSession(seed...)`: a blank chat from the model catalog.
    fn create_session(&self, seed: &ReplacementSeed, cx: &App) -> Session;
    /// `orchestrator.forSession` plus `stopRun` for an active or paused run.
    fn stop_active_run(&self, _session_id: &str, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }
    /// `orchestrator.deleteSession(sessionId, remove)`.
    fn delete_session(
        &self,
        _session_id: &str,
        remove: DeleteSession,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        remove(cx)
    }
    /// `isPreparingHandoff` then `completeHandoff(session,
    /// buildDeterministicHandoff(session))`. `None` keeps the session.
    fn finish_preparing_handoff(&self, _session: &Session) -> Option<Session> {
        None
    }
}

/// `SessionRemovalOptions`.
#[derive(Clone)]
pub struct SessionRemovalOptions {
    pub mode: SessionRemovalMode,
    pub scope: WorkspaceTabCloseScope,
    pub replacement: ReplacementSeed,
    pub adapter: Rc<dyn RemovalAdapter>,
}

/// `createSessionRemover`.
#[derive(Clone)]
pub struct SessionRemover {
    options: SessionRemovalOptions,
}

/// `createSessionRemover`.
pub fn create_session_remover(options: SessionRemovalOptions) -> SessionRemover {
    SessionRemover { options }
}

impl SessionRemover {
    /// `remove(sessionId)`: `Ok(false)` when the user declined.
    pub fn remove(&self, session_id: &str, cx: &mut App) -> Task<Result<bool, String>> {
        let options = self.options.clone();
        let session_id = session_id.to_string();
        cx.spawn(async move |cx| remove_session(&session_id, &options, cx).await)
    }
}

fn create_replacement(
    adapter: &dyn RemovalAdapter,
    fallback: &ReplacementSeed,
    latest: Option<&Session>,
    cx: &App,
) -> Session {
    let seed = latest.map(ReplacementSeed::from_session);
    let mut seed = seed.unwrap_or_else(|| fallback.clone());
    seed.harness = Some(seed.harness.unwrap_or(HarnessId::Cursor));
    adapter.create_session(&seed, cx)
}

/// `removeSession`: the same lifecycle for archive and delete, reading
/// state after each wait.
async fn remove_session(
    session_id: &str,
    options: &SessionRemovalOptions,
    cx: &mut AsyncApp,
) -> Result<bool, String> {
    let adapter = options.adapter.clone();
    let mode = options.mode;
    let (initial, plan) = cx.update(|cx| {
        let initial = adapter.snapshot(cx);
        let mut replace = |latest: Option<&Session>| {
            create_replacement(adapter.as_ref(), &options.replacement, latest, cx)
        };
        let plan = remove_session_from_workspace(RemoveSessionFromWorkspace {
            tabs: &initial.tabs,
            sessions: &initial.sessions,
            session_id,
            active_tab_id: &initial.active_tab_id,
            scope: options.scope,
            create_replacement: &mut replace,
            can_close_tab: None,
        });
        (initial, plan)
    });
    let confirm = cx.update(|cx| adapter.confirm(plan.closed_tabs.clone(), mode, cx));
    if !confirm.await {
        return Ok(false);
    }

    if mode == SessionRemovalMode::Delete {
        cx.update(|cx| adapter.stop_active_run(session_id, cx))
            .await;
    }
    cx.update(|cx| adapter.stop(session_id, cx)).await;
    let stopped = cx.update(|cx| {
        let latest = adapter
            .snapshot(cx)
            .sessions
            .into_iter()
            .find(|session| session.id == session_id)?;
        let mut stopped = if latest.is_busy() {
            stop_streaming(&latest, now_ms())
        } else {
            latest
        };
        if let Some(finished) = adapter.finish_preparing_handoff(&stopped) {
            stopped = finished;
        }
        if stopped
            .queued_messages
            .as_ref()
            .is_some_and(|queued| !queued.is_empty())
        {
            stopped.queue_status = Some(MessageQueueStatus::Paused);
        }
        // Cancellation invalidates normal turn completion. Keep a usable
        // stopped session even when the following storage operation fails.
        adapter.apply(WorkspaceChange::Stopped(stopped.clone()), cx);
        Some(stopped)
    });
    let harnesses: Vec<HarnessId> = match stopped.as_ref() {
        Some(stopped) => session_child_harnesses(stopped),
        None => vec![options.replacement.harness.unwrap_or(HarnessId::Cursor)],
    };
    if mode == SessionRemovalMode::Delete {
        // Release native processes before deleting the record, so a
        // following worktree removal cannot race fire-and-forget cleanup.
        let forgets: Vec<Task<()>> = cx.update(|cx| {
            let hooks = Engine::hooks(cx);
            harnesses
                .iter()
                .map(|harness| hooks.harness.forget_session(*harness, session_id, cx))
                .collect()
        });
        futures::future::join_all(forgets).await;
    }

    if stopped.is_some() {
        let flush = cx.update(|cx| Engine::checkpoints(cx).flush_session_checkpoint(session_id));
        flush.await;
    }
    let mut saved_summary = None;
    if mode == SessionRemovalMode::Delete {
        let image_paths: Vec<String> = stopped
            .as_ref()
            .map(|stopped| {
                stopped
                    .blocks
                    .iter()
                    .filter(|block| block.role == BlockRole::Image)
                    .filter_map(|block| block.image.as_ref().map(|image| image.path.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let id = session_id.to_string();
        let remove: DeleteSession =
            Box::new(move |cx: &mut App| Engine::writer(cx).delete_session(&id, image_paths));
        let delete = cx.update(|cx| adapter.delete_session(session_id, remove, cx));
        delete.await?;
        cx.update(|cx| {
            adapter.apply(
                WorkspaceChange::OrchestrationReleased {
                    lead_id: session_id.to_string(),
                },
                cx,
            )
        });
    } else {
        if let Some(stopped) = stopped
            .as_ref()
            .filter(|stopped| should_persist_session(stopped))
        {
            let upsert = cx.update(|cx| Engine::writer(cx).upsert_session(stopped));
            match upsert.await? {
                Some(saved) => saved_summary = Some(Box::new(saved)),
                None => return Err("The conversation could not be saved.".into()),
            }
        }
        let archive = cx.update(|cx| Engine::writer(cx).set_session_archived(session_id, true));
        archive.await?;
    }

    // No await between the final read and the commit: unrelated streaming
    // updates, tabs, and focus changes must survive this operation.
    cx.update(|cx| {
        let current = adapter.snapshot(cx);
        let removed_session = current
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .cloned();
        let confirmed: HashMap<&str, &WorkspaceTab> = plan
            .closed_tabs
            .iter()
            .map(|tab| (tab.id.as_str(), tab))
            .collect();
        let can_close_tab = |tab: &WorkspaceTab| {
            let Some(before) = confirmed.get(tab.id.as_str()) else {
                return false;
            };
            // File and terminal panes opened or rearranged during a dialog or
            // save were never confirmed. Remove the conversation but keep
            // those panes open. The TypeScript compared by identity.
            if before.editor_panes != tab.editor_panes
                || before.terminal_panes != tab.terminal_panes
            {
                return false;
            }
            !tab.editor_panes.iter().any(|pane| {
                pane.files.iter().any(|file| {
                    is_filesystem_tab(file)
                        && current.dirty_files.contains(&file.id)
                        && !initial.dirty_files.contains(&file.id)
                })
            })
        };
        let mut replace = |latest: Option<&Session>| {
            create_replacement(adapter.as_ref(), &options.replacement, latest, cx)
        };
        let removal = remove_session_from_workspace(RemoveSessionFromWorkspace {
            tabs: &current.tabs,
            sessions: &current.sessions,
            session_id,
            active_tab_id: &current.active_tab_id,
            scope: options.scope,
            create_replacement: &mut replace,
            can_close_tab: Some(&can_close_tab),
        });
        if mode == SessionRemovalMode::Archive {
            let hooks = Engine::hooks(cx);
            for harness in &harnesses {
                hooks
                    .harness
                    .forget_session(*harness, session_id, cx)
                    .detach();
            }
        }
        adapter.apply(
            WorkspaceChange::Removed {
                mode,
                removal,
                session: removed_session,
                saved_summary,
            },
            cx,
        );
    });
    Ok(true)
}

#[cfg(test)]
#[path = "session_removal_tests.rs"]
mod tests;
