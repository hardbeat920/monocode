//! What the quick composer and the git popup need from the app. The app
//! implements these with the engine: `QuickLaunch::submit` for launches,
//! the projects package for git, and the submit package's attachment IO.
//! The Tauri app reached the same things through `invoke` and events.

use gpui::{App, Task, Window};
use monocode_core::block::ModelSettings;
use monocode_core::models::{LastModelChoice, ModelCatalog, ModelPrefs};
use monocode_core::{Attachment, HarnessId};
use monocode_view_composer::composer::model::clipboard::ClipboardFile;

use crate::model::appearance::ProjectAppearance;
use crate::model::launch::{GitBranches, QuickLaunchRequest, Worktree};

/// The panel's data. The app reads it on every show, because projects and
/// defaults can change in a workspace between shows.
#[derive(Clone, Debug)]
pub struct QuickSnapshot {
    /// `loadQuickProjects`: recents, then pinned, then the rail, without
    /// archived projects.
    pub projects: Vec<String>,
    /// `initialQuickProject`: the last quick project while it is offered,
    /// else the first.
    pub initial_project: Option<String>,
    /// `loadQuickProjectAppearance`.
    pub appearance: ProjectAppearance,
    /// `initialQuickChoice`: the Providers default.
    pub choice: LastModelChoice,
    /// `loadLastModelSettings`.
    pub model_settings: ModelSettings,
    /// Bundled and live model lists.
    pub catalog: ModelCatalog,
    /// Favorites, hidden providers, and the last settings.
    pub prefs: ModelPrefs,
    /// `availableHarnesses`: `None` until a workspace reported which CLIs
    /// are installed.
    pub available: Option<Vec<HarnessId>>,
}

impl QuickSnapshot {
    /// A snapshot with no projects and the given default model.
    pub fn new(choice: LastModelChoice) -> Self {
        Self {
            projects: Vec::new(),
            initial_project: None,
            appearance: ProjectAppearance::default(),
            choice,
            model_settings: ModelSettings::new(),
            catalog: ModelCatalog::new(),
            prefs: ModelPrefs::default(),
            available: None,
        }
    }
}

/// `nativeClipboardAttachments`: what the native clipboard held that a
/// GPUI clipboard read cannot see (a screenshot, or a file a file manager
/// copied).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NativeClipboard {
    pub files: Vec<Attachment>,
    pub warning: Option<String>,
}

/// A blocking result the app computes off the UI thread.
pub type HostTask<T> = Task<Result<T, String>>;

/// Git for the working copy controls and the git popup.
pub trait QuickGitHost {
    /// `useProjectBranchesState`: the branches of `cwd`, or `None` outside
    /// a repository. The task settles the state.
    fn branches(&self, cwd: &str, cx: &mut App) -> Task<Option<GitBranches>>;

    /// `useProjectWorktrees`.
    fn worktrees(&self, cwd: &str, cx: &mut App) -> HostTask<Vec<Worktree>>;

    /// `gitCheckout`.
    fn checkout(
        &self,
        cwd: &str,
        name: &str,
        remote: Option<&str>,
        force: bool,
        cx: &mut App,
    ) -> HostTask<()>;

    /// `gitCreateBranch`.
    fn create_branch(&self, cwd: &str, name: &str, force: bool, cx: &mut App) -> HostTask<()>;

    /// `gitStash`.
    fn stash(&self, cwd: &str, message: &str, cx: &mut App) -> HostTask<()>;

    /// `gitStageAll`, then `gitCommit`.
    fn commit_all(&self, cwd: &str, message: &str, cx: &mut App) -> HostTask<()>;

    /// `notifyGitChanged`: a branch or working copy changed.
    fn git_changed(&self, _cx: &mut App) {}
}

/// Everything else the quick composer asks for.
pub trait QuickComposerHost: QuickGitHost {
    /// The data for a show.
    fn snapshot(&self, cx: &mut App) -> QuickSnapshot;

    /// `QUICK_COMPOSER_CATALOG_REQUEST_EVENT`: refresh a provider's live
    /// model list. Call [`crate::QuickComposer::set_catalog`] when it lands.
    fn request_catalog(&self, _harness: HarnessId, _cx: &mut App) {}

    /// `quick_composer_submit`: queue the launch for a workspace window and
    /// hide the panel. An error stays in the panel, with the draft.
    fn submit(&self, request: QuickLaunchRequest, cx: &mut App) -> HostTask<()>;

    /// After a launch was queued: `rememberQuickProject`,
    /// `saveLastModelSettings`, and `saveRecentModelChoice`.
    fn remember(&self, _request: &QuickLaunchRequest, _cx: &mut App) {}

    /// `saveFavoriteModels`.
    fn save_favorites(&self, _favorites: &[String], _cx: &mut App) {}

    /// `pickAttachments`: the file dialog.
    fn pick_attachments(&self, window: &mut Window, cx: &mut App) -> HostTask<Vec<Attachment>>;

    /// `attachmentsFromPaths`: dropped or captured files.
    fn attachments_from_paths(&self, paths: Vec<String>, cx: &mut App)
    -> HostTask<Vec<Attachment>>;

    /// `attachmentsFromFiles`: pasted images.
    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        cx: &mut App,
    ) -> HostTask<Vec<Attachment>>;

    /// `nativeClipboardAttachments`. `text` is what the paste carried.
    fn native_clipboard(&self, text: &str, cx: &mut App) -> HostTask<NativeClipboard>;

    /// `storeQuickAttachments`: write pasted bytes to files, so every
    /// attachment has a path that survives the handoff to a window.
    fn store_attachments(&self, files: Vec<Attachment>, cx: &mut App) -> HostTask<Vec<Attachment>>;

    /// `revokeAttachment`.
    fn revoke_attachment(&self, _file: &Attachment, _cx: &mut App) {}

    /// `quick_composer_capture`: hide the panel, run the interactive
    /// capture, and bring the panel back. `None` means the user cancelled.
    fn capture_screenshot(&self, window: &mut Window, cx: &mut App) -> HostTask<Option<String>>;

    /// `quick_composer_release_capture`: delete captures the draft dropped.
    fn release_captures(&self, _paths: Vec<String>, _cx: &mut App) {}
}
