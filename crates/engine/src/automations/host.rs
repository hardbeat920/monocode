//! Calls this package makes into the window's workspace, which the app
//! fills in. Sessions are read and changed through the runtime's `Sessions`
//! entity directly; submissions and drafts default to the submit package.
//!
//! The TypeScript launch code called App.tsx state setters (`appendTab`,
//! `setActiveTabId`, `setSidebarTab`, the page toggles). The workspace
//! package keeps those private to its `Workspace` entity, so the app maps
//! these methods onto the window's workspace (NEEDS.md).

use std::rc::Rc;

use gpui::{App, AppContext, Task};
use monocode_core::block::ModelSettings;
use monocode_core::{Attachment, HarnessId, RuntimeMode, Session};
use monocode_layout::{SplitDir, WorkspaceTab};

use crate::projects::ProjectsGlobal;
use crate::runtime::Engine;
use crate::submit::attachments::{local_io, prepare_attachments};
use crate::submit::{SubmissionAcceptance, Submit, SubmitOptions};

/// `AppSessionPlacement`: put the new session in a split beside an open
/// pane instead of a new tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlacement {
    pub direction: SplitDir,
    pub beside_session_id: String,
}

/// The workspace calls a launch makes: automations and quick launches.
pub trait LaunchHost {
    /// `newSession` with the app's model catalog and preferences.
    fn new_session(
        &self,
        harness: HarnessId,
        cwd: &str,
        model: Option<&str>,
        runtime_mode: Option<RuntimeMode>,
        model_settings: Option<&ModelSettings>,
        cx: &App,
    ) -> Session {
        let inputs = ProjectsGlobal::model_inputs(cx);
        monocode_core::session::new_session(
            &inputs.env(),
            uuid::Uuid::new_v4().to_string(),
            harness,
            cwd,
            model,
            runtime_mode,
            model_settings,
        )
    }

    /// `mergeModelSettings(resolveModel(harness, model), settings)`.
    fn merge_model_settings(
        &self,
        harness: HarnessId,
        model: &str,
        settings: &ModelSettings,
        cx: &App,
    ) -> ModelSettings {
        let inputs = ProjectsGlobal::model_inputs(cx);
        let resolved = inputs.catalog.resolve_model(harness, Some(model));
        inputs
            .catalog
            .merge_model_settings(&resolved, Some(settings))
    }

    /// `appendTab(tab, cwd)`: add the tab beside the active one.
    fn append_tab(&self, _tab: WorkspaceTab, _cwd: &str, _cx: &mut App) {}

    /// `setActiveTabId(id)` and `setComposerFocused(false)`.
    fn activate_tab(&self, _tab_id: &str, _cx: &mut App) {}

    /// `focusOpenSession`: activate the tab that shows this session.
    fn focus_open_session(&self, _session_id: &str, _cx: &mut App) {}

    /// Close the search, Inbox, notes, and automations pages and show the
    /// sessions sidebar tab for `cwd`.
    fn show_sessions(&self, _cwd: &str, _cx: &mut App) {}

    /// `placeSession`: split the new session in beside an open pane of the
    /// same project, focusing it when `reveal`. Returns the tab id.
    fn place_session(
        &self,
        _session_id: &str,
        _placement: &SessionPlacement,
        _cwd: &str,
        _reveal: bool,
        _cx: &mut App,
    ) -> Result<String, String> {
        Err("The target session must be open in this project".into())
    }

    /// `setProjectCwd`.
    fn set_project_cwd(&self, _cwd: &str, _cx: &mut App) {}

    /// `setRecents(rememberProject(cwd))`.
    fn remember_project(&self, cwd: &str, cx: &mut App) {
        crate::projects::actions::remember_project(cwd, cx);
    }

    /// `revealTab` for a quick launch: activate the tab, then show the
    /// sessions list.
    fn reveal_tab(&self, tab_id: &str, cwd: &str, cx: &mut App) {
        self.activate_tab(tab_id, cx);
        self.show_sessions(cwd, cx);
    }

    /// `submitSession`.
    fn submit(
        &self,
        session_id: &str,
        text: &str,
        attachments: Vec<Attachment>,
        options: SubmitOptions,
        cx: &mut App,
    ) -> SubmissionAcceptance {
        match Submit::try_global(cx) {
            Some(submit) => submit.update(cx, |submit, cx| {
                submit.submit(session_id, text, attachments, options, cx)
            }),
            None => SubmissionAcceptance::Ready(false),
        }
    }

    /// `onSaveDraft`.
    fn save_draft(
        &self,
        session_id: &str,
        text: &str,
        attachments: Vec<Attachment>,
        request_id: &str,
        cx: &mut App,
    ) -> bool {
        match Submit::try_global(cx) {
            Some(submit) => submit.update(cx, |submit, cx| {
                submit.save_draft(
                    session_id,
                    text,
                    attachments,
                    Some(request_id.to_string()),
                    cx,
                )
            }),
            None => false,
        }
    }

    /// The window has focus, for choosing where a quick launch goes.
    fn is_focused(&self, _cx: &App) -> bool {
        false
    }

    /// The window is visible. Hidden windows still count: close-to-dock
    /// keeps them running.
    fn is_visible(&self, _cx: &App) -> bool {
        true
    }

    /// Unminimize, show, and focus the window.
    fn bring_forward(&self, _cx: &mut App) {}

    /// `prepareAttachments`: read vision images back from their paths.
    fn prepare_attachments(&self, files: Vec<Attachment>, cx: &App) -> Task<Vec<Attachment>> {
        let io = Submit::try_global(cx)
            .map(|submit| submit.read(cx).config().attachment_io.clone())
            .unwrap_or_else(local_io);
        cx.background_spawn(async move { prepare_attachments(io.as_ref(), &files).await })
    }
}

/// A host with no workspace: launches still create and submit sessions.
pub struct NoWorkspace;

impl LaunchHost for NoWorkspace {}

/// One workspace window, for reminder delivery.
pub trait ReminderHost {
    /// `openReminderSession`: show this open session in the window (close
    /// the full pages, select its project and its sidebar row). The
    /// reminders entity opens the session first.
    fn show_session(&self, _session: &Session, _cx: &mut App) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }

    /// The window has focus (`window.is_focused()`).
    fn is_focused(&self, _cx: &App) -> bool {
        false
    }

    /// Unminimize, show, and focus the window.
    fn bring_forward(&self, _cx: &mut App) {}
}

/// What reminders need from the app beyond its windows.
pub trait ReminderApp {
    /// `ensureReminderSessionsSaved`: save the open sessions before their
    /// reminder is set.
    fn ensure_saved(&self, session_ids: &[String], cx: &mut App) -> Task<Result<(), String>> {
        super::reminders::ensure_sessions_saved(session_ids, cx)
    }

    /// `message(String(error), { title: "Reminder", kind: "error" })`.
    fn show_error(&self, _message: &str, _cx: &mut App) {}

    /// `open_new_window`: no window can take an open request.
    fn open_new_window(&self, _cx: &mut App) {}
}

/// The default `ReminderApp`.
pub struct NoReminderApp;

impl ReminderApp for NoReminderApp {}

/// `ensureOpenSession` through the runtime: the open session, or the stored
/// one opened now.
pub fn ensure_open_session(session_id: &str, cx: &mut App) -> Task<Option<Session>> {
    let sessions = Engine::sessions(cx);
    sessions.update(cx, |sessions, cx| sessions.ensure_open(session_id, cx))
}

/// Shared handle type for hosts.
pub type SharedLaunchHost = Rc<dyn LaunchHost>;
