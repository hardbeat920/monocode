//! The file pane a review opens beside the chat (FilePane.tsx with
//! SurfaceTabs.tsx), holding files, Changes and commit tabs. WorkingTreeDiff.tsx and
//! CommitDiff.tsx load their files four at a time off the UI thread and
//! build each diff there too; the view only lays out rows.
use crate::{
    diff_view::{DiffEvent, DiffFile, DiffView},
    file_editor::{FileEditor, FileEvent},
    file_icons::file_type_icon,
    git_status::{GitChanged, ProjectGit},
    pane_motion::PaneReveal,
    popover::{Anchor, Menu, MenuAction, MenuEvent, menu_popover},
    right_panel::{RED_400, selection, spinner, stroke},
    theme::*,
};
use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, Role, ScrollHandle,
    SharedString, Subscription, Task, Window, div, prelude::*, px, rgb,
};
use monocode_core::{
    diff,
    scm::{self, ChangedFile, FileDiff, FileDiffVersion},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const CONCURRENCY: usize = 4;
const MAX_CACHED_SOURCES: usize = 8;
const MAX_CACHED_TEXT_BYTES: usize = 32 * 1024 * 1024;

/// A review tab as the workspace saves it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum ReviewTab {
    File {
        cwd: String,
        path: String,
    },
    /// Working-tree changes, one side when opened from a sidebar section.
    Changes {
        cwd: String,
        path: Option<String>,
        staged: Option<bool>,
    },
    Commit {
        cwd: String,
        sha: String,
        short_sha: String,
        subject: String,
    },
    /// SessionChangesDiff: a session's recorded turn, focused on one file.
    Session {
        cwd: String,
        session: i64,
        path: Option<String>,
    },
}
impl ReviewTab {
    /// SurfaceTabs's `surfaceTabPresentation`.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::File { path, .. } => std::path::Path::new(path)
                .file_name()
                .map_or_else(|| path.clone(), |p| p.to_string_lossy().into_owned()),
            Self::Changes {
                path: Some(path), ..
            } => std::path::Path::new(path)
                .file_name()
                .map_or_else(|| path.clone(), |p| p.to_string_lossy().into_owned()),
            Self::Changes { staged, .. } => {
                if *staged == Some(true) {
                    "Staged Changes".into()
                } else {
                    "Changes".into()
                }
            }
            Self::Commit {
                short_sha, subject, ..
            } => {
                let subject = subject.trim();
                if subject.is_empty() {
                    short_sha.clone()
                } else {
                    subject.into()
                }
            }
            Self::Session { .. } => "Session Changes".into(),
        }
    }
    /// `editorTabKey`: one Changes tab per checkout and side, one per commit.
    pub(crate) fn same_tab(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::File { path: a, .. }, Self::File { path: b, .. }) => a == b,
            (
                Self::Changes {
                    cwd: a,
                    staged: x,
                    path: p,
                },
                Self::Changes {
                    cwd: b,
                    staged: y,
                    path: q,
                },
            ) => a == b && p == q && (x == y || x.is_none() || y.is_none()),
            (Self::Commit { cwd: a, sha: x, .. }, Self::Commit { cwd: b, sha: y, .. }) => {
                a == b && x == y
            }
            (
                Self::Session {
                    cwd: a, session: x, ..
                },
                Self::Session {
                    cwd: b, session: y, ..
                },
            ) => a == b && x == y,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ReviewState {
    pub tabs: Vec<ReviewTab>,
    pub active: usize,
    /// The pane's share of the split.
    pub ratio: f32,
    /// The chat beside it was closed; the review fills the card.
    pub chat_hidden: bool,
}

pub(crate) enum ReviewEvent {
    Changed,
    Comment(String),
    DragPane(Point<Pixels>),
    /// The last tab closed.
    Empty,
    WordWrap(bool),
}

#[derive(Clone)]
struct Loaded {
    version: Option<FileDiffVersion>,
    diff: Arc<FileDiff>,
    unified: Option<Arc<Vec<diff::Block>>>,
    additions: u32,
    deletions: u32,
    error: Option<String>,
}

fn file_model(entry: &ReviewEntry, file: &ChangedFile, loaded: Option<&Loaded>) -> DiffFile {
    if let Some(kind) = entry.staged {
        let unchanged = loaded.is_some_and(|l| {
            l.unified.is_some() && l.additions == 0 && l.deletions == 0 && !l.diff.binary
        });
        let index_counts =
            loaded.is_some_and(|l| l.error.is_none()) && !(file.staged && file.unstaged);
        let label = if file.staged && file.unstaged {
            format!(
                "{} ({})",
                file.relative,
                if kind { "Staged" } else { "Unstaged" }
            )
        } else {
            file.relative.clone()
        };
        DiffFile {
            id: entry.id.clone(),
            path: file.path.clone(),
            label,
            binary: loaded.is_some_and(|l| l.diff.binary),
            too_large: loaded.is_some_and(|l| l.diff.too_large),
            empty_message: match loaded {
                None => Some("Loading…".into()),
                Some(l) if l.error.is_some() => Some(format!(
                    "Couldn’t load diff: {}",
                    l.error.clone().unwrap_or_default()
                )),
                _ if unchanged => Some(
                    if kind {
                        "No staged changes"
                    } else {
                        "No unstaged changes"
                    }
                    .into(),
                ),
                _ => None,
            },
            additions: loaded
                .filter(|l| l.unified.is_some())
                .map(|l| l.additions)
                .unwrap_or(if index_counts {
                    file.additions as u32
                } else {
                    0
                }),
            deletions: loaded
                .filter(|l| l.unified.is_some())
                .map(|l| l.deletions)
                .unwrap_or(if index_counts {
                    file.deletions as u32
                } else {
                    0
                }),
            blocks: if unchanged {
                Arc::new(Vec::new())
            } else {
                loaded
                    .and_then(|l| l.unified.clone())
                    .unwrap_or_else(|| Arc::new(Vec::new()))
            },
            can_stage: !kind,
            can_discard: !kind,
            can_stage_hunk: !kind && !loaded.is_some_and(|l| l.diff.binary || l.diff.too_large),
        }
    } else {
        let unified = loaded.and_then(|l| l.unified.clone());
        DiffFile {
            id: entry.id.clone(),
            path: file.path.clone(),
            label: file.relative.clone(),
            binary: loaded.is_some_and(|l| l.diff.binary),
            too_large: loaded.is_some_and(|l| l.diff.too_large),
            empty_message: match loaded {
                None => Some("Loading…".into()),
                Some(l) if l.error.is_some() => Some(format!(
                    "Couldn’t load diff: {}",
                    l.error.clone().unwrap_or_default()
                )),
                Some(l)
                    if l.unified.is_some()
                        && l.additions == 0
                        && l.deletions == 0
                        && !l.diff.binary =>
                {
                    Some("No textual diff".into())
                }
                _ => None,
            },
            additions: loaded
                .filter(|l| l.unified.is_some())
                .map_or(file.additions as u32, |l| l.additions),
            deletions: loaded
                .filter(|l| l.unified.is_some())
                .map_or(file.deletions as u32, |l| l.deletions),
            blocks: unified.unwrap_or_else(|| Arc::new(Vec::new())),
            can_stage: false,
            can_discard: false,
            can_stage_hunk: false,
        }
    }
}

struct ReviewEntry {
    id: SharedString,
    file: usize,
    staged: Option<bool>,
}

/// One tab's files and diffs.
struct Source {
    tab: ReviewTab,
    view: Entity<DiffView>,
    editor: Option<Entity<FileEditor>>,
    editor_mode: bool,
    _editor_events: Option<Subscription>,
    files: Option<Vec<ChangedFile>>,
    entries: Vec<ReviewEntry>,
    entry_indices: HashMap<String, usize>,
    dirty: BTreeSet<usize>,
    reset_models: bool,
    error: Option<String>,
    loaded: HashMap<String, Arc<Loaded>>,
    generation: Arc<AtomicU64>,
    complete: bool,
    loading: bool,
    refresh_pending: bool,
    _tasks: Vec<Task<()>>,
    _events: Subscription,
}
impl Source {
    fn set_files(&mut self, files: Vec<ChangedFile>) {
        self.files = Some(files);
        self.index_entries();
    }
    fn index_entries(&mut self) {
        self.entries = match (&self.tab, &self.files) {
            (ReviewTab::Changes { staged, .. }, Some(files)) => entries(files, *staged)
                .into_iter()
                .map(|(id, staged, file)| ReviewEntry {
                    id: id.into(),
                    staged: Some(staged),
                    file,
                })
                .collect(),
            (ReviewTab::Commit { .. } | ReviewTab::Session { .. }, Some(files)) => files
                .iter()
                .enumerate()
                .map(|(file, entry)| ReviewEntry {
                    id: entry.relative.clone().into(),
                    file,
                    staged: None,
                })
                .collect(),
            _ => Vec::new(),
        };
        self.entry_indices = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| (entry.id.to_string(), i))
            .collect();
        self.dirty.clear();
        self.reset_models = true;
    }
    fn set_loaded(&mut self, id: String, loaded: Arc<Loaded>) {
        self.dirty.insert(self.entry_indices[&id]);
        self.loaded.insert(id, loaded);
    }
    fn cancel(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self._tasks.clear();
        self.loading = false;
        self.refresh_pending = false;
    }
}

