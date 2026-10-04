//! The `History` entity: the history rows of every visited project and the
//! history actions. Port of App.tsx: the history state (lines 1054-1081,
//! 1489-1496, 1802-1836), selecting, renaming, archiving, pinning, linking,
//! and deleting history sessions (lines 4054-4084, 4219-4256, 4432-4847),
//! the title-tab actions (4849-4877), the sidebar inputs (9436-9468,
//! 9502-9521), and session list navigation (9894-9961).
//!
//! `runtime::session_history` holds the row helpers, and `Sessions` already
//! ports `loadStoredSession`, `ensureOpenSession`, and the history prefetch.
//! The sidebar list state (folders, filters, selection) is in `sidebar.rs`.
//!
//! Views observe the entity (`cx.observe`). Calls into the workspace,
//! dialogs, and the orchestrator go through `HistoryHost`.

use std::collections::HashSet;
use std::rc::Rc;

use gpui::{App, AsyncApp, Context, Entity, Subscription, Task, WeakEntity};
use monocode_core::session::{LinkedWorkItem, format_session_title, session_display_title};
use monocode_core::{HarnessId, RuntimeMode, Session};
use monocode_layout::layout::{WorkspaceTab, leaf_ids};
use monocode_layout::paths::project_name;
use monocode_layout::tab_keys::adjacent_item_id;
use monocode_settings::Kv;

use super::archive_shortcut::{ArchiveContext, ArchiveKeyEvent, archive_focused_session};
use super::host::{AlertKind, HistoryHost, NoHistoryHost};
use super::session_removal::{
    DeleteSession, RemovalAdapter, RemovalWorkspace, ReplacementSeed, SessionRemovalMode,
    SessionRemovalOptions, WorkspaceChange, create_session_remover,
};
use super::sidebar::SidebarState;
use crate::runtime::session_history::{
    LiveRun, SessionGitHint, history_with_live_sessions, merge_history_summary,
    merge_project_history_summary, replace_project_history, summary_from_session,
};
use crate::runtime::session_store::{SessionSummary, should_persist_session};
use crate::runtime::sessions::get_stored_session;
use crate::runtime::util::project_path::{
    is_remote_project_path, normalize_project_path, same_project_path,
};
use crate::runtime::{Engine, Sessions, SessionsEvent};

/// The history rows and actions. One per window.
pub struct History {
    pub(super) kv: Kv,
    pub(super) host: Rc<dyn HistoryHost>,
    /// `history`: every visited project's rows.
    rows: Vec<SessionSummary>,
    /// `storedLinkedSessions`: saved sessions with a linked work item.
    stored_linked_sessions: Vec<SessionSummary>,
    /// `loadedProjects`: projects whose rows are already in `rows`.
    loaded_projects: HashSet<String>,
    /// `historyErrorCwd`: the project whose listing failed.
    error_cwd: Option<String>,
    /// `sidebarCwd`.
    pub(super) sidebar_cwd: String,
    /// `sessionNavigationIdsRef`: the sidebar's keyboard order.
    pub(super) navigation_ids: Vec<String>,
    /// `deleteConfirmationPending`.
    delete_confirmation_pending: bool,
    pub(super) sidebar: SidebarState,
    _sessions_events: Subscription,
}

impl History {
    /// A history over the engine's `Sessions`. Call after `Engine::init`.
    pub fn new(kv: Kv, cx: &mut Context<Self>) -> Self {
        let sessions = Engine::sessions(cx);
        let subscription = cx.subscribe(&sessions, Self::on_sessions_event);
        Self {
            sidebar: SidebarState::new(&kv),
            kv,
            host: Rc::new(NoHistoryHost),
            rows: Vec::new(),
            stored_linked_sessions: Vec::new(),
            loaded_projects: HashSet::new(),
            error_cwd: None,
            sidebar_cwd: String::new(),
            navigation_ids: Vec::new(),
            delete_confirmation_pending: false,
            _sessions_events: subscription,
        }
    }

    /// Install the host. The default does nothing outside the engine.
    pub fn set_host(&mut self, host: Rc<dyn HistoryHost>) {
        self.host = host;
    }

    pub fn host(&self) -> Rc<dyn HistoryHost> {
        self.host.clone()
    }

    pub fn kv(&self) -> &Kv {
        &self.kv
    }

    /// The boot listing (`bootHistory` and `bootHistoryCwd`).
    pub fn set_boot_rows(
        &mut self,
        rows: Vec<SessionSummary>,
        cwd: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        self.stored_linked_sessions = rows
            .iter()
            .filter(|row| row.linked_work_item.is_some())
            .cloned()
            .collect();
        self.rows = rows;
        self.loaded_projects = cwd
            .filter(|cwd| !cwd.is_empty())
            .map(normalize_project_path)
            .into_iter()
            .collect();
        cx.notify();
    }

