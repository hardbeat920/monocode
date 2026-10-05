//! [`Scm`]: the handle every source control view takes. It carries the git
//! backend, the engine's git status registry, the app hooks, and the state
//! the React views kept in module variables (which sections are open, the
//! list or tree choice, cached PR status and history).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Task};
use monocode_core::appearance::ChangesView;
use monocode_engine::projects::{GitStatus, GitStatuses, ProjectsBackend};

use crate::git::{GitBackend, GitHistoryCommit, GitPr, LocalGit};
use crate::hooks::ScmHooks;
use crate::ui::history_graph::GRAPH_PANEL_DEFAULT;

/// Emitted when a git mutation made here, or one the app reports, may have
/// changed what the views show (`subscribeGitChanged`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScmEvent {
    GitChanged,
}

/// Module-level state from GitChangesPanel.tsx and GitHistoryGraph.tsx,
/// shared by every panel in the app.
pub struct ScmState {
    pub staged_open: bool,
    pub changes_open: bool,
    pub graph_open: bool,
    pub changes_view: ChangesView,
    /// Folders the user collapsed in tree view, keyed `<kind>:<dir>`.
    pub collapsed_dirs: HashSet<String>,
    /// `graphPanelHeight`, in CSS px.
    pub graph_height: f32,
    /// `prByCwd`.
    pub pr_by_cwd: HashMap<String, Option<GitPr>>,
    /// `historyByCwd`.
    pub history_by_cwd: HashMap<String, Vec<GitHistoryCommit>>,
}

impl EventEmitter<ScmEvent> for ScmState {}

impl ScmState {
    fn new(changes_view: ChangesView) -> Self {
        Self {
            staged_open: true,
            changes_open: true,
            graph_open: true,
            changes_view,
            collapsed_dirs: HashSet::new(),
            graph_height: GRAPH_PANEL_DEFAULT,
            pr_by_cwd: HashMap::new(),
            history_by_cwd: HashMap::new(),
        }
    }

    /// `notifyGitChanged` reaching the views.
    pub fn git_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(ScmEvent::GitChanged);
    }
}

#[derive(Clone)]
pub struct Scm {
    pub git: Arc<dyn GitBackend>,
    pub statuses: Entity<GitStatuses>,
    pub hooks: Rc<ScmHooks>,
    pub state: Entity<ScmState>,
}

impl Scm {
    /// `statuses` is the engine's registry (`ProjectsGlobal::git` in the app).
    /// `changes_view` is the saved `monocode.changesView`.
    pub fn new(
        git: Arc<dyn GitBackend>,
        statuses: Entity<GitStatuses>,
        hooks: ScmHooks,
        changes_view: ChangesView,
        cx: &mut App,
    ) -> Self {
        let state = cx.new(|_| ScmState::new(changes_view));
        Self {
            git,
            statuses,
            hooks: Rc::new(hooks),
            state,
        }
    }

    /// Everything over [`LocalGit`], with a git status registry of its own.
    /// For a window without the engine, such as the gallery.
    pub fn local(hooks: ScmHooks, cx: &mut App) -> Self {
        let git = Arc::new(LocalGit::new());
        let backend: Arc<dyn ProjectsBackend> = git.clone();
        let statuses = cx.new(|_| GitStatuses::new(backend, Arc::new(now_ms)));
        Self::new(git, statuses, hooks, ChangesView::List, cx)
    }

    /// The engine's status entity for `cwd`, created on first use.
    pub fn status(&self, cwd: &str, cx: &mut App) -> Entity<GitStatus> {
        self.statuses
            .update(cx, |statuses, cx| statuses.status(cwd, cx))
    }

    /// Run a blocking git call on the background executor.
    pub fn run<T: Send + 'static>(
        &self,
        cx: &App,
        call: impl FnOnce(&dyn GitBackend) -> T + Send + 'static,
    ) -> Task<T> {
        let git = self.git.clone();
        cx.background_spawn(async move { call(git.as_ref()) })
    }

    /// `notifyGitChanged` after a mutation made here: reload the views, the
    /// shared git status, and tell the rest of the app.
    pub fn notify_git_changed(&self, cx: &mut App) {
        self.state.update(cx, |state, cx| state.git_changed(cx));
        self.statuses
            .update(cx, |statuses, cx| statuses.git_changed(cx));
        if let Some(hook) = self.hooks.git_changed.clone() {
            hook(cx);
        }
    }

    /// A git change reported by the rest of the app. Reloads the views that
    /// read git outside the shared status (history, diffs).
    pub fn git_changed(&self, cx: &mut App) {
        self.state.update(cx, |state, cx| state.git_changed(cx));
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}
