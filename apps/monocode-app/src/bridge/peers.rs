//! The calls between engine packages, filled in the way App.tsx made them
//! inline: each package's local hook trait (see the packages' NEEDS.md)
//! implemented over the other packages and the window's `Workspace`.
//!
//! Window calls go to the workspace in [`ActiveWorkspace`]. Page and
//! sidebar state belongs to the shell, so those calls become
//! [`ShellRequest`]s.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{App, AppContext as _, Context, Entity, Task};
use monocode_core::inbox::InboxAskContext;
use monocode_core::models::{HarnessAvailability, ModelCatalog};
use monocode_core::session::LinkedWorkItem;
use monocode_core::{HarnessId, RuntimeMode, Session};
use monocode_engine::attention::Attention;
use monocode_engine::automations::{
    AutomationsPackage, LaunchHost, ReminderApp, ReminderHost, SessionPlacement,
};
use monocode_engine::history::HistoryPackage;
use monocode_engine::history::host::{ActivePane, AlertKind, HistoryHost, WorkspaceTabs};
use monocode_engine::history::session_removal::ReplacementSeed;
use monocode_engine::history::session_workspace_lifecycle::SessionWorkspaceRemoval;
use monocode_engine::inbox::ci_repair::CiRepairRequest;
use monocode_engine::inbox::hooks::{CiRepairSettle, InboxHooks};
use monocode_engine::inbox::inbox::Inbox;
use monocode_engine::inbox::inbox_notifications::{
    InboxNotificationCategory, InboxNotificationSubject,
};
use monocode_engine::inbox::types::InboxItem;
use monocode_engine::orchestration::{
    AppLaunch, AppSessionPlacement, Orchestration, OrchestrationPeers, RunStatus,
};
use monocode_engine::projects::{ProjectsHooks, SessionFolderTarget};
use monocode_engine::remote::RemoteGlobal;
use monocode_engine::runtime::session_history::LiveRun;
use monocode_engine::runtime::session_store::SessionSummary;
use monocode_engine::side_threads::{SideThreadPeers, SideThreads};
use monocode_engine::submit::acceptance::ProjectLocationSync;
use monocode_engine::submit::hooks::{
    SubmitAttentionHooks, SubmitHistoryHooks, SubmitInboxHooks, SubmitProjectsHooks,
    SubmitPromptHooks, WorktreeInfo,
};
use monocode_engine::submit::{Submit, SubmitOptions};
use monocode_engine::workspace::delegate::WorkspaceDelegate;
use monocode_engine::workspace::{SessionFactory as _, Workspace};
use monocode_layout::layout::{FilePaneTab, PaneEdge, SplitDir, WorkspaceTab};
use monocode_layout::leaf_ids;

use super::ActiveWorkspace;
use super::dialogs;
use super::shell::{ShellPage, ShellRequest, ShellRequests};
use crate::boot::AppServices;

/// The window's workspace.
fn workspace(cx: &App) -> Option<Entity<Workspace>> {
    ActiveWorkspace::get(cx).and_then(|weak| weak.upgrade())
}

/// Run `update` on the window's workspace, if there is one.
fn with_workspace<R>(
    cx: &mut App,
    update: impl FnOnce(&mut Workspace, &mut Context<Workspace>) -> R,
) -> Option<R> {
    workspace(cx).map(|workspace| workspace.update(cx, update))
}

/// A tab's single session, for the `appendTab(newTab(id))` calls this
/// bridge turns into `Workspace::open_session` (the workspace keeps its
/// `append_tab` private).
fn tab_session(tab: &WorkspaceTab) -> Option<String> {
    let ids = leaf_ids(&tab.layout);
    (ids.len() == 1).then(|| ids[0].clone())
}

/// Open the session a new tab carries. The session is already in
/// `Sessions`, so the workspace adds a tab for it.
fn open_tab_session(tab: &WorkspaceTab, cx: &mut App) {
    if let Some(id) = tab_session(tab) {
        with_workspace(cx, |workspace, cx| workspace.open_session(&id, cx).detach());
    }
}

/// Unminimize, show, and focus the app's window.
pub fn bring_forward(cx: &mut App) {
    cx.activate(true);
    ShellRequests::send(ShellRequest::BringForward, cx);
}

/// The window has focus.
fn window_focused(cx: &App) -> bool {
    cx.active_window().is_some()
}

fn history_refresh(cwd: &str, cx: &mut App) {
    if let Some(package) = HistoryPackage::try_global(cx) {
        let history = package.history.clone();
        history.update(cx, |history, cx| history.refresh(cwd, cx));
    }
}

fn kv(cx: &App) -> Option<monocode_settings::Kv> {
    AppServices::try_global(cx).map(|services| services.kv.clone())
}

fn history_folder_target(
    target: &SessionFolderTarget,
) -> monocode_engine::history::session_folders::SessionFolderTarget {
    use monocode_engine::history::session_folders::SessionFolderTarget as HistoryTarget;
    match target {
        SessionFolderTarget::Existing { folder_id } => HistoryTarget::Existing {
            folder_id: folder_id.clone(),
        },
        SessionFolderTarget::New { name } => HistoryTarget::New { name: name.clone() },
    }
}

fn submit_ci_repair_request(
    request: &CiRepairRequest,
) -> monocode_engine::submit::ci_repair::CiRepairRequest {
    use monocode_engine::submit::ci_repair as submit;
    submit::CiRepairRequest {
        text: request.text.clone(),
        prompt: request.prompt.clone(),
        target: submit::CiRepairTarget {
            repo: request.target.repo.clone(),
            number: request.target.number,
            head_oid: request.target.head_oid.clone(),
            checks: request
                .target
                .checks
                .iter()
                .map(|check| submit::CiRepairCheck {
                    name: check.name.clone(),
                    workflow: check.workflow.clone(),
                    url: check.url.clone(),
                })
                .collect(),
        },
    }
}

