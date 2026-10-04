//! What the settings page needs from the rest of the app.
//!
//! Settings that lived in localStorage read and write `Kv` directly (see
//! `store.rs`). Everything else, such as native calls, provider CLIs,
//! integration accounts, and archived projects, goes through one small
//! trait per section. The engine implements them; every method has a
//! harmless default, so a gallery or a test only fills in what it checks.
//!
//! Views other crates build plug into slots: the connections page, the MCP
//! settings page, the skills page, the worktrees page, the project
//! notification card, and the provider accounts card.

use std::rc::Rc;

use gpui::{AnyElement, AnyView, App, Entity, Task, Window};
use monocode_core::harness::HarnessId;
use monocode_core::models::{HarnessAvailability, ModelCatalog};
use monocode_core::settings::{CollapsedProjectRailMode, KeybindingOverrides, SettingsSectionId};

/// An async host call. Errors carry the message the page shows.
pub type HostTask<T> = Task<Result<T, String>>;

fn ready<T: 'static>(value: T) -> HostTask<T> {
    Task::ready(Ok(value))
}

fn unavailable<T: 'static>() -> HostTask<T> {
    Task::ready(Err("Not available in this build".into()))
}

/// `NotificationPermission`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationPermission {
    #[default]
    Prompt,
    Granted,
    Denied,
    Unsupported,
}

/// `UpdaterSnapshot.phase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    Downloading,
    Available,
    Current,
    Error,
}

/// `UpdaterSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterSnapshot {
    pub phase: UpdatePhase,
    pub current_version: String,
    pub available_version: Option<String>,
    /// Download progress in percent, when known.
    pub progress: Option<i64>,
    pub error: Option<String>,
}

impl Default for UpdaterSnapshot {
    fn default() -> Self {
        Self {
            phase: UpdatePhase::Idle,
            current_version: "…".into(),
            available_version: None,
            progress: None,
            error: None,
        }
    }
}

/// Receives each updater snapshot as the flow runs.
pub type UpdateReporter = Rc<dyn Fn(UpdaterSnapshot, &mut App)>;

/// The General section: alerts, quick composer registration, and updates.
pub trait GeneralHost {
    /// `playCue("switch")`, played when a settings switch flips.
    fn play_switch_cue(&self, _cx: &mut App) {}
    /// `cachedNotificationPermission`.
    fn cached_notification_permission(&self, _cx: &App) -> NotificationPermission {
        NotificationPermission::Prompt
    }
    /// `probeNotificationPermission`.
    fn probe_notification_permission(&self, cx: &mut App) -> Task<NotificationPermission> {
        Task::ready(self.cached_notification_permission(cx))
    }
    /// `requestNotificationPermission`.
    fn request_notification_permission(&self, cx: &mut App) -> Task<NotificationPermission> {
        self.probe_notification_permission(cx)
    }
    /// `openNotificationSettings`.
    fn open_notification_settings(&self, _cx: &mut App) {}
    /// `readAppVersion`.
    fn app_version(&self, _cx: &mut App) -> Task<String> {
        Task::ready(env!("CARGO_PKG_VERSION").to_string())
    }
    /// `runUpdateFlow(manual, onSnapshot)`.
    fn run_update_flow(&self, _manual: bool, _report: UpdateReporter, _cx: &mut App) {}
    /// `installPendingUpdate(onSnapshot)`.
    fn install_pending_update(&self, _report: UpdateReporter, _cx: &mut App) {}
}

/// Native shortcut registration for the Keybindings section and the quick
/// composer switch.
pub trait KeybindingsHost {
    /// `setQuickComposerShortcut(enabled, shortcut)`: registers or drops the
    /// global hotkey. An error leaves the stored chord unchanged.
    fn set_quick_composer_shortcut(
        &self,
        _enabled: bool,
        _shortcut: Option<&str>,
        _cx: &mut App,
    ) -> HostTask<()> {
        ready(())
    }
    /// `keybindings_set_overrides`: macOS menus show the rebound keys.
    fn set_keybinding_overrides(
        &self,
        _overrides: &KeybindingOverrides,
        _cx: &mut App,
    ) -> HostTask<()> {
        ready(())
    }
}