    // Reading.

    /// Every visited project's rows.
    pub fn rows(&self) -> &[SessionSummary] {
        &self.rows
    }

    /// `storedLinkedSessions`.
    pub fn stored_linked_sessions(&self) -> &[SessionSummary] {
        &self.stored_linked_sessions
    }

    pub fn sidebar_cwd(&self) -> &str {
        &self.sidebar_cwd
    }

    /// `projectHistory`: the sidebar project's rows.
    pub fn project_history(&self) -> Vec<SessionSummary> {
        self.rows
            .iter()
            .filter(|row| same_project_path(&row.cwd, &self.sidebar_cwd))
            .cloned()
            .collect()
    }

    fn sidebar_cwd_key(&self) -> Option<String> {
        (!self.sidebar_cwd.is_empty() && self.sidebar_cwd != "~")
            .then(|| normalize_project_path(&self.sidebar_cwd))
    }

    /// `historyFailed`: the sidebar project's listing failed.
    pub fn has_failed(&self) -> bool {
        self.sidebar_cwd_key()
            .is_some_and(|key| self.error_cwd.as_ref() == Some(&key))
    }

    /// `historyPending`: the sidebar shows a project never listed yet.
    pub fn is_pending(&self) -> bool {
        self.sidebar_cwd_key()
            .is_some_and(|key| !self.loaded_projects.contains(&key))
            && !self.has_failed()
    }

    /// The `{ branch, repo }` overlay App.tsx passed with live sessions.
    fn git_overlay(&self, branch: Option<&str>) -> SessionGitHint {
        SessionGitHint {
            branch: branch.filter(|b| !b.is_empty()).map(str::to_string),
            repo: (!self.sidebar_cwd.is_empty() && self.sidebar_cwd != "~")
                .then(|| project_name(&self.sidebar_cwd)),
        }
    }

    /// `sidebarHistory`: the project's rows plus live sessions not saved yet.
    /// `branch` is the project's current branch.
    pub fn sidebar_history(
        &self,
        sessions: &[Session],
        branch: Option<&str>,
        runs: &[LiveRun],
    ) -> Vec<SessionSummary> {
        history_with_live_sessions(
            &self.rows,
            sessions,
            &self.sidebar_cwd,
            Some(&self.git_overlay(branch)),
            runs,
        )
    }

    /// `openProjectSessions`: open chats of the sidebar project, as rows.
    pub fn open_project_sessions(
        &self,
        sessions: &[Session],
        branch: Option<&str>,
    ) -> Vec<SessionSummary> {
        let hint = self.git_overlay(branch);
        sessions
            .iter()
            .filter(|session| {
                session.inbox_ask.is_none()
                    && session.orchestration_lead_id.is_none()
                    && same_project_path(&session.cwd, &self.sidebar_cwd)
            })
            .map(|session| summary_from_session(session, Some(&hint)))
            .collect()
    }

    // Listing.

    /// Show another project in the sidebar and revalidate its rows.
    pub fn set_sidebar_cwd(&mut self, cwd: &str, cx: &mut Context<Self>) {
        if self.sidebar_cwd == cwd {
            return;
        }
        self.sidebar_cwd = cwd.to_string();
        self.sidebar_project_changed(cx);
        self.refresh(cwd, cx);
        cx.notify();
    }

