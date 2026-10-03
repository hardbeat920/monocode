//! Ports of src/features/source-control/ui/WorkingTreeDiff.tsx,
//! CommitDiff.tsx, and SessionChangesDiff.tsx: each lists files, loads their
//! two sides four at a time, and shows them in monocode-editor's
//! [`DiffView`] (the React `UnifiedDiffView`).

use std::collections::HashMap;
use std::sync::Arc;

use futures::StreamExt as _;
use futures::stream::FuturesUnordered;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Task, Window, div,
};
use monocode_editor::git_diff::stage_chunk_text;
use monocode_editor::unified_diff::{
    DiffCommentTarget, PatchStatus, UNIFIED_CONTEXT_DEFAULT, UnifiedFileDiff, build_unified_file,
    file_hunks,
};
use monocode_editor::{
    DiffAction, DiffFile, DiffFileActions, DiffView, HunkActionRequest, InitialExpansion,
};
use monocode_engine::runtime::checkpoint::{Checkpoints, ReviewChanged, ReviewChanges};
use monocode_store::checkpoint::CheckpointFile;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::git::{GitChangedFile, GitFileDiff, GitFileDiffKind};
use crate::model::stable_diff::{HasId, ShallowEq, reuse_unchanged_by_id};
use crate::model::working_tree_diff::{
    WorkingTreeDiffEntry, prioritize_working_tree_diff_entries, working_tree_diff_entries,
    working_tree_diff_entry_label, working_tree_diff_focus_id,
};
use crate::scm::{Scm, ScmEvent};
use crate::ui::common::{centered, editor_theme, spin_icon};
use crate::ui::diff_comment_composer::{DiffCommentComposer, DiffCommentEvent};

const DIFF_LOAD_CONCURRENCY: usize = 4;
/// Loaded diffs reach the view at most this often, because each update
/// rebuilds the view's rows and restarts highlighting.
const PUBLISH_BATCH: std::time::Duration = std::time::Duration::from_millis(150);

/// `LoadedDiff`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedDiff {
    pub binary: bool,
    pub too_large: bool,
    pub original: String,
    pub current: String,
    pub unified: Option<UnifiedFileDiff>,
    pub error: Option<String>,
}

impl LoadedDiff {
    fn from_sides(binary: bool, too_large: bool, original: String, current: String) -> Self {
        let unified = (!binary && !too_large)
            .then(|| build_unified_file(&original, &current, UNIFIED_CONTEXT_DEFAULT));
        Self {
            binary,
            too_large,
            original,
            current,
            unified,
            error: None,
        }
    }

    fn from_git(diff: GitFileDiff) -> Self {
        Self::from_sides(diff.binary, diff.too_large, diff.original, diff.current)
    }

    fn failed(error: String) -> Self {
        Self {
            binary: false,
            too_large: false,
            original: String::new(),
            current: String::new(),
            unified: None,
            error: Some(error),
        }
    }
}

/// `UnifiedDiffFileModel`, kept shared between refreshes so the view only
/// rebuilds when a file changed.
#[derive(Clone, Debug)]
struct FileModel(DiffFile);

impl HasId for FileModel {
    fn id(&self) -> &str {
        &self.0.id
    }
}

impl ShallowEq for FileModel {
    fn shallow_eq(&self, other: &Self) -> bool {
        let (a, b) = (&self.0, &other.0);
        a.id == b.id
            && a.path == b.path
            && a.label == b.label
            && a.status == b.status
            && a.binary == b.binary
            && a.too_large == b.too_large
            && a.empty_message == b.empty_message
            && a.actions == b.actions
            && a.diff == b.diff
    }
}

fn patch_status(status: &str) -> PatchStatus {
    match status {
        "added" | "untracked" => PatchStatus::Added,
        "deleted" => PatchStatus::Deleted,
        "renamed" => PatchStatus::Renamed,
        _ => PatchStatus::Modified,
    }
}

