//! Engine package `projects`: the project rail and everything a project
//! remembers, plus git status per project.
//!
//! - `Projects`: recents, the rail order, pins, the archive, project groups,
//!   per-project appearance (labels, colors, logos, mascots), chat
//!   backgrounds, the Workspace tab each project shows, and folder identity
//!   for renamed projects (src/features/projects/model, recents.ts,
//!   projectSidebarTab.ts).
//! - `GitStatuses` and `GitStatus`: per-project branch, ahead and behind,
//!   changed files, diff stats, and worktrees, loaded off the main thread
//!   (src/features/source-control/hooks, the changes panel's index poll).
//! - `actions`: the App.tsx flows that switch a session's folder, branch, or
//!   working copy, open and remove projects, delete worktrees, and follow a
//!   renamed folder.
//!
//! Start with `ProjectsGlobal::init` (or `init_native`) after `Engine::init`.
//! The other packages fill in `ProjectsHooks` with
//! `ProjectsGlobal::set_hooks`.

pub mod actions;
pub mod backend;
pub mod chat_background;
pub mod git_status;
pub mod hooks;
pub mod js_object;
pub mod project_chat_background;
pub mod project_data;
pub mod project_groups;
pub mod project_location;
pub mod project_logos;
pub mod project_mascots;
pub mod project_open_run;
pub mod project_sidebar_tab;
pub mod recents;
pub mod worktrees;

#[cfg(test)]
mod project_return_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod testing;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::FutureExt;
use futures::future::Shared;
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, Task};
use monocode_core::appearance::SidebarTabId;
use monocode_core::models::{HarnessAvailability, ModelCatalog, ModelEnv, ModelPrefs};
use monocode_core::paths::path_key;
use monocode_core::project_providers::{PROJECT_PROVIDER_SETTINGS_KEY, ProjectProviders};
use monocode_layout::tab_groups::{
    AppearanceEvent, AppearanceStore, COLLAPSED_KEY, COLOR_KEY, CUSTOM_COLOR_KEY, JsRecord,
    KEY_VERSION_KEY, LABEL_KEY, LOGO_KEY, MASCOT_KEY, TabGroupAppearance,
};
use monocode_settings::{Kv, Subscription};
use monocode_store::session_store::SessionStore;

pub use backend::{
    InUse, LocalProjectsBackend, ProjectLocation, ProjectsBackend, Worktree, WorktreeRemoval,
    Worktrees,
};
pub use git_status::{
    GitStatus, GitStatusMap, GitStatuses, GitWatch, ProjectBranchesState, WatchKind,
    WorktreesSnapshot,
};
pub use hooks::{NoProjectsHooks, ProjectsHooks, SessionFolderTarget};
pub use project_chat_background::{
    ProjectChatBackground, ProjectChatBackgroundSettings, ProjectChatBackgrounds,
};
pub use project_groups::ProjectGroup;
pub use project_location::{ProjectLocationSync, ProjectNotFoundError};
pub use project_open_run::ProjectOpenStep;
pub use recents::{ArchivedProject, ProjectRailSections, RecentProject};

use crate::runtime::engine::Engine;

/// Epoch milliseconds. Tests pass a clock they move with the executor's.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// The system clock.
pub fn system_clock() -> Clock {
    Arc::new(now_ms)
}

/// `AppearanceStore` over `Kv`, for the tab group appearance overrides the
/// rail shows (labels, colors, logos, mascots).
#[derive(Clone)]
pub struct KvAppearanceStore(pub Kv);

impl AppearanceStore for KvAppearanceStore {
    fn get_item(&self, key: &str) -> Option<String> {
        self.0.get_item(key)
    }

    fn set_item(&mut self, key: &str, value: &str) -> bool {
        self.0.set_item(key, value);
        true
    }

    fn known_project_paths(&self) -> Vec<String> {
        recents::known_project_paths(&self.0)
    }
}