fn ci_repair_settle(settle: CiRepairSettle) -> monocode_engine::submit::acceptance::OnSettled {
    let settle = RefCell::new(Some(settle));
    Rc::new(move |outcome, cx| {
        if let Some(settle) = settle.borrow_mut().take() {
            use monocode_engine::inbox::ci_repair_tracking::CiRepairOutcome;
            use monocode_engine::submit::acceptance::ControlStatus;
            settle(
                match outcome.status {
                    ControlStatus::Completed => CiRepairOutcome::Completed,
                    ControlStatus::Failed => CiRepairOutcome::Failed,
                    ControlStatus::Cancelled => CiRepairOutcome::Cancelled,
                },
                cx,
            );
        }
    })
}

fn inbox_project(
    item: &InboxItem,
) -> monocode_engine::attention::notification_projects::NotificationProject {
    use monocode_engine::attention::notification_projects::{
        NotificationWorkItem, inbox_notification_project,
    };
    inbox_notification_project(&NotificationWorkItem {
        provider: item.provider,
        repo: item.repo.clone(),
        url: item.url.clone(),
        project_path: (!item.project_path.is_empty()).then(|| item.project_path.clone()),
        project_id: item.project_id.clone(),
        project_name: item.project_name.clone(),
        team_id: item.team_id.clone(),
        team_name: item.team_name.clone(),
    })
}

fn inbox_subject(
    subject: &InboxNotificationSubject,
) -> monocode_engine::attention::notification_preferences::NotificationSubject {
    use monocode_engine::attention::notification_preferences::{
        NotificationCategory, NotificationSubject,
    };
    NotificationSubject {
        project_id: subject.project_id.clone(),
        category: match subject.category {
            InboxNotificationCategory::Issues => NotificationCategory::Issues,
            InboxNotificationCategory::PullRequests => NotificationCategory::PullRequests,
        },
        occurred_at: subject.occurred_at,
    }
}

fn run_is_live(status: RunStatus) -> bool {
    matches!(status, RunStatus::Active | RunStatus::Paused)
}

/// `orchestrator.forSession(id)?.leadId` for an active or paused run.
fn live_lead_for(session_id: &str, cx: &App) -> Option<String> {
    Orchestration::try_global(cx)?;
    let run = Orchestration::orchestrator(cx)
        .read(cx)
        .for_session(session_id)?;
    run_is_live(run.status).then(|| run.lead_id.clone())
}

// The workspace.

/// `WorkspaceDelegate`: dialogs, history refreshes, and recents.
pub struct AppWorkspaceDelegate;

impl WorkspaceDelegate for AppWorkspaceDelegate {
    fn confirm(&self, message: &str, ok_label: &str, cx: &mut App) -> Task<bool> {
        dialogs::confirm(message, ok_label, cx)
    }

    fn refresh_history(&self, cwd: &str, cx: &mut App) {
        history_refresh(cwd, cx);
    }

    fn remember_project(&self, path: &str, cx: &mut App) {
        monocode_engine::projects::actions::remember_project(path, cx);
    }

    fn move_session_to_worktree(
        &self,
        session_id: &str,
        target: monocode_engine::workspace::WorktreeTarget,
        is_current: monocode_engine::workspace::IsCurrent,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        let tree = monocode_engine::projects::backend::Worktree {
            is_main: target.is_main,
            ..monocode_engine::projects::backend::Worktree::new(
                target.path,
                target.branch.as_deref(),
            )
        };
        monocode_engine::projects::actions::on_worktree_change_with(
            session_id,
            tree,
            Some(is_current),
            cx,
        )
    }

    fn remember_remote_session(&self, shell_id: &str, cx: &mut App) {
        RemoteGlobal::forget_tab(shell_id, cx);
    }

    fn remote_session_for(&self, shell_id: &str, cx: &App) -> Option<String> {
        RemoteGlobal::remote_session_for(shell_id, cx)
    }

    fn remote_pending_worktree(&self, shell_id: &str, cx: &App) -> bool {
        RemoteGlobal::remote_pending_worktree(shell_id, cx).is_some()
    }

    fn remote_working_cwd(&self, project: &str, shell_id: &str, cx: &App) -> Option<String> {
        if !RemoteGlobal::is_remote_project(project, cx) {
            return None;
        }
        let host_cwd = RemoteGlobal::remote_tab_cwd(project, Some(shell_id), cx)?;
        let project = monocode_layout::paths::parse_remote_path(project)?;
        Some(monocode_layout::paths::remote_path(
            &project.environment_id,
            &host_cwd,
        ))
    }

    fn remote_summary(
        &self,
        session: &Session,
        cx: &App,
    ) -> Option<monocode_engine::workspace::delegate::RemoteSummary> {
        RemoteGlobal::cached_summary(session, cx).map(|summary| {
            monocode_engine::workspace::delegate::RemoteSummary {
                title: summary.title,
                harness: summary.harness,
            }
        })
    }

    fn hide_window(&self, cx: &mut App) {
        ShellRequests::send(ShellRequest::HideWindow, cx);
    }

    fn close_window(&self, cx: &mut App) {
        if let Some(window) = cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next())
        {
            window
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    }
}

// Projects.

/// `ProjectsHooks`: the workspace, history, submit, orchestration, and the
/// live catalog.
pub struct AppProjectsHooks;