/// Closed diff tabs retain their models, viewport and completed highlights.
/// Editors are deliberately excluded, so no dirty document is kept here.
#[derive(Default)]
pub(crate) struct ReviewCache {
    sources: VecDeque<CachedSource>,
}
struct CachedSource {
    tab: ReviewTab,
    view: Entity<DiffView>,
    files: Option<Vec<ChangedFile>>,
    loaded: HashMap<String, Arc<Loaded>>,
    complete: bool,
    text_bytes: usize,
}
impl ReviewCache {
    fn matches(a: &ReviewTab, b: &ReviewTab) -> bool {
        match (a, b) {
            (
                ReviewTab::Changes {
                    cwd: a, staged: x, ..
                },
                ReviewTab::Changes {
                    cwd: b, staged: y, ..
                },
            ) => a == b && x == y,
            (
                ReviewTab::Commit { cwd: a, sha: x, .. },
                ReviewTab::Commit { cwd: b, sha: y, .. },
            ) => a == b && x == y,
            _ => false,
        }
    }
    fn take(&mut self, tab: &ReviewTab) -> Option<CachedSource> {
        let ix = self
            .sources
            .iter()
            .position(|s| Self::matches(&s.tab, tab))?;
        self.sources.remove(ix)
    }
    fn remember(&mut self, source: &Source) {
        if source.editor.is_some() || source.files.is_none() || source.error.is_some() {
            return;
        }
        let text_bytes = source
            .loaded
            .values()
            .map(|loaded| {
                loaded.diff.original.len()
                    + loaded.diff.current.len()
                    + loaded.unified.as_ref().map_or(0, |blocks| {
                        blocks
                            .iter()
                            .flat_map(|b| b.lines())
                            .map(|l| l.text.len())
                            .sum::<usize>()
                    })
            })
            .sum();
        self.sources.retain(|s| !Self::matches(&s.tab, &source.tab));
        if text_bytes > MAX_CACHED_TEXT_BYTES {
            return;
        }
        self.sources.push_back(CachedSource {
            tab: source.tab.clone(),
            view: source.view.clone(),
            files: source.files.clone(),
            loaded: source.loaded.clone(),
            complete: source.complete,
            text_bytes,
        });
        while self.sources.len() > MAX_CACHED_SOURCES
            || self.sources.iter().map(|s| s.text_bytes).sum::<usize>() > MAX_CACHED_TEXT_BYTES
        {
            self.sources.pop_front();
        }
    }
}

