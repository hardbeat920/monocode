//! The sidebar session list: folders, the pinned and reminder groups,
//! filters, search, paging, and multi-select. Port of the model state in
//! src/app/shell/Sidebar.tsx (lines 534-1010, 1200-1480): the derived list,
//! its effects (folder pruning, selection pruning, the 30-second clock), and
//! the session and folder menu actions.
//!
//! Menu positions, the drawer, and the remote-project branches stay in the
//! view; it passes remote rows in as `project_sessions` and routes remote
//! actions to the remote package.

use std::collections::HashSet;
use std::time::Duration;

use gpui::{Context, Task};
use monocode_core::HarnessId;
use monocode_settings::Kv;

use super::history::History;
use super::session_filters::{
    SessionSidebarFilters, filter_sessions_by_harness, filter_sessions_by_status,
    filter_sessions_by_time, harnesses_in_sessions, has_active_session_filters,
    load_session_sidebar_filters, save_session_sidebar_filters,
};
use super::session_folders::{
    ReminderGroup, SessionFolder, SessionFolderTarget, SessionListDropTarget, SessionListEntry,
    add_session_to_folder, apply_session_list_drop, build_session_list,
    create_folder_with_sessions, dissolve_folder, folder_containing,
    load_pinned_sessions_collapsed, load_reminder_sessions_collapsed, load_session_folders,
    merge_folder_session_summaries, place_session_in_folder, prune_session_folders,
    remove_session_from_folder, rename_folder, reorder_session_folders,
    save_pinned_sessions_collapsed, save_reminder_sessions_collapsed, save_session_folders,
    session_list_navigation_ids, set_folder_collapsed, set_folder_color, set_folder_custom_color,
    subscribe_session_folders, ungrouped_sessions,
};
use super::session_selection::{
    ordered_session_action_ids, prune_session_selection, toggle_session_selection, unique_selection,
};
use crate::runtime::reducer::now_ms;
use crate::runtime::session_history::{
    compare_session_summaries, filter_sessions_by_archive, filter_sessions_by_query,
};
use crate::runtime::session_store::SessionSummary;
use crate::runtime::util::list_window::{LIST_PAGE_SIZE, list_window_size};

/// How often relative times and the time filter move on.
pub const SIDEBAR_CLOCK_INTERVAL: Duration = Duration::from_secs(30);

/// The sidebar list state that Sidebar.tsx kept in React state.
pub struct SidebarState {
    pub folders: Vec<SessionFolder>,
    pub pinned_collapsed: bool,
    pub reminders_collapsed: bool,
    pub filters: SessionSidebarFilters,
    pub search_query: String,
    /// `sessionListLimit`: how many ungrouped cards are mounted.
    pub list_limit: usize,
    /// The multi-selection, in the order cards were added.
    pub selected: Vec<String>,
    selection_anchor: Option<String>,
    /// The context menu selected its card itself, so closing it clears it.
    context_selection: bool,
    /// The card whose context menu is open.
    pub session_menu: Option<String>,
    pub renaming_session_id: Option<String>,
    pub renaming_folder_id: Option<String>,
    /// `sessionDrop`: where a dragged card would land.
    pub drop_target: Option<SessionListDropTarget>,
    /// Sessions created inside a folder that history has not listed yet.
    pending_folder_session_ids: Vec<String>,
    /// The sessions tab is the sidebar's visible tab.
    pub sessions_tab_active: bool,
    /// `now`: the clock the time filter and relative times read.
    pub now: i64,
    clock: Option<Task<()>>,
    folder_watch: Option<(monocode_settings::Subscription, Task<()>)>,
}

impl SidebarState {
    pub(super) fn new(kv: &Kv) -> Self {
        Self {
            folders: Vec::new(),
            pinned_collapsed: false,
            reminders_collapsed: false,
            filters: load_session_sidebar_filters(kv),
            search_query: String::new(),
            list_limit: LIST_PAGE_SIZE,
            selected: Vec::new(),
            selection_anchor: None,
            context_selection: false,
            session_menu: None,
            renaming_session_id: None,
            renaming_folder_id: None,
            drop_target: None,
            pending_folder_session_ids: Vec::new(),
            sessions_tab_active: false,
            now: now_ms(),
            clock: None,
            folder_watch: None,
        }
    }
}