    /// `refreshHistory`: re-list one project. A project loaded once keeps its
    /// cached rows on screen while this runs, and a failed revalidate keeps
    /// them too.
    pub fn refresh(&mut self, cwd: &str, cx: &mut Context<Self>) {
        if cwd.is_empty() || cwd == "~" {
            return;
        }
        let key = normalize_project_path(cwd);
        if self.error_cwd.as_ref() == Some(&key) {
            self.error_cwd = None;
            cx.notify();
        }
        let list = Engine::writer(cx).list_sessions_by_project(cwd);
        let cwd = cwd.to_string();
        cx.spawn(async move |this, cx| {
            let listed = list.await;
            this.update(cx, |this, cx| {
                if cwd != this.sidebar_cwd {
                    return;
                }
                match listed {
                    Ok(rows) => {
                        this.rows = replace_project_history(&this.rows, &cwd, rows);
                        this.loaded_projects.insert(key);
                    }
                    Err(_) => {
                        if !this.loaded_projects.contains(&key) {
                            this.error_cwd = Some(key);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The Inbox opened: list every saved session with a linked work item.
    /// Already-loaded and live sessions stay as the fallback on failure.
    pub fn refresh_linked_sessions(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let list = Engine::writer(cx).list_linked_sessions();
        cx.spawn(async move |this, cx| {
            if let Ok(rows) = list.await {
                this.update(cx, |this, cx| {
                    this.stored_linked_sessions = rows;
                    cx.notify();
                })
                .ok();
            }
        })
    }

    /// Change rows for packages outside history (`setHistory`), such as a
    /// worktree removal detaching its sessions.
    pub fn update_rows(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Vec<SessionSummary>),
    ) {
        update(&mut self.rows);
        cx.notify();
    }

    /// Change the linked rows (`setStoredLinkedSessions`).
    pub fn update_linked_sessions(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Vec<SessionSummary>),
    ) {
        update(&mut self.stored_linked_sessions);
        cx.notify();
    }

    /// Patch rows and linked rows together (`setHistory` and
    /// `setStoredLinkedSessions` with the same map). `patch` returns `None`
    /// to keep a row. The projects package calls this for worktree changes.
    pub fn patch_summaries(
        &mut self,
        patch: &dyn Fn(&SessionSummary) -> Option<SessionSummary>,
        cx: &mut Context<Self>,
    ) {
        for row in self
            .rows
            .iter_mut()
            .chain(self.stored_linked_sessions.iter_mut())
        {
            if let Some(next) = patch(row) {
                *row = next;
            }
        }
        cx.notify();
    }

    /// `setLoadedProjects` after a project folder moved: the listing of
    /// `from` now counts for `to`.
    pub fn rebase_loaded_project(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        self.loaded_projects.remove(&normalize_project_path(from));
        self.loaded_projects.insert(normalize_project_path(to));
        cx.notify();
    }

    fn on_sessions_event(
        &mut self,
        _sessions: Entity<Sessions>,
        event: &SessionsEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SessionsEvent::Persisted(summary) => {
                // `persistSession` merged only rows of the sidebar project,
                // compared as written.
                if summary.cwd == self.sidebar_cwd {
                    self.rows = merge_project_history_summary(&self.rows, (**summary).clone());
                    cx.notify();
                }
            }
            SessionsEvent::LoadFailed { .. } => {
                let cwd = self.sidebar_cwd.clone();
                self.refresh(&cwd, cx);
            }
            _ => {}
        }
    }

    // Opening.

    /// `onSelectHistorySession`: open a stored session, or its lead for an
    /// orchestration worker, and show it.
    pub fn select_session(&mut self, session_id: &str, cx: &mut Context<Self>) -> Task<()> {
        let id = session_id.to_string();
        let host = self.host.clone();
        cx.spawn(async move |_, cx| {
            let Some(mut session) = ensure_open(&id, cx).await else {
                return;
            };
            if session.inbox_ask.is_some() {
                return;
            }
            let parent = session
                .orchestration_lead_id
                .clone()
                .or_else(|| cx.update(|cx| host.run_lead_for_session(&id, cx)));
            if let Some(parent) = parent.filter(|parent| *parent != id) {
                cx.update(|cx| host.inspect_worker(&id, cx));
                let Some(lead) = ensure_open(&parent, cx).await else {
                    return;
                };
                session = lead;
            }
            cx.update(|cx| {
                host.open_session(&session, cx);
                host.reveal_linked_update(&session.id, cx);
            });
        })
    }

    /// `onSessionNavigationOrder`.
    pub fn set_navigation_order(&mut self, ids: Vec<String>) {
        self.navigation_ids = ids;
    }

    pub fn navigation_ids(&self) -> &[String] {
        &self.navigation_ids
    }

    /// `onNavigateSessionList`: step through the sidebar order, opening the
    /// next session in a tab or (`in_current_tab`) in the focused pane.
    pub fn navigate_session_list(
        &mut self,
        delta: i64,
        in_current_tab: bool,
        cx: &mut Context<Self>,
    ) {
        let host = self.host.clone();
        let Some(active) = host.active_pane(cx).filter(|active| !active.diff_focused) else {
            return;
        };
        let Some(current) = Engine::sessions(cx)
            .read(cx)
            .get(&active.focused_id)
            .cloned()
        else {
            return;
        };
        let remote = is_remote_project_path(&current.cwd);
        let navigation_id = if remote {
            host.remote_session_for(&current.id, cx)
        } else {
            Some(current.id.clone())
        };
        let Some(navigation_id) = navigation_id else {
            return;
        };
        let ids = self.navigation_ids.clone();
        let Some(next) = adjacent_item_id(&ids, Some(&navigation_id), delta)
            .filter(|next| *next != navigation_id)
        else {
            return;
        };
        if remote {
            if in_current_tab {
                host.remember_remote_session(&current.id, &next, cx);
            } else {
                host.select_remote_session(&current.cwd, &next, cx);
            }
            return;
        }
        // Stepping gives no hover to warm the transcript, so load the one a
        // further step away once this switch has its own session.
        let prefetch_ahead = {
            let next = next.clone();
            let current_id = current.id.clone();
            move |cx: &mut App| {
                let ahead = adjacent_item_id(&ids, Some(&next), delta);
                if let Some(ahead) = ahead.filter(|ahead| *ahead != current_id) {
                    Engine::sessions(cx).update(cx, |sessions, cx| sessions.prefetch(&ahead, cx));
                }
            }
        };
        if !in_current_tab {
            let select = self.select_session(&next, cx);
            cx.spawn(async move |_, cx| {
                select.await;
                cx.update(prefetch_ahead);
            })
            .detach();
            return;
        }
        let focused_id = current.id;
        cx.spawn(async move |_, cx| {
            let session = ensure_open(&next, cx).await;
            cx.update(|cx| {
                prefetch_ahead(cx);
                let Some(session) = session.filter(|session| session.inbox_ask.is_none()) else {
                    return;
                };
                let now = host.active_pane(cx);
                if now.as_ref().map(|pane| pane.tab_id.as_str()) != Some(active.tab_id.as_str()) {
                    return;
                }
                if now.as_ref().map(|pane| pane.focused_id.as_str()) != Some(focused_id.as_str()) {
                    return;
                }
                host.switch_session_in_tab(&active.tab_id, &focused_id, &session.id, cx);
                host.reveal_linked_update(&session.id, cx);
            });
        })
        .detach();
    }

    // Renaming.

    /// `onRenameHistorySession`: retitle an open or stored session.
    pub fn rename_session(
        &mut self,
        session_id: &str,
        display_title: &str,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let trimmed = monocode_core::js::trim(display_title).to_string();
        if trimmed.is_empty() {
            return Task::ready(());
        }
        let id = session_id.to_string();
        let sessions = Engine::sessions(cx);
        let renamed_open = sessions.update(cx, |sessions, cx| {
            sessions.invalidate_loaded(&id);
            let open = sessions.get(&id)?.clone();
            let title = format_session_title(open.harness, &trimmed);
            sessions.update(&id, cx, |session| session.title = title);
            sessions.persist(&id, cx);
            Some(())
        });
        if renamed_open.is_some() {
            let cwd = self.sidebar_cwd.clone();
            self.refresh(&cwd, cx);
            return Task::ready(());
        }
        let stored = get_stored_session(&id, cx);
        cx.spawn(async move |this, cx| {
            let Some(mut restored) = stored.await else {
                refresh_sidebar(&this, cx);
                return;
            };
            restored.title = format_session_title(restored.harness, &trimmed);
            let upsert = cx.update(|cx| Engine::writer(cx).upsert_session(&restored));
            if let Ok(Some(_)) = upsert.await {
                cx.update(|cx| {
                    Engine::sessions(cx).update(cx, |sessions, _| {
                        // TODO(port): App.tsx set only `lastPersisted`;
                        // `Sessions` has no setter for it alone.
                        sessions.adopt_restored(std::slice::from_ref(&restored));
                        sessions.remember_loaded(restored);
                    })
                });
            }
            refresh_sidebar(&this, cx);
        })
    }

    // Removing.

    /// `onRemoveHistorySession`: archive or delete a session, closing or
    /// replacing its panes. Resolves to whether it went away.
    pub fn remove_session(
        &mut self,
        session_id: &str,
        mode: SessionRemovalMode,
        skip_delete_confirm: bool,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        let sessions = Engine::sessions(cx);
        let (blocked, open) = {
            let state = sessions.read(cx);
            (
                state.is_removing(session_id) || state.switching_worktree(session_id).is_some(),
                state.get(session_id).cloned(),
            )
        };
        if blocked || self.delete_confirmation_pending {
            return Task::ready(false);
        }
        let summary = self.rows.iter().find(|row| row.id == session_id).cloned();
        let seed = Seed::from(open.as_ref(), summary.as_ref());
        let label = seed
            .as_ref()
            .map(|seed| session_display_title(&seed.title, seed.harness))
            .unwrap_or_else(|| "this session".into());
        sessions.update(cx, |sessions, _| sessions.begin_removal(session_id));
        let host = self.host.clone();
        let id = session_id.to_string();
        let sidebar_cwd = self.sidebar_cwd.clone();
        cx.spawn(async move |this, cx| {
            let mut delete_worktree_path = None;
            if mode == SessionRemovalMode::Delete && !skip_delete_confirm {
                set_delete_pending(&this, cx, true);
                let unused = match seed.as_ref().and_then(|seed| {
                    seed.worktree_cwd
                        .clone()
                        .filter(|w| !w.is_empty())
                        .map(|w| (seed.cwd.clone(), w))
                }) {
                    Some((cwd, worktree)) => {
                        cx.update(|cx| host.unused_worktree(&cwd, &worktree, &id, cx))
                            .await
                    }
                    None => None,
                };
                match unused {
                    None => set_delete_pending(&this, cx, false),
                    Some(unused) => {
                        let choice = cx.update(|cx| host.choose_delete(&label, &unused, cx)).await;
                        set_delete_pending(&this, cx, false);
                        if !choice.confirmed {
                            end_removal(&id, cx);
                            return false;
                        }
                        if choice.delete_worktree {
                            delete_worktree_path = Some(unused);
                        }
                    }
                }
            }
            cx.update(|cx| {
                Engine::sessions(cx).update(cx, |sessions, _| sessions.invalidate_loaded(&id));
            });
            let adapter = Rc::new(HistoryRemoval {
                history: this.clone(),
                host: host.clone(),
                session_id: id.clone(),
                summary,
            });
            let scope = cx.update(|cx| host.tab_close_scope(cx));
            let remover = create_session_remover(SessionRemovalOptions {
                mode,
                scope,
                replacement: ReplacementSeed {
                    harness: Some(seed.as_ref().map(|seed| seed.harness).unwrap_or(HarnessId::Cursor)),
                    cwd: seed.as_ref().map(|seed| seed.cwd.clone()).unwrap_or(sidebar_cwd),
                    model: seed.as_ref().map(|seed| seed.model.clone()),
                    runtime_mode: seed.as_ref().map(|seed| seed.runtime_mode),
                    model_settings: open.as_ref().map(|open| open.model_settings.clone()),
                },
                adapter,
            });
            let result = cx.update(|cx| remover.remove(&id, cx)).await;
            let removed = match result {
                Ok(removed) => {
                    if removed
                        && let (Some(path), Some(seed)) = (delete_worktree_path, seed.as_ref())
                    {
                        let removal = cx.update(|cx| host.remove_worktree(&seed.cwd, &path, false, cx));
                        if let Err(error) = removal.await {
                            cx.update(|cx| {
                                host.alert(
                                    &format!(
                                        "The session was deleted. Its worktree was kept.\n\n{error}\n\nYou can manage it in Settings → Worktrees."
                                    ),
                                    AlertKind::Warning,
                                    cx,
                                )
                            });
                        }
                    }
                    removed
                }
                Err(detail) => {
                    cx.update(|cx| {
                        host.alert(
                            &format!("Could not {} this conversation.\n\n{detail}", mode.verb()),
                            AlertKind::Error,
                            cx,
                        )
                    });
                    false
                }
            };
            end_removal(&id, cx);
            removed
        })
    }

    /// `onArchiveHistorySession`: archive through removal; unarchive in place.
    pub fn archive_session(
        &mut self,
        session_id: &str,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        if archived {
            return self.remove_session(session_id, SessionRemovalMode::Archive, false, cx);
        }
        if Engine::sessions(cx).read(cx).is_removing(session_id) {
            return Task::ready(false);
        }
        let write = Engine::writer(cx).set_session_archived(session_id, false);
        let id = session_id.to_string();
        let host = self.host.clone();
        cx.spawn(async move |this, cx| match write.await {
            Ok(()) => {
                this.update(cx, |this, cx| {
                    for row in &mut this.rows {
                        if row.id == id {
                            row.archived = Some(false);
                        }
                    }
                    cx.notify();
                })
                .ok();
                true
            }
            Err(error) => {
                cx.update(|cx| {
                    host.alert(
                        &format!("Could not unarchive this conversation.\n\n{error}"),
                        AlertKind::Error,
                        cx,
                    )
                });
                false
            }
        })
    }

    /// `onArchiveHistorySessions`: one at a time, stopping at the first that
    /// does not go.
    pub fn archive_sessions(
        &mut self,
        session_ids: Vec<String>,
        archived: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            for id in session_ids {
                let Ok(step) = this.update(cx, |this, cx| this.archive_session(&id, archived, cx))
                else {
                    return;
                };
                if !step.await {
                    break;
                }
            }
        })
    }

    /// `onArchiveFocusedSession`: `true` when the key archived the focused
    /// session, and the caller should stop it from propagating.
    pub fn archive_focused_session(
        &mut self,
        event: &ArchiveKeyEvent,
        context: &ArchiveContext,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session_id) = archive_focused_session(event, context) else {
            return false;
        };
        self.archive_session(&session_id, true, cx).detach();
        true
    }

    /// `onPinHistorySession`: save an open session first so the pin has a
    /// row, then pin it.
    pub fn pin_session(
        &mut self,
        session_id: &str,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let open = Engine::sessions(cx).read(cx).get(session_id).cloned();
        let upsert = open
            .as_ref()
            .filter(|open| should_persist_session(open))
            .map(|open| Engine::writer(cx).upsert_session(open));
        let id = session_id.to_string();
        cx.spawn(async move |this, cx| {
            if let Some(upsert) = upsert {
                let _ = upsert.await;
            }
            let write = cx.update(|cx| Engine::writer(cx).set_session_pinned(&id, pinned));
            let _ = write.await;
            this.update(cx, |this, cx| {
                if let Some(existing) = this.rows.iter().find(|row| row.id == id).cloned() {
                    this.rows = merge_project_history_summary(
                        &this.rows,
                        SessionSummary {
                            pinned: Some(pinned),
                            ..existing
                        },
                    );
                } else if let Some(open) = open.as_ref() {
                    this.rows = merge_project_history_summary(
                        &this.rows,
                        SessionSummary {
                            pinned: Some(pinned),
                            ..summary_from_session(open, None)
                        },
                    );
                }
                cx.notify();
            })
            .ok();
        })
    }

    /// `onPinHistorySessions`: pin all at once.
    pub fn pin_sessions(
        &mut self,
        session_ids: &[String],
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let pins: Vec<Task<()>> = session_ids
            .iter()
            .map(|id| self.pin_session(id, pinned, cx))
            .collect();
        cx.spawn(async move |_, _| {
            futures::future::join_all(pins).await;
        })
    }

    /// `onSetHistorySessionLinkedWorkItem`: link or unlink at once, and roll
    /// back if the store refuses.
    pub fn set_linked_work_item(
        &mut self,
        session_id: &str,
        item: Option<LinkedWorkItem>,
        cx: &mut Context<Self>,
    ) {
        let sessions = Engine::sessions(cx);
        let previous = sessions
            .read(cx)
            .get(session_id)
            .and_then(|session| session.linked_work_item.clone())
            .or_else(|| {
                self.rows
                    .iter()
                    .find(|row| row.id == session_id)
                    .and_then(|row| row.linked_work_item.clone())
            });
        sessions.update(cx, |sessions, cx| {
            sessions.invalidate_loaded(session_id);
            let next = item.clone();
            sessions.update(session_id, cx, |session| session.linked_work_item = next);
        });
        for row in &mut self.rows {
            if row.id == session_id {
                row.linked_work_item = item.clone();
            }
        }
        set_linked(&mut self.stored_linked_sessions, session_id, item.as_ref());
        self.host.linked_work_item_changed(session_id, cx);
        cx.notify();

        let write = Engine::writer(cx).set_session_linked_work_item(session_id, item.as_ref());
        let id = session_id.to_string();
        let host = self.host.clone();
        cx.spawn(async move |this, cx| {
            let Err(error) = write.await else {
                return;
            };
            cx.update(|cx| {
                Engine::sessions(cx).update(cx, |sessions, cx| {
                    if sessions
                        .get(&id)
                        .is_some_and(|session| session.linked_work_item == item)
                    {
                        let previous = previous.clone();
                        sessions.update(&id, cx, |session| session.linked_work_item = previous);
                    }
                });
            });
            this.update(cx, |this, cx| {
                for row in &mut this.rows {
                    if row.id == id && row.linked_work_item == item {
                        row.linked_work_item = previous.clone();
                    }
                }
                set_linked(&mut this.stored_linked_sessions, &id, previous.as_ref());
                let cwd = this.sidebar_cwd.clone();
                this.refresh(&cwd, cx);
                cx.notify();
            })
            .ok();
            cx.update(|cx| {
                host.alert(
                    &format!("Could not update this conversation's GitHub link.\n\n{error}"),
                    AlertKind::Error,
                    cx,
                )
            });
        })
        .detach();
    }

    /// `onDeleteHistorySession`: delete with the worktree dialog.
    pub fn delete_session(&mut self, session_id: &str, cx: &mut Context<Self>) -> Task<bool> {
        self.remove_session(session_id, SessionRemovalMode::Delete, false, cx)
    }

    /// `onDeleteHistorySessions`: confirm once, then delete one at a time.
    pub fn delete_sessions(
        &mut self,
        session_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if session_ids.is_empty() {
            return Task::ready(());
        }
        let confirm = self.host.confirm(
            &format!(
                "Delete {} selected conversations? This can’t be undone.",
                session_ids.len()
            ),
            cx,
        );
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            for id in session_ids {
                let Ok(step) = this.update(cx, |this, cx| {
                    this.remove_session(&id, SessionRemovalMode::Delete, true, cx)
                }) else {
                    return;
                };
                if !step.await {
                    break;
                }
            }
        })
    }

