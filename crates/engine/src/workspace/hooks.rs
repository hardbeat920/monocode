//! The workspace package's side of `runtime::WorkspaceHooks`, and the
//! package setup.
//!
//! The runtime calls most hooks while it holds the `Sessions` entity, and a
//! `Workspace` may be in the middle of its own update when it asks
//! `Sessions` to save. So hooks never read the `Workspace` entity. Each
//! window's workspace copies what the hooks need into a `Mirror` after
//! every change, and the hooks read the mirrors.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use gpui::{App, Global, Task};
use monocode_core::Session;
use monocode_layout::project_return::{ProjectReturnMemory, reconcile_project_return};
use monocode_layout::project_terminal::{DockSide, ProjectTerminalDock};
use monocode_layout::workspace_snapshot::{
    WorkspaceSnapshot, collect_workspace_snapshot, hydrate_workspace_snapshot_value,
    parse_project_return_targets, parse_workspace_snapshot,
};
use monocode_layout::{WorkspaceTab, leaf_ids, new_tab};
use serde_json::{Value, json};

use super::delegate::{NoDelegate, WorkspaceDelegate};
use super::files::{Files, FsBackend, LocalFs};
use super::session_factory::{ModelEnvSessions, SessionFactory};
use super::terminals::Terminals;
use crate::runtime::Engine;
use crate::runtime::hooks::WorkspaceHooks;
use crate::runtime::in_flight::{ResumedWorkspace, mark_turn_interrupted};

/// What the hooks know about one window's workspace.
#[derive(Debug, Clone, Default)]
pub struct Mirror {
    pub tabs: Vec<WorkspaceTab>,
    pub active_tab_id: String,
    pub project_cwd: String,
    pub docks: Vec<ProjectTerminalDock>,
    pub last_dock_side: Option<DockSide>,
    pub project_return: ProjectReturnMemory,
    /// This window saves the workspace snapshot. A window opened by a
    /// window transfer does not.
    pub autosave: bool,
    /// The window is hidden or minimized.
    pub hidden: bool,
    /// A full page (settings, search, inbox) covers the workspace.
    pub covered: bool,
    /// The mounted Inbox Ask conversation in this window.
    pub inbox_session_id: Option<String>,
    pub inbox_visible: bool,
}

type Windows = Rc<RefCell<Vec<Weak<RefCell<Mirror>>>>>;

/// The package global: the open windows' mirrors and the services every
/// workspace shares.
pub struct WorkspacePackage {
    windows: Windows,
    pub sessions: Rc<dyn SessionFactory>,
    pub delegate: Rc<dyn WorkspaceDelegate>,
}

impl Global for WorkspacePackage {}

/// What `init` needs.
pub struct WorkspaceSetup {
    pub fs: Arc<dyn FsBackend>,
    pub sessions: Rc<dyn SessionFactory>,
    pub delegate: Rc<dyn WorkspaceDelegate>,
    /// Real PTYs. Tests install fake terminals themselves and pass `false`.
    pub terminals: bool,
}

impl Default for WorkspaceSetup {
    fn default() -> Self {
        Self {
            fs: Arc::new(LocalFs),
            sessions: Rc::new(ModelEnvSessions::default()),
            delegate: Rc::new(NoDelegate),
            terminals: true,
        }
    }
}

/// Install the files model, the terminals, and the workspace hooks. Call
/// after `Engine::init`; without an engine the hooks are not installed.
pub fn init(setup: WorkspaceSetup, cx: &mut App) {
    Files::init(setup.fs, cx);
    if setup.terminals {
        Terminals::init(cx);
    }
    let windows: Windows = Rc::default();
    let hooks = Rc::new(WorkspaceHooksImpl {
        windows: windows.clone(),
        sessions: setup.sessions.clone(),
        delegate: setup.delegate.clone(),
    });
    cx.set_global(WorkspacePackage {
        windows,
        sessions: setup.sessions,
        delegate: setup.delegate,
    });
    if Engine::try_global(cx).is_some() {
        Engine::set_hooks(cx, |engine_hooks| engine_hooks.workspace = hooks);
    }
}