struct ModelInput<'a> {
    id: &'a str,
    path: &'a str,
    label: String,
    status: &'a str,
    loaded: Option<&'a LoadedDiff>,
    /// The index counts to show while a file loads.
    fallback: (i64, i64),
    unchanged_message: &'a str,
    /// Error text, from the error message.
    error_message: fn(&str) -> String,
    actions: DiffFileActions,
}

fn file_model(input: ModelInput) -> FileModel {
    let unified = input.loaded.and_then(|loaded| loaded.unified.clone());
    let unchanged = unified
        .as_ref()
        .is_some_and(|u| u.additions == 0 && u.deletions == 0)
        && !input.loaded.is_some_and(|loaded| loaded.binary);
    let empty_message: Option<SharedString> = match input.loaded {
        None => Some("Loading…".into()),
        Some(loaded) => match &loaded.error {
            Some(error) => Some((input.error_message)(error).into()),
            None if unchanged => Some(input.unchanged_message.to_string().into()),
            None => None,
        },
    };
    let diff = match unified {
        Some(_) if unchanged => UnifiedFileDiff {
            additions: 0,
            deletions: 0,
            ..Default::default()
        },
        Some(unified) => unified,
        None => UnifiedFileDiff {
            additions: input.fallback.0.max(0) as usize,
            deletions: input.fallback.1.max(0) as usize,
            ..Default::default()
        },
    };
    let hunks = file_hunks(&diff);
    FileModel(DiffFile {
        id: input.id.to_string().into(),
        path: input.path.to_string().into(),
        label: input.label.into(),
        previous_path: None,
        status: patch_status(input.status),
        binary: input.loaded.is_some_and(|loaded| loaded.binary),
        too_large: input.loaded.is_some_and(|loaded| loaded.too_large),
        empty_message,
        diff,
        hunks,
        actions: input.actions,
    })
}

/// Shared state of the three wrappers: the files, what loaded, the view.
struct DiffState {
    view: Entity<DiffView>,
    models: Arc<Vec<Arc<FileModel>>>,
    focus: Option<String>,
    focused: bool,
    comment: Option<Entity<DiffCommentComposer>>,
    publish_task: Option<Task<()>>,
}

impl DiffState {
    fn new(cx: &mut App) -> Self {
        let theme = editor_theme(cx);
        Self {
            view: cx.new(|cx| DiffView::new(Vec::new(), theme, cx)),
            models: Arc::new(Vec::new()),
            focus: None,
            focused: false,
            comment: None,
            publish_task: None,
        }
    }

    /// Push models to the view when any of them changed.
    fn publish(&mut self, models: Vec<FileModel>, cx: &mut App) {
        let next: Vec<Arc<FileModel>> = models.into_iter().map(Arc::new).collect();
        let merged = reuse_unchanged_by_id(&self.models, next);
        if Arc::ptr_eq(&merged, &self.models) {
            return;
        }
        self.models = merged;
        let files: Vec<DiffFile> = self.models.iter().map(|model| model.0.clone()).collect();
        let focus = self.focus.clone();
        let scroll = !self.focused
            && focus
                .as_deref()
                .is_some_and(|id| files.iter().any(|file| file.id.as_ref() == id));
        if scroll {
            self.focused = true;
        }
        self.view.update(cx, |view, cx| {
            view.set_files(files, InitialExpansion::All, cx);
            if scroll && let Some(id) = &focus {
                view.scroll_to_file(id, cx);
            }
        });
    }

    fn totals(&self) -> (usize, usize) {
        self.models.iter().fold((0, 0), |(a, d), model| {
            (a + model.0.diff.additions, d + model.0.diff.deletions)
        })
    }
}