/// A reminder, as the list reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderDue {
    pub session_id: String,
    pub due_at: i64,
}

/// What the derived list needs from outside history.
#[derive(Debug, Clone, Copy)]
pub struct SessionListInput<'a> {
    /// The project's rows: `History::sidebar_history`, or the remote host's
    /// sessions as rows.
    pub project_sessions: &'a [SessionSummary],
    /// Open chats of the project (`History::open_project_sessions`); empty
    /// for a remote project.
    pub open_sessions: &'a [SessionSummary],
    pub busy_ids: &'a HashSet<String>,
    pub approval_ids: &'a HashSet<String>,
    pub unseen_finished_ids: &'a HashSet<String>,
    pub reminders: &'a [ReminderDue],
    /// The focused session as the list knows it (the host id for a remote
    /// project).
    pub active_listed_session_id: Option<&'a str>,
    /// The focused session's local id.
    pub active_session_id: Option<&'a str>,
    pub remote: bool,
    /// The project's first listing has not arrived (`pending`).
    pub listing_pending: bool,
    /// The project's listing failed (`status === "error"`).
    pub listing_failed: bool,
    /// A remote project's sessions have loaded (`remote.loaded`).
    pub remote_loaded: bool,
}

/// The derived list for one render.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionListView {
    /// `listedSessions`: the project's rows plus open folder members.
    pub listed: Vec<SessionSummary>,
    /// `visibleSessions`: after the filters and the search.
    pub visible: Vec<SessionSummary>,
    /// The mounted entries: folders, groups, and a page of loose cards.
    pub entries: Vec<SessionListEntry>,
    /// The keyboard order over the whole list.
    pub navigation_ids: Vec<String>,
    /// The order over the mounted entries, for shift-click ranges.
    pub mounted_navigation_ids: Vec<String>,
    pub has_more: bool,
    pub filters_active: bool,
    pub search_narrowed: bool,
    /// `narrowedByUser`: a search or a filter hides sessions.
    pub narrowed_by_user: bool,
    /// Providers present in the project, for the filter menu.
    pub harnesses: Vec<HarnessId>,
    pub reminder_ids: HashSet<String>,
    /// `visibleFolderIds`: folders shown, for drag reordering.
    pub folder_ids: Vec<String>,
}

/// Modifier keys of a card click.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CardClick {
    pub shift: bool,
    /// Ctrl, or Cmd on macOS.
    pub toggle: bool,
}

/// What the session context menu shows for its sessions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionMenuState {
    /// `menuSessionIds`.
    pub session_ids: Vec<String>,
    pub sessions: Vec<SessionSummary>,
    pub all_pinned: bool,
    pub all_archived: bool,
    /// The folder of a single menu session.
    pub folder: Option<SessionFolder>,
    pub can_remove_from_folders: bool,
    /// Each folder and whether every menu session is in it.
    pub folders_checked: Vec<(String, bool)>,
}

/// The model actions of the session context menu. Reminders, copying ids,
/// and linking work items belong to other packages and stay in the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMenuAction {
    TogglePin,
    Rename,
    NewFolder,
    AddToFolder(String),
    RemoveFromFolders,
    ToggleArchive,
    Delete,
}

impl History {
    /// The sidebar list state.
    pub fn sidebar(&self) -> &SidebarState {
        &self.sidebar
    }