impl WorkspacePackage {
    pub fn try_global(cx: &App) -> Option<&WorkspacePackage> {
        cx.try_global::<WorkspacePackage>()
    }

    /// Track a window's mirror. The registry holds it weakly, so a closed
    /// window drops out on its own.
    pub(crate) fn register(&self, mirror: &Rc<RefCell<Mirror>>) {
        let mut windows = self.windows.borrow_mut();
        windows.retain(|window| window.strong_count() > 0);
        windows.push(Rc::downgrade(mirror));
    }
}

/// `WorkspaceHooks` over the registered mirrors.
pub struct WorkspaceHooksImpl {
    windows: Windows,
    sessions: Rc<dyn SessionFactory>,
    delegate: Rc<dyn WorkspaceDelegate>,
}

impl WorkspaceHooksImpl {
    fn mirrors(&self) -> Vec<Rc<RefCell<Mirror>>> {
        self.windows
            .borrow()
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }
}

/// The snapshot JSON for a set of tabs.
pub(crate) fn snapshot_value(mirror: &Mirror, sessions: &[Session]) -> Value {
    let memory = reconcile_project_return(
        &mirror.project_return,
        &mirror.tabs,
        sessions,
        &mirror.active_tab_id,
    );
    let snapshot = collect_workspace_snapshot(
        &mirror.tabs,
        sessions,
        &mirror.active_tab_id,
        &mirror.project_cwd,
        &memory,
        &mirror.docks,
        mirror.last_dock_side,
    );
    serde_json::to_value(snapshot).unwrap_or(Value::Null)
}

/// The workspace part of a runtime `ResumedWorkspace`: its `layout` JSON in
/// the snapshot's shape, read back.
pub fn mirror_from_layout(layout: &Value, project_cwd: &str) -> Option<Mirror> {
    let snapshot: WorkspaceSnapshot = serde_json::from_value(layout.clone()).ok()?;
    Some(Mirror {
        project_return: parse_project_return_targets(layout.get("projectReturnTargets")),
        tabs: snapshot.tabs,
        active_tab_id: snapshot.active_tab_id,
        project_cwd: if snapshot.project_cwd.is_empty() {
            project_cwd.to_string()
        } else {
            snapshot.project_cwd
        },
        docks: snapshot.project_terminals,
        last_dock_side: snapshot.last_dock_side,
        autosave: true,
        hidden: false,
        covered: false,
        inbox_session_id: None,
        inbox_visible: false,
    })
}

impl WorkspaceHooks for WorkspaceHooksImpl {
    fn window_hidden(&self, _cx: &App) -> bool {
        let mirrors = self.mirrors();
        !mirrors.is_empty() && mirrors.iter().all(|mirror| mirror.borrow().hidden)
    }

    fn is_foreground(&self, session_id: &str, _cx: &App) -> bool {
        self.mirrors().iter().any(|mirror| {
            let mirror = mirror.borrow();
            if mirror.hidden {
                return false;
            }
            if mirror.covered {
                return mirror.inbox_visible
                    && mirror.inbox_session_id.as_deref() == Some(session_id);
            }
            let Some(tab) = mirror
                .tabs
                .iter()
                .find(|tab| tab.id == mirror.active_tab_id)
                .or(mirror.tabs.first())
            else {
                return false;
            };
            if leaf_ids(&tab.layout).iter().any(|id| id == session_id) {
                return true;
            }
            // The agent behind an active agent tab.
            tab.editor_panes.iter().any(|pane| {
                pane.files
                    .iter()
                    .find(|file| file.id == pane.active_file_id)
                    .and_then(|file| file.agent.as_ref())
                    .is_some_and(|agent| agent.session_id == session_id)
            })
        })
    }