/// Run `load` over `items`, `DIFF_LOAD_CONCURRENCY` at a time
/// (`forEachConcurrent`), and hand each result to `done` on the entity.
fn load_concurrently<V: 'static, T: Clone + 'static, R: 'static>(
    items: Vec<T>,
    load: impl Fn(&T, &mut App) -> Task<R> + 'static,
    done: impl Fn(&mut V, T, R, &mut Context<V>) + 'static,
    cx: &mut Context<V>,
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        let mut queue = items.into_iter();
        let mut running = FuturesUnordered::new();
        let start = |item: T, cx: &mut gpui::AsyncApp| {
            let task = cx.update(|cx| load(&item, cx));
            async move { (item, task.await) }
        };
        for item in queue.by_ref().take(DIFF_LOAD_CONCURRENCY) {
            running.push(start(item, cx));
        }
        while let Some((item, result)) = running.next().await {
            if this
                .update(cx, |this, cx| done(this, item, result, cx))
                .is_err()
            {
                return;
            }
            if let Some(next) = queue.next() {
                running.push(start(next, cx));
            }
        }
    })
}

/// Publish once `PUBLISH_BATCH` has passed, folding the arrivals in between.
fn schedule_publish<V: 'static>(
    state_of: fn(&mut V) -> &mut DiffState,
    publish: fn(&mut V, &mut Context<V>),
    this: &mut V,
    cx: &mut Context<V>,
) {
    if state_of(this).publish_task.is_some() {
        return;
    }
    state_of(this).publish_task = Some(cx.spawn(async move |this, cx| {
        cx.background_executor().timer(PUBLISH_BATCH).await;
        let _ = this.update(cx, |this, cx| {
            state_of(this).publish_task = None;
            publish(this, cx);
        });
    }));
}

fn render_state(
    cwd: &str,
    error: Option<&str>,
    error_title: &'static str,
    loading: bool,
    view: Entity<DiffView>,
    comment: Option<Entity<DiffCommentComposer>>,
    cx: &App,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    if cwd.is_empty() || cwd == "~" {
        return centered(
            div()
                .text_px(13.)
                .text_color(theme.content(0.45))
                .child("No project folder"),
        )
        .into_any_element();
    }
    if let Some(error) = error {
        return centered(
            div()
                .flex()
                .flex_col()
                .items_center()
                .p(u(24.))
                .child(
                    div().mb(u(12.)).child(
                        icon(IconName::AlertCircle)
                            .size(u(20.))
                            .text_color(theme.colors.danger),
                    ),
                )
                .child(
                    div()
                        .text_px(13.)
                        .text_color(theme.colors.content)
                        .child(error_title),
                )
                .child(
                    div()
                        .mt(u(4.))
                        .text_px(12.)
                        .text_color(theme.content(0.50))
                        .child(error.to_string()),
                ),
        )
        .into_any_element();
    }
    if loading {
        return centered(spin_icon("diff-loading", 16., theme.content(0.40))).into_any_element();
    }
    let mut root = div().size_full().child(view);
    if let Some(comment) = comment {
        root = root.child(comment);
    }
    root.into_any_element()
}

/// Open the comment composer for a line, at the pointer. The subscription
/// clears it from the owner's state when it closes.
fn open_comment<V: 'static>(
    scm: &Scm,
    state_of: fn(&mut V) -> &mut DiffState,
    target: DiffCommentTarget,
    window: &mut Window,
    cx: &mut Context<V>,
) -> (Entity<DiffCommentComposer>, Subscription) {
    let position = window.mouse_position();
    let composer = cx.new(|cx| DiffCommentComposer::new(scm.clone(), target, position, window, cx));
    let subscription = cx.subscribe(
        &composer,
        move |this: &mut V, _, event: &DiffCommentEvent, cx| match event {
            DiffCommentEvent::Dismiss => {
                state_of(this).comment = None;
                cx.notify();
            }
        },
    );
    (composer, subscription)
}

// WorkingTreeDiff.