impl ProjectsHooks for AppProjectsHooks {
    fn project_return_memory(
        &self,
        cx: &mut App,
    ) -> monocode_layout::project_return::ProjectReturnMemory {
        with_workspace(cx, |workspace, cx| workspace.read_project_return_memory(cx))
            .unwrap_or_default()
    }

    fn session_project_changed(&self, session_id: &str, cwd: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            workspace.session_project_changed(session_id, cwd, cx)
        });
    }

    fn project_sidebar_tab_removed(&self, path: &str, cx: &mut App) {
        ShellRequests::send(ShellRequest::ProjectSidebarRemoved(path.to_owned()), cx);
    }

    fn project_sidebar_tab_moved(&self, from: &str, to: &str, cx: &mut App) {
        ShellRequests::send(
            ShellRequest::ProjectSidebarMoved {
                from: from.to_owned(),
                to: to.to_owned(),
            },
            cx,
        );
    }
    fn project_cwd(&self, cx: &App) -> String {
        workspace(cx)
            .map(|workspace| workspace.read(cx).project_cwd().to_string())
            .unwrap_or_else(|| "~".into())
    }

    fn set_project_cwd(&self, cwd: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| workspace.set_project_cwd(cwd, cx));
    }

    fn tabs(&self, cx: &App) -> Vec<WorkspaceTab> {
        workspace(cx)
            .map(|workspace| workspace.read(cx).tabs().to_vec())
            .unwrap_or_default()
    }

    fn active_tab_id(&self, cx: &App) -> String {
        workspace(cx)
            .map(|workspace| workspace.read(cx).active_tab_id().to_string())
            .unwrap_or_default()
    }

    fn open_files(&self, cx: &App) -> Vec<FilePaneTab> {
        let Some(workspace) = workspace(cx) else {
            return Vec::new();
        };
        let workspace = workspace.read(cx);
        let mut files: Vec<FilePaneTab> = workspace
            .tabs()
            .iter()
            .flat_map(|tab| tab.editor_panes.iter().chain(&tab.terminal_panes))
            .flat_map(|pane| pane.files.iter().cloned())
            .collect();
        for dock in workspace.terminals().read(cx).docks() {
            files.extend(dock.pane.files.iter().cloned());
        }
        files
    }

    fn append_tab(&self, tab: WorkspaceTab, cwd: Option<&str>, cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            workspace.append_project_tab(tab, cwd, cx)
        });
    }

    fn insert_tab_beside(
        &self,
        tab: WorkspaceTab,
        anchor_id: Option<&str>,
        cwd: Option<&str>,
        cx: &mut App,
    ) {
        with_workspace(cx, |workspace, cx| {
            let anchor = anchor_id.unwrap_or(workspace.active_tab_id()).to_string();
            workspace.insert_project_tab_beside(tab, &anchor, cwd, cx);
        });
    }

    fn set_tabs(&self, tabs: Vec<WorkspaceTab>, active_tab_id: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            workspace.replace_project_tabs(tabs, active_tab_id, cx)
        });
    }

    fn remove_project_terminals(&self, path: &str, cx: &mut App) {
        if let Some(workspace) = workspace(cx) {
            let terminals = workspace.read(cx).terminals().clone();
            terminals.update(cx, |terminals, cx| {
                let docks = terminals
                    .docks()
                    .iter()
                    .filter(|dock| {
                        !monocode_layout::paths::same_project_path(&dock.project_path, path)
                    })
                    .cloned()
                    .collect();
                terminals.replace(docks, cx);
            });
        }
    }

    fn remote_session_for(&self, shell_id: &str, cx: &App) -> Option<String> {
        RemoteGlobal::remote_session_for(shell_id, cx)
    }

    fn set_active_tab(&self, tab_id: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| workspace.activate_tab(tab_id, None, cx));
    }

    fn activate_tab(&self, tab_id: &str, pane_id: Option<&str>, cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            workspace.activate_tab(tab_id, pane_id, cx)
        });
    }

    fn set_composer_focused(&self, focused: bool, cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            workspace.set_composer_focused(focused, cx)
        });
    }

    fn select_project_workspace(&self, path: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| workspace.select_project(path, cx));
    }

    fn cancel_workspace_navigation(&self, cx: &mut App) {
        with_workspace(cx, |workspace, cx| workspace.cancel_navigation(cx));
    }

    fn close_pages(&self, cx: &mut App) {
        ShellRequests::send(ShellRequest::ClosePages, cx);
    }

    fn forget_dirty_files(&self, file_ids: &[String], cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            for id in file_ids {
                workspace.file_dirty_change(id, false, cx);
            }
        });
    }

    fn project_location_changed(&self, from: &str, to: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            workspace.project_location_changed(from, to, cx)
        });
    }

    fn patch_summaries(
        &self,
        patch: &dyn Fn(&SessionSummary) -> Option<SessionSummary>,
        cx: &mut App,
    ) {
        if let Some(package) = HistoryPackage::try_global(cx) {
            let history = package.history.clone();
            history.update(cx, |history, cx| history.patch_summaries(patch, cx));
        }
    }

    fn refresh_history(&self, cwd: &str, cx: &mut App) {
        history_refresh(cwd, cx);
    }

    fn rebase_loaded_project(&self, from: &str, to: &str, cx: &mut App) {
        if let Some(package) = HistoryPackage::try_global(cx) {
            let history = package.history.clone();
            history.update(cx, |history, cx| {
                history.rebase_loaded_project(from, to, cx)
            });
        }
    }

    fn rebase_session_folder_settings(&self, from: &str, to: &str, cx: &mut App) {
        if let Some(kv) = kv(cx) {
            monocode_engine::history::session_folders::rebase_session_folder_settings(
                &kv, from, to,
            );
        }
    }

    fn place_session_in_folder(
        &self,
        cwd: &str,
        session_id: &str,
        target: &SessionFolderTarget,
        cx: &mut App,
    ) {
        if let Some(kv) = kv(cx) {
            monocode_engine::history::sidebar::place_session_in_project_folder(
                &kv,
                cwd,
                session_id,
                &history_folder_target(target),
            );
        }
    }

    fn rebase_ci_repairs(&self, from: &str, to: &str, cx: &mut App) {
        if let Some(inbox) = Inbox::try_global(cx) {
            inbox.update(cx, |inbox, cx| inbox.rebase_ci_repairs(from, to, cx));
        }
    }

    fn build_deterministic_handoff(&self, session: &Session) -> String {
        monocode_engine::submit::handoff::build_deterministic_handoff(session, None, None)
    }

    fn append_ready_handoff(
        &self,
        session: &Session,
        from: HarnessId,
        to: HarnessId,
        text: &str,
    ) -> Session {
        monocode_engine::submit::handoff::append_ready_handoff(session, from, to, text)
    }

    fn orchestration_running(&self, session_id: &str, cx: &App) -> bool {
        live_lead_for(session_id, cx).is_some()
    }

    fn model_catalog(&self, cx: &App) -> ModelCatalog {
        AppServices::try_global(cx)
            .map(|services| services.catalog.snapshot())
            .unwrap_or_default()
    }

    fn harness_availability(&self, cx: &App) -> HarnessAvailability {
        AppServices::try_global(cx)
            .map(|services| services.availability.snapshot())
            .unwrap_or_default()
    }
}

