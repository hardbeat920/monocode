//! Calls the projects package makes into code it does not own. App.tsx did
//! these inline against React state that now belongs to other packages: the
//! tabs and the current project (workspace), the session sidebar and session
//! folders (history), CI repairs and handoffs (submit), orchestration runs,
//! remote shells, and the live model catalog (the harness bridge).
//!
//! The runtime's `EngineHooks` has none of these yet (see NEEDS.md), so the
//! package keeps its own trait. Every method has a default, so the package
//! runs in tests and in the headless host. The owner of each piece installs
//! an implementation with `ProjectsGlobal::set_hooks`.
//!
//! Hooks run outside `Sessions` and outside the `Projects` entity, so they
//! may read and update both.

use gpui::App;
use monocode_core::models::{HarnessAvailability, ModelCatalog};
use monocode_core::{HarnessId, Session};
use monocode_layout::layout::{FilePaneTab, WorkspaceTab};
use monocode_layout::project_return::ProjectReturnMemory;
use serde::{Deserialize, Serialize};

use crate::runtime::session_store::SessionSummary;

/// `SessionFolderTarget` from src/features/sessions/model/sessionFolders.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionFolderTarget {
    #[serde(rename = "existing", rename_all = "camelCase")]
    Existing { folder_id: String },
    #[serde(rename = "new")]
    New { name: String },
}

/// What the projects package asks of the workspace, history, submit,
/// orchestration, remote, and the harness bridge.
pub trait ProjectsHooks {
    // The workspace: tabs, the current project, and the window chrome.

    /// `projectCwd`: the project the window shows.
    fn project_cwd(&self, _cx: &App) -> String {
        "~".into()
    }

    /// `setProjectCwd`.
    fn set_project_cwd(&self, _cwd: &str, _cx: &mut App) {}

    /// The open tabs (`tabsRef.current`).
    fn tabs(&self, _cx: &App) -> Vec<WorkspaceTab> {
        Vec::new()
    }

    /// `activeTabIdRef.current`.
    fn active_tab_id(&self, _cx: &App) -> String {
        String::new()
    }

    /// `readProjectReturnMemory`: the memory reconciled against the tabs now.
    fn project_return_memory(&self, _cx: &mut App) -> ProjectReturnMemory {
        ProjectReturnMemory::new()
    }

    /// Every file open in the tabs and the project terminal docks
    /// (`filesInWorkspaceTabs` plus each dock pane's files).
    fn open_files(&self, _cx: &App) -> Vec<FilePaneTab> {
        Vec::new()
    }

    /// `appendTab`: insert beside the active tab. `cwd` scopes tab group
    /// inheritance.
    fn append_tab(&self, _tab: WorkspaceTab, _cwd: Option<&str>, _cx: &mut App) {}

    /// `insertBeside` on the current tabs, anchored to `anchor_id`.
    fn insert_tab_beside(
        &self,
        _tab: WorkspaceTab,
        _anchor_id: Option<&str>,
        _cwd: Option<&str>,
        _cx: &mut App,
    ) {
    }

    /// Replace every tab and the active tab at once (`setTabs` plus
    /// `setActiveTabId`).
    fn set_tabs(&self, _tabs: Vec<WorkspaceTab>, _active_tab_id: &str, _cx: &mut App) {}

    /// `setActiveTabId`.
    fn set_active_tab(&self, _tab_id: &str, _cx: &mut App) {}

    /// `activateTab`.
    fn activate_tab(&self, _tab_id: &str, _pane_id: Option<&str>, _cx: &mut App) {}

    /// `setComposerFocused`.
    fn set_composer_focused(&self, _focused: bool, _cx: &mut App) {}

    /// `workspaceNavigation.selectProject`: the project rail is about to
    /// open `path`. Once its landing tab shows, the workspace returns to the
    /// worktree it showed last in that project.
    fn select_project_workspace(&self, _path: &str, _cx: &mut App) {}

    /// `workspaceNavigation.cancel`: drop a pending workspace switch.
    fn cancel_workspace_navigation(&self, _cx: &mut App) {}

    /// Close the full pages (search, inbox, notes, automations) before a
    /// project opens.
    fn close_pages(&self, _cx: &mut App) {}