/// Staged and unstaged changes in the working tree, with stage, discard,
/// and hunk staging.
pub struct WorkingTreeDiff {
    scm: Scm,
    cwd: String,
    focus_path: Option<String>,
    focus_kind: Option<GitFileDiffKind>,
    files: Option<Vec<GitChangedFile>>,
    entries: Vec<WorkingTreeDiffEntry>,
    diffs: HashMap<String, LoadedDiff>,
    error: Option<String>,
    busy_id: Option<String>,
    state: DiffState,
    generation: u64,
    load: Option<Task<()>>,
    action: Option<Task<()>>,
    refresh_scheduled: bool,
    _subscriptions: Vec<Subscription>,
}

impl WorkingTreeDiff {
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        focus_path: Option<String>,
        focus_kind: Option<GitFileDiffKind>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = DiffState::new(cx);
        let this = cx.entity().downgrade();
        state.view.update(cx, |view, _| {
            let hunk_target = this.clone();
            view.on_hunk_action(Some(std::rc::Rc::new(
                move |request: HunkActionRequest, _, cx: &mut App| {
                    let _ = hunk_target.update(cx, |this, cx| this.stage_hunk(request, cx));
                },
            )));
            let file_target = this.clone();
            view.on_file_action(Some(std::rc::Rc::new(
                move |action, id: SharedString, _, cx: &mut App| {
                    let _ = file_target.update(cx, |this, cx| this.file_action(action, &id, cx));
                },
            )));
            let comment_target = this.clone();
            view.on_comment(Some(std::rc::Rc::new(
                move |target, window, cx: &mut App| {
                    let _ = comment_target.update(cx, |this, cx| this.comment(target, window, cx));
                },
            )));
        });
        let subscriptions = vec![
            cx.subscribe(&scm.state, |this, _, event: &ScmEvent, cx| match event {
                ScmEvent::GitChanged => this.schedule_run(cx),
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.schedule_run(cx);
                }
            }),
        ];
        let mut this = Self {
            scm,
            cwd: cwd.into(),
            focus_path,
            focus_kind,
            files: None,
            entries: Vec::new(),
            diffs: HashMap::new(),
            error: None,
            busy_id: None,
            state,
            generation: 0,
            load: None,
            action: None,
            refresh_scheduled: false,
            _subscriptions: subscriptions,
        };
        if this.cwd.is_empty() || this.cwd == "~" {
            this.files = Some(Vec::new());
        } else {
            this.run(cx);
        }
        this
    }

    pub fn view(&self) -> &Entity<DiffView> {
        &self.state.view
    }

    pub fn files(&self) -> Option<&[GitChangedFile]> {
        self.files.as_deref()
    }

    pub fn loaded(&self, id: &str) -> Option<&LoadedDiff> {
        self.diffs.get(id)
    }

    pub fn totals(&self) -> (usize, usize) {
        self.state.totals()
    }

    fn schedule_run(&mut self, cx: &mut Context<Self>) {
        if self.refresh_scheduled || self.cwd.is_empty() || self.cwd == "~" {
            return;
        }
        self.refresh_scheduled = true;
        let this = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = this.update(cx, |this, cx| {
                this.refresh_scheduled = false;
                this.run(cx);
            });
        });
    }

    /// `run`: list the files, then load their diffs.
    pub fn run(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let cwd = self.cwd.clone();
        let list = self.scm.run(cx, move |git| git.git_diff_files(&cwd));
        self.load = Some(cx.spawn(async move |this, cx| {
            let result = list.await;
            let next = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return None;
                }
                match result {
                    Ok(index) => {
                        this.entries = working_tree_diff_entries(&index.files);
                        this.files = Some(index.files);
                        this.diffs.clear();
                        this.error = None;
                        this.publish(cx);
                        Some(prioritize_working_tree_diff_entries(
                            &this.entries,
                            this.focus_path.as_deref(),
                            this.focus_kind,
                        ))
                    }
                    Err(error) => {
                        this.error = Some(error);
                        this.files = Some(Vec::new());
                        cx.notify();
                        None
                    }
                }
            });
            let Ok(Some(order)) = next else {
                return;
            };
            let Ok(task) = this.update(cx, |this, cx| {
                let scm = this.scm.clone();
                let cwd = this.cwd.clone();
                load_concurrently(
                    order,
                    move |entry: &WorkingTreeDiffEntry, cx| {
                        let (cwd, relative, kind) =
                            (cwd.clone(), entry.file.relative.clone(), entry.kind);
                        scm.run(cx, move |git| git.git_file_diff(&cwd, &relative, kind))
                    },
                    move |this: &mut Self, entry, result, cx| {
                        if this.generation != generation {
                            return;
                        }
                        let loaded = match result {
                            Ok(diff) => LoadedDiff::from_git(diff),
                            Err(error) => LoadedDiff::failed(error),
                        };
                        this.diffs.insert(entry.id.clone(), loaded);
                        schedule_publish(
                            |this: &mut Self| &mut this.state,
                            Self::publish,
                            this,
                            cx,
                        );
                    },
                    cx,
                )
            }) else {
                return;
            };
            task.await;
        }));
    }

    fn publish(&mut self, cx: &mut Context<Self>) {
        let focus =
            working_tree_diff_focus_id(&self.entries, self.focus_path.as_deref(), self.focus_kind);
        if self.state.focus != focus {
            self.state.focus = focus;
            self.state.focused = false;
        }
        let models: Vec<FileModel> = self
            .entries
            .iter()
            .map(|entry| {
                let loaded = self.diffs.get(&entry.id);
                let unstaged = entry.kind == GitFileDiffKind::Unstaged;
                let can_use_index_counts = loaded.is_some_and(|l| l.error.is_none())
                    && !(entry.file.staged && entry.file.unstaged);
                file_model(ModelInput {
                    id: &entry.id,
                    path: &entry.file.path,
                    label: working_tree_diff_entry_label(entry),
                    status: &entry.file.status,
                    loaded,
                    fallback: if can_use_index_counts {
                        (entry.file.additions, entry.file.deletions)
                    } else {
                        (0, 0)
                    },
                    unchanged_message: if unstaged {
                        "No unstaged changes"
                    } else {
                        "No staged changes"
                    },
                    error_message: |error| format!("Couldn’t load diff: {error}"),
                    actions: DiffFileActions {
                        stage: unstaged,
                        discard: unstaged,
                        stage_hunk: unstaged && !loaded.is_some_and(|l| l.binary || l.too_large),
                        comment: self.scm.hooks.add_to_chat.is_some(),
                        ..Default::default()
                    },
                })
            })
            .collect();
        self.state.publish(models, cx);
        cx.notify();
    }

    fn set_busy(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.busy_id = id.clone();
        self.state
            .view
            .update(cx, |view, cx| view.set_busy(id.map(SharedString::from), cx));
    }

    fn entry(&self, id: &str) -> Option<WorkingTreeDiffEntry> {
        self.entries
            .iter()
            .find(|entry| entry.id == id && entry.kind == GitFileDiffKind::Unstaged)
            .cloned()
    }

    /// `onStageFile` and `onDiscardFile`.
    pub fn file_action(&mut self, action: DiffAction, id: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(id) else {
            return;
        };
        let (cwd, relative) = (self.cwd.clone(), entry.file.relative.clone());
        let call = match action {
            DiffAction::Stage => self
                .scm
                .run(cx, move |git| git.git_stage_file(&cwd, &relative)),
            DiffAction::Discard => self
                .scm
                .run(cx, move |git| git.git_discard_file(&cwd, &relative)),
            DiffAction::Unstage => return,
        };
        self.finish_action(id.to_string(), call, cx);
    }

    /// `onStageHunk`: write the index with this hunk applied.
    pub fn stage_hunk(&mut self, request: HunkActionRequest, cx: &mut Context<Self>) {
        if request.action != DiffAction::Stage {
            return;
        }
        let id = request.file_id.to_string();
        let Some(entry) = self.entry(&id) else {
            return;
        };
        let Some(loaded) = self.diffs.get(&id) else {
            return;
        };
        let Some(pos) = request.hunk.pos else {
            return;
        };
        let Some(next) = stage_chunk_text(&loaded.original, &loaded.current, pos, None) else {
            return;
        };
        let (cwd, relative) = (self.cwd.clone(), entry.file.relative.clone());
        let call = self.scm.run(cx, move |git| {
            git.git_stage_contents(&cwd, &relative, &next)
        });
        self.finish_action(id, call, cx);
    }

    fn finish_action(
        &mut self,
        id: String,
        call: Task<Result<(), String>>,
        cx: &mut Context<Self>,
    ) {
        self.set_busy(Some(id), cx);
        self.action = Some(cx.spawn(async move |this, cx| {
            let result = call.await;
            let _ = this.update(cx, |this, cx| {
                if result.is_ok() {
                    this.scm.notify_git_changed(cx);
                }
                this.set_busy(None, cx);
            });
        }));
    }

    fn comment(&mut self, target: DiffCommentTarget, window: &mut Window, cx: &mut Context<Self>) {
        let (composer, subscription) = open_comment(
            &self.scm,
            |this: &mut Self| &mut this.state,
            target,
            window,
            cx,
        );
        self.state.comment = Some(composer);
        self._subscriptions.push(subscription);
        cx.notify();
    }
}