pub(crate) struct ReviewPane {
    git: Entity<ProjectGit>,
    focus: FocusHandle,
    /// Bodies replaced before the next paint may still own keyboard focus.
    retiring_focus: Vec<FocusHandle>,
    pub(crate) state: ReviewState,
    sources: Vec<Source>,
    /// Search results share one replaceable file tab until it is pinned or edited.
    preview_path: Option<String>,
    visible: bool,
    closed: bool,
    deferred: bool,
    publish_task: Option<Task<()>>,
    /// The deferred body fades from its first rendered frame.
    revealed: Option<PaneReveal>,
    hovered_tab: Option<usize>,
    pub(crate) pane_draggable: bool,
    tab_scroll: ScrollHandle,
    /// Tab animations: opening and closing file tabs, with each tab's last look.
    tab_motion: crate::tab_motion::TabMotion<String, ReviewTab>,
    tab_drag: Option<FileTabDrag>,
    skip_tab_click: bool,
    pending_close: Option<(usize, bool)>,
    close_prompted: bool,
    menu: Option<(usize, Point<gpui::Pixels>, Entity<Menu>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}
struct FileTabDrag {
    from: usize,
    to: usize,
    origin: Pixels,
    offset: Pixels,
    bounds: Vec<Bounds<Pixels>>,
    active: bool,
}
impl EventEmitter<ReviewEvent> for ReviewPane {}
impl Drop for ReviewPane {
    fn drop(&mut self) {
        for source in &mut self.sources {
            source.cancel();
        }
    }
}

/// workingTreeDiffEntries: staged entries first, or just one side.
fn entries(files: &[ChangedFile], staged: Option<bool>) -> Vec<(String, bool, usize)> {
    let kinds: &[bool] = match staged {
        Some(true) => &[true],
        Some(false) => &[false],
        None => &[true, false],
    };
    let mut out = Vec::new();
    for &kind in kinds {
        for (index, file) in files.iter().enumerate() {
            if (kind && file.staged) || (!kind && file.unstaged) {
                let id = format!(
                    "{}:{}",
                    if kind { "staged" } else { "unstaged" },
                    file.relative
                );
                out.push((id, kind, index));
            }
        }
    }
    out
}

/// SessionChangesDiff's list: the recorded turn's files, as a commit lists them.
type Listed = (Vec<ChangedFile>, Option<HashMap<String, FileDiffVersion>>);
fn session_files(cwd: &str, session: i64) -> Result<Listed, String> {
    let store = monocode_core::checkpoint::store().ok_or("Session changes are unavailable")?;
    let status = store.exclusive(|s| s.status(&session.to_string(), cwd))?;
    Ok((
        status
            .files
            .into_iter()
            .map(|f| ChangedFile {
                path: f.path,
                relative: f.relative,
                status: f.status,
                additions: f.additions,
                deletions: f.deletions,
                staged: false,
                unstaged: true,
            })
            .collect(),
        None,
    ))
}
fn session_file_diff(cwd: &str, session: i64, relative: &str) -> Result<FileDiff, String> {
    let store = monocode_core::checkpoint::store().ok_or("Session changes are unavailable")?;
    store.exclusive(|s| s.file_diff(&session.to_string(), cwd, relative))
}

impl ReviewPane {
    pub(crate) fn new(git: Entity<ProjectGit>, state: ReviewState, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&git, |this, _, _: &GitChanged, cx| {
                this.refresh(cx);
            }),
            cx.observe_global::<crate::settings::Preferences>(|this, cx| {
                let editor_mode = cx.global::<crate::settings::Preferences>().diff_view == "editor";
                if this.sources.get(this.state.active).is_some_and(|source| {
                    matches!(source.tab, ReviewTab::Changes { path: Some(_), .. })
                        && source.editor_mode != editor_mode
                }) {
                    this.remember_body_focus(cx);
                }
                for source in &mut this.sources {
                    if matches!(source.tab, ReviewTab::Changes { path: Some(_), .. }) {
                        source.editor_mode = editor_mode;
                        if !editor_mode && let Some(editor) = &source.editor {
                            editor.update(cx, |e, cx| e.set_visible(false, cx));
                        }
                    }
                }
                this.load_active(cx);
                cx.notify();
            }),
        ];
        let mut this = Self {
            git,
            focus: cx.focus_handle(),
            retiring_focus: Vec::new(),
            state: ReviewState {
                ratio: if state.ratio > 0.05 { state.ratio } else { 0.5 },
                ..state
            },
            sources: Vec::new(),
            preview_path: None,
            visible: false,
            closed: false,
            publish_task: None,
            deferred: false,
            revealed: None,
            hovered_tab: None,
            pane_draggable: false,
            tab_scroll: ScrollHandle::new(),
            tab_motion: Default::default(),
            tab_drag: None,
            skip_tab_click: false,
            pending_close: None,
            close_prompted: false,
            menu: None,
            _subscriptions: subscriptions,
        };
        let tabs = this.state.tabs.clone();
        for tab in tabs {
            let source = this.source(tab, cx);
            this.sources.push(source);
        }
        this.state.active = this.state.active.min(this.sources.len().saturating_sub(1));
        this
    }
    fn source(&mut self, tab: ReviewTab, cx: &mut Context<Self>) -> Source {
        let cached = self.git.update(cx, |git, _| git.review_cache.take(&tab));
        let view = cached
            .as_ref()
            .map_or_else(|| cx.new(DiffView::new), |s| s.view.clone());
        let events = cx.subscribe(&view, |this, view, event: &DiffEvent, cx| {
            let Some(ix) = this.sources.iter().position(|s| s.view == view) else {
                return;
            };
            this.diff_event(ix, event, cx);
        });
        let editor_mode = matches!(tab, ReviewTab::File { .. })
            || cx
                .try_global::<crate::settings::Preferences>()
                .is_none_or(|p| p.diff_view == "editor");
        let editor_path = match &tab {
            ReviewTab::File { path, .. } => Some((path, false)),
            ReviewTab::Changes {
                path: Some(path), ..
            } => Some((path, true)),
            _ => None,
        };
        let editor = if let Some((path, show_diff)) = editor_path {
            Some(cx.new(|cx| {
                let mut editor = if self.deferred || !editor_mode {
                    FileEditor::new_deferred(path.clone(), self.git.clone(), cx)
                } else {
                    FileEditor::new(path.clone(), self.git.clone(), cx)
                };
                editor.show_diff = show_diff;
                editor
            }))
        } else {
            None
        };
        let editor_events = editor.as_ref().map(|editor| {
            cx.subscribe(editor, |this, editor, event: &FileEvent, cx| {
                if matches!(event, FileEvent::Dirty) {
                    let editor = editor.read(cx);
                    if editor.dirty && this.preview_path.as_ref() == Some(&editor.path) {
                        this.preview_path = None;
                    }
                }
                if let FileEvent::AddToChat(text) = event {
                    cx.emit(ReviewEvent::Comment(text.clone()));
                }
                if let FileEvent::WordWrap(wrap) = event {
                    cx.emit(ReviewEvent::WordWrap(*wrap));
                }
                if matches!(event, FileEvent::SaveFailed) {
                    this.pending_close = None;
                    this.close_prompted = false;
                }
                if matches!(event, FileEvent::Saved)
                    && this.pending_close.is_some()
                    && this.closing_editors(cx).is_empty()
                {
                    this.approve_close(cx);
                }
                cx.notify();
            })
        });
        let mut source = Source {
            editor_mode,
            editor,
            _editor_events: editor_events,
            tab,
            view,
            files: cached.as_ref().and_then(|s| s.files.clone()),
            entries: Vec::new(),
            entry_indices: HashMap::new(),
            dirty: BTreeSet::new(),
            reset_models: true,
            error: None,
            loaded: cached
                .as_ref()
                .map_or_else(HashMap::new, |s| s.loaded.clone()),
            generation: Arc::new(AtomicU64::new(0)),
            complete: cached.is_some_and(|s| s.complete),
            loading: false,
            refresh_pending: false,
            _tasks: Vec::new(),
            _events: events,
        };
        source.index_entries();
        source
    }
    /// openChangesTab / openCommitTab: an existing tab takes the new focus.
    pub(crate) fn open(&mut self, tab: ReviewTab, cx: &mut Context<Self>) {
        self.remember_body_focus(cx);
        self.tab_drag = None;
        if matches!(&tab, ReviewTab::File { path, .. } if self.preview_path.as_ref() == Some(path))
        {
            self.preview_path = None;
        }
        if let Some(ix) = self.sources.iter().position(|s| s.tab.same_tab(&tab)) {
            let reload = !matches!(
                (&self.sources[ix].tab, &tab),
                (
                    ReviewTab::Changes { staged: a, .. },
                    ReviewTab::Changes { staged: b, .. }
                ) if a == b
            ) && matches!(tab, ReviewTab::Changes { .. } | ReviewTab::Session { .. });
            self.sources[ix].tab = tab;
            if reload {
                self.sources[ix].cancel();
                self.sources[ix].files = None;
                self.sources[ix].loaded.clear();
            }
            self.state.active = ix;
        } else {
            let source = self.source(tab, cx);
            self.sources.push(source);
            self.state.active = self.sources.len() - 1;
        }
        self.state.chat_hidden = self.state.chat_hidden && !self.sources.is_empty();
        self.sync_state();
        self.load_active(cx);
        cx.emit(ReviewEvent::Changed);
        cx.notify();
    }
    /// Single-clicked search matches reuse a clean preview; explicit opens pin it.
    pub(crate) fn open_file(&mut self, tab: ReviewTab, pin: bool, cx: &mut Context<Self>) {
        let ReviewTab::File { path, .. } = &tab else {
            self.open(tab, cx);
            return;
        };
        if pin {
            self.open(tab, cx);
            return;
        }
        self.remember_body_focus(cx);
        let path = path.clone();
        let existing = self.sources.iter().any(|source| source.tab.same_tab(&tab));
        let keep_preview = self.preview_path.as_ref() == Some(&path)
            && !self.sources.iter().any(|source| {
                source.tab.same_tab(&tab)
                    && source
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.read(cx).dirty)
            });
        if !existing
            && let Some(preview) = self.preview_path.take()
            && let Some(ix) = self.sources.iter().position(
                |source| matches!(&source.tab, ReviewTab::File { path, .. } if path == &preview),
            )
            && self.pending_close.is_none()
            && !self.sources[ix]
                .editor
                .as_ref()
                .is_some_and(|editor| editor.read(cx).dirty)
        {
            let mut removed = self.sources.remove(ix);
            removed.cancel();
            if self.state.active >= ix && self.state.active > 0 {
                self.state.active -= 1;
            }
            self.hovered_tab = None;
        }
        self.open(tab, cx);
        // Selecting a permanent tab must not make it replaceable again.
        if !existing || keep_preview {
            self.preview_path = Some(path);
        }
        cx.notify();
    }
    pub(crate) fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.tab_drag = None;
        self.close_prompted = false;
        self.pending_close = Some((ix, false));
        if !self.closing_editors(cx).is_empty() {
            cx.notify();
            return;
        }
        self.pending_close = None;
        self.remove_tab(ix, cx);
    }
    fn remove_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.sources.len() {
            return;
        }
        // The last tab's files stay on screen while the pane slides away;
        // the shell drops the pane once it has gone.
        if self.sources.len() == 1 {
            self.preview_path = None;
            self.hovered_tab = None;
            cx.emit(ReviewEvent::Empty);
            return;
        }
        if ix == self.state.active {
            self.remember_body_focus(cx);
        }
        let mut removed = self.sources.remove(ix);
        removed.cancel();
        self.git
            .update(cx, |git, _| git.review_cache.remember(&removed));
        if self.state.active >= ix && self.state.active > 0 {
            self.state.active -= 1;
        }
        self.hovered_tab = None;
        self.sync_state();
        self.load_active(cx);
        cx.emit(ReviewEvent::Changed);
        cx.notify();
    }
    pub(crate) fn close_active(&mut self, cx: &mut Context<Self>) {
        self.close_tab(self.state.active, cx);
    }
    fn close_others(&mut self, keep: usize, cx: &mut Context<Self>) {
        self.tab_drag = None;
        self.close_prompted = false;
        self.pending_close = Some((keep, true));
        if !self.closing_editors(cx).is_empty() {
            cx.notify();
            return;
        }
        self.pending_close = None;
        self.remove_others(keep, cx);
    }
    fn remove_others(&mut self, keep: usize, cx: &mut Context<Self>) {
        if keep != self.state.active {
            self.remember_body_focus(cx);
        }
        let source = self.sources.remove(keep);
        for mut removed in self.sources.drain(..) {
            removed.cancel();
            self.git
                .update(cx, |git, _| git.review_cache.remember(&removed));
        }
        self.sources.push(source);
        self.state.active = 0;
        self.sync_state();
        cx.emit(ReviewEvent::Changed);
        cx.notify();
    }
    pub(crate) fn unsaved_files(&self, cx: &gpui::App) -> bool {
        self.sources
            .iter()
            .filter_map(|s| s.editor.as_ref())
            .any(|e| e.read(cx).dirty || e.read(cx).saving)
    }
    fn closing_editors(&self, cx: &gpui::App) -> Vec<Entity<FileEditor>> {
        let Some((target, others)) = self.pending_close else {
            return Vec::new();
        };
        self.sources
            .iter()
            .enumerate()
            .filter(|(ix, _)| if others { *ix != target } else { *ix == target })
            .filter_map(|(_, s)| s.editor.clone())
            .filter(|e| e.read(cx).dirty || e.read(cx).saving)
            .collect()
    }
    fn approve_close(&mut self, cx: &mut Context<Self>) {
        self.close_prompted = false;
        if let Some((ix, others)) = self.pending_close.take() {
            if others {
                self.remove_others(ix, cx);
            } else {
                self.remove_tab(ix, cx);
            }
        }
    }
    fn prompt_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_close.is_none() || self.close_prompted {
            return;
        }
        self.close_prompted = true;
        let editors = self.closing_editors(cx);
        let names = editors
            .iter()
            .map(|e| {
                std::path::Path::new(&e.read(cx).path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>()
            .join(", ");
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("Save changes to {names}?"),
            Some("Your unsaved edits will be lost if you discard them."),
            &["Save", "Discard", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match answer {
                    Ok(0) => {
                        let editors = this.closing_editors(cx);
                        if editors.is_empty() {
                            this.approve_close(cx);
                        } else {
                            for editor in editors {
                                editor.update(cx, |e, cx| e.save(cx));
                            }
                        }
                    }
                    Ok(1) if !this.closing_editors(cx).iter().any(|e| e.read(cx).saving) => {
                        this.approve_close(cx)
                    }
                    _ => {
                        this.pending_close = None;
                        this.close_prompted = false;
                        this.focus_active(window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    #[cfg(test)]
    pub(crate) fn active_editor(&self) -> Option<Entity<FileEditor>> {
        self.sources
            .get(self.state.active)
            .and_then(|s| s.editor.clone())
    }
    pub(crate) fn navigate_file(&mut self, line: usize, column: usize, cx: &mut Context<Self>) {
        if let Some(editor) = self
            .sources
            .get(self.state.active)
            .and_then(|s| s.editor.clone())
        {
            editor.update(cx, |editor, cx| editor.navigate(line, column, cx));
        }
    }
    pub(crate) fn focus_active(&self, window: &mut Window, cx: &mut gpui::App) {
        // Keep workspace shortcuts reachable while the active body is loading,
        // deferred or showing an error. Rendering hands focus to a ready body.
        window.focus(&self.focus, cx);
    }
    fn remember_body_focus(&mut self, cx: &gpui::App) {
        if let Some(source) = self.sources.get(self.state.active) {
            for handle in std::iter::once(source.view.focus_handle(cx))
                .chain(source.editor.as_ref().map(|editor| editor.focus_handle(cx)))
            {
                if !self
                    .retiring_focus
                    .iter()
                    .any(|existing| existing == &handle)
                {
                    self.retiring_focus.push(handle);
                }
            }
        }
    }
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.sources.len() && ix != self.state.active {
            self.remember_body_focus(cx);
            self.state.active = ix;
            self.sync_state();
            self.load_active(cx);
            cx.emit(ReviewEvent::Changed);
            cx.notify();
        }
    }
    fn sync_state(&mut self) {
        self.state.tabs = self.sources.iter().map(|s| s.tab.clone()).collect();
        if self.preview_path.as_ref().is_some_and(|preview| {
            !self.sources.iter().any(
                |source| matches!(&source.tab, ReviewTab::File { path, .. } if path == preview),
            )
        }) {
            self.preview_path = None;
        }
    }

    fn start_tab_drag(&mut self, ix: usize, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if self.pending_close.is_some() {
            return;
        }
        self.skip_tab_click = false;
        self.select(ix, cx);
        let bounds: Vec<_> = (0..self.sources.len())
            .filter_map(|ix| self.tab_scroll.bounds_for_item(ix))
            .collect();
        if bounds.len() != self.sources.len() {
            return;
        }
        self.tab_drag = Some(FileTabDrag {
            from: ix,
            to: ix,
            origin: event.position.x,
            offset: px(0.),
            bounds,
            active: false,
        });
    }
    fn move_tab_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.tab_drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.tab_drag = None;
            cx.notify();
            return;
        }
        let delta = event.position.x - drag.origin;
        if !drag.active && delta.abs() < px(4.) {
            return;
        }
        drag.active = true;
        let start = drag.bounds[drag.from].left();
        drag.offset = delta.clamp(
            drag.bounds[0].left() - start,
            drag.bounds.last().unwrap().left() - start,
        );
        drag.to = drag
            .bounds
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.left() - start - drag.offset)
                    .abs()
                    .partial_cmp(&(b.left() - start - drag.offset).abs())
                    .unwrap()
            })
            .map_or(drag.from, |(ix, _)| ix);
        cx.notify();
    }
    fn finish_tab_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.tab_drag.take() else {
            return;
        };
        self.skip_tab_click = drag.active;
        if drag.active
            && drag.from != drag.to
            && drag.from < self.sources.len()
            && drag.to < self.sources.len()
        {
            let source = self.sources.remove(drag.from);
            self.sources.insert(drag.to, source);
            self.state.active = drag.to;
            self.hovered_tab = None;
            self.sync_state();
            cx.emit(ReviewEvent::Changed);
        }
        cx.notify();
    }
    fn tab_shift(&self, ix: usize) -> Pixels {
        let Some(drag) = self.tab_drag.as_ref().filter(|d| d.active) else {
            return px(0.);
        };
        if ix == drag.from {
            drag.offset
        } else if drag.from < ix && ix <= drag.to {
            drag.bounds[ix - 1].left() - drag.bounds[ix].left()
        } else if drag.to <= ix && ix < drag.from {
            drag.bounds[ix + 1].left() - drag.bounds[ix].left()
        } else {
            px(0.)
        }
    }
    pub(crate) fn set_deferred(&mut self, deferred: bool, cx: &mut Context<Self>) {
        if self.deferred != deferred {
            if deferred {
                self.remember_body_focus(cx);
            }
            if self.deferred {
                self.revealed = (!cx.reduce_motion()).then(PaneReveal::default);
            }
            self.deferred = deferred;
            if !deferred {
                self.load_active(cx);
            }
            cx.notify();
        }
    }
    pub(crate) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        let was = self.visible;
        self.visible = visible;
        if !visible && was {
            for source in &mut self.sources {
                source.cancel();
            }
        }
        for (ix, source) in self.sources.iter_mut().enumerate() {
            if ix != self.state.active && source.loading {
                source.cancel();
            }
            if let Some(editor) = &source.editor {
                editor.update(cx, |e, cx| {
                    e.set_visible(
                        source.editor_mode && visible && !self.deferred && ix == self.state.active,
                        cx,
                    )
                });
            }
        }
        if visible && !was {
            self.load_active(cx);
        }
    }
    pub(crate) fn cache_closed(&mut self, cx: &mut Context<Self>) {
        self.closed = true;
        self.visible = false;
        for source in &mut self.sources {
            source.cancel();
            if let Some(editor) = &source.editor {
                editor.update(cx, |editor, cx| editor.set_visible(false, cx));
            }
            self.git
                .update(cx, |git, _| git.review_cache.remember(source));
        }
    }
    /// Focus and Git changes reread the working tree; commits never change.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.visible
            && let Some(source) = self.sources.get(self.state.active)
            && matches!(
                source.tab,
                ReviewTab::Changes { .. } | ReviewTab::Session { .. }
            )
        {
            self.load_active(cx);
        }
    }

    fn load_active(&mut self, cx: &mut Context<Self>) {
        if !self.visible || self.deferred {
            return;
        }
        for (ix, source) in self.sources.iter_mut().enumerate() {
            if ix != self.state.active && source.loading {
                source.cancel();
            }
            if let Some(editor) = &source.editor {
                editor.update(cx, |e, cx| {
                    e.set_visible(
                        source.editor_mode && self.visible && ix == self.state.active,
                        cx,
                    )
                });
            }
        }
        let ix = self.state.active;
        let Some(source) = self.sources.get_mut(ix) else {
            return;
        };
        if source.editor_mode && source.editor.is_some() {
            return;
        }
        if source.complete && matches!(source.tab, ReviewTab::Commit { .. }) {
            return;
        }
        if source.loading {
            source.refresh_pending = true;
            return;
        }
        source.loading = true;
        let generation = source.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let tab = source.tab.clone();
        let counter = source.generation.clone();
        let known_index = match &tab {
            ReviewTab::Changes { cwd, .. } if self.git.read(cx).project() == cwd => {
                self.git.read(cx).index.clone()
            }
            _ => None,
        };
        let list = cx.background_executor().spawn({
            let tab = tab.clone();
            async move {
                match &tab {
                    ReviewTab::File { .. } => unreachable!(),
                    ReviewTab::Changes { cwd, .. } => {
                        let root = PathBuf::from(cwd);
                        let index = scm::review_index(&root, known_index.as_deref())?;
                        let versions = scm::file_diff_versions(&root, &index).ok();
                        Ok((index.files, versions))
                    }
                    ReviewTab::Commit { cwd, sha, .. } => {
                        scm::commit_files(&PathBuf::from(cwd), sha).map(|files| (files, None))
                    }
                    ReviewTab::Session { cwd, session, .. } => session_files(cwd, *session),
                }
            }
        });
        let task = cx.spawn(async move |this, cx| {
            let listed = list.await;
            let order = this.update(cx, |this, cx| {
                let source = this
                    .sources
                    .iter_mut()
                    .find(|s| Arc::ptr_eq(&s.generation, &counter))?;
                if counter.load(Ordering::SeqCst) != generation {
                    return None;
                }
                match listed {
                    Ok((files, versions)) => {
                        source.error = None;
                        let order: Vec<(String, Option<bool>, ChangedFile)> = match &tab {
                            ReviewTab::File { .. } => unreachable!(),
                            ReviewTab::Changes { path, staged, .. } => {
                                let mut entries: Vec<_> = entries(&files, *staged)
                                    .into_iter()
                                    .map(|(id, kind, file)| (id, Some(kind), files[file].clone()))
                                    .collect();
                                // The focused file loads first.
                                if let Some(at) = path.as_ref().and_then(|p| {
                                    entries
                                        .iter()
                                        .position(|e| &e.2.path == p || &e.2.relative == p)
                                }) {
                                    let focused = entries.remove(at);
                                    entries.insert(0, focused);
                                }
                                entries
                            }
                            ReviewTab::Commit { .. } => files
                                .iter()
                                .map(|f| (f.relative.clone(), None, f.clone()))
                                .collect(),
                            ReviewTab::Session { path, .. } => {
                                let mut entries: Vec<_> = files
                                    .iter()
                                    .map(|f| (f.relative.clone(), None, f.clone()))
                                    .collect();
                                if let Some(at) = path
                                    .as_ref()
                                    .and_then(|p| entries.iter().position(|e| &e.2.path == p))
                                {
                                    let focused = entries.remove(at);
                                    entries.insert(0, focused);
                                }
                                entries
                            }
                        };
                        let retained: std::collections::HashSet<_> =
                            order.iter().map(|e| e.0.clone()).collect();
                        source.loaded.retain(|id, _| retained.contains(id));
                        source.set_files(files);
                        this.publish(cx);
                        Some((order, versions))
                    }
                    Err(error) => {
                        source.error = Some(error);
                        source.files = Some(Vec::new());
                        source.loading = false;
                        source.refresh_pending = false;
                        cx.notify();
                        None
                    }
                }
            });
            let Ok(Some((order, versions))) = order else {
                return;
            };
            let queue = Arc::new(std::sync::Mutex::new(order.into_iter()));
            let versions = Arc::new(versions);
            let mut workers = Vec::new();
            for _ in 0..CONCURRENCY {
                let queue = queue.clone();
                let versions = versions.clone();
                let counter = counter.clone();
                let tab = tab.clone();
                let this = this.clone();
                let mut cx = cx.clone();
                workers.push(async move {
                    loop {
                        if counter.load(Ordering::SeqCst) != generation {
                            return;
                        }
                        let Some((id, staged, file)) = queue.lock().unwrap().next() else {
                            return;
                        };
                        let previous = this
                            .update(&mut cx, |this, _| {
                                this.sources
                                    .iter()
                                    .find(|s| Arc::ptr_eq(&s.generation, &counter))
                                    .and_then(|s| s.loaded.get(&id).cloned())
                            })
                            .ok()
                            .flatten();
                        let version = versions.as_ref().as_ref().and_then(|v| v.get(&id)).cloned();
                        if previous.as_ref().is_some_and(|p| {
                            p.error.is_none()
                                && (matches!(tab, ReviewTab::Commit { .. })
                                    || (version.is_some() && p.version == version))
                        }) {
                            continue;
                        }
                        let tab = tab.clone();
                        let relative = file.relative.clone();
                        let work_counter = counter.clone();
                        let loaded = cx
                            .background_executor()
                            .spawn(async move {
                                let diff = match &tab {
                                    ReviewTab::File { .. } => unreachable!(),
                                    ReviewTab::Changes { cwd, .. } => scm::file_diff(
                                        &PathBuf::from(cwd),
                                        &relative,
                                        staged == Some(true),
                                    ),
                                    ReviewTab::Commit { cwd, sha, .. } => {
                                        scm::commit_file_diff(&PathBuf::from(cwd), sha, &relative)
                                    }
                                    ReviewTab::Session { cwd, session, .. } => {
                                        session_file_diff(cwd, *session, &relative)
                                    }
                                };
                                if work_counter.load(Ordering::SeqCst) != generation {
                                    return None;
                                }
                                match diff {
                                    Ok(diff) => {
                                        if let Some(prev) = previous.filter(|p| {
                                            p.error.is_none()
                                                && p.diff.original == diff.original
                                                && p.diff.current == diff.current
                                                && p.diff.binary == diff.binary
                                                && p.diff.too_large == diff.too_large
                                        }) {
                                            return Some(Arc::new(Loaded {
                                                version,
                                                ..prev.as_ref().clone()
                                            }));
                                        }
                                        let unified =
                                            (!diff.binary && !diff.too_large).then(|| {
                                                diff::build_unified_file(
                                                    &diff.original,
                                                    &diff.current,
                                                    diff::CONTEXT_DEFAULT,
                                                )
                                            });
                                        Some(Arc::new(Loaded {
                                            version,
                                            additions: unified.as_ref().map_or(0, |u| u.additions),
                                            deletions: unified.as_ref().map_or(0, |u| u.deletions),
                                            unified: unified.map(|u| Arc::new(u.blocks)),
                                            diff: Arc::new(diff),
                                            error: None,
                                        }))
                                    }
                                    Err(error) => Some(Arc::new(Loaded {
                                        version: None,
                                        diff: Arc::new(FileDiff::default()),
                                        unified: None,
                                        additions: 0,
                                        deletions: 0,
                                        error: Some(error),
                                    })),
                                }
                            })
                            .await;
                        let Some(loaded) = loaded else {
                            return;
                        };
                        let _ = this.update(&mut cx, |this, cx| {
                            if counter.load(Ordering::SeqCst) != generation {
                                return;
                            }
                            if let Some(source) = this
                                .sources
                                .iter_mut()
                                .find(|s| Arc::ptr_eq(&s.generation, &counter))
                            {
                                source.set_loaded(id, loaded);
                            }
                            this.queue_publish(cx);
                        });
                    }
                });
            }
            futures_join_all(workers).await;
            let _ = this.update(cx, |this, cx| {
                let mut refresh = false;
                if let Some(source) = this
                    .sources
                    .iter_mut()
                    .find(|s| Arc::ptr_eq(&s.generation, &counter))
                    && counter.load(Ordering::SeqCst) == generation
                {
                    source.complete = source.loaded.values().all(|l| l.error.is_none());
                    source.loading = false;
                    refresh = std::mem::take(&mut source.refresh_pending);
                }
                if counter.load(Ordering::SeqCst) == generation {
                    this.publish(cx);
                }
                if refresh {
                    this.load_active(cx);
                }
            });
        });
        if let Some(source) = self.sources.get_mut(ix) {
            source._tasks = vec![task];
        }
    }

    fn queue_publish(&mut self, cx: &mut Context<Self>) {
        if self.publish_task.is_some() {
            return;
        }
        self.publish_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(50))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.publish_task = None;
                this.publish(cx);
            });
        }));
    }

    /// WorkingTreeDiff's and CommitDiff's file models.
    fn publish(&mut self, cx: &mut Context<Self>) {
        self.publish_task = None;
        let ix = self.state.active;
        let Some(source) = self.sources.get_mut(ix) else {
            return;
        };
        let Some(files) = &source.files else {
            cx.notify();
            return;
        };
        let reset = std::mem::take(&mut source.reset_models);
        let changed: Vec<_> = if reset {
            (0..source.entries.len()).collect()
        } else {
            std::mem::take(&mut source.dirty).into_iter().collect()
        };
        source.dirty.clear();
        let models: Vec<_> = changed
            .into_iter()
            .map(|i| {
                let entry = &source.entries[i];
                file_model(
                    entry,
                    &files[entry.file],
                    source.loaded.get(entry.id.as_ref()).map(Arc::as_ref),
                )
            })
            .collect();
        let totals = reset.then(|| {
            models
                .iter()
                .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions))
        });
        let presentation = reset.then(|| match &source.tab {
            ReviewTab::File { .. } => (None, 0),
            ReviewTab::Changes { path, staged, .. } => (
                path.as_ref().and_then(|p| {
                    entries(files, *staged)
                        .into_iter()
                        .find(|(_, kind, i)| {
                            let f = &files[*i];
                            staged.is_none_or(|s| s == *kind) && (&f.path == p || &f.relative == p)
                        })
                        .map(|(id, _, _)| SharedString::from(id))
                }),
                if staged.is_some() {
                    models.len()
                } else {
                    files.len()
                },
            ),
            ReviewTab::Commit { .. } => (None, models.len()),
            ReviewTab::Session { path, .. } => (
                path.as_ref().and_then(|p| {
                    files
                        .iter()
                        .find(|f| &f.path == p)
                        .map(|f| SharedString::from(f.relative.clone()))
                }),
                models.len(),
            ),
        });
        source.view.update(cx, |view, cx| {
            if let Some((focus, file_count)) = presentation {
                view.totals = totals;
                view.file_count = Some(file_count);
                view.set_files(models, cx);
                view.focus_file(focus, cx);
            } else {
                view.update_files(models, cx);
            }
        });
        cx.notify();
    }

    fn diff_event(&mut self, ix: usize, event: &DiffEvent, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        let source = &self.sources[ix];
        let ReviewTab::Changes { cwd, .. } = &source.tab else {
            if let DiffEvent::Comment(text) = event {
                cx.emit(ReviewEvent::Comment(text.clone()));
            }
            return;
        };
        let cwd = PathBuf::from(cwd);
        let (id, work): (SharedString, Box<dyn FnOnce() -> Result<(), String> + Send>) = match event
        {
            DiffEvent::Comment(text) => {
                cx.emit(ReviewEvent::Comment(text.clone()));
                return;
            }
            DiffEvent::StageFile(id) | DiffEvent::DiscardFile(id) => {
                let Some(relative) = id.strip_prefix("unstaged:").map(str::to_owned) else {
                    return;
                };
                let stage = matches!(event, DiffEvent::StageFile(_));
                (
                    id.clone(),
                    Box::new(move || {
                        if stage {
                            scm::stage_file(&cwd, &relative)
                        } else {
                            scm::discard_file(&cwd, &relative)
                        }
                    }),
                )
            }
            DiffEvent::StageHunk(id, chunk) => {
                let Some(relative) = id.strip_prefix("unstaged:").map(str::to_owned) else {
                    return;
                };
                let Some(loaded) = source.loaded.get(id.as_ref()) else {
                    return;
                };
                // The same diff the view showed, so the same hunk is staged.
                let Some(next) =
                    diff::stage_chunk(&loaded.diff.original, &loaded.diff.current, *chunk)
                else {
                    return;
                };
                (
                    id.clone(),
                    Box::new(move || scm::stage_contents(&cwd, &relative, next.as_bytes())),
                )
            }
        };
        let view = source.view.clone();
        view.update(cx, |v, cx| {
            v.busy_id = Some(id.clone());
            cx.notify();
        });
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                view.update(cx, |v, cx| {
                    v.busy_id = None;
                    cx.notify();
                });
                this.git.update(cx, |git, cx| git.changed(cx));
                if let Err(error) = result {
                    log_error(&error);
                }
            });
        })
        .detach();
    }

    fn open_tab_menu(
        &mut self,
        ix: usize,
        at: Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = vec![
            MenuAction::new("close", "Close").into(),
            MenuAction::new("close-others", "Close Others")
                .disabled(self.sources.len() < 2)
                .into(),
        ];
        let menu = cx.new(|cx| Menu::new("File tab actions", items, window, cx));
        let events = cx.subscribe(&menu, move |this, _, event: &MenuEvent, cx| {
            this.menu = None;
            if let MenuEvent::Picked(id, _) = event {
                match id.as_ref() {
                    "close" => this.close_tab(ix, cx),
                    "close-others" => this.close_others(ix, cx),
                    _ => {}
                }
            }
            cx.notify();
        });
        self.menu = Some((ix, at, menu, events));
        cx.notify();
    }

    /// SurfaceTabs: `h-9` strip of `w-56` tabs with a close button each.
    fn tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let animate = cx
            .try_global::<crate::settings::Preferences>()
            .is_some_and(|p| p.tab_animations)
            && !cx.reduce_motion()
            && !self.tab_drag.as_ref().is_some_and(|d| d.active);
        let keyed = self
            .sources
            .iter()
            .map(|s| (format!("{:?}", s.tab), s.tab.clone()))
            .collect();
        let (slots, moving) = self
            .tab_motion
            .sync(keyed, animate, std::time::Instant::now());
        if moving {
            window.request_animation_frame();
        }
        let mut live: Vec<Option<AnyElement>> =
            self.tab_elements(cx).into_iter().map(Some).collect();
        let keys: Vec<String> = self
            .sources
            .iter()
            .map(|s| format!("{:?}", s.tab))
            .collect();
        // TabWidthMotion: `w-56 min-w-28` eases in from nothing, or out to it.
        let slot = |open: f32| {
            div()
                .h_full()
                .w(px(224. * open))
                .min_w_0()
                .flex_shrink(1.)
                .mr(px(-crate::tab_motion::GAP * (1. - open)))
                .overflow_hidden()
                .flex()
        };
        let elements: Vec<AnyElement> = slots
            .into_iter()
            .filter_map(|s| match s {
                crate::tab_motion::Slot::Live { key, open } => {
                    let ix = keys.iter().position(|k| *k == key)?;
                    let element = live.get_mut(ix)?.take()?;
                    Some(if open >= 1. {
                        element
                    } else {
                        slot(open).child(element).into_any_element()
                    })
                }
                crate::tab_motion::Slot::Closing { ghost, open, .. } => {
                    let label: SharedString = ghost.label().into();
                    Some(
                        slot(open)
                            .child(
                                div()
                                    .w(px(224.))
                                    .h_full()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .h(px(TAB_PLATE_HEIGHT))
                                            .w_full()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .px_2()
                                            .rounded_md()
                                            .text_size(px(13.))
                                            .text_color(content(0.5))
                                            .child(if let ReviewTab::File { .. } = &ghost {
                                                file_type_icon(label.as_ref(), false, false, 14.)
                                            } else {
                                                icon("git-compare", 14.).into_any_element()
                                            })
                                            .child(div().min_w_0().truncate().child(label)),
                                    ),
                            )
                            .into_any_element(),
                    )
                }
            })
            .collect();
        div()
            .h(px(PANE_HEADER_HEIGHT))
            .debug_selector(|| "review-tab-strip".into())
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(stroke())
            .when(self.pane_draggable, |d| {
                d.child(
                    div()
                        .id("drag-file-pane")
                        .debug_selector(|| "drag-file-pane".into())
                        .role(Role::Button)
                        .aria_label("Drag to reorder pane")
                        .w(px(20.))
                        .h(px(TAB_PLATE_HEIGHT))
                        .ml(px(6.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .cursor(CursorStyle::OpenHand)
                        .text_color(content(0.35))
                        .hover(|s| s.bg(content(0.05)).text_color(content(0.7)))
                        .child(icon("grip-vertical", 14.))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_, event: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                cx.emit(ReviewEvent::DragPane(event.position));
                            }),
                        ),
                )
            })
            .child(
                div()
                    .id("review-tabs")
                    .role(Role::TabList)
                    .aria_label("Files")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .pl(px(if self.pane_draggable { 2. } else { 6. }))
                    .pr(px(10.))
                    // Every tab fits, sharing the strip however many there are.
                    .overflow_hidden()
                    .track_scroll(&self.tab_scroll)
                    .children(elements),
            )
    }
    fn tab_elements(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.sources
            .iter()
            .enumerate()
            .map(|(ix, source)| {
                let active = ix == self.state.active;
                let hovered = self.hovered_tab == Some(ix);
                let preview = matches!(&source.tab, ReviewTab::File { path, .. } if self.preview_path.as_ref() == Some(path));
                let label: SharedString = source.tab.label().into();
                div()
                    .id(("review-tab", ix))
                    .relative()
                    .left(self.tab_shift(ix))
                    .debug_selector(move || format!("file-tab-{ix}"))
                    .h_full()
                    .w(px(224.))
                    .min_w_0()
                    .flex_shrink(1.)
                    .flex()
                    .items_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.start_tab_drag(ix, event, cx);
                            this.focus_active(window, cx);
                        }),
                    )
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        let next = if *hovering {
                            Some(ix)
                        } else if this.hovered_tab == Some(ix) {
                            None
                        } else {
                            this.hovered_tab
                        };
                        if next != this.hovered_tab {
                            this.hovered_tab = next;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, _, cx| this.close_tab(ix, cx)),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.select(ix, cx);
                            this.open_tab_menu(ix, event.position, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .id(("review-tab-button", ix))
                            .role(Role::Tab)
                            .aria_label(label.clone())
                            .aria_selected(active)
                            .relative()
                            .h(px(TAB_PLATE_HEIGHT))
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px_2()
                            .pr(px(28.))
                            .rounded_md()
                            .text_size(px(13.))
                            .map(|d| {
                                if active {
                                    d.bg(selection()).text_color(content(1.))
                                } else if hovered {
                                    d.bg(content(0.05)).text_color(content(1.))
                                } else {
                                    d.text_color(content(0.5))
                                }
                            })
                            .child(if let ReviewTab::File { .. } = &source.tab {
                                file_type_icon(label.as_ref(), false, false, 14.)
                            } else {
                                icon("git-compare", 14.)
                                    .text_color(if active || hovered {
                                        content(1.)
                                    } else {
                                        content(0.5)
                                    })
                                    .into_any_element()
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .when(preview, |d| d.italic())
                                    .child(label.clone()),
                            )
                            .when(
                                source.editor.as_ref().is_some_and(|e| e.read(cx).dirty),
                                |d| d.child(div().size(px(6.)).rounded_full().bg(content(0.7))),
                            )
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                                if std::mem::take(&mut this.skip_tab_click) {
                                    return;
                                }
                                if preview && event.click_count() == 2 {
                                    this.preview_path = None;
                                    cx.notify();
                                }
                                this.select(ix, cx);
                                this.focus_active(window, cx);
                            })),
                    )
                    .child(
                        div()
                            .id(("review-tab-close", ix))
                            .role(Role::Button)
                            .aria_label(SharedString::from(format!("Close {label}")))
                            .absolute()
                            .right_1()
                            .top(gpui::relative(0.5))
                            .mt(px(-10.))
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .when(!(active || hovered), |d| d.opacity(0.))
                            .hover(|s| s.bg(content(0.1)))
                            .child(icon("x", 12.))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.close_tab(ix, cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

fn log_error(error: &str) {
    eprintln!("monocode: review action failed: {error}");
}

/// Polls every future to completion; the workers share one queue.
async fn futures_join_all(workers: Vec<impl std::future::Future<Output = ()>>) {
    let mut workers: Vec<_> = workers.into_iter().map(Box::pin).collect();
    std::future::poll_fn(move |cx| {
        workers.retain_mut(|w| w.as_mut().poll(cx).is_pending());
        if workers.is_empty() {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await
}

impl Render for ReviewPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.retiring_focus)
            .iter()
            .any(|handle| handle.contains_focused(window, cx))
        {
            window.focus(&self.focus, cx);
        }
        self.prompt_close(window, cx);
        // PaneTree's reveal: the files fade in once the pane has arrived.
        if cx.reduce_motion() {
            self.revealed = None;
        }
        let fade = match &self.revealed {
            Some(reveal) => {
                let (opacity, done) = reveal.opacity(std::time::Instant::now());
                if !done {
                    window.request_animation_frame();
                } else {
                    self.revealed = None;
                }
                opacity
            }
            None => 1.,
        };
        let body: AnyElement = match self.sources.get(self.state.active) {
            None => div().into_any_element(),
            Some(_) if self.deferred => div().into_any_element(),
            Some(source) if source.editor_mode && source.editor.is_some() => {
                let editor = source.editor.as_ref().unwrap();
                if self.focus.is_focused(window) {
                    window.focus(&editor.focus_handle(cx), cx);
                }
                editor.clone().into_any_element()
            }
            Some(source) => {
                let cwd = match &source.tab {
                    ReviewTab::File { cwd, .. }
                    | ReviewTab::Changes { cwd, .. }
                    | ReviewTab::Commit { cwd, .. }
                    | ReviewTab::Session { cwd, .. } => cwd,
                };
                if cwd.is_empty() || cwd == "~" {
                    centered()
                        .text_size(px(13.))
                        .text_color(content(0.45))
                        .child("No project folder")
                        .into_any_element()
                } else if let Some(error) = &source.error {
                    centered()
                        .p_6()
                        .flex_col()
                        .text_center()
                        .child(icon("circle-alert", 20.).mb_3().text_color(rgb(RED_400)))
                        .child(div().text_size(px(13.)).text_color(content(1.)).child(
                            match source.tab {
                                ReviewTab::File { .. } => "Couldn’t open file",
                                ReviewTab::Changes { .. } | ReviewTab::Session { .. } => {
                                    "Couldn’t load changes"
                                }
                                ReviewTab::Commit { .. } => "Couldn’t load commit",
                            },
                        ))
                        .child(
                            div()
                                .mt_1()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child(error.clone()),
                        )
                        .into_any_element()
                } else if source.files.is_none() {
                    centered()
                        .child(spinner("review-loading", "loader", 16.))
                        .into_any_element()
                } else {
                    if self.focus.is_focused(window) {
                        window.focus(&source.view.focus_handle(cx), cx);
                    }
                    source.view.clone().into_any_element()
                }
            }
        };
        let menu = self
            .menu
            .as_ref()
            .map(|(_, at, menu, _)| menu_popover(menu, Anchor::Point(*at), cx).gap(0.));
        div()
            .id("review-pane")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .on_mouse_move(
                cx.listener(|this, event: &MouseMoveEvent, _, cx| this.move_tab_drag(event, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_tab_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_tab_drag(cx)),
            )
            .font_weight(FontWeight::NORMAL)
            .child(self.tabs(window, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .opacity(fade)
                    .child(body),
            )
            .children(menu)
    }
}

fn centered() -> gpui::Div {
    div().size_full().flex().items_center().justify_center()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Repo(PathBuf);
    impl Repo {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "monocode-review-cache-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&root).unwrap();
            let repo = Self(root);
            repo.git(&["init"]);
            for name in ["a.txt", "b.txt"] {
                std::fs::write(repo.0.join(name), "base\n").unwrap();
            }
            repo.git(&["add", "."]);
            repo.git(&["commit", "-m", "base"]);
            for name in ["a.txt", "b.txt"] {
                std::fs::write(repo.0.join(name), "edit1\n").unwrap();
            }
            repo
        }
        fn git(&self, args: &[&str]) {
            let result = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(&self.0)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        fn cwd(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn loaded_diff(original: &str, current: &str) -> Arc<Loaded> {
        let unified = diff::build_unified_file(original, current, diff::CONTEXT_DEFAULT);
        Arc::new(Loaded {
            version: None,
            diff: Arc::new(FileDiff {
                original: original.into(),
                current: current.into(),
                ..Default::default()
            }),
            unified: Some(Arc::new(unified.blocks)),
            additions: unified.additions,
            deletions: unified.deletions,
            error: None,
        })
    }

    #[gpui::test]
    fn streamed_publications_keep_totals_and_file_count_across_manifest_changes(
        cx: &mut gpui::TestAppContext,
    ) {
        let git = cx.new(|cx| ProjectGit::new("~".into(), cx));
        let pane = cx.new(|cx| {
            ReviewPane::new(
                git,
                ReviewState {
                    tabs: vec![ReviewTab::Changes {
                        cwd: "~".into(),
                        path: None,
                        staged: None,
                    }],
                    ..Default::default()
                },
                cx,
            )
        });
        pane.update(cx, |pane, cx| {
            pane.sources[0].set_files(vec![file("a", true, true), file("b", false, true)]);
            pane.publish(cx);
            assert_eq!(pane.sources[0].view.read(cx).totals, Some((0, 0)));
            let a = loaded_diff("one\n", "two\nthree\nfour\n");
            let b = loaded_diff("one\ntwo\n", "three\nfour\nfive\nsix\n");
            pane.sources[0].set_loaded("staged:a".into(), a);
            pane.publish(cx);
            assert_eq!(pane.sources[0].view.read(cx).totals, Some((3, 1)));
            pane.sources[0].set_loaded("unstaged:b".into(), b.clone());
            pane.publish(cx);
            assert_eq!(pane.sources[0].view.read(cx).totals, Some((7, 3)));
            assert_eq!(pane.sources[0].view.read(cx).file_count, Some(2));
            pane.sources[0].set_loaded("unstaged:b".into(), b);
            pane.publish(cx);
            assert_eq!(pane.sources[0].view.read(cx).totals, Some((7, 3)));
            pane.sources[0].set_files(vec![file("b", false, true)]);
            pane.publish(cx);
            assert_eq!(pane.sources[0].view.read(cx).totals, Some((4, 2)));
            assert_eq!(pane.sources[0].view.read(cx).file_count, Some(1));
            pane.sources[0].set_loaded("unstaged:b".into(), loaded_diff("same\n", "same\n"));
            pane.publish(cx);
            assert_eq!(pane.sources[0].view.read(cx).totals, Some((0, 0)));
        });
    }

    #[gpui::test]
    #[ignore]
    fn review_publication_timing(cx: &mut gpui::TestAppContext) {
        let files: Vec<_> = (0..2_000)
            .map(|i| file(&format!("file-{i}.rs"), false, true))
            .collect();
        let text = "added line\n".repeat(250);
        let unified = diff::build_unified_file("", &text, diff::CONTEXT_DEFAULT);
        let loaded = Arc::new(Loaded {
            version: None,
            diff: Arc::new(FileDiff::default()),
            unified: Some(Arc::new(unified.blocks)),
            additions: unified.additions,
            deletions: unified.deletions,
            error: None,
        });
        for trial in 0..5 {
            let git = cx.new(|cx| ProjectGit::new("~".into(), cx));
            let pane = cx.new(|cx| {
                ReviewPane::new(
                    git,
                    ReviewState {
                        tabs: vec![ReviewTab::Commit {
                            cwd: "~".into(),
                            sha: "benchmark".into(),
                            short_sha: "bench".into(),
                            subject: "Benchmark".into(),
                        }],
                        ..Default::default()
                    },
                    cx,
                )
            });
            pane.update(cx, |pane, cx| {
                pane.sources[0].set_files(files.clone());
                pane.publish(cx);
            });
            let start = std::time::Instant::now();
            let mut longest = std::time::Duration::ZERO;
            for batch in (0..files.len()).step_by(4) {
                let tick = std::time::Instant::now();
                pane.update(cx, |pane, cx| {
                    for file in &files[batch..batch + 4] {
                        pane.sources[0].set_loaded(file.relative.clone(), loaded.clone());
                    }
                    pane.publish(cx);
                });
                longest = longest.max(tick.elapsed());
            }
            let elapsed = start.elapsed();
            pane.update(cx, |pane, cx| {
                assert_eq!(pane.sources[0].loaded.len(), 2_000);
                let view = pane.sources[0].view.read(cx);
                assert_eq!(view.totals, Some((500_000, 0)));
                assert_eq!(view.file_count, Some(2_000));
            });
            eprintln!(
                "review-publication trial={trial} total_ms={:.3} longest_update_ms={:.3}",
                elapsed.as_secs_f64() * 1000.,
                longest.as_secs_f64() * 1000.
            );
        }
    }

    #[gpui::test]
    fn review_focus_follows_the_visible_body_after_loading(cx: &mut gpui::TestAppContext) {
        let repo = Repo::new();
        let cwd = repo.cwd();
        let preview = repo.0.join("preview.md");
        std::fs::write(&preview, "# Preview\n").unwrap();
        let window = cx.update(|cx| {
            crate::input::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                let git = cx.new(|cx| ProjectGit::new(cwd.clone(), cx));
                cx.new(|cx| ReviewPane::new(git, Default::default(), cx))
            })
            .unwrap()
        });
        window
            .update(cx, |pane, w, cx| {
                pane.set_deferred(true, cx);
                pane.open(
                    ReviewTab::Changes {
                        cwd: cwd.clone(),
                        path: None,
                        staged: None,
                    },
                    cx,
                );
                pane.set_visible(true, cx);
                pane.focus_active(w, cx);
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        window
            .update(visual, |pane, w, cx| {
                assert!(pane.focus.is_focused(w), "the deferred pane retains focus");
                pane.set_deferred(false, cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |pane, w, cx| {
                assert!(pane.sources[0].view.focus_handle(cx).is_focused(w));
            })
            .unwrap();
        for path in [repo.0.join("a.txt"), preview] {
            window
                .update(visual, |pane, w, cx| {
                    pane.open(
                        ReviewTab::File {
                            cwd: cwd.clone(),
                            path: path.to_string_lossy().into_owned(),
                        },
                        cx,
                    );
                    pane.focus_active(w, cx);
                })
                .unwrap();
            visual.run_until_parked();
            window
                .update(visual, |pane, w, cx| {
                    let editor = pane.active_editor().unwrap();
                    assert!(editor.focus_handle(cx).contains_focused(w, cx));
                    if path.extension().unwrap() == "txt" {
                        assert!(editor.read(cx).input.focus_handle(cx).is_focused(w));
                    } else {
                        assert!(editor.focus_handle(cx).is_focused(w));
                    }
                })
                .unwrap();
        }
        // Closing a cached preview must not leave focus in its hidden editor.
        window
            .update(visual, |pane, _, cx| pane.close_active(cx))
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |pane, w, cx| {
                let editor = pane.active_editor().unwrap();
                assert!(editor.focus_handle(cx).contains_focused(w, cx));
                assert!(editor.read(cx).input.focus_handle(cx).is_focused(w));
            })
            .unwrap();
    }

    #[gpui::test]
    fn entering_panes_defer_file_reads_and_diff_construction(cx: &mut gpui::TestAppContext) {
        let repo = Repo::new();
        let cwd = repo.cwd();
        let git = cx.new(|cx| ProjectGit::new(cwd.clone(), cx));
        let pane = cx.new(|cx| ReviewPane::new(git, Default::default(), cx));
        pane.update(cx, |pane, cx| {
            pane.set_deferred(true, cx);
            pane.open(
                ReviewTab::Changes {
                    cwd: cwd.clone(),
                    path: None,
                    staged: None,
                },
                cx,
            );
            pane.set_visible(true, cx);
        });
        cx.run_until_parked();
        pane.read_with(cx, |pane, _| {
            assert!(!pane.sources[0].loading);
            assert!(pane.sources[0].files.is_none());
            assert!(pane.sources[0].loaded.is_empty());
        });
        pane.update(cx, |pane, cx| pane.set_deferred(false, cx));
        cx.run_until_parked();
        pane.read_with(cx, |pane, _| assert!(pane.sources[0].complete));
        pane.update(cx, |pane, cx| {
            pane.set_deferred(true, cx);
            pane.open(
                ReviewTab::File {
                    cwd,
                    path: repo.0.join("a.txt").to_string_lossy().into_owned(),
                },
                cx,
            );
        });
        cx.run_until_parked();
        let input = pane.read_with(cx, |pane, cx| {
            pane.active_editor().unwrap().read(cx).input.clone()
        });
        assert!(input.read_with(cx, |input, _| input.content.is_empty()));
        pane.update(cx, |pane, cx| pane.set_deferred(false, cx));
        cx.run_until_parked();
        assert_eq!(
            input.read_with(cx, |input, _| input.content.to_string()),
            "edit1\n"
        );
    }

    #[gpui::test]
    fn reopening_diffs_reuses_views_and_only_reads_changed_files(cx: &mut gpui::TestAppContext) {
        let repo = Repo::new();
        let cwd = repo.cwd();
        let git = cx.new(|cx| ProjectGit::new(cwd.clone(), cx));
        cx.run_until_parked();
        let tab = ReviewTab::Changes {
            cwd: cwd.clone(),
            path: None,
            staged: None,
        };
        let pane = cx.new(|cx| ReviewPane::new(git.clone(), Default::default(), cx));
        pane.update(cx, |pane, cx| {
            pane.open(tab.clone(), cx);
            pane.set_visible(true, cx);
        });
        cx.run_until_parked();
        let (view, a, b) = pane.read_with(cx, |pane, _| {
            let source = &pane.sources[0];
            assert!(source.complete);
            (
                source.view.clone(),
                source.loaded["unstaged:a.txt"].clone(),
                source.loaded["unstaged:b.txt"].clone(),
            )
        });
        pane.update(cx, |pane, cx| {
            pane.set_visible(false, cx);
            pane.set_visible(true, cx);
        });
        cx.run_until_parked();
        pane.read_with(cx, |pane, _| {
            assert!(Arc::ptr_eq(&a, &pane.sources[0].loaded["unstaged:a.txt"]));
            assert!(Arc::ptr_eq(&b, &pane.sources[0].loaded["unstaged:b.txt"]));
        });
        pane.update(cx, |pane, cx| pane.cache_closed(cx));
        drop(pane);
        let reopened = cx.new(|cx| ReviewPane::new(git.clone(), Default::default(), cx));
        reopened.update(cx, |pane, cx| {
            pane.open(tab.clone(), cx);
            pane.set_visible(true, cx);
        });
        cx.run_until_parked();
        reopened.read_with(cx, |pane, _| {
            assert_eq!(
                view, pane.sources[0].view,
                "viewport and highlighting survive closing"
            );
            assert!(Arc::ptr_eq(&a, &pane.sources[0].loaded["unstaged:a.txt"]));
            assert!(Arc::ptr_eq(&b, &pane.sources[0].loaded["unstaged:b.txt"]));
        });
        reopened.update(cx, |pane, cx| pane.cache_closed(cx));
        drop(reopened);
        std::fs::write(repo.0.join("a.txt"), "edit2\n").unwrap();
        std::fs::write(repo.0.join("new.txt"), "new\n").unwrap();
        let reopened = cx.new(|cx| ReviewPane::new(git, Default::default(), cx));
        reopened.update(cx, |pane, cx| {
            pane.open(tab, cx);
            pane.set_visible(true, cx);
        });
        cx.run_until_parked();
        reopened.read_with(cx, |pane, _| {
            let source = &pane.sources[0];
            assert!(!Arc::ptr_eq(&a, &source.loaded["unstaged:a.txt"]));
            assert_eq!(source.loaded["unstaged:a.txt"].diff.current, "edit2\n");
            assert!(Arc::ptr_eq(&b, &source.loaded["unstaged:b.txt"]));
            assert!(source.loaded.contains_key("unstaged:new.txt"));
        });
    }

    #[gpui::test]
    fn closed_diff_cache_is_scoped_and_bounded(cx: &mut gpui::TestAppContext) {
        let git = cx.new(|cx| ProjectGit::new("~".into(), cx));
        let pane = cx.new(|cx| ReviewPane::new(git.clone(), Default::default(), cx));
        let tab = |sha: String| ReviewTab::Commit {
            cwd: "/r".into(),
            short_sha: sha.clone(),
            sha,
            subject: "Test".into(),
        };
        pane.update(cx, |pane, cx| {
            for i in 0..=MAX_CACHED_SOURCES {
                let mut source = pane.source(tab(i.to_string()), cx);
                source.files = Some(Vec::new());
                git.update(cx, |git, _| git.review_cache.remember(&source));
            }
            git.update(cx, |git, _| {
                assert_eq!(git.review_cache.sources.len(), MAX_CACHED_SOURCES);
                assert!(git.review_cache.take(&tab("0".into())).is_none());
                assert!(git.review_cache.take(&tab("1".into())).is_some());
                assert!(!ReviewCache::matches(
                    &ReviewTab::Changes {
                        cwd: "/r".into(),
                        path: None,
                        staged: Some(true)
                    },
                    &ReviewTab::Changes {
                        cwd: "/r".into(),
                        path: None,
                        staged: Some(false)
                    }
                ));
                assert!(!ReviewCache::matches(
                    &tab("2".into()),
                    &ReviewTab::Commit {
                        cwd: "/other-worktree".into(),
                        sha: "2".into(),
                        short_sha: "2".into(),
                        subject: "Test".into()
                    }
                ));
            });
            let mut source = pane.source(tab("huge".into()), cx);
            source.files = Some(Vec::new());
            source.loaded.insert(
                "huge".into(),
                Arc::new(Loaded {
                    version: None,
                    diff: Arc::new(FileDiff {
                        current: "x".repeat(MAX_CACHED_TEXT_BYTES + 1),
                        ..Default::default()
                    }),
                    unified: None,
                    additions: 0,
                    deletions: 0,
                    error: None,
                }),
            );
            git.update(cx, |git, _| {
                git.review_cache.remember(&source);
                assert!(git.review_cache.take(&source.tab).is_none());
            });
        });
    }
    #[gpui::test]
    fn file_reviews_are_editable_and_view_switches_preserve_the_unsaved_document(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::EntityInputHandler;
        let repo = Repo::new();
        cx.update(|cx| cx.set_global(crate::settings::Preferences::default()));
        let cwd = repo.cwd();
        let path = repo.0.join("a.txt").to_string_lossy().into_owned();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                let git = cx.new(|cx| ProjectGit::new(cwd.clone(), cx));
                cx.new(|cx| {
                    let mut pane = ReviewPane::new(git, Default::default(), cx);
                    pane.open(
                        ReviewTab::Changes {
                            cwd: cwd.clone(),
                            path: Some(path.clone()),
                            staged: Some(false),
                        },
                        cx,
                    );
                    pane.set_visible(true, cx);
                    pane
                })
            })
            .unwrap()
        });
        cx.run_until_parked();
        let editor = window
            .update(cx, |pane, w, cx| {
                assert!(pane.sources[0].editor_mode);
                let editor = pane.active_editor().unwrap();
                assert!(editor.read(cx).show_diff);
                editor
                    .read(cx)
                    .input
                    .clone()
                    .update(cx, |i, cx| i.replace_text_in_range(None, "draft ", w, cx));
                editor
            })
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            cx.update_global::<crate::settings::Preferences, _>(|p, _| {
                p.diff_view = "unified".into()
            })
        });
        cx.run_until_parked();
        window
            .update(cx, |pane, _, cx| {
                assert!(!pane.sources[0].editor_mode);
                assert!(editor.read(cx).dirty);
            })
            .unwrap();
        cx.update(|cx| {
            cx.update_global::<crate::settings::Preferences, _>(|p, _| {
                p.diff_view = "editor".into()
            })
        });
        cx.run_until_parked();
        window
            .update(cx, |pane, _, cx| {
                assert!(pane.sources[0].editor_mode);
                assert_eq!(pane.active_editor().unwrap(), editor);
                assert!(editor.read(cx).input.read(cx).content.contains("draft"));
                pane.open(
                    ReviewTab::Changes {
                        cwd,
                        path: Some(repo.0.join("b.txt").to_string_lossy().into_owned()),
                        staged: Some(false),
                    },
                    cx,
                );
                assert_eq!(pane.sources.len(), 2);
            })
            .unwrap();
    }
    fn file(relative: &str, staged: bool, unstaged: bool) -> ChangedFile {
        ChangedFile {
            path: format!("/r/{relative}"),
            relative: relative.into(),
            status: "modified".into(),
            additions: 1,
            deletions: 0,
            staged,
            unstaged,
        }
    }
    #[test]
    fn entries_put_staged_first_and_scope_to_one_side() {
        let files = [file("a", true, true), file("b", false, true)];
        let ids: Vec<_> = entries(&files, None).into_iter().map(|e| e.0).collect();
        assert_eq!(ids, ["staged:a", "unstaged:a", "unstaged:b"]);
        assert_eq!(entries(&files, Some(true)).len(), 1);
        let changes = ReviewTab::Changes {
            cwd: "/r".into(),
            path: None,
            staged: Some(true),
        };
        assert_eq!(changes.label(), "Staged Changes");
        assert!(!changes.same_tab(&ReviewTab::Changes {
            cwd: "/r".into(),
            path: Some("a".into()),
            staged: None
        }));
    }
}
