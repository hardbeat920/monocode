use super::*;
use monocode_engine::history::sidebar::{ReminderDue, SessionListInput};
use monocode_engine::runtime::session_store::SessionSummary;
use monocode_layout::paths::{is_remote_project_path, project_name};

impl SessionList {
    pub(super) fn history(&self, cx: &App) -> Option<Entity<monocode_engine::history::History>> {
        self.shell.upgrade()?.read(cx).history.clone()
    }

    pub(super) fn sync_observations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(history) = self.history(cx)
            && self
                .history_observation
                .as_ref()
                .is_none_or(|(id, _)| *id != history.entity_id())
        {
            self.history_observation = Some((
                history.entity_id(),
                cx.observe(&history, |_, _, cx| cx.notify()),
            ));
        }
        if let Some(history) = self.history(cx) {
            let query = history.read(cx).sidebar().search_query.clone();
            if self.session_search.read(cx).value().as_ref() != query.as_str() {
                self.session_search
                    .update(cx, |input, cx| input.set_value(query, window, cx));
            }
        }
        let cwd = self
            .shell
            .upgrade()
            .map(|shell| shell.read(cx).sidebar_cwd(cx))
            .unwrap_or_default();
        if self
            .remote_watch
            .as_ref()
            .is_some_and(|(path, _)| path != &cwd)
        {
            self.remote_watch = None;
        }
        if is_remote_project_path(&cwd)
            && self.remote_watch.is_none()
            && let Some(remote) = monocode_engine::remote::RemoteGlobal::try_global(cx)
        {
            let connections = remote.connections.clone();
            let watch = connections.update(cx, |connections, cx| {
                connections.watch_project_sessions(&cwd, cx)
            });
            self.remote_watch = Some((cwd, watch));
        }
    }

    pub(super) fn data(&self, cx: &mut App) -> ListData {
        let mut data = ListData::default();
        let Some(shell) = self.shell.upgrade() else {
            return data;
        };
        let (cwd, workspace, history) = {
            let shell = shell.read(cx);
            (
                shell.sidebar_cwd(cx),
                shell.workspace.clone(),
                shell.history.clone(),
            )
        };
        if Engine::try_global(cx).is_none() {
            return data;
        }
        let sessions = Engine::sessions(cx);
        let open = sessions.read(cx).all().to_vec();
        let mut busy = sessions.read(cx).busy_session_ids().clone();
        let (mut approvals, mut unseen) = Attention::try_global(cx)
            .map(|attention| {
                (
                    attention.approvals.read(cx).approval_session_ids().clone(),
                    attention.notifier.read(cx).unseen_finished_ids().clone(),
                )
            })
            .unwrap_or_default();
        if let Some(workspace) = &workspace {
            data.active_session_id = workspace.read(cx).active_session(cx).map(|s| s.id);
        }
        let local_active_session_id = data.active_session_id.clone();
        let Some(history) = history else { return data };
        let remote_project = is_remote_project_path(&cwd);
        let mut remote_loaded = false;
        let branch = monocode_engine::projects::ProjectsGlobal::try_global(cx)
            .and_then(|projects| projects.git.read(cx).get(&cwd))
            .and_then(|status| {
                status
                    .read(cx)
                    .index()
                    .and_then(|index| index.branch.clone())
            });
        let (rows, open_rows) = if remote_project {
            let remote = monocode_engine::remote::RemoteGlobal::try_global(cx);
            let project = remote.map(|remote| remote.connections.read(cx).project_sessions(&cwd));
            let mut rows = Vec::new();
            if let Some(project) = project {
                remote_loaded = project.loaded;
                for host in project.sessions {
                    if host.status == monocode_remote::host::protocol::HostSessionStatus::Running {
                        busy.insert(host.id.clone());
                    }
                    if host.needs_input == Some(true) {
                        approvals.insert(host.id.clone());
                    }
                    let mut row = SessionSummary::new(&host.id, &cwd, host.harness);
                    row.title = host.title;
                    row.model = host.model.unwrap_or_default();
                    row.runtime_mode = host.runtime_mode.unwrap_or_default();
                    row.created_at = host.created_at.unwrap_or(host.updated_at);
                    row.updated_at = host.updated_at;
                    row.archived = host.archived;
                    row.pinned = host.pinned;
                    row.provider_session_id = host.provider_session_id;
                    row.branch = host.branch;
                    row.worktree_cwd = host.worktree_cwd;
                    row.repo = host.repo.or_else(|| Some(project_name(&cwd)));
                    row.draft = host.draft;
                    row.linked_work_item = host.linked_work_item;
                    rows.push(row);
                }
            }
            if let Some(remote) = remote {
                let connections = remote.connections.read(cx);
                for session in open
                    .iter()
                    .filter(|session| monocode_layout::paths::same_project_path(&session.cwd, &cwd))
                {
                    if let Some(host_id) = connections.remote_session_for(&session.id)
                        && unseen.contains(&session.id)
                    {
                        unseen.insert(host_id);
                    }
                }
                data.active_session_id = data
                    .active_session_id
                    .as_ref()
                    .and_then(|id| connections.remote_session_for(id));
            }
            (rows, Vec::new())
        } else {
            let runs = history.read(cx).host().live_runs(cx);
            let history = history.read(cx);
            (
                history.sidebar_history(&open, branch.as_deref(), &runs),
                history.open_project_sessions(&open, branch.as_deref()),
            )
        };
        // The sidebar shows only the focused worktree's sessions.
        let focus = if remote_project {
            None
        } else {
            workspace
                .as_ref()
                .and_then(|workspace| workspace.read(cx).worktree_focus(&cwd).cloned())
        };
        let in_focus = |row: &SessionSummary| {
            monocode_engine::workspace::in_worktree_focus(
                &row.cwd,
                row.worktree_cwd.as_deref(),
                focus.as_ref(),
            )
        };
        let rows: Vec<SessionSummary> = rows.into_iter().filter(|row| in_focus(row)).collect();
        let open_rows: Vec<SessionSummary> =
            open_rows.into_iter().filter(|row| in_focus(row)).collect();
        let reminders = monocode_engine::automations::AutomationsPackage::try_global(cx)
            .map(|package| {
                package
                    .reminders
                    .read(cx)
                    .reminders()
                    .iter()
                    .filter(|reminder| {
                        monocode_layout::paths::same_project_path(&reminder.cwd, &cwd)
                    })
                    .map(|reminder| ReminderDue {
                        session_id: reminder.session_id.clone(),
                        due_at: reminder.due_at,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let pending = history.read(cx).is_pending();
        let failed = history.read(cx).has_failed();
        let input = SessionListInput {
            project_sessions: &rows,
            open_sessions: &open_rows,
            busy_ids: &busy,
            approval_ids: &approvals,
            unseen_finished_ids: &unseen,
            reminders: &reminders,
            active_listed_session_id: data.active_session_id.as_deref(),
            active_session_id: local_active_session_id.as_deref(),
            remote: remote_project,
            listing_pending: pending,
            listing_failed: failed,
            remote_loaded,
        };
        let view = history.update(cx, |history, cx| {
            let view = history.session_list(&input);
            history.sync_session_list(&view, &input, cx);
            view
        });
        data.sessions_loading = if remote_project {
            !remote_loaded && rows.is_empty()
        } else {
            pending
        };
        data.selected_ids = history.read(cx).sidebar().selected.clone();
        data.entries = view.entries;
        data.has_more = view.has_more;
        data.filters_active = view.filters_active;
        data.search_narrowed = view.search_narrowed;
        data.harnesses = view.harnesses;
        data.listed = view.listed;
        data.navigation_ids = view.mounted_navigation_ids;
        data.sessions = view
            .visible
            .iter()
            .map(|row| {
                let status = if approvals.contains(&row.id) {
                    SessionStatus::NeedsApproval
                } else if busy.contains(&row.id) {
                    SessionStatus::Busy
                } else if unseen.contains(&row.id) {
                    SessionStatus::Done
                } else if row.draft == Some(true) {
                    SessionStatus::Draft
                } else {
                    SessionStatus::Idle
                };
                SessionCard {
                    provider: format::provider_logo(row.harness),
                    model: model_name(row.harness, &row.model, cx),
                    title: session_display_title(&row.title, row.harness),
                    git: if row.worktree_removed == Some(true) {
                        NO_BRANCH_LABEL.into()
                    } else {
                        format_git_label(row.repo.as_deref(), row.branch.as_deref())
                    },
                    additions: row.additions.unwrap_or(0),
                    deletions: row.deletions.unwrap_or(0),
                    updated_at: row.updated_at,
                    created_at: row.created_at,
                    status,
                    pinned: row.pinned == Some(true),
                    id: row.id.clone(),
                }
            })
            .collect();
        data
    }
}