    /// The `[cwd]` effects of Sidebar.tsx: load the project's folders and
    /// group state and listen for folder changes.
    pub(super) fn sidebar_project_changed(&mut self, cx: &mut Context<Self>) {
        let cwd = self.sidebar_cwd.clone();
        self.sidebar.folders = load_session_folders(&self.kv, &cwd);
        self.sidebar.pinned_collapsed = load_pinned_sessions_collapsed(&self.kv, &cwd);
        self.sidebar.reminders_collapsed = load_reminder_sessions_collapsed(&self.kv, &cwd);
        self.sidebar.renaming_folder_id = None;
        self.sidebar.drop_target = None;
        self.sidebar.pending_folder_session_ids.clear();
        self.sidebar.list_limit = LIST_PAGE_SIZE;
        self.sidebar.folder_watch = None;
        let (sender, receiver) = async_channel::unbounded::<()>();
        let Some(subscription) = subscribe_session_folders(&self.kv, &cwd, move || {
            let _ = sender.try_send(());
        }) else {
            return;
        };
        let watch = cx.spawn(async move |this, cx| {
            while receiver.recv().await.is_ok() {
                let reloaded = this.update(cx, |this, cx| {
                    this.sidebar.folders = load_session_folders(&this.kv, &this.sidebar_cwd);
                    cx.notify();
                });
                if reloaded.is_err() {
                    break;
                }
            }
        });
        self.sidebar.folder_watch = Some((subscription, watch));
    }

    /// `commitSessionFolders`.
    fn commit_folders(&mut self, next: Vec<SessionFolder>, cx: &mut Context<Self>) {
        save_session_folders(&self.kv, &self.sidebar_cwd, &next);
        self.sidebar.folders = next;
        cx.notify();
    }

    // The derived list.

    /// The list for one render, from the sidebar state and `input`.
    pub fn session_list(&self, input: &SessionListInput<'_>) -> SessionListView {
        let state = &self.sidebar;
        let listed: Vec<SessionSummary> = merge_folder_session_summaries(
            input.project_sessions,
            input.open_sessions,
            &state.folders,
        )
        .into_iter()
        .filter(|session| session.orchestration_lead_id.is_none())
        .collect();
        let filters = &state.filters;
        let mut visible = filter_sessions_by_query(
            &filter_sessions_by_status(
                &filter_sessions_by_time(
                    &filter_sessions_by_harness(
                        &filter_sessions_by_archive(&listed, filters.show_archived),
                        &filters.hidden_harnesses,
                    ),
                    filters.time,
                    state.now,
                ),
                filters.status,
                input.busy_ids,
                input.approval_ids,
                input.unseen_finished_ids,
            ),
            &state.search_query,
        );
        visible.sort_by(compare_session_summaries);
        let filters_active = has_active_session_filters(filters);
        let search_narrowed = !monocode_core::js::trim(&state.search_query).is_empty();
        let reminder_ids: HashSet<String> = input
            .reminders
            .iter()
            .map(|r| r.session_id.clone())
            .collect();
        let mut by_due: Vec<&ReminderDue> = input.reminders.iter().collect();
        by_due.sort_by_key(|reminder| reminder.due_at);
        let reminder_group = ReminderGroup {
            session_ids: by_due.iter().map(|r| r.session_id.clone()).collect(),
            collapsed: state.reminders_collapsed,
        };
        let ungrouped: Vec<SessionSummary> = ungrouped_sessions(&visible, &state.folders)
            .into_iter()
            .filter(|session| !reminder_ids.contains(&session.id))
            .collect();
        let active_index = input
            .active_listed_session_id
            .and_then(|id| ungrouped.iter().position(|session| session.id == id));
        let shown = list_window_size(ungrouped.len(), state.list_limit, active_index);
        let full = build_session_list(
            &visible,
            &state.folders,
            &ungrouped,
            state.pinned_collapsed,
            Some(&reminder_group),
        );
        let entries = build_session_list(
            &visible,
            &state.folders,
            &ungrouped[..shown],
            state.pinned_collapsed,
            Some(&reminder_group),
        );
        let folder_ids = entries
            .iter()
            .filter_map(|entry| match entry {
                SessionListEntry::Folder { folder, .. } => Some(folder.id.clone()),
                _ => None,
            })
            .collect();
        SessionListView {
            navigation_ids: session_list_navigation_ids(&full, search_narrowed),
            mounted_navigation_ids: session_list_navigation_ids(&entries, search_narrowed),
            has_more: shown < ungrouped.len(),
            filters_active,
            search_narrowed,
            narrowed_by_user: search_narrowed || filters_active,
            harnesses: harnesses_in_sessions(input.project_sessions),
            reminder_ids,
            folder_ids,
            listed,
            visible,
            entries,
        }
    }