    /// `onDeleteWorktreeSessions`: delete without asking; `false` at the
    /// first that does not go.
    pub fn delete_worktree_sessions(
        &mut self,
        session_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        cx.spawn(async move |this, cx| {
            for id in session_ids {
                let Ok(step) = this.update(cx, |this, cx| {
                    this.remove_session(&id, SessionRemovalMode::Delete, true, cx)
                }) else {
                    return false;
                };
                if !step.await {
                    return false;
                }
            }
            true
        })
    }

    /// `sessionIdsInTitleTab`: open sessions in a tab, in layout order.
    pub fn session_ids_in_tab(&self, tab_id: &str, cx: &App) -> Vec<String> {
        let tabs = self.host.workspace_tabs(cx).tabs;
        let Some(tab) = tabs.iter().find(|tab| tab.id == tab_id) else {
            return Vec::new();
        };
        let sessions = Engine::sessions(cx);
        let open = sessions.read(cx);
        leaf_ids(&tab.layout)
            .into_iter()
            .filter(|id| open.contains(id))
            .collect()
    }

    /// `onArchiveTitleTab`.
    pub fn archive_title_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let ids = self.session_ids_in_tab(tab_id, cx);
        self.archive_sessions(ids, true, cx).detach();
    }

    /// `onDeleteTitleTab`.
    pub fn delete_title_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let ids = self.session_ids_in_tab(tab_id, cx);
        match ids.len() {
            0 => {}
            1 => self.delete_session(&ids[0], cx).detach(),
            _ => self.delete_sessions(ids, cx).detach(),
        }
    }

    // Removal state changes.

    fn apply_removal_change(
        &mut self,
        session_id: &str,
        summary: Option<&SessionSummary>,
        change: WorkspaceChange,
        cx: &mut Context<Self>,
    ) {
        let sessions = Engine::sessions(cx);
        match change {
            WorkspaceChange::Stopped(stopped) => {
                sessions.update(cx, |sessions, cx| {
                    sessions.update(session_id, cx, |session| *session = stopped);
                });
            }
            WorkspaceChange::OrchestrationReleased { lead_id } => {
                sessions.update(cx, |sessions, cx| {
                    sessions.update_all(cx, |session| {
                        release_orchestration_worker(session, &lead_id)
                    });
                    let cached: Vec<String> = sessions
                        .loaded_cache()
                        .ids()
                        .into_iter()
                        .filter(|id| {
                            sessions.loaded_cache().get(id).is_some_and(|cached| {
                                release_orchestration_worker(cached, &lead_id).is_some()
                            })
                        })
                        .map(str::to_string)
                        .collect();
                    for id in cached {
                        sessions.invalidate_loaded(&id);
                    }
                    sessions.invalidate_pending_loads();
                });
                let release = |row: &mut SessionSummary| {
                    if row.orchestration_lead_id.as_deref() == Some(lead_id.as_str()) {
                        row.orchestration_lead_id = None;
                    }
                };
                self.rows.iter_mut().for_each(release);
                self.stored_linked_sessions.iter_mut().for_each(release);
                cx.notify();
            }
            WorkspaceChange::Removed {
                mode,
                removal,
                session,
                saved_summary,
            } => {
                sessions.update(cx, |sessions, cx| {
                    sessions.forget_persisted(session_id);
                    let open: HashSet<String> = sessions.ids().into_iter().collect();
                    let kept: HashSet<&str> =
                        removal.sessions.iter().map(|s| s.id.as_str()).collect();
                    sessions.retain(cx, |open| kept.contains(open.id.as_str()));
                    for added in removal.sessions.iter().filter(|s| !open.contains(&s.id)) {
                        sessions.insert(added.clone(), cx);
                    }
                    if mode == SessionRemovalMode::Archive
                        && let Some(session) =
                            session.as_ref().filter(|s| should_persist_session(s))
                    {
                        sessions.remember_loaded(session.clone());
                    }
                });
                self.host.commit_removal(&removal, cx);
                match mode {
                    SessionRemovalMode::Archive => {
                        let archived = saved_summary
                            .map(|saved| *saved)
                            .or_else(|| summary.cloned())
                            .or_else(|| session.as_ref().map(|s| summary_from_session(s, None)));
                        if let Some(archived) = archived {
                            self.rows = merge_history_summary(
                                &self.rows,
                                SessionSummary {
                                    archived: Some(true),
                                    ..archived
                                },
                            );
                        }
                    }
                    SessionRemovalMode::Delete => {
                        self.rows.retain(|row| row.id != session_id);
                        let cwd = self.sidebar_cwd.clone();
                        self.refresh(&cwd, cx);
                    }
                }
                cx.notify();
            }
        }
    }
}