/// Owned copies of what the default-model functions read. `env()` borrows
/// them as a `ModelEnv`.
#[derive(Debug, Clone, Default)]
pub struct ModelInputs {
    pub catalog: ModelCatalog,
    pub prefs: ModelPrefs,
    pub availability: HarnessAvailability,
    pub projects: ProjectProviders,
}

impl ModelInputs {
    pub fn env(&self) -> ModelEnv<'_> {
        ModelEnv {
            catalog: &self.catalog,
            prefs: &self.prefs,
            availability: &self.availability,
            projects: &self.projects,
        }
    }
}

/// What changed in `Projects`, for views that need more than `cx.notify()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectsEvent {
    /// `monocode:project-paths-changed`: rail projects, pins, groups, or
    /// their appearance changed.
    PathsChanged,
    /// `monocode:archived-projects-changed`.
    ArchivedChanged,
    /// `monocode:tab-group-labels-changed`.
    LabelsChanged,
    /// `monocode:tab-group-logos-changed`.
    LogosChanged,
    /// `monocode:project-chat-background-changed`.
    ChatBackgroundChanged { image_changed: bool },
}

/// Every stored key this entity mirrors. A change from elsewhere (another
/// package writing the same key) reloads the mirror.
const WATCHED_KEYS: [&str; 15] = [
    recents::KEY,
    recents::RAIL_ORDER_KEY,
    recents::RAIL_PINNED_KEY,
    recents::ARCHIVED_KEY,
    project_groups::GROUPS_KEY,
    project_groups::ASSIGNMENTS_KEY,
    COLOR_KEY,
    CUSTOM_COLOR_KEY,
    LABEL_KEY,
    LOGO_KEY,
    MASCOT_KEY,
    KEY_VERSION_KEY,
    COLLAPSED_KEY,
    project_chat_background::KEY,
    project_sidebar_tab::KEY,
];

/// A shared folder lookup (`synchronizeProjectLocation`).
pub type LocationSync = Shared<Task<Result<Option<ProjectLocationSync>, String>>>;

/// The rail's projects and what each project remembers.
pub struct Projects {
    kv: Kv,
    backend: Arc<dyn ProjectsBackend>,
    recents: Vec<RecentProject>,
    archived: Vec<ArchivedProject>,
    rail_order: Vec<String>,
    pinned: Vec<String>,
    groups: Vec<ProjectGroup>,
    assignments: JsRecord<String>,
    appearance: TabGroupAppearance,
    backgrounds: ProjectChatBackgrounds,
    /// `projectLocationSyncs`: one folder lookup per project at a time.
    location_syncs: HashMap<String, LocationSync>,
    location_task: Option<Task<()>>,
    /// `removingWorktreePaths`: worktrees a deletion is running for.
    removing_worktree_paths: HashSet<String>,
    _kv_changes: Option<(Subscription, Task<()>)>,
}

impl EventEmitter<ProjectsEvent> for Projects {}

impl Projects {
    pub fn new(kv: Kv, backend: Arc<dyn ProjectsBackend>, cx: &mut Context<Self>) -> Self {
        let mut projects = Self {
            kv: kv.clone(),
            backend,
            recents: Vec::new(),
            archived: Vec::new(),
            rail_order: Vec::new(),
            pinned: Vec::new(),
            groups: Vec::new(),
            assignments: JsRecord::new(),
            appearance: TabGroupAppearance::new(),
            backgrounds: ProjectChatBackgrounds::new(),
            location_syncs: HashMap::new(),
            location_task: None,
            removing_worktree_paths: HashSet::new(),
            _kv_changes: None,
        };
        projects.load_mirror();
        // The appearance overrides migrate to path keys on first read.
        let mut store = KvAppearanceStore(kv.clone());
        projects
            .appearance
            .migrate_project_appearance_keys(&mut store);
        projects.appearance.take_events();

        let (changes, changed) = async_channel::unbounded::<()>();
        let subscription = kv.subscribe(move |change| {
            if WATCHED_KEYS.contains(&change.key.as_str()) {
                let _ = changes.try_send(());
            }
        });
        let task = cx.spawn(async move |this, cx| {
            while changed.recv().await.is_ok() {
                while changed.try_recv().is_ok() {}
                let Some(this) = this.upgrade() else {
                    break;
                };
                cx.update(|cx| this.update(cx, |this, cx| this.reload(cx)));
            }
        });
        projects._kv_changes = Some((subscription, task));
        projects.remember_locations(cx);
        projects
    }