impl Render for WorkingTreeDiff {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_state(
            &self.cwd,
            self.error.as_deref(),
            "Couldn’t load changes",
            self.files.is_none(),
            self.state.view.clone(),
            self.state.comment.clone(),
            cx,
        )
    }
}

// CommitDiff.

/// The files one commit changed, against its first parent.
pub struct CommitDiff {
    scm: Scm,
    cwd: String,
    sha: String,
    files: Option<Vec<GitChangedFile>>,
    diffs: HashMap<String, LoadedDiff>,
    error: Option<String>,
    state: DiffState,
    load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl CommitDiff {
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        sha: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = DiffState::new(cx);
        let this = cx.entity().downgrade();
        state.view.update(cx, |view, _| {
            view.on_comment(Some(std::rc::Rc::new(
                move |target, window, cx: &mut App| {
                    let _ = this.update(cx, |this, cx| this.comment(target, window, cx));
                },
            )));
        });
        let mut this = Self {
            scm,
            cwd: cwd.into(),
            sha: sha.into(),
            files: None,
            diffs: HashMap::new(),
            error: None,
            state,
            load: None,
            _subscriptions: Vec::new(),
        };
        if this.cwd.is_empty() || this.cwd == "~" || this.sha.is_empty() {
            this.files = Some(Vec::new());
        } else {
            this.run(cx);
        }
        this
    }