/// The fields of the open session or its row that removal reads.
struct Seed {
    title: String,
    harness: HarnessId,
    cwd: String,
    worktree_cwd: Option<String>,
    model: String,
    runtime_mode: RuntimeMode,
}

impl Seed {
    fn from(open: Option<&Session>, summary: Option<&SessionSummary>) -> Option<Self> {
        if let Some(open) = open {
            return Some(Self {
                title: open.title.clone(),
                harness: open.harness,
                cwd: open.cwd.clone(),
                worktree_cwd: open.worktree_cwd.clone(),
                model: open.model.clone(),
                runtime_mode: open.runtime_mode,
            });
        }
        summary.map(|row| Self {
            title: row.title.clone(),
            harness: row.harness,
            cwd: row.cwd.clone(),
            worktree_cwd: row.worktree_cwd.clone(),
            model: row.model.clone(),
            runtime_mode: row.runtime_mode,
        })
    }
}

/// The `workspace` adapter `onRemoveHistorySession` gave the remover.
struct HistoryRemoval {
    history: WeakEntity<History>,
    host: Rc<dyn HistoryHost>,
    session_id: String,
    summary: Option<SessionSummary>,
}

impl RemovalAdapter for HistoryRemoval {
    fn snapshot(&self, cx: &App) -> RemovalWorkspace {
        let tabs = self.host.workspace_tabs(cx);
        RemovalWorkspace {
            tabs: tabs.tabs,
            sessions: Engine::sessions(cx).read(cx).all().to_vec(),
            active_tab_id: tabs.active_tab_id,
            dirty_files: tabs.dirty_files,
        }
    }