    fn load_mirror(&mut self) {
        let kv = &self.kv;
        self.recents = recents::load_recents(kv);
        self.archived = recents::load_archived_projects(kv);
        self.rail_order = recents::load_project_rail_order(kv);
        self.pinned = recents::load_pinned_projects(kv);
        self.groups = project_groups::load_project_groups(kv);
        self.assignments = project_groups::load_project_group_assignments(kv, Some(&self.groups));
    }

    /// Re-read everything from storage, emitting for what changed.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let before = (
            self.recents.clone(),
            self.archived.clone(),
            self.rail_order.clone(),
            self.pinned.clone(),
            self.groups.clone(),
            self.assignments.clone(),
        );
        self.load_mirror();
        let archived_changed = before.1 != self.archived;
        let paths_changed = before.0 != self.recents
            || before.2 != self.rail_order
            || before.3 != self.pinned
            || before.4 != self.groups
            || before.5 != self.assignments;
        if archived_changed {
            cx.emit(ProjectsEvent::ArchivedChanged);
        }
        if paths_changed {
            cx.emit(ProjectsEvent::PathsChanged);
        }
        if before.0 != self.recents {
            self.remember_locations(cx);
        }
        // Appearance and backgrounds are read on demand, so any change to
        // their keys is worth a repaint.
        cx.notify();
    }

    /// The settings store.
    pub fn kv(&self) -> &Kv {
        &self.kv
    }

    pub fn backend(&self) -> &Arc<dyn ProjectsBackend> {
        &self.backend
    }

    fn store(&self) -> KvAppearanceStore {
        KvAppearanceStore(self.kv.clone())
    }

    fn emit_appearance_events(&mut self, cx: &mut Context<Self>) {
        for event in self.appearance.take_events() {
            cx.emit(match event {
                AppearanceEvent::LabelsChanged => ProjectsEvent::LabelsChanged,
                AppearanceEvent::LogosChanged => ProjectsEvent::LogosChanged,
                AppearanceEvent::ProjectPathsChanged => ProjectsEvent::PathsChanged,
            });
        }
        cx.notify();
    }

    fn emit_background_events(&mut self, cx: &mut Context<Self>) {
        for image_changed in self.backgrounds.take_events() {
            cx.emit(ProjectsEvent::ChatBackgroundChanged { image_changed });
        }
        cx.notify();
    }

    fn paths_changed(&mut self, cx: &mut Context<Self>) {
        self.load_mirror();
        cx.emit(ProjectsEvent::PathsChanged);
        cx.notify();
    }

    // Recents and the rail.

    /// `recents`: most recent first.
    pub fn recents(&self) -> &[RecentProject] {
        &self.recents
    }

    /// `loadArchivedProjects`.
    pub fn archived(&self) -> &[ArchivedProject] {
        &self.archived
    }

    /// `loadProjectRailOrder`.
    pub fn rail_order(&self) -> &[String] {
        &self.rail_order
    }

    /// `loadPinnedProjects`.
    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }

    /// `projectRailSections` with the stored order and pins.
    pub fn rail_sections(&self, current_cwd: &str) -> ProjectRailSections {
        recents::project_rail_sections(&self.recents, current_cwd, &self.rail_order, &self.pinned)
    }

    /// `projectRailItems`.
    pub fn rail_items(&self, current_cwd: &str) -> Vec<RecentProject> {
        recents::project_rail_items(&self.kv, &self.recents, current_cwd)
    }

    /// `knownProjectPaths`.
    pub fn known_project_paths(&self) -> Vec<String> {
        recents::known_project_paths(&self.kv)
    }

    /// `lastProjectPath`.
    pub fn last_project_path(&self) -> Option<String> {
        recents::last_project_path(&self.kv)
    }

    /// `setRecents(rememberProject(path))`.
    pub fn remember_project(&mut self, path: &str, cx: &mut Context<Self>) {
        let archived_before = self.archived.len();
        self.recents = recents::remember_project(&self.kv, path);
        self.archived = recents::load_archived_projects(&self.kv);
        if self.archived.len() != archived_before {
            cx.emit(ProjectsEvent::ArchivedChanged);
        }
        self.remember_locations(cx);
        cx.emit(ProjectsEvent::PathsChanged);
        cx.notify();
    }

    /// `forgetProject` (Delete).
    pub fn forget_project(&mut self, path: &str, cx: &mut Context<Self>) {
        recents::forget_project(&self.kv, path);
        self.paths_changed(cx);
        cx.emit(ProjectsEvent::ArchivedChanged);
        self.remember_locations(cx);
    }

    /// `archiveProject` (Archive).
    pub fn archive_project(&mut self, path: &str, cx: &mut Context<Self>) {
        recents::archive_project(&self.kv, path);
        self.paths_changed(cx);
        cx.emit(ProjectsEvent::ArchivedChanged);
        self.remember_locations(cx);
    }

    /// `replaceProjectPath`: a renamed project keeps its rail slot and pin.
    pub fn replace_project_path(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        recents::replace_project_path(&self.kv, from, to);
        self.paths_changed(cx);
        cx.emit(ProjectsEvent::ArchivedChanged);
        self.remember_locations(cx);
    }

    /// `saveProjectRailOrder`.
    pub fn save_rail_order(&mut self, order: &[String], cx: &mut Context<Self>) {
        recents::save_project_rail_order(&self.kv, order);
        self.rail_order = recents::load_project_rail_order(&self.kv);
        cx.notify();
    }

    /// `savePinnedProjects`.
    pub fn save_pinned(&mut self, pinned: &[String], cx: &mut Context<Self>) {
        recents::save_pinned_projects(&self.kv, pinned);
        self.paths_changed(cx);
    }

    /// `toggleProjectPin`.
    pub fn toggle_pin(&mut self, path: &str, cx: &mut Context<Self>) {
        recents::toggle_project_pin(&self.kv, path);
        self.paths_changed(cx);
    }

    // Groups.

    /// `loadProjectGroups`.
    pub fn groups(&self) -> &[ProjectGroup] {
        &self.groups
    }

    /// `loadProjectGroupAssignments`.
    pub fn assignments(&self) -> &JsRecord<String> {
        &self.assignments
    }

    /// `projectGroupIdForPath`.
    pub fn group_id_for_path(&self, path: &str) -> Option<String> {
        project_groups::project_group_id_for_path(path, &self.assignments)
    }

    /// `saveProjectGroups`.
    pub fn save_groups(&mut self, groups: &[ProjectGroup], cx: &mut Context<Self>) -> bool {
        let saved = project_groups::save_project_groups(&self.kv, groups);
        self.paths_changed(cx);
        saved
    }

    /// `createProjectGroup` appended to the stored groups. Returns it.
    pub fn create_group(&mut self, cx: &mut Context<Self>) -> ProjectGroup {
        let group = project_groups::create_project_group(&self.groups);
        let mut next = self.groups.clone();
        next.push(group.clone());
        self.save_groups(&next, cx);
        group
    }

    /// `setProjectGroupAssignment`.
    pub fn set_group_assignment(
        &mut self,
        path: &str,
        group_id: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        project_groups::set_project_group_assignment(&self.kv, path, group_id);
        self.paths_changed(cx);
    }

    /// `updateProjectGroup`.
    pub fn update_group(
        &mut self,
        id: &str,
        update: impl FnOnce(ProjectGroup) -> ProjectGroup,
        cx: &mut Context<Self>,
    ) {
        if project_groups::update_project_group(&self.kv, id, update) {
            self.paths_changed(cx);
        }
    }

    /// `deleteProjectGroup`.
    pub fn delete_group(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let deleted = project_groups::delete_project_group(&self.kv, id);
        self.paths_changed(cx);
        deleted
    }

    // Appearance overrides.

    /// `loadTabGroupLabels`.
    pub fn labels(&mut self) -> JsRecord<String> {
        let mut store = self.store();
        self.appearance.load_tab_group_labels(&mut store)
    }

    /// `loadTabGroupColors`.
    pub fn colors(&mut self) -> JsRecord<usize> {
        let mut store = self.store();
        self.appearance.load_tab_group_colors(&mut store)
    }

    /// `loadTabGroupCustomColors`.
    pub fn custom_colors(&mut self) -> JsRecord<String> {
        let mut store = self.store();
        self.appearance.load_tab_group_custom_colors(&mut store)
    }

    /// `loadTabGroupLogos`.
    pub fn logos(&mut self) -> JsRecord<String> {
        let mut store = self.store();
        self.appearance.load_tab_group_logos(&mut store)
    }

    /// `loadTabGroupMascots`.
    pub fn mascots(&mut self) -> JsRecord<String> {
        let mut store = self.store();
        self.appearance.load_tab_group_mascots(&mut store)
    }

    /// `tabGroupLogoDisplayRevision`: append it to a logo path as a cache key
    /// (`projectLogoSrc`).
    pub fn logo_display_revision(&self) -> u64 {
        self.appearance.tab_group_logo_display_revision()
    }

    /// `saveTabGroupLabel`.
    pub fn save_label(&mut self, project: &str, label: &str, cx: &mut Context<Self>) {
        let mut store = self.store();
        self.appearance
            .save_tab_group_label(&mut store, project, label);
        self.emit_appearance_events(cx);
    }

    /// `saveTabGroupColor`.
    pub fn save_color(&mut self, project: &str, index: Option<usize>, cx: &mut Context<Self>) {
        let mut store = self.store();
        self.appearance
            .save_tab_group_color(&mut store, project, index);
        self.emit_appearance_events(cx);
    }

    /// `saveTabGroupCustomColor`.
    pub fn save_custom_color(
        &mut self,
        project: &str,
        color: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let mut store = self.store();
        self.appearance
            .save_tab_group_custom_color(&mut store, project, color);
        self.emit_appearance_events(cx);
    }

    /// `saveTabGroupMascot`.
    pub fn save_mascot(&mut self, project: &str, name: Option<&str>, cx: &mut Context<Self>) {
        let mut store = self.store();
        self.appearance
            .save_tab_group_mascot(&mut store, project, name);
        self.emit_appearance_events(cx);
    }

    // The Workspace tab per project.

    /// `loadProjectSidebarTab`.
    pub fn sidebar_tab(&self, project: &str) -> SidebarTabId {
        project_sidebar_tab::load_project_sidebar_tab(&self.kv, project)
    }

    /// `saveProjectSidebarTab`.
    pub fn save_sidebar_tab(&mut self, project: &str, tab: SidebarTabId, cx: &mut Context<Self>) {
        project_sidebar_tab::save_project_sidebar_tab(&self.kv, project, tab);
        cx.notify();
    }

    // Chat backgrounds.

    /// `loadProjectChatBackgroundSettings`.
    pub fn chat_background_settings(&self, project: &str) -> Option<ProjectChatBackgroundSettings> {
        project_chat_background::load_project_chat_background_settings(&self.kv, project)
    }

    /// `loadProjectChatBackground`.
    pub fn chat_background(&self, project: &str) -> Option<ProjectChatBackground> {
        project_chat_background::load_project_chat_background(&self.kv, project)
    }

    /// `saveProjectChatBackgroundSettings`.
    pub fn save_chat_background_settings(
        &mut self,
        project: &str,
        value: &ProjectChatBackgroundSettings,
        image_changed: bool,
        cx: &mut Context<Self>,
    ) {
        self.backgrounds
            .save_settings(&self.kv, project, value, image_changed);
        self.emit_background_events(cx);
    }

    /// `saveProjectChatBackground`.
    pub fn save_chat_background(
        &mut self,
        project: &str,
        value: &ProjectChatBackground,
        cx: &mut Context<Self>,
    ) {
        self.backgrounds.save(&self.kv, project, value);
        self.emit_background_events(cx);
    }

    /// `clearProjectChatBackgroundSetting`.
    pub fn clear_chat_background_setting(&mut self, project: &str, cx: &mut Context<Self>) {
        self.backgrounds.clear(&self.kv, project);
        self.emit_background_events(cx);
    }

    /// `notifyProjectChatBackgroundChanged`.
    pub fn notify_chat_background_changed(&mut self, image_changed: bool, cx: &mut Context<Self>) {
        self.backgrounds.notify_changed(image_changed);
        self.emit_background_events(cx);
    }

    /// `projectChatBackgroundRevision`.
    pub fn chat_background_revision(&self) -> i64 {
        self.backgrounds.revision()
    }

    /// `projectChatBackgroundImageRevision`.
    pub fn chat_background_image_revision(&self) -> i64 {
        self.backgrounds.image_revision()
    }

    // Folder identity.

    /// The recents effect: remember every recent project's folder identity
    /// (`rememberProjectLocation` for each, errors ignored).
    fn remember_locations(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<String> = self.recents.iter().map(|item| item.path.clone()).collect();
        let kv = self.kv.clone();
        let backend = self.backend.clone();
        let executor = cx.background_executor().clone();
        self.location_task = Some(cx.background_spawn(async move {
            for path in paths {
                let _ =
                    project_location::remember_project_location(&kv, &backend, &path, &executor)
                        .await;
            }
        }));
    }

    /// `synchronizeProjectLocation`, shared while one lookup per project is
    /// running (`projectLocationSyncs`).
    pub fn synchronize_location(&mut self, cwd: &str, cx: &mut Context<Self>) -> LocationSync {
        let key = path_key(cwd);
        if let Some(sync) = self.location_syncs.get(&key) {
            return sync.clone();
        }
        let lookup = project_location::synchronize_project_location(
            &self.kv,
            &self.backend,
            cwd,
            cx.background_executor(),
        );
        let sync = lookup.shared();
        self.location_syncs.insert(key.clone(), sync.clone());
        let done = sync.clone();
        cx.spawn(async move |this, cx| {
            let _ = done.await;
            this.update(cx, |this, _| this.location_syncs.remove(&key))
                .ok();
        })
        .detach();
        sync
    }

    /// `forgetProjectLocation`.
    pub fn forget_location(&mut self, path: &str) {
        project_location::forget_project_location(&self.kv, path);
    }
}