    fn tab_session_ids(&self, _cx: &App) -> Vec<String> {
        self.mirrors()
            .iter()
            .flat_map(|mirror| {
                mirror
                    .borrow()
                    .tabs
                    .iter()
                    .flat_map(|tab| leaf_ids(&tab.layout))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn collect_snapshot(&self, sessions: &[Session], _cx: &App) -> Option<Value> {
        let mirror = self
            .mirrors()
            .into_iter()
            .find(|mirror| mirror.borrow().autosave)?;
        let mirror = mirror.borrow();
        Some(snapshot_value(&mirror, sessions))
    }

    fn collect_resumed_snapshot(&self, workspace: &ResumedWorkspace) -> Option<Value> {
        let mirror = mirror_from_layout(&workspace.layout, &workspace.project_cwd)?;
        Some(snapshot_value(&mirror, &workspace.sessions))
    }

    fn snapshot_session_ids(&self, snapshot: &Value) -> Vec<String> {
        let Some(parsed) = parse_workspace_snapshot(snapshot) else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        parsed
            .sessions
            .iter()
            .map(|stub| stub.id.clone())
            .chain(parsed.tabs.iter().flat_map(|tab| leaf_ids(&tab.layout)))
            .filter(|id| seen.insert(id.clone()))
            .collect()
    }

    fn hydrate_snapshot(
        &self,
        snapshot: &Value,
        loaded: &HashMap<String, Session>,
        interrupted: &HashSet<String>,
    ) -> Option<ResumedWorkspace> {
        // TODO(port): the TypeScript appended tabs for interrupted chats in
        // quit-list order. The hook gets a set, so the order is by id.
        let mut interrupted_ids: Vec<String> = interrupted.iter().cloned().collect();
        interrupted_ids.sort();
        let resumed = hydrate_workspace_snapshot_value(
            snapshot,
            loaded,
            &interrupted_ids,
            &self.sessions.env(),
            |session| mark_turn_interrupted(&session),
        )?;
        let memory = resumed.project_return_memory.unwrap_or_default();
        let snapshot = collect_workspace_snapshot(
            &resumed.tabs,
            &resumed.sessions,
            &resumed.active_tab_id,
            &resumed.project_cwd,
            &memory,
            resumed.project_terminals.as_deref().unwrap_or_default(),
            resumed.last_dock_side,
        );
        Some(ResumedWorkspace {
            layout: serde_json::to_value(snapshot).unwrap_or(Value::Null),
            sessions: resumed.sessions,
            project_cwd: resumed.project_cwd,
        })
    }

    fn layout_for_sessions(&self, session_ids: &[String]) -> Value {
        let tabs: Vec<WorkspaceTab> = session_ids.iter().map(|id| new_tab(id)).collect();
        let active = tabs.first().map(|tab| tab.id.clone()).unwrap_or_default();
        json!({ "tabs": tabs, "activeTabId": active, "projectTerminals": [] })
    }

    fn confirm(&self, message: &str, ok_label: &str, cx: &mut App) -> Task<bool> {
        self.delegate.confirm(message, ok_label, cx)
    }

    fn hide_window(&self, cx: &mut App) {
        self.delegate.hide_window(cx);
    }

    fn close_window(&self, cx: &mut App) {
        self.delegate.close_window(cx);
    }

    fn kill_terminals(&self, cx: &mut App) -> Task<()> {
        match Terminals::try_global(cx) {
            Some(terminals) => terminals.kill_all(cx),
            None => Task::ready(()),
        }
    }

    fn resolve_workspace_path(&self, path: &str, cwd: &str) -> Option<String> {
        super::paths::resolve_workspace_path(path, Some(cwd))
    }

    fn nudge_watched_files(&self, paths: Option<&[String]>, cx: &mut App) {
        Files::nudge_watched_files(paths, cx);
    }

    fn invalidate_watched_files(&self, paths: Option<&[String]>, cx: &mut App) {
        Files::invalidate_watched_files(paths, cx);
    }

    fn notify_git_changed(&self, cx: &mut App) {
        Files::notify_git_changed(cx);
    }

    fn nudge_workspace(&self, cwd: Option<&str>, cx: &mut App) {
        Files::nudge_workspace(cwd, cx);
    }
}