    fn apply(&self, change: WorkspaceChange, cx: &mut App) {
        let summary = self.summary.clone();
        let id = self.session_id.clone();
        self.history
            .update(cx, |history, cx| {
                history.apply_removal_change(&id, summary.as_ref(), change, cx)
            })
            .ok();
    }

    fn confirm(
        &self,
        tabs: Vec<WorkspaceTab>,
        mode: SessionRemovalMode,
        cx: &mut App,
    ) -> Task<bool> {
        self.host.confirm_removal(&tabs, mode, cx)
    }

    fn stop(&self, session_id: &str, cx: &mut App) -> Task<()> {
        let stop = Engine::sessions(cx)
            .update(cx, |sessions, cx| sessions.stop_for_removal(session_id, cx));
        cx.spawn(async move |_| {
            stop.await;
        })
    }

    fn create_session(&self, seed: &ReplacementSeed, cx: &App) -> Session {
        self.host.new_session(seed, cx)
    }

    fn stop_active_run(&self, session_id: &str, cx: &mut App) -> Task<()> {
        self.host.stop_active_run(session_id, cx)
    }

    fn delete_session(
        &self,
        session_id: &str,
        remove: DeleteSession,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        self.host.delete_session(session_id, remove, cx)
    }

    fn finish_preparing_handoff(&self, session: &Session) -> Option<Session> {
        self.host.finish_preparing_handoff(session)
    }
}

