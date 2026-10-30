//! Viewport state: closing a tab never deletes or stops a session.
//! A title tab is keyed by one of its sessions; a tab holding more than one
//! pane keeps its split tree in `layouts` and its last focused pane in
//! `focused`. `active` is always the focused session.
use crate::pane_layout::{Edge, PaneNode, SplitDir, leaf};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum AppView {
    Inbox,
    Notes,
    Automations,
}
impl AppView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Notes => "Notes",
            Self::Automations => "Automations",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Notes => "note",
            Self::Automations => "zap",
        }
    }
}

/// The title strip has one order, including conversations, app views and Monos.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum TitleTab {
    Session(i64),
    Search,
    App(AppView),
    Mono {
        id: String,
        project: String,
        worktree: String,
    },
}

impl TitleTab {
    pub fn session(&self) -> Option<i64> {
        match self {
            Self::Session(id) => Some(*id),
            Self::Mono { .. } | Self::Search | Self::App(_) => None,
        }
    }
}

/// A named section of projects in the original project navigation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct ProjectRailGroup {
    pub id: String,
    pub name: String,
    pub collapsed: bool,
    pub color_index: Option<usize>,
    pub custom_color: Option<String>,
    pub mascot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Workspace {
    pub window: crate::window_state::WindowState,
    pub tabs: Vec<i64>,
    pub title_order: Vec<TitleTab>,
    pub scope_focus: HashMap<String, TitleTab>,
    /// Sidebar narrowing belongs to this run. Reopen on the full project,
    /// while saved conversations retain their own execution directories.
    #[serde(skip)]
    pub worktree_focus: HashMap<String, crate::worktree_switcher::WorktreeFocus>,
    /// Directly opened chats join the workspace on screen without changing
    /// their execution directory. Like workspace selection, these pins are local
    /// to the current run and disappear when their tabs close.
    #[serde(skip)]
    pub tab_workspaces: HashMap<i64, String>,
    pub active: Option<i64>,
    pub projects: Vec<String>,
    /// Projects archived from the rail, newest first. Their chats stay in
    /// history; adding the project again takes it out of this list.
    pub archived_projects: Vec<String>,
    pub project: String,
    /// Last focused open session in each project, independent of tab order.
    pub project_focus: HashMap<String, i64>,
    pub recent_files: HashMap<String, Vec<String>>,
    pub collapsed: HashSet<String>,
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    pub compact_session_cards: bool,
    pub sidebar_notes_visible: bool,
    pub sidebar_automations_visible: bool,
    pub sidebar_inbox_visible: bool,
    pub sidebar_activity_visible: bool,
    /// The inset right panel: open state, tab, width and tab order.
    pub right_panel_open: bool,
    pub right_panel_tab: crate::right_panel::RightTab,
    pub right_panel_width: f32,
    pub right_panel_tabs: Vec<crate::right_panel::RightTab>,
    pub changes_view: crate::changes::ChangesView,
    /// Each tab's file/review pane beside its chat.
    pub reviews: std::collections::HashMap<i64, crate::review::ReviewState>,
    /// Independent file panes use negative IDs in the layout tree. Their
    /// originating session supplies project/worktree scope and Add to Chat.
    pub review_owners: HashMap<i64, i64>,
    /// Monos dragged into a split, by their negative pane IDs. Each keeps the
    /// Mono tab it came from, which it becomes again once it stands alone.
    pub mono_panes: HashMap<i64, TitleTab>,
    /// Split trees of tabs with more than one pane, by tab key.
    pub layouts: HashMap<i64, PaneNode>,
    /// The pane each split tab returns to when it is activated.
    pub focused: HashMap<i64, i64>,
    /// Each project's terminal dock (ProjectTerminalDock), by project path.
    pub docks: HashMap<String, TerminalDock>,
    /// Each project's saved commands, run from the footer's Actions button.
    pub actions: HashMap<String, Vec<ProjectAction>>,
    /// The id of the action each project last ran; the footer shows it.
    pub last_actions: HashMap<String, String>,
    /// Original layout: separate project and session sidebars.
    pub classic_layout: bool,
    /// The new single-sidebar layout can keep its full-height, square frame.
    pub new_layout_boxy: bool,
    pub project_rail_open: bool,
    pub project_rail_monos_pinned: bool,
    pub project_rail_width: f32,
    pub project_rail_pinned: HashSet<String>,
    pub project_rail_groups: Vec<ProjectRailGroup>,
    pub project_rail_assignments: HashMap<String, String>,
    pub classic_sidebar_tabs: HashMap<String, String>,
    pub classic_sidebar_tab_order: Vec<String>,
    pub new_layout_prompt_dismissed: bool,
    pub preferences: crate::settings::Preferences,
    pub session_filters: monocode_core::session_filters::Filters,
    pub project_appearance: HashMap<String, monocode_core::project_appearance::ProjectAppearance>,
    /// Last desktop source values, used to reconcile subsequent desktop changes
    /// while retaining deliberate native project and preference edits.
    pub desktop_project_preferences: Option<serde_json::Value>,
    /// Unmodified desktop viewport metadata, including pane types and explicit
    /// tab-group assignments that do not yet have a native representation.
    pub desktop_workspace_snapshot: Option<serde_json::Value>,
    /// Exact source strings retained for preferences with no native equivalent.
    pub desktop_preferences_metadata: Option<serde_json::Value>,
}

/// A saved command the footer runs in a terminal with one click.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ProjectAction {
    pub id: String,
    pub name: String,
    pub command: String,
    /// One of `ACTION_ICONS`.
    pub icon: String,
}
/// The icons an action can wear, in picker order; the first is the default.
pub(crate) const ACTION_ICONS: [&str; 6] =
    ["play", "test-tube", "hammer", "rocket", "bug", "package"];
impl ProjectAction {
    pub fn icon(&self) -> &'static str {
        ACTION_ICONS
            .iter()
            .find(|icon| **icon == self.icon)
            .copied()
            .unwrap_or(ACTION_ICONS[0])
    }
}