/// What `ProjectsGlobal::init` needs.
pub struct ProjectsConfig {
    pub kv: Kv,
    pub backend: Arc<dyn ProjectsBackend>,
    pub clock: Clock,
}

/// The projects entities, as a GPUI global.
pub struct ProjectsGlobal {
    pub projects: Entity<Projects>,
    pub git: Entity<GitStatuses>,
    pub kv: Kv,
    pub backend: Arc<dyn ProjectsBackend>,
    pub clock: Clock,
    hooks: Rc<dyn ProjectsHooks>,
}

impl Global for ProjectsGlobal {}

impl ProjectsGlobal {
    /// Create the entities and install the global.
    pub fn init(config: ProjectsConfig, cx: &mut App) {
        let projects = cx.new(|cx| Projects::new(config.kv.clone(), config.backend.clone(), cx));
        let git = cx.new(|_| GitStatuses::new(config.backend.clone(), config.clock.clone()));
        cx.set_global(ProjectsGlobal {
            projects,
            git,
            kv: config.kv,
            backend: config.backend,
            clock: config.clock,
            hooks: Rc::new(NoProjectsHooks),
        });
    }

    /// `init` with git and the app data directory on this machine. `store`
    /// holds the session rows worktrees list; `terminals_in_use` and
    /// `in_use` guard worktree removal.
    pub fn init_native(
        kv: Kv,
        data_dir: PathBuf,
        store: Arc<SessionStore>,
        terminals_in_use: InUse,
        in_use: InUse,
        cx: &mut App,
    ) {
        let backend = Arc::new(LocalProjectsBackend::new(
            data_dir,
            store,
            terminals_in_use,
            in_use,
        ));
        Self::init(
            ProjectsConfig {
                kv,
                backend,
                clock: system_clock(),
            },
            cx,
        );
    }