// History.

/// `HistoryHost`: the workspace, dialogs, worktrees, and the orchestrator.
pub struct AppHistoryHost;

impl HistoryHost for AppHistoryHost {
    fn remote_session_for(&self, shell_id: &str, cx: &App) -> Option<String> {
        RemoteGlobal::remote_session_for(shell_id, cx)
    }

    fn remember_remote_session(&self, shell_id: &str, host_id: &str, cx: &mut App) {
        RemoteGlobal::remember_remote_session(shell_id, Some(host_id), cx);
        with_workspace(cx, |workspace, cx| workspace.set_composer_focused(true, cx));
    }

    fn select_remote_session(&self, cwd: &str, host_id: &str, cx: &mut App) {
        let Some(workspace) = workspace(cx) else {
            return;
        };
        let Some(remote) = RemoteGlobal::try_global(cx) else {
            return;
        };
        let remote_sessions = remote.sessions.clone();
        let shell_ids = workspace
            .read(cx)
            .tabs()
            .iter()
            .flat_map(|tab| leaf_ids(&tab.layout))
            .collect::<Vec<_>>();
        let existing = remote_sessions
            .read(cx)
            .tab_for_remote_session(host_id, &shell_ids, cx);
        if let Some(shell_id) = existing {
            workspace.update(cx, |workspace, cx| {
                workspace.focus_open_session(&shell_id, cx);
            });
            return;
        }
        let session = workspace.read(cx).new_default_session(cwd, None);
        let shell_id = session.id.clone();
        monocode_engine::runtime::Engine::sessions(cx)
            .update(cx, |sessions, cx| sessions.insert(session, cx));
        remote_sessions.update(cx, |sessions, cx| sessions.bind_tab(&shell_id, host_id, cx));
        workspace
            .update(cx, |workspace, cx| workspace.open_session(&shell_id, cx))
            .detach();
    }

    fn new_session(&self, seed: &ReplacementSeed, cx: &App) -> Session {
        let Some(services) = AppServices::try_global(cx) else {
            return Session::blank(
                uuid::Uuid::new_v4().to_string(),
                seed.harness.unwrap_or(HarnessId::Claude),
                seed.model.clone().unwrap_or_default(),
                &seed.cwd,
            );
        };
        match seed.harness {
            Some(harness) => services.factory.new_session(
                harness,
                &seed.cwd,
                seed.model.as_deref(),
                seed.runtime_mode,
                seed.model_settings.as_ref(),
            ),
            None => services
                .factory
                .new_default_session(&seed.cwd, seed.runtime_mode),
        }
    }

    fn new_default_session(
        &self,
        cwd: &str,
        runtime_mode: Option<RuntimeMode>,
        cx: &App,
    ) -> Session {
        match AppServices::try_global(cx) {
            Some(services) => services.factory.new_default_session(cwd, runtime_mode),
            None => Session::blank(uuid::Uuid::new_v4().to_string(), HarnessId::Claude, "", cwd),
        }
    }

    fn workspace_tabs(&self, cx: &App) -> WorkspaceTabs {
        let Some(workspace) = workspace(cx) else {
            return WorkspaceTabs::default();
        };
        let workspace = workspace.read(cx);
        WorkspaceTabs {
            tabs: workspace.tabs().to_vec(),
            active_tab_id: workspace.active_tab_id().to_string(),
            dirty_files: workspace.dirty_files().clone(),
        }
    }

    fn commit_removal(&self, removal: &SessionWorkspaceRemoval, cx: &mut App) {
        let removal = removal.clone();
        with_workspace(cx, |workspace, cx| {
            workspace.apply_history_removal(removal, cx)
        });
    }

    fn open_session(&self, session: &Session, cx: &mut App) {
        let id = session.id.clone();
        with_workspace(cx, |workspace, cx| workspace.open_session(&id, cx).detach());
    }

    fn active_pane(&self, cx: &App) -> Option<ActivePane> {
        let workspace = workspace(cx)?;
        let tab = workspace.read(cx).active_tab()?;
        Some(ActivePane {
            tab_id: tab.id.clone(),
            focused_id: tab.focused_id.clone(),
            diff_focused: tab.diff_focused == Some(true),
        })
    }