/// `ensureOpenSession` from an async context.
async fn ensure_open(session_id: &str, cx: &mut AsyncApp) -> Option<Session> {
    let open = cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| sessions.ensure_open(session_id, cx))
    });
    open.await
}

fn end_removal(session_id: &str, cx: &mut AsyncApp) {
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, _| sessions.end_removal(session_id));
    });
}

fn set_delete_pending(this: &WeakEntity<History>, cx: &mut AsyncApp, pending: bool) {
    this.update(cx, |this, _| this.delete_confirmation_pending = pending)
        .ok();
}

fn refresh_sidebar(this: &WeakEntity<History>, cx: &mut AsyncApp) {
    this.update(cx, |this, cx| {
        let cwd = this.sidebar_cwd.clone();
        this.refresh(&cwd, cx);
    })
    .ok();
}

/// The `setStoredLinkedSessions` update for a link change: keep and update
/// the row while linked, drop it when unlinked.
fn set_linked(rows: &mut Vec<SessionSummary>, session_id: &str, item: Option<&LinkedWorkItem>) {
    match item {
        Some(item) => {
            for row in rows.iter_mut() {
                if row.id == session_id {
                    row.linked_work_item = Some(item.clone());
                }
            }
        }
        None => rows.retain(|row| row.id != session_id),
    }
}

/// `releaseOrchestrationWorker` from orchestrationWorkspace.ts: drop the
/// lead from a worker and its blocks. `None` when the session has no tie to
/// this lead.
pub fn release_orchestration_worker(session: &Session, lead_id: &str) -> Option<Session> {
    if session.orchestration_lead_id.as_deref() != Some(lead_id)
        && !session
            .blocks
            .iter()
            .any(|block| block.orchestration_lead_id.as_deref() == Some(lead_id))
    {
        return None;
    }
    let mut next = session.clone();
    if next.orchestration_lead_id.as_deref() == Some(lead_id) {
        next.orchestration_lead_id = None;
    }
    for block in &mut next.blocks {
        if block.orchestration_lead_id.as_deref() == Some(lead_id) {
            block.orchestration_lead_id = None;
        }
    }
    Some(next)
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