    /// The effects that follow a render: publish the keyboard order, prune
    /// the selection, and drop folder members that no longer exist.
    pub fn sync_session_list(
        &mut self,
        view: &SessionListView,
        input: &SessionListInput<'_>,
        cx: &mut Context<Self>,
    ) {
        if self.navigation_ids != view.navigation_ids {
            self.navigation_ids = view.navigation_ids.clone();
        }
        if self.sidebar.sessions_tab_active {
            let available: HashSet<String> = view.navigation_ids.iter().cloned().collect();
            if self
                .sidebar
                .selection_anchor
                .as_ref()
                .is_some_and(|anchor| !available.contains(anchor))
            {
                self.sidebar.selection_anchor = None;
            }
            let pruned = prune_session_selection(&self.sidebar.selected, &available);
            if pruned != self.sidebar.selected {
                self.sidebar.selected = pruned;
                cx.notify();
            }
        }
        self.prune_folders(input, cx);
    }

    fn prune_folders(&mut self, input: &SessionListInput<'_>, cx: &mut Context<Self>) {
        if input.listing_pending || input.listing_failed {
            return;
        }
        if input.remote && !input.remote_loaded {
            return;
        }
        let mut known: HashSet<String> = input
            .project_sessions
            .iter()
            .map(|s| s.id.clone())
            .collect();
        let mut completed: Vec<(String, String)> = Vec::new();
        if !input.remote {
            known.extend(input.open_sessions.iter().map(|s| s.id.clone()));
        }
        if let Some(active) = input.active_listed_session_id {
            known.insert(active.to_string());
        }
        if input.remote
            && let Some(active) = input.active_session_id
        {
            known.insert(active.to_string());
        }
        if input.remote {
            for folder in &self.sidebar.folders {
                for shell_id in &folder.session_ids {
                    if let Some(host_id) = self.host.remote_session_for(shell_id, cx)
                        && known.contains(&host_id)
                    {
                        completed.push((shell_id.clone(), host_id));
                    }
                }
            }
        }
        let pending = std::mem::take(&mut self.sidebar.pending_folder_session_ids);
        let mut still_pending = Vec::new();
        for id in pending {
            known.insert(id.clone());
            let host_id = if input.remote {
                self.host.remote_session_for(&id, cx)
            } else {
                None
            };
            if let Some(host_id) = host_id.filter(|host_id| known.contains(host_id)) {
                completed.push((id, host_id));
                continue;
            }
            let listed = input.project_sessions.iter().any(|s| s.id == id)
                || input.open_sessions.iter().any(|s| s.id == id);
            if !listed {
                still_pending.push(id);
            }
        }
        self.sidebar.pending_folder_session_ids = still_pending;
        let current = self.sidebar.folders.clone();
        let migrated: Vec<SessionFolder> = if completed.is_empty() {
            current.clone()
        } else {
            current
                .iter()
                .map(|folder| SessionFolder {
                    session_ids: folder
                        .session_ids
                        .iter()
                        .map(|id| {
                            completed
                                .iter()
                                .find(|(shell, _)| shell == id)
                                .map(|(_, host)| host.clone())
                                .unwrap_or_else(|| id.clone())
                        })
                        .collect(),
                    ..folder.clone()
                })
                .collect()
        };
        let next = prune_session_folders(&migrated, &known);
        if next != current {
            self.commit_folders(next, cx);
        }
    }

    // Filters, search, and paging.

    /// `onSessionFiltersChange`.
    pub fn set_filters(&mut self, filters: SessionSidebarFilters, cx: &mut Context<Self>) {
        save_session_sidebar_filters(&self.kv, &filters);
        self.sidebar.filters = filters;
        self.sidebar.list_limit = LIST_PAGE_SIZE;
        cx.notify();
    }