    pub fn view(&self) -> &Entity<DiffView> {
        &self.state.view
    }

    pub fn files(&self) -> Option<&[GitChangedFile]> {
        self.files.as_deref()
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        let (cwd, sha) = (self.cwd.clone(), self.sha.clone());
        let list = self
            .scm
            .run(cx, move |git| git.git_commit_files(&cwd, &sha));
        self.load = Some(cx.spawn(async move |this, cx| {
            let result = list.await;
            let next = this.update(cx, |this, cx| match result {
                Ok(files) => {
                    this.files = Some(files.clone());
                    this.diffs.clear();
                    this.error = None;
                    this.publish(cx);
                    Some(files)
                }
                Err(error) => {
                    this.error = Some(error);
                    this.files = Some(Vec::new());
                    cx.notify();
                    None
                }
            });
            let Ok(Some(files)) = next else {
                return;
            };
            let Ok(task) = this.update(cx, |this, cx| {
                let (scm, cwd, sha) = (this.scm.clone(), this.cwd.clone(), this.sha.clone());
                load_concurrently(
                    files,
                    move |file: &GitChangedFile, cx| {
                        let (cwd, sha, relative) =
                            (cwd.clone(), sha.clone(), file.relative.clone());
                        scm.run(cx, move |git| {
                            git.git_commit_file_diff(&cwd, &sha, &relative)
                        })
                    },
                    |this: &mut Self, file, result, cx| {
                        let loaded = match result {
                            Ok(diff) => LoadedDiff::from_git(diff),
                            Err(error) => LoadedDiff::failed(error),
                        };
                        this.diffs.insert(file.relative.clone(), loaded);
                        schedule_publish(
                            |this: &mut Self| &mut this.state,
                            Self::publish,
                            this,
                            cx,
                        );
                    },
                    cx,
                )
            }) else {
                return;
            };
            task.await;
        }));
    }