/// The Appearance section's native and file calls.
pub trait AppearanceHost {
    /// `applySidebarBlur`: the window's background blur radius.
    fn set_window_background_blur(&self, _radius: i64, _cx: &mut App) {}
    /// `pickAndSaveChatBackground`: opens a picker and copies the image into
    /// app data. `Ok(None)` when the user cancelled.
    fn pick_and_save_chat_background(&self, _cx: &mut App) -> HostTask<Option<String>> {
        ready(None)
    }
    /// `removeChatBackground`.
    fn remove_chat_background(&self, _cx: &mut App) -> HostTask<()> {
        ready(())
    }
}

/// `HarnessBinaryInspection`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BinaryInspection {
    pub path: String,
    pub version: Option<String>,
    pub error: Option<String>,
}

/// The Providers section: the model catalog, CLI detection, and CLI paths.
pub trait ProvidersHost {
    /// The live model catalog. The default has only the bundled models.
    fn catalog(&self, _cx: &App) -> ModelCatalog {
        ModelCatalog::new()
    }
    /// Which CLIs the installer probe found.
    fn availability(&self, _cx: &App) -> HarnessAvailability {
        HarnessAvailability::default()
    }
    /// `harnessUnavailableHint`.
    fn harness_unavailable_hint(&self, _harness: HarnessId) -> String {
        String::new()
    }
    /// `probeHarnessAvailability`.
    fn probe_harness_availability(&self, _cx: &mut App) {}
    /// `refreshHarnessCatalogs([harness])`.
    fn refresh_harness_catalog(&self, _harness: HarnessId, _cx: &mut App) {}
    /// `runtimeProviderBinaryPath`: the CLI path this process launched with.
    fn runtime_binary_path(&self, _provider: HarnessId) -> Option<String> {
        None
    }
    /// `inspectHarnessBinary`: resolve the CLI and read its version.
    fn inspect_binary(
        &self,
        _provider: HarnessId,
        _path: Option<&str>,
        _cx: &mut App,
    ) -> HostTask<BinaryInspection> {
        unavailable()
    }
    /// `revealPath`.
    fn reveal_path(&self, _path: &str, _cx: &mut App) -> HostTask<()> {
        ready(())
    }
    /// `ProjectScopeIcon`: the project's logo or mascot, when the host has one.
    fn project_icon(&self, _path: &str, _window: &mut Window, _cx: &mut App) -> Option<AnyElement> {
        None
    }
}

/// `GithubStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GithubStatus {
    pub installed: bool,
    pub connected: bool,
}

/// GitLab and Azure DevOps connection state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UrlStatus {
    pub connected: bool,
    pub url: String,
}

/// `LinearTeam`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinearTeam {
    pub id: String,
    pub name: String,
    pub key: Option<String>,
}

/// `JiraStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JiraStatus {
    pub connected: bool,
    pub site: String,
    pub email: String,
}

/// `JiraProject`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraProject {
    pub id: String,
    pub key: String,
    pub name: String,
}