    fn open_note_chat(&self, session_id: &str, cwd: &str, cx: &mut App) {
        ShellRequests::send(ShellRequest::ClosePages, cx);
        ShellRequests::send(
            ShellRequest::ShowSessions {
                cwd: Some(cwd.to_string()),
            },
            cx,
        );
        let id = session_id.to_string();
        with_workspace(cx, |workspace, cx| workspace.open_session(&id, cx).detach());
    }

    fn alert(&self, message: &str, kind: AlertKind, cx: &mut App) {
        dialogs::alert(message, kind == AlertKind::Error, cx);
    }

    fn confirm(&self, message: &str, cx: &mut App) -> Task<bool> {
        dialogs::confirm(message, "OK", cx)
    }

    fn remove_worktree(
        &self,
        cwd: &str,
        path: &str,
        force: bool,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        let removing =
            monocode_engine::projects::actions::remove_worktree(cwd, path, force, false, cx);
        cx.spawn(async move |_| removing.await.map(|_| ()))
    }

    fn run_lead_for_session(&self, session_id: &str, cx: &App) -> Option<String> {
        Orchestration::try_global(cx)?;
        let run = Orchestration::orchestrator(cx)
            .read(cx)
            .for_session(session_id)?;
        Some(run.lead_id.clone())
    }

    fn stop_active_run(&self, session_id: &str, cx: &mut App) -> Task<()> {
        let Some(lead) = live_lead_for(session_id, cx) else {
            return Task::ready(());
        };
        let stopping = Orchestration::stop_run(&lead, cx);
        cx.spawn(async move |_| {
            if let Err(error) = stopping.await {
                log::error!("[monocode] stop run {lead}: {error}");
            }
        })
    }

    fn delete_session(
        &self,
        session_id: &str,
        remove: monocode_engine::history::session_removal::DeleteSession,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        if Orchestration::try_global(cx).is_none() {
            return remove(cx);
        }
        // `orchestrator.deleteSession`: stop the run and drain control
        // writes before the rows go.
        let async_cx = cx.to_async();
        Orchestration::delete_session(
            session_id,
            move || async move {
                let removing = async_cx.update(|cx| remove(cx));
                removing.await
            },
            cx,
        )
    }

    fn live_runs(&self, cx: &App) -> Vec<LiveRun> {
        if Orchestration::try_global(cx).is_none() {
            return Vec::new();
        }
        Orchestration::orchestrator(cx)
            .read(cx)
            .snapshot()
            .iter()
            .map(|run| monocode_engine::orchestration::summary::live_run(run))
            .collect()
    }

    fn finish_preparing_handoff(&self, session: &Session) -> Option<Session> {
        use monocode_engine::submit::handoff;
        handoff::is_preparing_handoff(session).then(|| {
            let text = handoff::build_deterministic_handoff(session, None, None);
            handoff::complete_handoff(session, &text)
        })
    }
}

// Inbox.

/// `InboxHooks`: automations, new sessions, tabs, and CI repair turns.
pub struct AppInboxHooks;

impl InboxHooks for AppInboxHooks {
    fn notification_project_id(&self, item: &InboxItem) -> String {
        inbox_project(item).id
    }

    fn remember_notification_projects(&self, items: &[InboxItem], cx: &mut App) {
        if let Some(kv) = kv(cx) {
            let projects = items.iter().map(inbox_project).collect::<Vec<_>>();
            monocode_engine::attention::notification_projects::remember_notification_projects(
                &kv, &projects,
            );
        }
    }

    fn allows_notification_indicator(&self, subject: &InboxNotificationSubject, cx: &App) -> bool {
        let Some(kv) = kv(cx) else {
            return true;
        };
        let subject = inbox_subject(subject);
        use monocode_engine::attention::notification_preferences::{
            allows_project_notification_indicator, load_notification_preferences,
        };
        allows_project_notification_indicator(
            &subject.project_id,
            subject.category,
            &load_notification_preferences(&kv),
            monocode_engine::remote::now_ms(),
        )
    }

    fn play_inbox_cue(&self, subject: &InboxNotificationSubject, cx: &mut App) -> bool {
        let Some(attention) = Attention::try_global(cx) else {
            return false;
        };
        let notifier = attention.notifier.clone();
        let subject = inbox_subject(subject);
        notifier.update(cx, |notifier, _| {
            notifier.play_cue(
                monocode_engine::attention::sounds::SoundCue::InboxUnseen,
                Some(&subject),
            )
        })
    }

    fn inbox_appeared(&self, items: &[InboxItem], cx: &mut App) {
        let Some(package) = AutomationsPackage::try_global(cx) else {
            return;
        };
        let automations = package.automations.clone();
        let events: Vec<monocode_engine::automations::events::InboxEventItem> = items
            .iter()
            .filter_map(|item| {
                serde_json::to_value(item)
                    .ok()
                    .and_then(|value| serde_json::from_value(value).ok())
            })
            .collect();
        if events.is_empty() {
            return;
        }
        automations.update(cx, |automations, cx| {
            automations.inbox_appeared(events, cx).detach()
        });
    }

    fn new_default_session(&self, cwd: &str, inherit_runtime_mode: bool, cx: &mut App) -> Session {
        let runtime_mode = inherit_runtime_mode
            .then(|| {
                workspace(cx).and_then(|workspace| {
                    workspace
                        .read(cx)
                        .session_defaults(cx)
                        .map(|session| session.runtime_mode)
                })
            })
            .flatten();
        match AppServices::try_global(cx) {
            Some(services) => services.factory.new_default_session(cwd, runtime_mode),
            None => Session::blank(uuid::Uuid::new_v4().to_string(), HarnessId::Claude, "", cwd),
        }
    }