    /// The tab group check in `onCwdChange`: the session moved to another
    /// project in place, so its tab leaves a group of a different project.
    fn session_project_changed(&self, _session_id: &str, _cwd: &str, _cx: &mut App) {}

    /// Drop these files from the unsaved set (`setDirtyFiles`).
    fn forget_dirty_files(&self, _file_ids: &[String], _cx: &mut App) {}

    /// Close the project's terminal dock (`setProjectTerminals`).
    fn remove_project_terminals(&self, _path: &str, _cx: &mut App) {}

    /// The tab and dock part of `applyProjectLocationChange`: open sessions,
    /// the current project, docks, and file tabs follow the folder.
    fn project_location_changed(&self, _from: &str, _to: &str, _cx: &mut App) {}

    /// The sidebar showed the removed project's tab selection; show home's.
    fn project_sidebar_tab_removed(&self, _path: &str, _cx: &mut App) {}

    /// The sidebar tab selection follows a renamed project.
    fn project_sidebar_tab_moved(&self, _from: &str, _to: &str, _cx: &mut App) {}

    // History: the session sidebar and session folders.

    /// Patch the session sidebar rows and the stored linked sessions
    /// (`setHistory` and `setStoredLinkedSessions`). `patch` returns `None`
    /// to keep a row.
    fn patch_summaries(
        &self,
        _patch: &dyn Fn(&SessionSummary) -> Option<SessionSummary>,
        _cx: &mut App,
    ) {
    }

    /// `refreshHistory(cwd)`.
    fn refresh_history(&self, _cwd: &str, _cx: &mut App) {}

    /// `setLoadedProjects`: the set of projects whose history is loaded
    /// follows a rename.
    fn rebase_loaded_project(&self, _from: &str, _to: &str, _cx: &mut App) {}

    /// `rebaseSessionFolderSettings`.
    fn rebase_session_folder_settings(&self, _from: &str, _to: &str, _cx: &mut App) {}

    /// `loadSessionFolders`, the existing-folder check, `placeSessionInFolder`,
    /// and `saveSessionFolders` for one project.
    fn place_session_in_folder(
        &self,
        _cwd: &str,
        _session_id: &str,
        _target: &SessionFolderTarget,
        _cx: &mut App,
    ) {
    }

    // Submit.

    /// `rebaseCiRepairs`.
    fn rebase_ci_repairs(&self, _from: &str, _to: &str, _cx: &mut App) {}

    /// `buildDeterministicHandoff(session)`: the recap of a conversation.
    fn build_deterministic_handoff(&self, _session: &Session) -> String {
        String::new()
    }

    /// `appendReadyHandoff`: a pending handoff block carrying `text`.
    fn append_ready_handoff(
        &self,
        session: &Session,
        _from: HarnessId,
        _to: HarnessId,
        _text: &str,
    ) -> Session {
        session.clone()
    }

    // Orchestration and remote.

    /// `orchestrator.forSession(id)` has a run that is active or paused.
    fn orchestration_running(&self, _session_id: &str, _cx: &App) -> bool {
        false
    }

    /// `remoteSessionFor`: the remote session a local shell pane shows.
    fn remote_session_for(&self, _session_id: &str, _cx: &App) -> Option<String> {
        None
    }

    // The harness bridge.

    /// The live model catalog. The default has only the bundled models.
    fn model_catalog(&self, _cx: &App) -> ModelCatalog {
        ModelCatalog::new()
    }

    /// Which CLIs the installer probe found.
    fn harness_availability(&self, _cx: &App) -> HarnessAvailability {
        HarnessAvailability::default()
    }
}

/// The default hooks: nothing is wired.
pub struct NoProjectsHooks;

impl ProjectsHooks for NoProjectsHooks {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_targets_use_the_typescript_shape() {
        let existing = SessionFolderTarget::Existing {
            folder_id: "f1".into(),
        };
        assert_eq!(
            serde_json::to_string(&existing).unwrap(),
            r#"{"kind":"existing","folderId":"f1"}"#
        );
        let new = SessionFolderTarget::New {
            name: "Active".into(),
        };
        assert_eq!(
            serde_json::to_string(&new).unwrap(),
            r#"{"kind":"new","name":"Active"}"#
        );
    }
}
