//! Calls from the inbox into the rest of the app: notification preferences
//! and sounds (attention), automation claims (automations), session
//! creation and navigation (the app shell and workspace), and the submit
//! pipeline (submit).
//!
//! The runtime's `EngineHooks` has no inbox field, so the inbox keeps its
//! own hook object; `Inbox::set_hooks` replaces it. Every method has a
//! default so the inbox runs alone in tests. Hooks run while the `Inbox`
//! entity is updating, so they must not read or update `Inbox`.

use gpui::{App, Task};
use monocode_core::harness::HarnessId;
use monocode_core::session::{LinkedWorkItem, Session};

use super::ci_repair::CiRepairRequest;
use super::ci_repair_tracking::CiRepairOutcome;
use super::inbox_notifications::InboxNotificationSubject;
use super::types::{InboxItem, provider_str};

/// The callback a submitted CI repair calls when its own agent turn ends.
pub type CiRepairSettle = Box<dyn FnOnce(CiRepairOutcome, &mut App)>;

pub trait InboxHooks {
    /// `inboxNotificationProject(item).id` from the attention package. The
    /// default keys by provider and repository, or by local checkout.
    fn notification_project_id(&self, item: &InboxItem) -> String {
        if item.project_path.is_empty() {
            format!(
                "repository:{}:{}",
                provider_str(item.provider),
                item.repo.to_lowercase()
            )
        } else {
            format!("local:{}", item.project_path)
        }
    }

    /// `rememberNotificationProjects(items.map(inboxNotificationProject))`.
    fn remember_notification_projects(&self, _items: &[InboxItem], _cx: &mut App) {}

    /// `allowsProjectNotificationIndicator(subject, loadNotificationPreferences())`:
    /// category choices and mutes hide badges, never the unread state.
    fn allows_notification_indicator(
        &self,
        _subject: &InboxNotificationSubject,
        _cx: &App,
    ) -> bool {
        true
    }

    /// `playCue("inboxUnseen", subject)`. Returns whether a sound played.
    fn play_inbox_cue(&self, _subject: &InboxNotificationSubject, _cx: &mut App) -> bool {
        false
    }

    /// `onAppeared`: called on every successful poll with the items that
    /// appeared since the last one, so inbox automations can claim them.
    fn inbox_appeared(&self, _items: &[InboxItem], _cx: &mut App) {}

    /// `newDefaultSession(cwd, sessionDefaults?.runtimeMode)`, or
    /// `newDefaultSession(cwd)` when `inherit_runtime_mode` is false.
    fn new_default_session(
        &self,
        cwd: &str,
        _inherit_runtime_mode: bool,
        _cx: &mut App,
    ) -> Session {
        Session::blank(uuid::Uuid::new_v4().to_string(), HarnessId::Claude, "", cwd)
    }

    /// `newSession(harness, cwd, model, runtimeMode, modelSettings)` for a
    /// restarted Ask conversation.
    fn new_session_like(&self, stopped: &Session, _cx: &mut App) -> Session {
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

    /// `invoke("default_cwd")`.
    fn default_cwd(&self, _cx: &mut App) -> Task<Result<String, String>> {
        Task::ready(Ok(std::env::var("HOME").unwrap_or_else(|_| "~".into())))
    }

    /// The folder a started item opens in when it has no local project:
    /// `active?.cwd || sessionDefaults?.cwd || projectCwd`.
    fn start_cwd(&self, _cx: &App) -> String {
        "~".into()
    }

    /// The sidebar's project (`sidebarCwd`).
    fn sidebar_cwd(&self, _cx: &App) -> String {
        "~".into()
    }

    /// Close the Inbox page and the other full pages the action covers
    /// (`setInboxViewOpen(false)` and friends).
    fn leave_inbox(&self, _cx: &mut App) {}

    /// `setSidebarTab("sessions", cwd)`.
    fn show_sessions_sidebar(&self, _cwd: Option<&str>, _cx: &mut App) {}

    /// `appendTab(newTab(id), cwd)`, `setActiveTabId`, and focus the composer.
    fn open_session_tab(&self, _session_id: &str, _cwd: &str, _cx: &mut App) {}

    /// `onSelectHistorySession`: open the session in the workspace.
    fn select_session(&self, _session_id: &str, _cx: &mut App) -> Task<()> {
        Task::ready(())
    }

    /// `onSubmit(sessionId, request.text, [], { ciRepair: request, ... })`.
    /// Returns whether the chat accepted the turn; `settle` runs when the
    /// turn ends.
    fn submit_ci_repair(
        &self,
        _session_id: &str,
        _request: &CiRepairRequest,
        _settle: CiRepairSettle,
        _cx: &mut App,
    ) -> bool {
        false
    }

    /// A session's linked work item changed or rolled back; the session
    /// history should show it. `refresh` asks for `refreshHistory`.
    fn linked_work_item_changed(
        &self,
        _session_id: &str,
        _linked: Option<&LinkedWorkItem>,
        _refresh: bool,
        _cx: &mut App,
    ) {
    }
}

/// The defaults for every hook.
pub struct NoopInboxHooks;

impl InboxHooks for NoopInboxHooks {}