    fn new_session_like(&self, stopped: &Session, cx: &mut App) -> Session {
        match AppServices::try_global(cx) {
            Some(services) => services.factory.new_session(
                stopped.harness,
                &stopped.cwd,
                Some(&stopped.model),
                Some(stopped.runtime_mode),
                Some(&stopped.model_settings),
            ),
            None => {
                let mut fresh = Session::blank(
                    uuid::Uuid::new_v4().to_string(),
                    stopped.harness,
                    stopped.model.clone(),
                    stopped.cwd.clone(),
                );
                fresh.runtime_mode = stopped.runtime_mode;
                fresh.model_settings = stopped.model_settings.clone();
                fresh
            }
        }
    }

    fn default_cwd(&self, cx: &mut App) -> Task<Result<String, String>> {
        cx.background_spawn(async move {
            Ok(std::env::current_dir()
                .map(|path| monocode_platform::path_to_js(&path))
                .unwrap_or_else(|_| monocode_platform::dirs_home().unwrap_or_else(|| "~".into())))
        })
    }

    fn start_cwd(&self, cx: &App) -> String {
        let Some(workspace) = workspace(cx) else {
            return "~".into();
        };
        let workspace = workspace.read(cx);
        workspace
            .active_session(cx)
            .map(|session| session.cwd)
            .filter(|cwd| !cwd.is_empty())
            .or_else(|| {
                workspace
                    .session_defaults(cx)
                    .map(|session| session.cwd)
                    .filter(|cwd| !cwd.is_empty())
            })
            .unwrap_or_else(|| workspace.project_cwd().to_string())
    }

    fn sidebar_cwd(&self, cx: &App) -> String {
        workspace(cx)
            .map(|workspace| workspace.read(cx).sidebar_cwd(cx))
            .unwrap_or_else(|| "~".into())
    }

    fn leave_inbox(&self, cx: &mut App) {
        ShellRequests::send(ShellRequest::ClosePages, cx);
    }

    fn show_sessions_sidebar(&self, cwd: Option<&str>, cx: &mut App) {
        ShellRequests::send(
            ShellRequest::ShowSessions {
                cwd: cwd.map(str::to_string),
            },
            cx,
        );
    }

    fn open_session_tab(&self, session_id: &str, _cwd: &str, cx: &mut App) {
        let id = session_id.to_string();
        with_workspace(cx, |workspace, cx| workspace.open_session(&id, cx).detach());
    }

    fn select_session(&self, session_id: &str, cx: &mut App) -> Task<()> {
        let id = session_id.to_string();
        with_workspace(cx, |workspace, cx| workspace.open_session(&id, cx))
            .unwrap_or(Task::ready(()))
    }

    fn submit_ci_repair(
        &self,
        session_id: &str,
        request: &CiRepairRequest,
        settle: CiRepairSettle,
        cx: &mut App,
    ) -> bool {
        let Some(submit) = Submit::try_global(cx) else {
            return false;
        };
        let options = SubmitOptions {
            ci_repair: Some(submit_ci_repair_request(request)),
            on_settled: Some(ci_repair_settle(settle)),
            ..SubmitOptions::default()
        };
        submit.update(cx, |submit, cx| {
            submit.on_submit(session_id, &request.text, Vec::new(), options, cx)
        })
    }

    fn linked_work_item_changed(
        &self,
        _session_id: &str,
        _linked: Option<&LinkedWorkItem>,
        refresh: bool,
        cx: &mut App,
    ) {
        if refresh && let Some(workspace) = workspace(cx) {
            let cwd = workspace.read(cx).sidebar_cwd(cx);
            history_refresh(&cwd, cx);
        }
    }
}

// Automations and reminders.

/// `LaunchHost` for the window: tabs, focus, and placement.
pub struct AppLaunchHost;

impl LaunchHost for AppLaunchHost {
    fn append_tab(&self, tab: WorkspaceTab, _cwd: &str, cx: &mut App) {
        open_tab_session(&tab, cx);
    }

    fn activate_tab(&self, tab_id: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            workspace.activate_tab(tab_id, None, cx);
            workspace.set_composer_focused(false, cx);
        });
    }

    fn focus_open_session(&self, session_id: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| {
            workspace.focus_open_session(session_id, cx);
        });
    }

    fn show_sessions(&self, cwd: &str, cx: &mut App) {
        ShellRequests::send(ShellRequest::ClosePages, cx);
        ShellRequests::send(
            ShellRequest::ShowSessions {
                cwd: Some(cwd.to_string()),
            },
            cx,
        );
    }

    fn place_session(
        &self,
        session_id: &str,
        placement: &SessionPlacement,
        _cwd: &str,
        reveal: bool,
        cx: &mut App,
    ) -> Result<String, String> {
        let workspace = workspace(cx).ok_or("No MonoCode window is open.")?;
        let tab_id = workspace
            .read(cx)
            .tabs()
            .iter()
            .find(|tab| leaf_ids(&tab.layout).contains(&placement.beside_session_id))
            .map(|tab| tab.id.clone())
            .ok_or("The target session must be open in this project")?;
        let edge = match placement.direction {
            SplitDir::Right => PaneEdge::Right,
            SplitDir::Down => PaneEdge::Bottom,
        };
        let beside = placement.beside_session_id.clone();
        let id = session_id.to_string();
        workspace.update(cx, |workspace, cx| {
            workspace
                .place_session_on_pane(&id, &beside, edge, cx)
                .detach();
            if reveal {
                workspace.activate_tab(&tab_id, None, cx);
            }
        });
        Ok(tab_id)
    }

    fn set_project_cwd(&self, cwd: &str, cx: &mut App) {
        with_workspace(cx, |workspace, cx| workspace.set_project_cwd(cwd, cx));
    }

    fn is_focused(&self, cx: &App) -> bool {
        window_focused(cx)
    }

    fn bring_forward(&self, cx: &mut App) {
        bring_forward(cx);
    }
}