/// Port of src/features/projects/model/projectTerminal.ts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DockSide {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}
impl DockSide {
    pub fn vertical(self) -> bool {
        matches!(self, DockSide::Top | DockSide::Bottom)
    }
    /// DOCK_SIZE_DEFAULT.
    pub fn default_size(self) -> f32 {
        if self.vertical() { 220. } else { 360. }
    }
    /// clampDockSize: at least a usable strip, at most 70% of the window.
    pub fn clamp(self, value: f32, viewport: (f32, f32)) -> f32 {
        if !value.is_finite() {
            return self.default_size();
        }
        let (min, max) = self.limits(viewport);
        value.round().clamp(min, max)
    }
    pub fn limits(self, viewport: (f32, f32)) -> (f32, f32) {
        let (min, span) = if self.vertical() {
            (88., viewport.1)
        } else {
            (180., viewport.0)
        };
        (min, (span * 0.7).floor().max(min))
    }
}
/// A project's dock: its place, size and the terminals it holds. Shells are
/// not restored; the dock reopens the same tabs in their directories.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TerminalDock {
    pub side: DockSide,
    pub size: f32,
    pub open: bool,
    /// Each terminal's working directory, in tab order.
    pub terminals: Vec<String>,
    pub active: usize,
}
impl Default for TerminalDock {
    fn default() -> Self {
        Self {
            side: DockSide::Bottom,
            size: DockSide::Bottom.default_size(),
            open: true,
            terminals: Vec::new(),
            active: 0,
        }
    }
}
impl TerminalDock {
    /// addTerminalToDock: the new terminal is active and the dock opens.
    pub fn add(&mut self, cwd: String) -> usize {
        self.terminals.push(cwd);
        self.active = self.terminals.len() - 1;
        self.open = true;
        self.active
    }
    /// closeTerminalInDock: the next tab takes over a closed active one.
    /// False once the last terminal is gone.
    pub fn close(&mut self, ix: usize) -> bool {
        if ix >= self.terminals.len() {
            return true;
        }
        self.terminals.remove(ix);
        if self.terminals.is_empty() {
            self.active = 0;
            return false;
        }
        if self.active > ix || self.active >= self.terminals.len() {
            self.active = self.active.saturating_sub(1).min(self.terminals.len() - 1);
        }
        true
    }
    /// onReorderTerminals: the active terminal stays active wherever it lands.
    pub fn reorder(&mut self, from: usize, to: usize) {
        let len = self.terminals.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let active = self.active;
        let cwd = self.terminals.remove(from);
        self.terminals.insert(to, cwd);
        self.active = if active == from {
            to
        } else if from < active && active <= to {
            active - 1
        } else if to <= active && active < from {
            active + 1
        } else {
            active
        };
    }
    /// onCloseOtherProjectTerminals: `keep` is left, and active.
    pub fn keep_only(&mut self, keep: usize) {
        if keep < self.terminals.len() {
            self.terminals = vec![self.terminals.swap_remove(keep)];
            self.active = 0;
        }
    }
    pub fn set_side(&mut self, side: DockSide) {
        if side != self.side {
            // A dock moved across axes starts from that axis's default.
            if side.vertical() != self.side.vertical() {
                self.size = side.default_size();
            }
            self.side = side;
        }
    }
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            window: Default::default(),
            tabs: Vec::new(),
            title_order: Vec::new(),
            scope_focus: HashMap::new(),
            worktree_focus: HashMap::new(),
            tab_workspaces: HashMap::new(),
            active: None,
            projects: Vec::new(),
            archived_projects: Vec::new(),
            project: String::new(),
            project_focus: HashMap::new(),
            recent_files: HashMap::new(),
            collapsed: HashSet::new(),
            sidebar_visible: true,
            sidebar_width: 260.,
            compact_session_cards: true,
            sidebar_notes_visible: true,
            sidebar_automations_visible: true,
            sidebar_inbox_visible: true,
            sidebar_activity_visible: true,
            right_panel_open: false,
            right_panel_tab: Default::default(),
            right_panel_width: crate::right_panel::WIDTH_DEFAULT,
            right_panel_tabs: crate::right_panel::RightTab::normalize(&[]),
            changes_view: Default::default(),
            reviews: Default::default(),
            review_owners: Default::default(),
            mono_panes: Default::default(),
            layouts: Default::default(),
            focused: Default::default(),
            docks: Default::default(),
            actions: HashMap::new(),
            last_actions: HashMap::new(),
            classic_layout: false,
            // Fresh installs start on the boxy frame; saved choices are kept.
            new_layout_boxy: true,
            project_rail_open: true,
            project_rail_monos_pinned: false,
            project_rail_width: 220.,
            project_rail_pinned: HashSet::new(),
            project_rail_groups: Vec::new(),
            project_rail_assignments: HashMap::new(),
            classic_sidebar_tabs: HashMap::new(),
            classic_sidebar_tab_order: Vec::new(),
            new_layout_prompt_dismissed: false,
            preferences: crate::settings::Preferences::default(),
            session_filters: Default::default(),
            project_appearance: Default::default(),
            desktop_project_preferences: None,
            desktop_workspace_snapshot: None,
            desktop_preferences_metadata: None,
        }
    }
}
impl Workspace {
    /// Seed original-rail concepts from the exact desktop preference strings.
    /// Native edits then live in their own persisted fields.
    pub(crate) fn restore_project_rail_metadata(&mut self) {
        let Some(metadata) = self.desktop_preferences_metadata.as_ref() else {
            return;
        };
        let read = |key: &str| -> Option<serde_json::Value> {
            metadata
                .get(key)?
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
        };
        if let Some(value) = read("monocode.railMonosPinned") {
            self.project_rail_monos_pinned = value.as_bool().unwrap_or(false);
        }
        if self.classic_sidebar_tab_order.is_empty()
            && let Some(value) = read("monocode.sidebarTabOrder")
        {
            self.classic_sidebar_tab_order = serde_json::from_value::<Vec<String>>(value)
                .unwrap_or_default()
                .into_iter()
                .filter(|tab| ["sessions", "files", "changes"].contains(&tab.as_str()))
                .collect();
        }
        if self.classic_sidebar_tabs.is_empty()
            && let Some(value) = read("monocode.projectSidebarTabs.v1")
        {
            self.classic_sidebar_tabs = serde_json::from_value::<HashMap<String, String>>(value)
                .unwrap_or_default()
                .into_iter()
                .filter(|(_, tab)| ["sessions", "files", "changes"].contains(&tab.as_str()))
                .collect();
        }
        if let Some(value) = read("monocode.projectRailPinned") {
            self.project_rail_pinned = serde_json::from_value(value).unwrap_or_default();
        }
        if let Some(value) = read("monocode.projectGroups") {
            self.project_rail_groups = serde_json::from_value(value).unwrap_or_default();
        }
        if let Some(value) = read("monocode.projectGroupAssignments") {
            self.project_rail_assignments = serde_json::from_value(value).unwrap_or_default();
        }
        if let Some(value) = read("monocode.projectRailOrder")
            && let Ok(order) = serde_json::from_value::<Vec<String>>(value)
        {
            let mut projects: Vec<_> = order
                .into_iter()
                .filter(|p| self.projects.contains(p))
                .collect();
            projects.extend(
                self.projects
                    .iter()
                    .filter(|p| !projects.contains(p))
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            self.projects = projects;
        }
    }

    pub fn boxy_layout(&self) -> bool {
        self.classic_layout || self.new_layout_boxy
    }

    pub fn restore(saved: Option<&str>, sessions: &[(i64, String)], cwd: &str) -> Self {
        let mut state = saved
            .and_then(|s| serde_json::from_str::<Self>(s).ok())
            .unwrap_or_default();
        // Before the project rail returned, "classic" meant the boxy version
        // of the single-sidebar layout. Keep that appearance for existing users.
        if let Some(saved) = saved.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            && saved.get("new_layout_boxy").is_none()
        {
            state.new_layout_boxy = state.classic_layout;
            state.classic_layout = false;
        }
        let has_rail_state = saved
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .is_some_and(|value| value.get("project_rail_pinned").is_some());
        if !has_rail_state {
            state.restore_project_rail_metadata();
        }
        // Hidden project navigation is retired; collapsed always means icons.
        state.preferences.collapsed_project_rail = "compact".into();
        state.project_rail_width = if state.project_rail_width.is_finite() {
            state.project_rail_width.clamp(176., 360.)
        } else {
            220.
        };
        if state.project.is_empty() {
            state.project = cwd.into();
        }
        for appearance in state.project_appearance.values_mut() {
            appearance.normalize();
        }
        let mut known: HashSet<_> = sessions.iter().map(|s| s.0).collect();
        state.review_owners.retain(|id, owner| {
            *id < 0
                && known.contains(owner)
                && state.reviews.get(id).is_some_and(|r| !r.tabs.is_empty())
        });
        known.extend(state.review_owners.keys().copied());
        let owners = &state.review_owners;
        state.mono_panes.retain(|id, mono| {
            *id < 0 && !owners.contains_key(id) && matches!(mono, TitleTab::Mono { .. })
        });
        known.extend(state.mono_panes.keys().copied());
        let mut seen = HashSet::new();
        let mut layouts = std::mem::take(&mut state.layouts);
        let mut focused = std::mem::take(&mut state.focused);
        for tab in std::mem::take(&mut state.tabs) {
            let layout = layouts.remove(&tab).unwrap_or_else(|| leaf(tab));
            let Some(layout) = layout.retain(&mut |id| known.contains(&id) && seen.insert(id))
            else {
                continue;
            };
            let key = if layout.contains(tab) {
                tab
            } else {
                layout.first_leaf()
            };
            state.tabs.push(key);
            for entry in state
                .title_order
                .iter_mut()
                .chain(state.scope_focus.values_mut())
            {
                if *entry == TitleTab::Session(tab) {
                    *entry = TitleTab::Session(key);
                }
            }
            if layout.is_split() {
                if let Some(pane) = focused.remove(&tab).filter(|id| layout.contains(*id)) {
                    state.focused.insert(key, pane);
                }
                state.layouts.insert(key, layout);
            }
        }
        state.settle_mono_panes();
        state.normalize_title_order();
        if state
            .desktop_project_preferences
            .as_ref()
            .and_then(|source| source.get("projects"))
            .is_some()
            && state.active.is_some_and(|id| {
                sessions.iter().any(|(known, path)| {
                    *known == state.session_for_pane(id) && !state.projects.contains(path)
                })
            })
        {
            // An old native sidecar could focus an internal session folder.
            // It must not restore that folder as a remembered user project.
            state.active = None;
        }
        if state.active.is_none_or(|id| state.tab_of(id).is_none()) {
            state.active = state
                .project_focus
                .get(&state.project)
                .copied()
                .filter(|id| {
                    state.tab_of(*id).is_some()
                        && sessions.iter().any(|(known, path)| {
                            *known == state.session_for_pane(*id) && *path == state.project
                        })
                })
                .or_else(|| {
                    state
                        .tabs
                        .iter()
                        .rev()
                        .map(|tab| state.tab_focus(*tab))
                        .find(|id| {
                            sessions.iter().any(|(known, path)| {
                                *known == state.session_for_pane(*id) && *path == state.project
                            })
                        })
                });
        }
        if saved.is_none()
            && let Some((id, _)) = sessions.iter().find(|(_, path)| path == cwd)
        {
            state.open(*id);
        }
        state.sidebar_width = state.sidebar_width.clamp(200., 560.);
        state.right_panel_width = state
            .right_panel_width
            .clamp(crate::right_panel::WIDTH_MIN, crate::right_panel::WIDTH_MAX)
            .round();
        state.right_panel_tabs = crate::right_panel::RightTab::normalize(&state.right_panel_tabs);
        state
            .reviews
            .retain(|id, review| known.contains(id) && !review.tabs.is_empty());
        state.docks.retain(|_, dock| !dock.terminals.is_empty());
        for dock in state.docks.values_mut() {
            dock.active = dock.active.min(dock.terminals.len() - 1);
            dock.size = dock.side.clamp(dock.size, (4000., 4000.));
        }
        state.add_project(cwd);
        let open: HashSet<_> = state.open_sessions().into_iter().collect();
        let review_owners = &state.review_owners;
        state.project_focus.retain(|project, id| {
            open.contains(id)
                && sessions.iter().any(|(known, path)| {
                    *known == review_owners.get(id).copied().unwrap_or(*id) && path == project
                })
        });
        if let Some((_, project)) = sessions.iter().find(|(id, _)| {
            state
                .active
                .is_some_and(|pane| state.session_for_pane(pane) == *id)
        }) {
            state.project = project.clone();
            state
                .project_focus
                .insert(project.clone(), state.active.unwrap());
        }
        state.add_project(&state.project.clone());
        state
    }
    pub fn add_project(&mut self, path: &str) {
        self.archived_projects.retain(|p| p != path);
        if !self.projects.iter().any(|p| p == path) {
            self.projects.push(path.into());
        }
    }
    /// The rail group a project is listed under. Assignments to deleted
    /// groups leave the project in the ungrouped list.
    pub fn project_rail_group(&self, path: &str) -> Option<&str> {
        let id = self.project_rail_assignments.get(path)?;
        self.project_rail_groups
            .iter()
            .any(|group| &group.id == id)
            .then_some(id.as_str())
    }
    /// Whether two projects sit in the same rail section and reorder together.
    pub fn project_rail_peers(&self, a: &str, b: &str) -> bool {
        let pinned = self.project_rail_pinned.contains(a);
        pinned == self.project_rail_pinned.contains(b)
            && (pinned || self.project_rail_group(a) == self.project_rail_group(b))
    }
    /// Projects in the order the current layout shows them. The rail lists
    /// pinned projects, then each group's members, then the rest.
    pub fn visible_project_order(&self) -> Vec<String> {
        if !self.classic_layout {
            return self.projects.clone();
        }
        let unpinned = |p: &&String| !self.project_rail_pinned.contains(*p);
        let mut order: Vec<String> = self
            .projects
            .iter()
            .filter(|p| self.project_rail_pinned.contains(*p))
            .cloned()
            .collect();
        for group in &self.project_rail_groups {
            order.extend(
                self.projects
                    .iter()
                    .filter(unpinned)
                    .filter(|p| self.project_rail_group(p) == Some(group.id.as_str()))
                    .cloned(),
            );
        }
        order.extend(
            self.projects
                .iter()
                .filter(unpinned)
                .filter(|p| self.project_rail_group(p).is_none())
                .cloned(),
        );
        order
    }
    /// The projects that reorder alongside `path`, in the order shown: the
    /// whole list, or just its rail section.
    pub fn project_peers(&self, path: &str) -> Vec<String> {
        self.visible_project_order()
            .into_iter()
            .filter(|p| !self.classic_layout || self.project_rail_peers(p, path))
            .collect()
    }
    /// Moves a project to `to` in the saved order. Returns whether it moved.
    pub fn move_project(&mut self, path: &str, to: usize) -> bool {
        let Some(from) = self.projects.iter().position(|p| p == path) else {
            return false;
        };
        let to = to.min(self.projects.len() - 1);
        if from == to {
            return false;
        }
        let project = self.projects.remove(from);
        self.projects.insert(to, project);
        true
    }
    /// Moves a project one place along what the layout shows, staying inside
    /// its rail section. Returns whether it moved.
    pub fn step_project_order(&mut self, path: &str, step: isize) -> bool {
        let peers = self.project_peers(path);
        let Some(ix) = peers.iter().position(|p| p == path) else {
            return false;
        };
        let Some(neighbour) = ix
            .checked_add_signed(step.signum())
            .and_then(|ix| peers.get(ix))
        else {
            return false;
        };
        let to = self.projects.iter().position(|p| p == neighbour).unwrap();
        self.move_project(path, to)
    }
    /// Moves a project to either end of its section: `step < 0` for the start.
    pub fn move_project_to_end(&mut self, path: &str, step: isize) -> bool {
        let peers = self.project_peers(path);
        let target = if step < 0 {
            peers.first()
        } else {
            peers.last()
        };
        let Some(to) = target.and_then(|t| self.projects.iter().position(|p| p == t)) else {
            return false;
        };
        self.move_project(path, to)
    }
    pub fn session_for_pane(&self, id: i64) -> i64 {
        self.review_owners.get(&id).copied().unwrap_or(id)
    }
    /// File and Mono panes share the negative IDs below every session.
    pub fn next_pane_id(&self) -> i64 {
        self.review_owners
            .keys()
            .chain(self.mono_panes.keys())
            .copied()
            .min()
            .unwrap_or(0)
            .min(0)
            - 1
    }
    pub fn mono_pane(&self, mono: &str) -> Option<i64> {
        self.mono_panes.iter().find_map(|(pane, tab)| {
            matches!(tab, TitleTab::Mono { id, .. } if id == mono).then_some(*pane)
        })
    }
    /// The pane that scopes a tab to a project: its focus, unless that is a
    /// Mono, which works across projects.
    pub fn scope_pane(&self, tab: i64) -> i64 {
        let focus = self.tab_focus(tab);
        if !self.mono_panes.contains_key(&focus) {
            return focus;
        }
        self.layout(tab)
            .leaf_ids()
            .into_iter()
            .find(|id| !self.mono_panes.contains_key(id))
            .unwrap_or(focus)
    }
    /// Focus a session, opening it as its own tab unless a tab holds it.
    pub fn open(&mut self, id: i64) {
        match self.tab_of(id) {
            Some(tab) => {
                if self.layouts.contains_key(&tab) {
                    self.focused.insert(tab, id);
                }
            }
            None => {
                self.tabs.push(id);
                self.title_order.push(TitleTab::Session(id));
            }
        }
        self.active = Some(id);
    }
    /// Close the whole title tab holding `id`; its sessions keep running.
    pub fn close(&mut self, id: i64) {
        let Some(tab) = self.tab_of(id) else {
            return;
        };
        let ix = self.tabs.iter().position(|t| *t == tab).unwrap_or(0);
        let layout = self.layout(tab);
        self.tabs.remove(ix);
        self.tab_workspaces.remove(&tab);
        self.title_order
            .retain(|entry| *entry != TitleTab::Session(tab));
        self.scope_focus
            .retain(|_, entry| *entry != TitleTab::Session(tab));
        self.layouts.remove(&tab);
        self.focused.remove(&tab);
        self.project_focus.retain(|_, id| !layout.contains(*id));
        if self
            .active
            .is_some_and(|active| self.tab_of(active).is_none())
        {
            self.active = self
                .tabs
                .get(ix)
                .or_else(|| self.tabs.last())
                .map(|tab| self.tab_focus(*tab));
        }
    }
    /// The title tab holding a session.
    pub fn tab_of(&self, id: i64) -> Option<i64> {
        self.tabs.iter().copied().find(|tab| {
            *tab == id
                || self
                    .layouts
                    .get(tab)
                    .is_some_and(|layout| layout.contains(id))
        })
    }
    pub fn active_tab(&self) -> Option<i64> {
        self.active.and_then(|id| self.tab_of(id))
    }
    pub fn layout(&self, tab: i64) -> PaneNode {
        self.layouts.get(&tab).cloned().unwrap_or_else(|| leaf(tab))
    }
    /// Every session shown in some tab.
    pub fn open_sessions(&self) -> Vec<i64> {
        self.tabs
            .iter()
            .flat_map(|tab| self.layout(*tab).leaf_ids())
            .collect()
    }
    /// The pane a tab activates.
    pub fn tab_focus(&self, tab: i64) -> i64 {
        self.focused
            .get(&tab)
            .copied()
            .filter(|id| self.layouts.get(&tab).is_some_and(|l| l.contains(*id)))
            .unwrap_or(tab)
    }
    /// Store a tab's tree, rekeying it when its key session left and
    /// dropping the tree once a single pane remains.
    fn set_layout(&mut self, tab: i64, layout: PaneNode) -> i64 {
        let Some(ix) = self.tabs.iter().position(|t| *t == tab) else {
            return tab;
        };
        let focus = self.focused.remove(&tab);
        self.layouts.remove(&tab);
        // A tab is keyed by a conversation when it holds one, never a Mono.
        let key = if layout.contains(tab) && !self.mono_panes.contains_key(&tab) {
            tab
        } else {
            let leaves = layout.leaf_ids();
            leaves
                .iter()
                .copied()
                .find(|id| !self.mono_panes.contains_key(id))
                .unwrap_or(leaves[0])
        };
        self.tabs[ix] = key;
        if key != tab
            && let Some(scope) = self.tab_workspaces.remove(&tab)
        {
            self.tab_workspaces.insert(key, scope);
        }
        for entry in self
            .title_order
            .iter_mut()
            .chain(self.scope_focus.values_mut())
        {
            if *entry == TitleTab::Session(tab) {
                *entry = TitleTab::Session(key);
            }
        }
        if layout.is_split() {
            if let Some(focus) = focus.filter(|id| layout.contains(*id)) {
                self.focused.insert(key, focus);
            }
            self.layouts.insert(key, layout);
        }
        key
    }
    /// Take a session out of whichever tab holds it. A tab left empty
    /// closes; the active session moves to a neighbouring pane.
    fn take_out(&mut self, id: i64) {
        let Some(tab) = self.tab_of(id) else {
            return;
        };
        let layout = self.layout(tab);
        self.project_focus.retain(|_, focus| *focus != id);
        match layout.remove(id) {
            Some(rest) => {
                let next = layout.sibling(id).unwrap_or_else(|| rest.first_leaf());
                let key = self.set_layout(tab, rest);
                if self.active == Some(id) {
                    self.active = Some(next);
                }
                if self.layouts.contains_key(&key) && self.focused.get(&key) == Some(&id) {
                    self.focused.insert(key, next);
                }
            }
            None => self.close(id),
        }
    }
    /// Close one pane of a split tab. False when it is the tab's only pane.
    pub fn close_pane(&mut self, id: i64) -> bool {
        let Some(tab) = self.tab_of(id) else {
            return false;
        };
        if !self.layout(tab).is_split() {
            return false;
        }
        self.take_out(id);
        if let Some(active) = self.active
            && let Some(tab) = self.tab_of(active)
            && self.layouts.contains_key(&tab)
        {
            self.focused.insert(tab, active);
        }
        true
    }
    /// cmd-d: a new session beside the focused pane.
    pub fn split(&mut self, new: i64, dir: SplitDir) {
        let Some(focused) = self.active else {
            self.open(new);
            return;
        };
        let Some(tab) = self.tab_of(focused) else {
            self.open(new);
            return;
        };
        self.take_out(new);
        let tab = self.tab_of(focused).unwrap_or(tab);
        let key = self.set_layout(tab, self.layout(tab).split_pane(focused, dir, new));
        self.focused.insert(key, new);
        self.active = Some(new);
    }
    /// A pane, tab-strip session or sidebar session dropped on a pane edge.
    pub fn place(&mut self, id: i64, target: i64, edge: Edge) {
        if id == target || self.tab_of(target).is_none() {
            return;
        }
        let same = self.tab_of(id) == self.tab_of(target);
        if !same {
            self.take_out(id);
        }
        let Some(tab) = self.tab_of(target) else {
            return;
        };
        let key = self.set_layout(tab, self.layout(tab).place_pane(id, target, edge));
        self.focused.insert(key, id);
        self.active = Some(id);
    }
    /// A whole title tab dropped on a pane edge: its tree moves intact.
    pub fn place_tab(&mut self, source: i64, target: i64, edge: Edge) {
        let Some(target_tab) = self.tab_of(target) else {
            return;
        };
        if source == target_tab || !self.tabs.contains(&source) {
            return;
        }
        let incoming = self.layout(source);
        let focus = self.tab_focus(source);
        self.tabs.retain(|t| *t != source);
        self.tab_workspaces.remove(&source);
        self.title_order
            .retain(|entry| *entry != TitleTab::Session(source));
        self.layouts.remove(&source);
        self.focused.remove(&source);
        let key = self.set_layout(
            target_tab,
            self.layout(target_tab).place_layout(incoming, target, edge),
        );
        self.focused.insert(key, focus);
        self.active = Some(focus);
    }
    /// A Mono tab dropped on a pane edge leaves the strip for the split.
    pub fn place_mono(&mut self, mono: TitleTab, target: i64, edge: Edge) -> Option<i64> {
        if !matches!(mono, TitleTab::Mono { .. }) || self.tab_of(target).is_none() {
            return None;
        }
        let pane = self.next_pane_id();
        self.title_order.retain(|entry| *entry != mono);
        self.scope_focus.retain(|_, entry| *entry != mono);
        self.mono_panes.insert(pane, mono);
        self.place(pane, target, edge);
        Some(pane)
    }
    /// The visible Mono becomes a split's first pane, its tab keeping its
    /// slot, so a tab or pane can be dropped beside it.
    pub fn mono_tab_pane(&mut self, mono: TitleTab) -> i64 {
        let pane = self.next_pane_id();
        self.tabs.push(pane);
        for entry in self
            .title_order
            .iter_mut()
            .chain(self.scope_focus.values_mut())
        {
            if *entry == mono {
                *entry = TitleTab::Session(pane);
            }
        }
        if !self.title_order.contains(&TitleTab::Session(pane)) {
            self.title_order.push(TitleTab::Session(pane));
        }
        self.mono_panes.insert(pane, mono);
        self.active = Some(pane);
        pane
    }
    /// A Mono pane left alone in its tab becomes its Mono tab again, in the
    /// same slot; one no tab holds is forgotten. Returns the Mono when it
    /// had focus, for the caller to show.
    pub fn settle_mono_panes(&mut self) -> Option<TitleTab> {
        let mut focused = None;
        let panes: Vec<_> = self.mono_panes.keys().copied().collect();
        for pane in panes {
            match self.tab_of(pane) {
                Some(tab) if self.layout(tab).is_split() => continue,
                Some(tab) => {
                    let mono = self.mono_panes[&pane].clone();
                    self.tabs.retain(|t| *t != tab);
                    self.tab_workspaces.remove(&tab);
                    self.layouts.remove(&tab);
                    self.focused.remove(&tab);
                    let present = self.title_order.contains(&mono);
                    self.title_order
                        .retain(|entry| !(present && *entry == TitleTab::Session(tab)));
                    for entry in self
                        .title_order
                        .iter_mut()
                        .chain(self.scope_focus.values_mut())
                    {
                        if *entry == TitleTab::Session(tab) {
                            *entry = mono.clone();
                        }
                    }
                    if self.active == Some(pane) {
                        self.active = None;
                        focused = Some(mono);
                    }
                }
                None => {}
            }
            self.mono_panes.remove(&pane);
        }
        focused
    }
    /// A pane dragged onto the tab strip becomes its own tab at `to`.
    pub fn detach(&mut self, id: i64, to: usize) {
        if !self
            .tab_of(id)
            .is_some_and(|tab| self.layout(tab).is_split())
        {
            return;
        }
        let scope = self
            .tab_of(id)
            .and_then(|tab| self.tab_workspaces.get(&tab))
            .cloned();
        self.take_out(id);
        self.tabs.insert(to.min(self.tabs.len()), id);
        if let Some(scope) = scope {
            self.tab_workspaces.insert(id, scope);
        }
        let next = self
            .tabs
            .iter()
            .skip(to + 1)
            .find_map(|next| {
                self.title_order
                    .iter()
                    .position(|tab| *tab == TitleTab::Session(*next))
            })
            .unwrap_or(self.title_order.len());
        self.title_order.insert(next, TitleTab::Session(id));
        self.active = Some(id);
    }
    /// Show `to` in `from`'s pane (Session: Next/Previous in tab).
    pub fn swap_session(&mut self, from: i64, to: i64) {
        if self.tab_of(to).is_some() {
            return;
        }
        let Some(tab) = self.tab_of(from) else {
            return;
        };
        let key = self.set_layout(tab, self.layout(tab).replace_leaf(from, to));
        if self.layouts.contains_key(&key) {
            self.focused.insert(key, to);
        }
    }
    /// cmd-opt-arrows within the active tab.
    pub fn neighbor(&self, dir: Edge) -> Option<i64> {
        let active = self.active?;
        self.layout(self.tab_of(active)?).neighbor(active, dir)
    }
    pub fn place_sash(&mut self, tab: i64, path: &[usize], index: usize, boundary: f32) {
        if let Some(layout) = self.layouts.get_mut(&tab) {
            layout.place_sash(path, index, boundary);
        }
    }
    pub fn sash_range(&self, tab: i64, path: &[usize], index: usize) -> Option<(f32, f32, f32)> {
        self.layouts.get(&tab)?.sash_range(path, index)
    }
    pub fn normalize_title_order(&mut self) {
        self.tab_workspaces.retain(|tab, _| self.tabs.contains(tab));
        let mut seen = HashSet::new();
        self.title_order.retain(|tab| {
            tab.session().is_none_or(|id| self.tabs.contains(&id)) && seen.insert(tab.clone())
        });
        for id in &self.tabs {
            let tab = TitleTab::Session(*id);
            if seen.insert(tab.clone()) {
                self.title_order.push(tab);
            }
        }
    }
}

/// Port of src/features/workspace/model/tabVisitHistory.ts. Closed tabs are
/// pruned rather than reopened by Back, and a new visit discards Forward.
#[derive(Default)]
pub(crate) struct VisitHistory {
    pub back: Vec<i64>,
    pub forward: Vec<i64>,
    current: Option<i64>,
}
impl VisitHistory {
    fn push(&mut self, id: i64) {
        if self.back.last() != Some(&id) {
            self.back.push(id);
        }
        if self.back.len() > 50 {
            self.back.remove(0);
        }
    }
    pub fn record(&mut self, id: i64) {
        if self.current == Some(id) {
            return;
        }
        if let Some(current) = self.current {
            self.push(current);
        }
        self.current = Some(id);
        self.forward.clear();
    }
    pub fn step(&mut self, direction: isize) -> Option<i64> {
        let target = if direction < 0 {
            let target = self.back.pop()?;
            if let Some(current) = self.current {
                self.forward.insert(0, current);
            }
            target
        } else {
            if self.forward.is_empty() {
                return None;
            }
            let target = self.forward.remove(0);
            if let Some(current) = self.current {
                self.push(current);
            }
            target
        };
        self.current = Some(target);
        Some(target)
    }
    pub fn prune(&mut self, tabs: &[i64], active: Option<i64>) {
        self.back.retain(|id| tabs.contains(id));
        self.forward.retain(|id| tabs.contains(id));
        if self.current.is_none_or(|id| !tabs.contains(&id)) {
            self.current = active;
        }
        while self
            .back
            .last()
            .copied()
            .is_some_and(|id| Some(id) == self.current)
        {
            self.back.pop();
        }
        while self
            .forward
            .first()
            .copied()
            .is_some_and(|id| Some(id) == self.current)
        {
            self.forward.remove(0);
        }
    }
}
pub(crate) fn project_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .filter(|n| !n.is_empty())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn rail_moves_stay_inside_their_section_and_follow_the_drawn_order() {
        let mut state = super::Workspace {
            classic_layout: true,
            projects: ["/a", "/b", "/c", "/d", "/e", "/f"]
                .map(str::to_owned)
                .to_vec(),
            project_rail_pinned: ["/e".to_owned()].into(),
            project_rail_groups: vec![super::ProjectRailGroup {
                id: "g".into(),
                name: "Group".into(),
                ..Default::default()
            }],
            project_rail_assignments: [("/b", "g"), ("/d", "g"), ("/f", "deleted")]
                .map(|(p, g)| (p.to_owned(), g.to_owned()))
                .into(),
            ..Default::default()
        };
        // Pinned, then the group, then the rest. A deleted group's member
        // falls back to the ungrouped list.
        assert_eq!(
            state.visible_project_order(),
            ["/e", "/b", "/d", "/a", "/c", "/f"]
        );
        assert_eq!(state.project_peers("/f"), ["/a", "/c", "/f"]);
        assert!(state.step_project_order("/f", -1));
        assert_eq!(state.project_peers("/a"), ["/a", "/f", "/c"]);
        assert!(!state.step_project_order("/b", -1), "/b leads its group");
        assert!(state.move_project_to_end("/b", 1));
        assert_eq!(state.project_peers("/d"), ["/d", "/b"]);
        assert!(!state.step_project_order("/e", 1), "/e is the only pin");
        // The projects row shows the saved order as it is.
        state.classic_layout = false;
        assert_eq!(state.visible_project_order(), state.projects);
        assert!(state.move_project_to_end("/e", -1));
        assert_eq!(state.projects[0], "/e");
    }