    pub fn set_search_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.sidebar.search_query == query {
            return;
        }
        self.sidebar.search_query = query.to_string();
        self.sidebar.list_limit = LIST_PAGE_SIZE;
        cx.notify();
    }

    /// The load-more sentinel came into view.
    pub fn load_more_sessions(&mut self, cx: &mut Context<Self>) {
        self.sidebar.list_limit += LIST_PAGE_SIZE;
        cx.notify();
    }

    /// The sessions tab became visible or hidden. Leaving it clears the
    /// selection and the search; while it shows, the clock ticks.
    pub fn set_sessions_tab_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.sidebar.sessions_tab_active == active {
            return;
        }
        self.sidebar.sessions_tab_active = active;
        if !active {
            self.sidebar.clock = None;
            self.sidebar.selection_anchor = None;
            self.sidebar.selected.clear();
            self.sidebar.search_query.clear();
            cx.notify();
            return;
        }
        self.sidebar.now = now_ms();
        self.sidebar.clock = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SIDEBAR_CLOCK_INTERVAL).await;
                let ticked = this.update(cx, |this, cx| {
                    this.sidebar.now = now_ms();
                    cx.notify();
                });
                if ticked.is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    /// Collapse or expand the pinned group.
    pub fn set_pinned_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.sidebar.pinned_collapsed = collapsed;
        save_pinned_sessions_collapsed(&self.kv, &self.sidebar_cwd, collapsed);
        cx.notify();
    }

    /// Collapse or expand the reminder group.
    pub fn set_reminders_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.sidebar.reminders_collapsed = collapsed;
        save_reminder_sessions_collapsed(&self.kv, &self.sidebar_cwd, collapsed);
        cx.notify();
    }

    /// A reminder was set from the menu: show the reminder group.
    pub fn reminder_scheduled(&mut self, cx: &mut Context<Self>) {
        self.set_reminders_collapsed(false, cx);
    }

    // Folders.

    /// `onSessionListDrop`: join a folder or open a new one; a new folder
    /// starts renaming.
    pub fn drop_on_session_list(
        &mut self,
        dragged_id: &str,
        target: &SessionListDropTarget,
        cx: &mut Context<Self>,
    ) {
        let (folders, created) = apply_session_list_drop(&self.sidebar.folders, dragged_id, target);
        self.sidebar.drop_target = None;
        if folders == self.sidebar.folders {
            cx.notify();
            return;
        }
        self.commit_folders(folders, cx);
        if let Some(created) = created {
            self.sidebar.renaming_folder_id = Some(created);
        }
    }

    /// `setSessionDrop`.
    pub fn set_drop_target(
        &mut self,
        target: Option<SessionListDropTarget>,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar.drop_target != target {
            self.sidebar.drop_target = target;
            cx.notify();
        }
    }

    /// `onNewInFolder`: the caller created `session_id` (`onNew`); keep it in
    /// the folder until history lists it.
    pub fn new_in_folder(&mut self, folder_id: &str, session_id: &str, cx: &mut Context<Self>) {
        if !self
            .sidebar
            .pending_folder_session_ids
            .iter()
            .any(|id| id == session_id)
        {
            self.sidebar
                .pending_folder_session_ids
                .push(session_id.to_string());
        }
        self.sidebar.search_query.clear();
        let next = set_folder_collapsed(
            &add_session_to_folder(&self.sidebar.folders, folder_id, session_id),
            folder_id,
            false,
        );
        self.commit_folders(next, cx);
    }

    /// Drag-reorder folders.
    pub fn reorder_folders(&mut self, ids: &[String], cx: &mut Context<Self>) {
        let next = reorder_session_folders(&self.sidebar.folders, ids);
        if next != self.sidebar.folders {
            self.commit_folders(next, cx);
        }
    }

    pub fn start_folder_rename(&mut self, folder_id: Option<&str>, cx: &mut Context<Self>) {
        self.sidebar.renaming_folder_id = folder_id.map(str::to_string);
        cx.notify();
    }

    /// Commit a folder rename. A blank name keeps the old one.
    pub fn rename_folder(&mut self, folder_id: &str, name: &str, cx: &mut Context<Self>) {
        self.sidebar.renaming_folder_id = None;
        let next = rename_folder(&self.sidebar.folders, folder_id, name);
        self.commit_folders(next, cx);
    }

    /// The folder menu's Ungroup.
    pub fn dissolve_folder(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        let next = dissolve_folder(&self.sidebar.folders, folder_id);
        self.commit_folders(next, cx);
    }

    pub fn set_folder_collapsed(
        &mut self,
        folder_id: &str,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        let next = set_folder_collapsed(&self.sidebar.folders, folder_id, collapsed);
        self.commit_folders(next, cx);
    }

    /// `onFolderColorChange`.
    pub fn set_folder_color(
        &mut self,
        folder_id: &str,
        color_index: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        let next = set_folder_color(&self.sidebar.folders, folder_id, color_index);
        self.commit_folders(next, cx);
    }

    /// `onFolderCustomColorChange`.
    pub fn set_folder_custom_color(
        &mut self,
        folder_id: &str,
        color: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let next = set_folder_custom_color(&self.sidebar.folders, folder_id, color);
        self.commit_folders(next, cx);
    }

    // Selection and the session menu.

    /// `onSessionCardSelect`: shift selects a range from the anchor, the
    /// toggle key adds or removes one card, and a plain click clears the
    /// selection and returns the session to open.
    pub fn select_card(
        &mut self,
        session_id: &str,
        click: CardClick,
        mounted_ids: &[String],
        active_listed_id: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        self.sidebar.context_selection = false;
        self.sidebar.session_menu = None;
        cx.notify();
        if click.shift {
            if self
                .sidebar
                .selection_anchor
                .as_ref()
                .is_some_and(|anchor| !mounted_ids.contains(anchor))
            {
                self.sidebar.selection_anchor = None;
            }
            let anchor = self
                .sidebar
                .selection_anchor
                .clone()
                .or_else(|| active_listed_id.map(str::to_string))
                .unwrap_or_else(|| session_id.to_string());
            let start = mounted_ids.iter().position(|id| *id == anchor);
            let end = mounted_ids.iter().position(|id| id == session_id);
            let range: Vec<String> = match (start, end) {
                (Some(start), Some(end)) => mounted_ids[start.min(end)..=start.max(end)].to_vec(),
                _ => vec![session_id.to_string()],
            };
            self.sidebar.selection_anchor = Some(if start.is_none() {
                session_id.to_string()
            } else {
                anchor
            });
            self.sidebar.selected = if click.toggle {
                unique_selection(self.sidebar.selected.iter().cloned().chain(range))
            } else {
                unique_selection(range)
            };
            return None;
        }
        self.sidebar.selection_anchor = Some(session_id.to_string());
        if click.toggle {
            let next = toggle_session_selection(&self.sidebar.selected, session_id);
            if next.is_empty() {
                self.sidebar.selection_anchor = None;
            }
            self.sidebar.selected = next;
            return None;
        }
        self.sidebar.selected.clear();
        Some(session_id.to_string())
    }

    /// Escape, or a click off the cards: drop the selection.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.sidebar.selection_anchor = None;
        self.sidebar.context_selection = false;
        self.sidebar.selected.clear();
        self.sidebar.session_menu = None;
        cx.notify();
    }

    /// `onSessionContextMenu`: a card outside the selection selects itself
    /// for the menu's lifetime.
    pub fn open_session_menu(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.sidebar.context_selection = !self.sidebar.selected.iter().any(|id| id == session_id);
        if self.sidebar.context_selection {
            self.sidebar.selected = vec![session_id.to_string()];
        }
        self.sidebar.session_menu = Some(session_id.to_string());
        cx.notify();
    }

    /// `closeSessionMenu`.
    pub fn close_session_menu(&mut self, cx: &mut Context<Self>) {
        self.sidebar.session_menu = None;
        if self.sidebar.context_selection {
            self.sidebar.context_selection = false;
            self.sidebar.selection_anchor = None;
            self.sidebar.selected.clear();
        }
        cx.notify();
    }

    /// What the open session menu acts on.
    pub fn session_menu_state(&self, listed: &[SessionSummary]) -> SessionMenuState {
        let Some(clicked) = self.sidebar.session_menu.as_deref() else {
            return SessionMenuState::default();
        };
        let session_ids =
            ordered_session_action_ids(clicked, &self.sidebar.selected, &self.navigation_ids);
        let sessions: Vec<SessionSummary> = session_ids
            .iter()
            .filter_map(|id| listed.iter().find(|row| row.id == *id).cloned())
            .collect();
        let folders = &self.sidebar.folders;
        let multiple = session_ids.len() > 1;
        let folder = if session_ids.len() == 1 {
            folder_containing(folders, &session_ids[0]).cloned()
        } else {
            None
        };
        let any_foldered = session_ids
            .iter()
            .any(|id| folders.iter().any(|folder| folder.session_ids.contains(id)));
        SessionMenuState {
            all_pinned: !sessions.is_empty() && sessions.iter().all(|s| s.pinned == Some(true)),
            all_archived: !sessions.is_empty() && sessions.iter().all(|s| s.archived == Some(true)),
            can_remove_from_folders: if multiple {
                any_foldered
            } else {
                folder.is_some()
            },
            folders_checked: folders
                .iter()
                .map(|folder| {
                    (
                        folder.id.clone(),
                        !session_ids.is_empty()
                            && session_ids.iter().all(|id| folder.session_ids.contains(id)),
                    )
                })
                .collect(),
            folder,
            session_ids,
            sessions,
        }
    }

    /// `onSessionMenuPick` for the model actions. Closes the menu first.
    pub fn pick_session_menu(
        &mut self,
        action: SessionMenuAction,
        listed: &[SessionSummary],
        cx: &mut Context<Self>,
    ) {
        let state = self.session_menu_state(listed);
        let Some(session_id) = self.sidebar.session_menu.clone() else {
            return;
        };
        let ids = state.session_ids.clone();
        self.close_session_menu(cx);
        match action {
            SessionMenuAction::TogglePin => self.pin_sessions(&ids, !state.all_pinned, cx).detach(),
            SessionMenuAction::Rename => {
                self.sidebar.renaming_session_id = Some(session_id);
                cx.notify();
            }
            SessionMenuAction::NewFolder => {
                let (folders, created) =
                    create_folder_with_sessions(&self.sidebar.folders, &ids, None);
                if created.is_empty() {
                    return;
                }
                self.commit_folders(folders, cx);
                self.sidebar.renaming_folder_id = Some(created);
            }
            SessionMenuAction::AddToFolder(folder_id) => {
                let folders = ids
                    .iter()
                    .fold(self.sidebar.folders.clone(), |current, id| {
                        add_session_to_folder(&current, &folder_id, id)
                    });
                self.commit_folders(set_folder_collapsed(&folders, &folder_id, false), cx);
            }
            SessionMenuAction::RemoveFromFolders => {
                let folders = ids
                    .iter()
                    .fold(self.sidebar.folders.clone(), |current, id| {
                        remove_session_from_folder(&current, id)
                    });
                self.commit_folders(folders, cx);
            }
            SessionMenuAction::ToggleArchive => {
                self.archive_sessions(ids, !state.all_archived, cx).detach()
            }
            SessionMenuAction::Delete => {
                if ids.len() > 1 {
                    self.delete_sessions(ids, cx).detach();
                } else if let Some(id) = ids.first() {
                    self.delete_session(id, cx).detach();
                }
            }
        }
    }

    /// Start or stop renaming a card.
    pub fn set_renaming_session(&mut self, session_id: Option<&str>, cx: &mut Context<Self>) {
        self.sidebar.renaming_session_id = session_id.map(str::to_string);
        cx.notify();
    }
}

/// `placeSessionInFolder` for any project, as the `/add-to-folder` picker
/// does: load, place, save. A sidebar showing that project reloads through
/// its folder subscription.
pub fn place_session_in_project_folder(
    kv: &Kv,
    cwd: &str,
    session_id: &str,
    target: &SessionFolderTarget,
) {
    let folders = load_session_folders(kv, cwd);
    let next = place_session_in_folder(&folders, session_id, target);
    if next != folders {
        save_session_folders(kv, cwd, &next);
    }
}