    pub fn global(cx: &App) -> &ProjectsGlobal {
        cx.global::<ProjectsGlobal>()
    }

    pub fn try_global(cx: &App) -> Option<&ProjectsGlobal> {
        cx.try_global::<ProjectsGlobal>()
    }

    /// The `Projects` entity.
    pub fn projects(cx: &App) -> Entity<Projects> {
        Self::global(cx).projects.clone()
    }

    /// The `GitStatuses` registry.
    pub fn git(cx: &App) -> Entity<GitStatuses> {
        Self::global(cx).git.clone()
    }

    /// The `GitStatus` entity for a project folder.
    pub fn git_status(cwd: &str, cx: &mut App) -> Entity<GitStatus> {
        Self::git(cx).update(cx, |git, cx| git.status(cwd, cx))
    }

    /// A copy of the hooks. Clone it out before calling a hook so the global
    /// is not borrowed during the call.
    pub fn hooks(cx: &App) -> Rc<dyn ProjectsHooks> {
        Self::try_global(cx)
            .map(|global| global.hooks.clone())
            .unwrap_or_else(|| Rc::new(NoProjectsHooks))
    }

    /// Fill in the calls into the workspace and the other packages.
    pub fn set_hooks(cx: &mut App, hooks: Rc<dyn ProjectsHooks>) {
        cx.global_mut::<ProjectsGlobal>().hooks = hooks;
    }