impl ReminderHost for AppLaunchHost {
    fn show_session(&self, session: &Session, cx: &mut App) -> Task<Result<(), String>> {
        ShellRequests::send(ShellRequest::ClosePages, cx);
        let id = session.id.clone();
        match with_workspace(cx, |workspace, cx| workspace.open_session(&id, cx)) {
            Some(opening) => cx.spawn(async move |_| {
                opening.await;
                Ok(())
            }),
            None => Task::ready(Err("No MonoCode window is open.".into())),
        }
    }

    fn is_focused(&self, cx: &App) -> bool {
        window_focused(cx)
    }

    fn bring_forward(&self, cx: &mut App) {
        bring_forward(cx);
    }
}

/// `ReminderApp`: saves before scheduling, and errors.
pub struct AppReminderApp;

impl ReminderApp for AppReminderApp {
    fn show_error(&self, message: &str, cx: &mut App) {
        dialogs::alert(message, true, cx);
    }
}

// Orchestration.

/// `OrchestrationPeers`: session launches for `sessions.start`, open file
/// checks, and the availability probe.
pub struct AppOrchestrationPeers;

impl OrchestrationPeers for AppOrchestrationPeers {
    fn launch_session(
        &self,
        _owner: &str,
        launch: AppLaunch,
        id: &str,
        placement: Option<AppSessionPlacement>,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        let Some(package) = AutomationsPackage::try_global(cx) else {
            return Task::ready(Err("No MonoCode window can start sessions.".into()));
        };
        let quick_launch = package.quick_launch.clone();
        let request = match serde_json::to_value(&launch).and_then(serde_json::from_value) {
            Ok(request) => request,
            Err(error) => return Task::ready(Err(format!("Invalid launch: {error}"))),
        };
        let placement = placement.map(|placement| SessionPlacement {
            direction: placement.direction,
            beside_session_id: placement.beside_session_id,
        });
        let accepting = quick_launch.update(cx, |quick_launch, cx| {
            quick_launch.accept(super::WINDOW_LABEL, request, id.to_string(), placement, cx)
        });
        cx.spawn(async move |_| accepting.await.map_err(|error| error.message().to_string()))
    }

    fn check_open_worktree_files(&self, path: &str, cx: &mut App) {
        // `checkOpenWorktreeFiles`: open editors under the worktree lose
        // their files, the way a deleted folder closes them.
        with_workspace(cx, |workspace, cx| workspace.file_deleted(path, cx));
    }

    fn probe_availability(&self, cx: &mut App) -> Task<()> {
        let Some(services) = AppServices::try_global(cx) else {
            return Task::ready(());
        };
        let probe = monocode_harness::HarnessAvailabilityProbe::new(
            services.registry.clone(),
            services.children.clone(),
            services.availability.clone(),
        );
        cx.background_spawn(probe.probe_harness_availability(false))
    }
}

// Submit.

/// Submit's projects, attention, inbox, prompt, and history hooks.
pub struct AppSubmitPeers;

impl SubmitProjectsHooks for AppSubmitPeers {
    fn synchronize_project_location(
        &self,
        cwd: &str,
        cx: &mut App,
    ) -> Task<Result<Option<ProjectLocationSync>, String>> {
        let Some(sync) = monocode_engine::projects::actions::synchronize_project_location(cwd, cx)
        else {
            return Task::ready(Ok(Some(ProjectLocationSync {
                path: cwd.to_string(),
                identity: String::new(),
                moved: false,
            })));
        };
        cx.spawn(async move |_| {
            Ok(sync.await?.map(|sync| ProjectLocationSync {
                path: sync.path,
                identity: sync.identity,
                moved: sync.moved,
            }))
        })
    }

    fn apply_project_location_change(
        &self,
        from: &str,
        to: &str,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        monocode_engine::projects::actions::apply_project_location_change(from, to, cx)
    }

    fn create_worktree(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
        cx: &mut App,
    ) -> Task<Result<WorktreeInfo, String>> {
        let creating =
            monocode_engine::projects::actions::create_worktree(cwd, branch, base, existing, cx);
        cx.spawn(async move |_| {
            creating.await.map(|worktree| WorktreeInfo {
                path: worktree.path,
                branch: worktree.branch,
            })
        })
    }

    fn rename_worktree_branch(
        &self,
        cwd: &str,
        path: &str,
        branch: &str,
        cx: &mut App,
    ) -> Task<Result<WorktreeInfo, String>> {
        let renaming =
            monocode_engine::projects::actions::rename_worktree_branch(cwd, path, branch, cx);
        cx.spawn(async move |_| {
            renaming.await.map(|worktree| WorktreeInfo {
                path: worktree.path,
                branch: worktree.branch,
            })
        })
    }
}

impl SubmitAttentionHooks for AppSubmitPeers {
    fn dismiss_notices_for_continued_session(&self, session_id: &str, cx: &mut App) {
        if cx.has_global::<SideThreads>() {
            SideThreads::global(cx).dismiss_notices_for_continued_session(session_id, cx);
        }
    }

    fn announce_finished_later(&self, session_id: &str, cx: &mut App) {
        if let Some(attention) = Attention::try_global(cx) {
            let notifier = attention.notifier.clone();
            notifier.update(cx, |notifier, cx| {
                notifier.announce_finished_later(session_id, cx)
            });
        }
    }
}