    fn publish(&mut self, cx: &mut Context<Self>) {
        let comment = self.scm.hooks.add_to_chat.is_some();
        let models: Vec<FileModel> = self
            .files
            .iter()
            .flatten()
            .map(|file| {
                file_model(ModelInput {
                    id: &file.relative,
                    path: &file.path,
                    label: file.relative.clone(),
                    status: &file.status,
                    loaded: self.diffs.get(&file.relative),
                    fallback: (file.additions, file.deletions),
                    unchanged_message: "No textual diff",
                    error_message: |error| format!("Couldn’t load diff: {error}"),
                    actions: DiffFileActions {
                        comment,
                        ..Default::default()
                    },
                })
            })
            .collect();
        self.state.publish(models, cx);
        cx.notify();
    }

    fn comment(&mut self, target: DiffCommentTarget, window: &mut Window, cx: &mut Context<Self>) {
        let (composer, subscription) = open_comment(
            &self.scm,
            |this: &mut Self| &mut this.state,
            target,
            window,
            cx,
        );
        self.state.comment = Some(composer);
        self._subscriptions.push(subscription);
        cx.notify();
    }
}

impl Render for CommitDiff {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        render_state(
            &self.cwd,
            self.error.as_deref(),
            "Couldn’t load commit",
            self.files.is_none(),
            self.state.view.clone(),
            self.state.comment.clone(),
            cx,
        )
    }
}

// SessionChangesDiff.