/// The Inbox section's integration accounts.
pub trait InboxHost {
    fn open_url(&self, _url: &str, _cx: &mut App) {}
    /// `clearInboxCache`.
    fn clear_inbox_cache(&self, _cx: &mut App) {}
    fn github_status(&self, _cx: &mut App) -> HostTask<GithubStatus> {
        ready(GithubStatus::default())
    }
    fn gitlab_status(&self, _cx: &mut App) -> HostTask<UrlStatus> {
        ready(UrlStatus {
            connected: false,
            url: "https://gitlab.com".into(),
        })
    }
    fn save_gitlab(&self, _url: &str, _token: &str, _cx: &mut App) -> HostTask<UrlStatus> {
        unavailable()
    }
    fn disconnect_gitlab(&self, url: &str, _cx: &mut App) -> HostTask<UrlStatus> {
        ready(UrlStatus {
            connected: false,
            url: url.into(),
        })
    }
    fn azure_devops_status(&self, _cx: &mut App) -> HostTask<UrlStatus> {
        ready(UrlStatus::default())
    }
    fn save_azure_devops(&self, _url: &str, _token: &str, _cx: &mut App) -> HostTask<UrlStatus> {
        unavailable()
    }
    fn disconnect_azure_devops(&self, _url: &str, _cx: &mut App) -> HostTask<UrlStatus> {
        ready(UrlStatus::default())
    }
    fn linear_connected(&self, _cx: &mut App) -> HostTask<bool> {
        ready(false)
    }
    fn list_linear_teams(&self, _cx: &mut App) -> HostTask<Vec<LinearTeam>> {
        ready(Vec::new())
    }
    fn save_linear_token(&self, _token: &str, _cx: &mut App) -> HostTask<()> {
        unavailable()
    }
    fn disconnect_linear(&self, _cx: &mut App) -> HostTask<()> {
        ready(())
    }
    /// `notifyLinearChange`.
    fn notify_linear_change(&self, _cx: &mut App) {}
    fn jira_status(&self, _cx: &mut App) -> HostTask<JiraStatus> {
        ready(JiraStatus::default())
    }
    /// `saveJiraConfig`; `disconnectJira` saves empty values.
    fn save_jira_config(
        &self,
        _site: &str,
        _email: &str,
        _token: &str,
        _cx: &mut App,
    ) -> HostTask<JiraStatus> {
        unavailable()
    }
    fn list_jira_projects(&self, _cx: &mut App) -> HostTask<Vec<JiraProject>> {
        ready(Vec::new())
    }
    /// `notifyJiraChange`.
    fn notify_jira_change(&self, _cx: &mut App) {}
}

/// `ArchivedProject`, with the label the project rail shows for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedProject {
    pub path: String,
    pub label: String,
}

/// The Archive section's project data.
pub trait ArchiveHost {
    /// `loadArchivedProjects`, labeled with `resolveTabGroupLabel`.
    fn archived_projects(&self, _cx: &App) -> Vec<ArchivedProject> {
        Vec::new()
    }
    /// `projectSessionCount`, for the delete dialog.
    fn project_session_count(&self, _path: &str, _cx: &mut App) -> Task<Option<i64>> {
        Task::ready(None)
    }
}

/// The project background dialog's per-project settings, owned by the
/// engine's projects package.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBackgroundSettings {
    pub path: String,
    pub empty_opacity: f64,
    pub session_opacity: f64,
    pub scope: monocode_core::appearance::ChatBackgroundScope,
    pub effect: monocode_core::appearance::NewThreadBackgroundEffect,
}

/// `projectChatBackground.ts` and `chatBackground.ts` for one project.
pub trait ProjectBackgroundHost {
    /// `loadProjectChatBackgroundSettings`.
    fn load_settings(&self, _project: &str, _cx: &App) -> Option<ProjectBackgroundSettings> {
        None
    }
    /// `saveProjectChatBackgroundSettings`.
    fn save_settings(
        &self,
        _project: &str,
        _settings: &ProjectBackgroundSettings,
        _image_changed: bool,
        _cx: &mut App,
    ) {
    }
    /// `clearProjectChatBackgroundSetting`.
    fn clear_setting(&self, _project: &str, _cx: &mut App) {}
    /// `projectChatBackgroundImageRevision`.
    fn image_revision(&self, _cx: &App) -> i64 {
        0
    }
    /// `pickAndSaveProjectChatBackground`.
    fn pick_and_save(&self, _project: &str, _cx: &mut App) -> HostTask<Option<String>> {
        ready(None)
    }
    /// `clearProjectChatBackground`: delete the copied image.
    fn clear_image(&self, _project: &str, _cx: &mut App) -> HostTask<()> {
        ready(())
    }
}

/// A host with every default.
#[derive(Default)]
pub struct NoopHost;

impl GeneralHost for NoopHost {}
impl KeybindingsHost for NoopHost {}
impl AppearanceHost for NoopHost {}
impl ProvidersHost for NoopHost {}
impl InboxHost for NoopHost {}
impl ArchiveHost for NoopHost {}
impl ProjectBackgroundHost for NoopHost {}