    /// The model catalog, preferences, availability, and project defaults
    /// that new sessions use.
    pub fn model_inputs(cx: &App) -> ModelInputs {
        let hooks = Self::hooks(cx);
        let kv = Self::try_global(cx).map(|global| global.kv.clone());
        let get = |key: &str| kv.as_ref().and_then(|kv| kv.get_item(key));
        ModelInputs {
            catalog: hooks.model_catalog(cx),
            prefs: ModelPrefs::from_local_storage(get),
            availability: hooks.harness_availability(cx),
            projects: ProjectProviders::parse(get(PROJECT_PROVIDER_SETTINGS_KEY).as_deref()),
        }
    }

    /// `Date.now()` from the configured clock.
    pub fn now(cx: &App) -> i64 {
        Self::try_global(cx)
            .map(|global| (global.clock)())
            .unwrap_or_else(now_ms)
    }
}

/// `notifyGitChanged`: tell the workspace (file watchers and git views) and
/// reload every project's git status.
pub fn notify_git_changed(cx: &mut App) {
    Engine::hooks(cx).workspace.notify_git_changed(cx);
    if let Some(global) = ProjectsGlobal::try_global(cx) {
        let git = global.git.clone();
        git.update(cx, |git, cx| git.git_changed(cx));
    }
}

/// `notifyReviewChanged(sessionId)`.
fn notify_review_changed(session_id: &str, cx: &mut App) {
    if Engine::try_global(cx).is_some() {
        crate::runtime::checkpoint::notify_review_changed(Some(session_id), cx);
    }
}