/// Read-only review of the exact before and after snapshots one session
/// owns.
pub struct SessionChangesDiff {
    cwd: String,
    session_id: String,
    focus_path: Option<String>,
    checkpoints: Checkpoints,
    files: Option<Vec<CheckpointFile>>,
    diffs: HashMap<String, LoadedDiff>,
    error: Option<String>,
    state: DiffState,
    generation: u64,
    load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl SessionChangesDiff {
    /// `review` is the engine's `ReviewChanges` entity, when there is one.
    pub fn new(
        cwd: impl Into<String>,
        session_id: impl Into<String>,
        focus_path: Option<String>,
        checkpoints: Checkpoints,
        review: Option<Entity<ReviewChanges>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = DiffState::new(cx);
        let mut subscriptions = Vec::new();
        if let Some(review) = &review {
            subscriptions.push(cx.subscribe(review, |this, _, event: &ReviewChanged, cx| {
                if event.session_id.is_empty() || event.session_id == this.session_id {
                    this.run(cx);
                }
            }));
        }
        let mut this = Self {
            cwd: cwd.into(),
            session_id: session_id.into(),
            focus_path,
            checkpoints,
            files: None,
            diffs: HashMap::new(),
            error: None,
            state,
            generation: 0,
            load: None,
            _subscriptions: subscriptions,
        };
        if this.cwd.is_empty() || this.cwd == "~" || this.session_id.is_empty() {
            this.files = Some(Vec::new());
        } else {
            this.run(cx);
        }
        this
    }

    pub fn view(&self) -> &Entity<DiffView> {
        &self.state.view
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.files = None;
        self.diffs.clear();
        let status = self.checkpoints.status(&self.session_id, &self.cwd);
        self.load = Some(cx.spawn(async move |this, cx| {
            let result = status.await;
            let next = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return None;
                }
                match result {
                    Ok(status) => {
                        this.files = Some(status.files.clone());
                        this.error = None;
                        this.publish(cx);
                        Some(prioritize_file(&status.files, this.focus_path.as_deref()))
                    }
                    Err(error) => {
                        this.error = Some(error);
                        this.files = Some(Vec::new());
                        cx.notify();
                        None
                    }
                }
            });
            let Ok(Some(order)) = next else {
                return;
            };
            let Ok(task) = this.update(cx, |this, cx| {
                let (checkpoints, session, cwd) = (
                    this.checkpoints.clone(),
                    this.session_id.clone(),
                    this.cwd.clone(),
                );
                load_concurrently(
                    order,
                    move |file: &CheckpointFile, _| {
                        checkpoints.file_diff(&session, &cwd, &file.relative)
                    },
                    move |this: &mut Self, file, result, cx| {
                        if this.generation != generation {
                            return;
                        }
                        let loaded = match result {
                            Ok(diff) => LoadedDiff::from_sides(
                                diff.binary,
                                diff.too_large,
                                diff.original,
                                diff.current,
                            ),
                            Err(error) => LoadedDiff::failed(error),
                        };
                        this.diffs.insert(file.relative.clone(), loaded);
                        schedule_publish(
                            |this: &mut Self| &mut this.state,
                            Self::publish,
                            this,
                            cx,
                        );
                    },
                    cx,
                )
            }) else {
                return;
            };
            task.await;
        }));
        cx.notify();
    }

    fn publish(&mut self, cx: &mut Context<Self>) {
        self.state.focus = self.focus_path.as_ref().and_then(|focus| {
            self.files
                .iter()
                .flatten()
                .find(|file| &file.path == focus || &file.relative == focus)
                .map(|file| file.relative.clone())
        });
        let models: Vec<FileModel> = self
            .files
            .iter()
            .flatten()
            .map(|file| {
                file_model(ModelInput {
                    id: &file.relative,
                    path: &file.path,
                    label: file.relative.clone(),
                    status: &file.status,
                    loaded: self.diffs.get(&file.relative),
                    fallback: (file.additions, file.deletions),
                    unchanged_message: "No textual diff",
                    error_message: |error| error.to_string(),
                    actions: DiffFileActions::default(),
                })
            })
            .collect();
        self.state.publish(models, cx);
        cx.notify();
    }
}

/// `prioritizeFile`.
fn prioritize_file(files: &[CheckpointFile], focus: Option<&str>) -> Vec<CheckpointFile> {
    let Some(focus) = focus.filter(|focus| !focus.is_empty()) else {
        return files.to_vec();
    };
    let Some(index) = files
        .iter()
        .position(|file| file.path == focus || file.relative == focus)
    else {
        return files.to_vec();
    };
    let mut out = vec![files[index].clone()];
    out.extend(
        files
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, f)| f.clone()),
    );
    out
}

impl Render for SessionChangesDiff {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.error.is_none()
            && self.files.as_ref().is_some_and(|files| files.is_empty())
            && !(self.cwd.is_empty() || self.cwd == "~")
        {
            let theme = Theme::of(cx);
            return centered(
                div()
                    .text_px(13.)
                    .text_color(theme.content(0.45))
                    .child("No session changes"),
            )
            .into_any_element();
        }
        render_state(
            &self.cwd,
            self.error.as_deref(),
            "Couldn’t load session changes",
            self.files.is_none(),
            self.state.view.clone(),
            None,
            cx,
        )
    }
}