/// What a slot's view is built from.
#[derive(Clone, Debug, Default)]
pub struct SlotContext {
    pub section: SettingsSectionId,
    pub cwd: String,
    pub recents: Vec<String>,
    pub notification_project_path: Option<String>,
    pub notification_settings_request: u64,
    /// The project notification card flashes while this is true.
    pub highlighted: bool,
    /// The row or card Settings is highlighting, if any.
    pub revealed: Option<gpui::SharedString>,
    /// These values as they change after the slot was built: a repeated
    /// notification request, the highlight ending. Slot views observe it.
    pub live: Option<Entity<LiveSlotContext>>,
}

/// The page's current [`SlotContext`] (with `live` unset), kept up to date
/// for slot views to observe.
#[derive(Clone, Debug, Default)]
pub struct LiveSlotContext(pub SlotContext);

/// Builds a view another crate owns. The page builds a fresh one each time
/// the section opens, the way React remounted it.
pub type ViewSlot = Rc<dyn Fn(&SlotContext, &mut Window, &mut App) -> AnyView>;

/// Every host and slot the page uses.
#[derive(Clone)]
pub struct SettingsHosts {
    pub general: Rc<dyn GeneralHost>,
    pub keybindings: Rc<dyn KeybindingsHost>,
    pub appearance: Rc<dyn AppearanceHost>,
    pub providers: Rc<dyn ProvidersHost>,
    pub inbox: Rc<dyn InboxHost>,
    pub archive: Rc<dyn ArchiveHost>,
    /// `ConnectionsSettings`.
    pub connections: Option<ViewSlot>,
    /// `McpSettings`, from monocode-view-pages.
    pub mcp: Option<ViewSlot>,
    /// `SkillsPage`, from monocode-view-pages. It draws its own header with
    /// [`super::chrome::page_header`] and owns its scrolling.
    pub skills: Option<ViewSlot>,
    /// `WorktreesPage`.
    pub worktrees: Option<ViewSlot>,
    /// `ProjectNotificationSettings`, the first card on the Inbox page.
    pub project_notifications: Option<ViewSlot>,
    /// `ProviderAccountsSettings`, the first card on the Providers page.
    pub accounts: Option<ViewSlot>,
    /// `WindowControls` on Windows and Linux.
    pub window_controls: Option<ViewSlot>,
}

impl Default for SettingsHosts {
    fn default() -> Self {
        let noop = Rc::new(NoopHost);
        Self {
            general: noop.clone(),
            keybindings: noop.clone(),
            appearance: noop.clone(),
            providers: noop.clone(),
            inbox: noop.clone(),
            archive: noop,
            connections: None,
            mcp: None,
            skills: None,
            worktrees: None,
            project_notifications: None,
            accounts: None,
            window_controls: None,
        }
    }
}

/// A summary row the Archive section lists: `SessionSummary`'s fields it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub harness: HarnessId,
    pub updated_at: i64,
    pub archived: bool,
}

/// The props SettingsView took besides its callbacks.
#[derive(Debug, Clone, Default)]
pub struct SettingsProps {
    pub cwd: String,
    /// Recent project paths, newest first.
    pub recents: Vec<String>,
    pub sessions: Vec<SessionSummary>,
    /// The page sits beside the project rail, so no traffic light inset.
    pub beside_rail: bool,
    /// Project to focus when opening notification settings from a quick action.
    pub notification_project_path: Option<String>,
    /// Changes for each quick action, including repeated requests for one project.
    pub notification_settings_request: u64,
    /// The app shell's rail mode, when it controls it.
    pub collapsed_project_rail_mode: Option<CollapsedProjectRailMode>,
}

type Callback<A> = Option<Rc<dyn Fn(A, &mut Window, &mut App)>>;

/// SettingsView's callbacks.
#[derive(Clone, Default)]
pub struct SettingsCallbacks {
    pub on_close: Callback<()>,
    /// Lets search jump to a setting that lives on another page.
    pub on_select_section: Callback<SettingsSectionId>,
    pub on_open_session: Callback<String>,
    pub on_archive_session: Callback<(String, bool)>,
    pub on_delete_session: Callback<String>,
    pub on_restore_project: Callback<String>,
    pub on_delete_project: Callback<String>,
    pub on_open_whats_new: Callback<String>,
    pub on_collapsed_project_rail_mode_change: Callback<CollapsedProjectRailMode>,
}