impl SubmitInboxHooks for AppSubmitPeers {
    fn ask_prompt(&self, context: Option<&InboxAskContext>, text: String) -> String {
        monocode_engine::inbox::inbox_ask::inbox_ask_prompt(context, &text)
    }
}

impl SubmitPromptHooks for AppSubmitPeers {
    fn apply_file_mentions(&self, text: String, cwd: &str, cx: &mut App) -> Task<String> {
        let Some(files) = monocode_engine::workspace::Files::try_global(cx) else {
            return Task::ready(text);
        };
        let index = files.index.clone();
        index.update(cx, |index, cx| {
            index.apply_file_mentions_to_turn(&text, cwd, cx)
        })
    }

    fn apply_notes(&self, text: String, cx: &mut App) -> Task<String> {
        let Some(package) = HistoryPackage::try_global(cx) else {
            return Task::ready(text);
        };
        let notes = package.notes.clone();
        notes.update(cx, |notes, cx| notes.apply_notes_to_turn(&text, cx))
    }
}

impl SubmitHistoryHooks for AppSubmitPeers {
    fn draft_session_discarded(&self, session_id: &str, cx: &mut App) {
        // The sidebar lists stored rows; reloading the project drops the
        // draft's row.
        let _ = session_id;
        if let Some(workspace) = workspace(cx) {
            let cwd = workspace.read(cx).sidebar_cwd(cx);
            history_refresh(&cwd, cx);
        }
    }
}

// Side threads.

/// `SideThreadPeers`: the tab half of `openSessionBeside`, reminders, and
/// the inbox's linked update cards.
pub struct AppSideThreadPeers;

impl SideThreadPeers for AppSideThreadPeers {
    fn open_session_beside(
        &self,
        source_id: &str,
        session_id: &str,
        _cwd: &str,
        focus_composer: bool,
        cx: &mut App,
    ) {
        let (source, id) = (source_id.to_string(), session_id.to_string());
        with_workspace(cx, |workspace, cx| {
            let holds_source = workspace
                .tabs()
                .iter()
                .any(|tab| leaf_ids(&tab.layout).contains(&source));
            if holds_source {
                workspace
                    .place_session_on_pane(&id, &source, PaneEdge::Right, cx)
                    .detach();
            } else {
                workspace.open_session(&id, cx).detach();
            }
            workspace.set_composer_focused(focus_composer, cx);
        });
    }

    fn dismiss_due_reminders(&self, session_id: &str, cx: &mut App) {
        if let Some(package) = AutomationsPackage::try_global(cx) {
            let reminders = package.reminders.clone();
            reminders.update(cx, |reminders, cx| {
                reminders.dismiss_due(session_id, cx).detach()
            });
        }
    }

    fn mark_linked_session_update_seen(
        &self,
        session_id: &str,
        remote_updated_at: i64,
        cx: &mut App,
    ) {
        if let Some(inbox) = Inbox::try_global(cx) {
            inbox.update(cx, |inbox, cx| {
                inbox.mark_linked_session_update_seen(session_id, remote_updated_at, cx)
            });
        }
    }
}

/// Which pages a request closes, for shells that track them one by one.
pub fn pages_closed_by(request: &ShellRequest) -> Vec<ShellPage> {
    match request {
        ShellRequest::ClosePages => vec![
            ShellPage::Search,
            ShellPage::Inbox,
            ShellPage::Notes,
            ShellPage::Automations,
        ],
        ShellRequest::ClosePage(page) => vec![*page],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_engine::inbox::ci_repair::{CiRepairCheck, CiRepairTarget};
    use monocode_engine::inbox::ci_repair_tracking::CiRepairOutcome;
    use monocode_engine::submit::acceptance::{ControlOutcome, ControlStatus};

    #[test]
    fn ci_repair_preserves_visible_text_full_prompt_and_target() {
        let request = CiRepairRequest {
            text: "Repair CI".into(),
            prompt: "Repair the failed build with the complete check evidence.".into(),
            target: CiRepairTarget {
                repo: "owner/repo".into(),
                number: 42,
                head_oid: "abc123".into(),
                checks: vec![CiRepairCheck {
                    name: "build".into(),
                    workflow: "CI".into(),
                    url: Some("https://example.com/check".into()),
                }],
            },
        };
        let submitted = submit_ci_repair_request(&request);
        assert_eq!(submitted.text, request.text);
        assert_eq!(submitted.prompt, request.prompt);
        assert_eq!(submitted.target.repo, request.target.repo);
        assert_eq!(submitted.target.number, 42);
        assert_eq!(submitted.target.head_oid, "abc123");
        assert_eq!(submitted.target.checks[0].name, "build");
        assert_eq!(submitted.target.checks[0].workflow, "CI");
        assert_eq!(submitted.target.checks[0].url, request.target.checks[0].url);
    }

    #[gpui::test]
    fn ci_repair_settles_once_with_the_actual_turn_outcome(cx: &mut gpui::TestAppContext) {
        for (status, expected) in [
            (ControlStatus::Completed, CiRepairOutcome::Completed),
            (ControlStatus::Failed, CiRepairOutcome::Failed),
            (ControlStatus::Cancelled, CiRepairOutcome::Cancelled),
        ] {
            let outcomes = Rc::new(RefCell::new(Vec::new()));
            let observed = outcomes.clone();
            let settle = ci_repair_settle(Box::new(move |outcome, _| {
                observed.borrow_mut().push(outcome)
            }));
            cx.update(|cx| {
                let outcome = ControlOutcome {
                    status,
                    text: String::new(),
                    error: None,
                };
                settle(outcome.clone(), cx);
                settle(outcome, cx);
            });
            assert_eq!(*outcomes.borrow(), vec![expected]);
        }
    }
}