    #[test]
    fn legacy_boxy_layout_and_hidden_rail_migrate_without_changing_appearance() {
        let state = super::Workspace::restore(
            Some(r#"{"classic_layout":true,"preferences":{"collapsed_project_rail":"hidden"}}"#),
            &[],
            "/work",
        );
        assert!(!state.classic_layout);
        assert!(state.new_layout_boxy);
        assert!(state.boxy_layout());
        assert_eq!(state.preferences.collapsed_project_rail, "compact");
        let saved = serde_json::to_string(&state).unwrap();
        let mut state = super::Workspace::restore(Some(&saved), &[], "/work");
        state.classic_layout = true;
        state.project_rail_open = false;
        state.project_rail_width = 248.;
        let saved = serde_json::to_string(&state).unwrap();
        let state = super::Workspace::restore(Some(&saved), &[], "/work");
        assert!(state.classic_layout && state.new_layout_boxy);
        assert!(!state.project_rail_open);
        assert_eq!(state.project_rail_width, 248.);
    }

    use super::*;
    #[test]
    fn navigation_prunes_closed_tabs_and_a_new_visit_discards_forward() {
        let mut h = VisitHistory::default();
        h.record(1);
        h.record(2);
        h.record(3);
        assert_eq!(h.step(-1), Some(2));
        h.prune(&[1, 3], Some(3));
        assert_eq!(h.step(-1), Some(1));
        assert_eq!(h.step(1), Some(3));
        h.step(-1);
        h.record(4);
        assert!(h.forward.is_empty());
        for id in 5..100 {
            h.record(id);
        }
        assert_eq!(h.back.len(), 50);
    }
    fn mono(id: &str) -> TitleTab {
        TitleTab::Mono {
            id: id.into(),
            project: "/work".into(),
            worktree: "/work".into(),
        }
    }
    #[test]
    fn a_mono_joins_a_split_and_becomes_its_tab_again_when_alone() {
        let mut s = Workspace::default();
        s.open(1);
        s.title_order.push(mono("m"));
        s.open(2);
        let pane = s.place_mono(mono("m"), 1, Edge::Right).unwrap();
        assert!(pane < 0);
        assert_eq!(s.layout(1).leaf_ids(), vec![1, pane]);
        assert_eq!(s.active, Some(pane));
        assert_eq!(s.scope_pane(1), 1, "a Mono never scopes its tab");
        assert_eq!(
            s.title_order,
            vec![TitleTab::Session(1), TitleTab::Session(2)]
        );
        assert_eq!(s.mono_pane("m"), Some(pane));
        assert_eq!(s.settle_mono_panes(), None);
        // Restarting keeps the Mono in its split.
        let saved = serde_json::to_string(&s).unwrap();
        let restored = Workspace::restore(
            Some(&saved),
            &[(1, "/work".into()), (2, "/work".into())],
            "/work",
        );
        assert_eq!(restored.layout(1).leaf_ids(), vec![1, pane]);
        assert_eq!(restored.mono_pane("m"), Some(pane));
        // Detached to the strip, it is the Mono tab again in the same slot.
        s.detach(pane, 1);
        assert_eq!(s.settle_mono_panes(), Some(mono("m")));
        assert_eq!(
            s.title_order,
            vec![TitleTab::Session(1), mono("m"), TitleTab::Session(2)]
        );
        assert!(s.mono_panes.is_empty());
        assert_eq!(s.tabs, vec![1, 2]);
    }
    #[test]
    fn a_tab_dropped_on_the_visible_mono_splits_beside_it() {
        let mut s = Workspace::default();
        s.open(1);
        s.title_order.push(mono("m"));
        let pane = s.mono_tab_pane(mono("m"));
        assert_eq!(
            s.title_order,
            vec![TitleTab::Session(1), TitleTab::Session(pane)]
        );
        s.place_tab(1, pane, Edge::Left);
        assert_eq!(s.tabs, vec![1], "the conversation keys the tab");
        assert_eq!(s.layout(1).leaf_ids(), vec![1, pane]);
        assert_eq!(s.title_order, vec![TitleTab::Session(1)]);
        assert_eq!(s.settle_mono_panes(), None);
        // Closing the conversation's pane leaves the Mono on its own.
        assert!(s.close_pane(1));
        assert_eq!(s.settle_mono_panes(), Some(mono("m")));
        assert_eq!(s.title_order, vec![mono("m")]);
        assert!(s.tabs.is_empty());
        // A closed tab forgets its Mono pane.
        s.open(1);
        s.place_mono(mono("m"), 1, Edge::Bottom);
        s.close(1);
        assert_eq!(s.settle_mono_panes(), None);
        assert!(s.mono_panes.is_empty());
    }
    #[test]
    fn close_and_reopen_keep_session_identity() {
        let mut s = Workspace::default();
        s.open(1);
        s.open(2);
        s.open(3);
        s.open(2);
        assert_eq!(s.tabs, vec![1, 2, 3]);
        s.close(1);
        assert_eq!(s.active, Some(2));
        s.close(2);
        assert_eq!(s.active, Some(3));
        s.open(2);
        s.open(3);
        assert_eq!(s.active, Some(3));
        s.close(3);
        s.close(2);
        assert_eq!(s.active, None);
        s.open(1);
        assert_eq!(s.tabs, vec![1]);
    }
    #[test]
    fn split_tabs_place_detach_and_close_panes_without_losing_sessions() {
        let mut s = Workspace::default();
        s.open(1);
        s.open(2);
        s.open(1);
        s.split(3, SplitDir::Right);
        s.tab_workspaces.insert(1, "/selected".into());
        s.tab_workspaces.insert(2, "/other".into());
        assert_eq!(s.tabs, [1, 2]);
        assert_eq!(s.layout(1).leaf_ids(), [1, 3]);
        assert_eq!((s.active, s.active_tab()), (Some(3), Some(1)));
        // Another tab's session joins this one; its old tab closes.
        s.place(2, 1, Edge::Bottom);
        assert_eq!(s.tabs, [1]);
        assert!(!s.tab_workspaces.contains_key(&2));
        assert_eq!(s.layout(1).leaf_ids(), [1, 2, 3]);
        assert_eq!(s.open_sessions(), [1, 2, 3]);
        // Closing the key pane rekeys the tab and keeps its place.
        s.open(4);
        s.open(1);
        assert!(s.close_pane(1));
        assert_eq!(s.tabs, [2, 4]);
        assert_eq!(s.tab_workspaces[&2], "/selected");
        assert!(!s.tab_workspaces.contains_key(&1));
        assert_eq!(s.active, Some(2));
        assert_eq!(s.tab_focus(2), 2);
        s.open(3);
        s.open(s.tab_focus(4));
        assert_eq!(s.active, Some(4));
        s.open(s.tab_focus(2));
        assert_eq!(s.active, Some(3));
        // A pane dragged to the strip becomes a tab where it was dropped.
        s.detach(3, 0);
        assert_eq!(s.tabs, [3, 2, 4]);
        assert_eq!(s.tab_workspaces[&3], "/selected");
        assert!(!s.layouts.contains_key(&2));
        assert!(!s.close_pane(3));
        // A whole tab dropped on a pane moves in intact.
        s.split(5, SplitDir::Down);
        s.place_tab(3, 4, Edge::Left);
        assert_eq!(s.tabs, [2, 4]);
        assert!(!s.tab_workspaces.contains_key(&3));
        assert_eq!(s.layout(4).leaf_ids(), [3, 5, 4]);
        assert_eq!(s.active, Some(5));
        s.close(5);
        assert_eq!(s.tabs, [2]);
        assert_eq!(s.tab_workspaces.len(), 1);
        assert_eq!(s.active, Some(2));
    }
    #[test]
    fn restore_keeps_split_tabs_and_prunes_their_missing_panes() {
        let mut s = Workspace::default();
        s.open(1);
        s.split(2, SplitDir::Right);
        s.split(3, SplitDir::Down);
        s.open(9);
        s.open(2);
        let json = serde_json::to_string(&s).unwrap();
        let sessions: Vec<_> = [2, 3].iter().map(|id| (*id, "/w".into())).collect();
        let s = Workspace::restore(Some(&json), &sessions, "/w");
        assert_eq!(s.tabs, [2]);
        assert_eq!(s.layout(2).leaf_ids(), [2, 3]);
        assert_eq!(s.active, Some(2));
        assert_eq!(s.tab_focus(2), 2);
    }
    #[test]
    fn terminal_docks_add_close_move_and_clamp_like_project_terminal() {
        let mut dock = TerminalDock::default();
        assert_eq!((dock.side, dock.size), (DockSide::Bottom, 220.));
        dock.add("/w/a".into());
        dock.add("/w/b".into());
        dock.add("/w/c".into());
        assert_eq!(dock.active, 2);
        dock.active = 1;
        assert!(dock.close(1));
        assert_eq!((dock.terminals.len(), dock.active), (2, 1));
        assert!(dock.close(0));
        assert_eq!(dock.active, 0);
        assert!(!dock.close(0));
        dock.set_side(DockSide::Right);
        assert_eq!(dock.size, 360.);
        dock.size = 500.;
        dock.set_side(DockSide::Left);
        assert_eq!(dock.size, 500.);
        assert_eq!(DockSide::Bottom.clamp(10., (1280., 800.)), 88.);
        assert_eq!(DockSide::Bottom.clamp(900., (1280., 800.)), 560.);
        assert_eq!(DockSide::Left.clamp(f32::NAN, (1280., 800.)), 360.);
        let mut s = Workspace::default();
        s.docks.insert("/w".into(), dock.clone());
        let mut kept = dock.clone();
        kept.add("/w".into());
        s.docks.insert("/k".into(), kept);
        let json = serde_json::to_string(&s).unwrap();
        let s = Workspace::restore(Some(&json), &[], "/w");
        assert!(!s.docks.contains_key("/w"));
        assert_eq!(s.docks["/k"].terminals, ["/w"]);
    }
    #[test]
    fn terminal_docks_reorder_keeping_the_active_terminal_and_close_others() {
        let mut dock = TerminalDock::default();
        for cwd in ["/a", "/b", "/c", "/d"] {
            dock.add(cwd.into());
        }
        dock.active = 1;
        dock.reorder(1, 3);
        assert_eq!(dock.terminals, ["/a", "/c", "/d", "/b"]);
        assert_eq!(dock.active, 3);
        dock.reorder(0, 2);
        assert_eq!(dock.terminals, ["/c", "/d", "/a", "/b"]);
        assert_eq!(dock.active, 3);
        dock.reorder(3, 0);
        assert_eq!(dock.terminals, ["/b", "/c", "/d", "/a"]);
        assert_eq!(dock.active, 0);
        dock.active = 2;
        dock.reorder(3, 1);
        assert_eq!(dock.terminals, ["/b", "/a", "/c", "/d"]);
        assert_eq!(dock.active, 3);
        dock.keep_only(2);
        assert_eq!(
            (dock.terminals.clone(), dock.active),
            (vec!["/c".to_string()], 0)
        );
    }
    #[test]
    fn restore_keeps_project_selection_and_prunes_stale_project_focus() {
        let sessions = vec![(3, "/b".into()), (2, "/a".into()), (1, "/a".into())];
        let mut state = Workspace::default();
        for id in [1, 2, 3] {
            state.open(id);
        }
        state.project = "/a".into();
        state.active = Some(99);
        state.project_focus.insert("/a".into(), 1);
        state.project_focus.insert("/b".into(), 99);
        state.worktree_focus.insert(
            "/a".into(),
            crate::worktree_switcher::WorktreeFocus {
                path: "/a-tree".into(),
                branch: Some("feature".into()),
            },
        );
        state.tab_workspaces.insert(1, "/a-tree".into());
        let mut saved = serde_json::to_value(&state).unwrap();
        assert!(saved.get("worktree_focus").is_none());
        assert!(saved.get("tab_workspaces").is_none());
        // Older native sidecars saved this transient filter. Ignore it too.
        saved["worktree_focus"] = serde_json::json!({
            "/a": {"path":"/a-tree", "branch":"feature"}
        });
        let json = serde_json::to_string(&saved).unwrap();
        let restored = Workspace::restore(Some(&json), &sessions, "/b");
        assert_eq!(restored.active, Some(1));
        assert_eq!(restored.project, "/a");
        assert!(restored.worktree_focus.is_empty());
        assert!(restored.tab_workspaces.is_empty());
        assert!(!restored.project_focus.contains_key("/b"));
        state.active = Some(3);
        let json = serde_json::to_string(&state).unwrap();
        let restored = Workspace::restore(Some(&json), &sessions, "/a");
        assert_eq!(restored.active, Some(3));
        assert_eq!(restored.project, "/b");
        let fresh = Workspace::restore(None, &sessions, "/a");
        assert_eq!(fresh.active, Some(2));
        assert_eq!(fresh.project, "/a");
        assert!(fresh.new_layout_boxy);
        let rounded = Workspace::restore(Some(r#"{"new_layout_boxy":false}"#), &sessions, "/a");
        assert!(!rounded.new_layout_boxy);
    }
    #[test]
    fn restoring_history_keeps_only_remembered_or_selected_projects() {
        let saved = serde_json::json!({
            "project": "/code/app",
            "projects": ["/code/app", "/code/other"],
            "tabs": [2],
            "active": 2,
            "desktop_project_preferences": {"projects": ["/code/app", "/code/other"]}
        })
        .to_string();
        let sessions = vec![
            (1, "/code/app".into()),
            (2, "/tmp/internal-mono-chat".into()),
            (3, "/private/var/folders/temporary".into()),
        ];
        let restored = Workspace::restore(Some(&saved), &sessions, "/code/app");
        assert_eq!(restored.projects, ["/code/app", "/code/other"]);
        assert_eq!(restored.project, "/code/app");
        assert_eq!(restored.active, None);
        let serialized = serde_json::to_value(restored).unwrap();
        assert_eq!(
            serialized["desktop_project_preferences"]["projects"],
            serde_json::json!(["/code/app", "/code/other"])
        );
    }
    #[test]
    fn restore_prunes_stale_tabs_and_preserves_an_intentionally_empty_workspace() {
        let mut s = Workspace::default();
        s.open(1);
        s.open(8);
        s.tabs.push(1);
        s.sidebar_width = 900.;
        let json = serde_json::to_string(&s).unwrap();
        let s = Workspace::restore(Some(&json), &[(1, "/work/a".into())], "/work/b");
        assert_eq!(s.tabs, vec![1]);
        assert_eq!(s.active, None);
        assert_eq!(s.project, "/work/b");
        assert_eq!(s.sidebar_width, 560.);
        assert_eq!(s.projects, vec!["/work/b"]);
        let s = Workspace::restore(Some("{}"), &[(1, "/work/a".into())], "/work/a");
        assert!(s.tabs.is_empty());
        assert!(
            s.compact_session_cards,
            "older workspaces retain compact cards"
        );
        assert!(
            s.sidebar_activity_visible,
            "older workspaces keep the activity card visible"
        );
    }
    #[test]
    fn desktop_snapshot_translation_deserializes_and_retains_unported_metadata() {
        let source = serde_json::json!({
            "projectCwd":"/work", "activeTabId":"tab",
            "tabs":[{"id":"tab", "focusedId":"b", "groupId":"assigned", "editorPanes":[], "layout":{"type":"split", "dir":"down", "children":[{"type":"leaf","id":"a"},{"type":"leaf","id":"b"}], "sizes":[0.4,0.6]}}],
            "projectTerminals":[{"projectPath":"/work", "side":"bottom", "size":240, "open":false, "pane":{"activeFileId":"term", "files":[{"id":"term", "terminal":true, "cwd":"/work/sub"}]}}]
        });
        let mapped = monocode_core::desktop_preferences::DesktopPreferences::workspace_snapshot(
            &source,
            |id| match id {
                "a" => Some(1),
                "b" => Some(2),
                _ => None,
            },
        );
        let saved = mapped.to_string();
        let restored = Workspace::restore(
            Some(&saved),
            &[(1, "/work".into()), (2, "/work".into())],
            "/work",
        );
        assert_eq!(restored.tabs, [1]);
        assert_eq!(restored.active, Some(2));
        assert!(restored.layouts[&1].is_split());
        assert_eq!(restored.focused[&1], 2);
        assert_eq!(restored.docks["/work"].terminals, ["/work/sub"]);
        assert!(!restored.docks["/work"].open);
        assert_eq!(restored.desktop_workspace_snapshot.as_ref(), Some(&source));
    }
}
