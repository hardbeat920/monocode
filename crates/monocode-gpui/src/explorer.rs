//! FileTree.tsx: the Explorer header, project root row and lazily listed
//! tree with Git colours, inline create/rename, clipboard, context menu,
//! dropped files and VS Code-style keyboard navigation. Directory reads and
//! every file operation run off the UI thread.
use crate::tooltip::Tip;
use crate::{
    file_icons::file_type_icon,
    git_status::{GitChanged, ProjectGit},
    input::{self, Input},
    popover::{Anchor, Menu, MenuAction, MenuEvent, MenuItem, menu_popover},
    project_search::{ProjectSearch, SearchEvent},
    right_panel::*,
    theme::*,
};
use gpui::{
    AnyElement, App, ClipboardEntry, ClipboardItem, Context, Entity, EventEmitter, ExternalPaths,
    FocusHandle, Focusable, FontWeight, Hsla, KeyBinding, KeyDownEvent, ListAlignment, ListState,
    MouseButton, MouseDownEvent, Point, PromptLevel, Role, SharedString, StyleRefinement,
    Subscription, Window, actions, div, list, prelude::*, px, rgb,
};
use monocode_core::files::{self, DirEntry, NameIssueKind, Severity};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::Instant,
};

const MOD: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl+"
};
const SHIFT: &str = if cfg!(target_os = "macos") {
    "⇧"
} else {
    "Shift+"
};
const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(target_os = "windows") {
    "Reveal in File Explorer"
} else {
    "Open Containing Folder"
};
const ROW: f32 = 30.;
const ENTRY_FONT_SIZE: f32 = 13.;

/// A reference to an existing file, distinct from files copied into the tree.
#[derive(Clone)]
pub(crate) struct DraggedFile(pub PathBuf);

impl Render for DraggedFile {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let name = self
            .0
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .px_2()
            .py_1p5()
            .rounded_md()
            .border_1()
            .border_color(stroke())
            .bg(pane_fill())
            .text_size(px(12.))
            .text_color(content(1.))
            .child(file_type_icon(&name, false, false, 16.))
            .child(div().max_w(px(240.)).truncate().child(name))
    }
}

actions!(
    explorer,
    [
        Down,
        Up,
        PageDown,
        PageUp,
        First,
        Last,
        Expand,
        Collapse,
        Activate,
        OpenSelected,
        Rename,
        Delete,
        ClearCut,
        CopyEntry,
        CutEntry,
        PasteEntry,
        CopyPath,
        CancelName
    ]
);

pub(crate) fn bind(cx: &mut App) {
    let m = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    let c = Some("Explorer");
    cx.bind_keys([
        KeyBinding::new("down", Down, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("home", First, c),
        KeyBinding::new("end", Last, c),
        KeyBinding::new("right", Expand, c),
        KeyBinding::new("left", Collapse, c),
        KeyBinding::new("enter", Activate, c),
        KeyBinding::new("cmd-down", OpenSelected, c),
        KeyBinding::new("f2", Rename, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("backspace", Delete, c),
        KeyBinding::new("escape", ClearCut, c),
        KeyBinding::new(&format!("{m}-c"), CopyEntry, c),
        KeyBinding::new(&format!("{m}-x"), CutEntry, c),
        KeyBinding::new(&format!("{m}-v"), PasteEntry, c),
        KeyBinding::new(&format!("{m}-shift-c"), CopyPath, c),
        KeyBinding::new("escape", CancelName, Some("ExplorerName")),
    ]);
}

#[derive(Clone, PartialEq)]
struct Clip {
    cut: bool,
    path: String,
    is_dir: bool,
}
#[derive(Clone)]
struct Target {
    path: String,
    is_dir: bool,
    root: bool,
}
#[derive(Clone, PartialEq)]
enum NameMode {
    Create {
        id: u64,
        parent: String,
        is_dir: bool,
    },
    Rename {
        path: String,
        is_dir: bool,
    },
}
#[derive(Clone)]
enum Line {
    Note {
        depth: usize,
        text: String,
    },
    Entry {
        entry: DirEntry,
        depth: usize,
        open: bool,
    },
    Name {
        depth: usize,
        is_dir: bool,
    },
    Issue {
        depth: usize,
        text: Vec<(String, bool)>,
        error: bool,
    },
}
struct OpenMenu {
    target: Target,
    anchor: Point<gpui::Pixels>,
    menu: Entity<Menu>,
    _events: Subscription,
}
#[derive(Default, Clone)]
struct Saved {
    expanded: HashSet<String>,
    selected: Option<String>,
}

pub(crate) struct Explorer {
    git: Entity<ProjectGit>,
    cwd: String,
    enabled: bool,
    show_excluded: bool,
    expanded: HashSet<String>,
    selected: Option<String>,
    /// loadExpanded / loadSelected: remembered per project for the session.
    saved: HashMap<String, Saved>,
    /// listCachedDir: the last listing of each folder, or its error.
    dirs: HashMap<String, Result<Vec<DirEntry>, String>>,
    reading: HashSet<String>,
    naming: Option<NameMode>,
    name_input: Entity<Input>,
    name_attempted: bool,
    name_busy: bool,
    name_error: Option<String>,
    clip: Option<Clip>,
    menu: Option<OpenMenu>,
    search: Option<Entity<ProjectSearch>>,
    search_events: Option<Subscription>,
    drag_over: Option<String>,
    op_error: Option<String>,
    lines: Vec<Line>,
    list: ListState,
    focus: FocusHandle,
    keyboard: bool,
    typeahead: (String, Option<Instant>),
    next_id: u64,
    _subscriptions: Vec<Subscription>,
}

fn parent_of(path: &str) -> String {
    files::parent_path(path)
}
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub(crate) struct OpenFile(pub String);
impl EventEmitter<OpenFile> for Explorer {}
/// Shared file-action toast state. Name validation remains local to its field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileActionError(pub Option<String>);
impl EventEmitter<FileActionError> for Explorer {}
pub(crate) struct OpenSearchMatch {
    pub cwd: String,
    pub matched: monocode_core::search::project::SearchMatch,
    pub pin: bool,
}
impl EventEmitter<OpenSearchMatch> for Explorer {}

impl Explorer {
    pub(crate) fn new(
        git: Entity<ProjectGit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input = cx.new(|cx| {
            let mut input = Input::field(
                "Type file name. Press Enter to confirm or Escape to cancel.",
                "",
                content(0.35),
                cx,
            );
            input.set_font_size(ENTRY_FONT_SIZE);
            input
        });
        let name_focus = name_input.read(cx).focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe(&git, |this, _, _: &GitChanged, cx| this.git_changed(cx)),
            cx.observe(&git, |this, _, cx| {
                // Git colours follow every status read.
                if this.enabled {
                    cx.notify();
                }
                let _ = this;
            }),
            cx.subscribe_in(
                &name_input,
                window,
                |this, _, _: &input::Submitted, window, cx| this.finish_name(true, window, cx),
            ),
            cx.subscribe(&name_input, |this, _, _: &input::Changed, cx| {
                this.name_error = None;
                this.rebuild(cx);
            }),
            cx.on_blur(&name_focus, window, |this, window, cx| {
                if this.naming.is_some() && !this.name_busy {
                    let ok = this.issue(cx).is_none_or(|i| i.severity != Severity::Error);
                    this.finish_name(ok, window, cx);
                }
            }),
        ];
        Self {
            git,
            cwd: String::new(),
            enabled: false,
            show_excluded: false,
            expanded: HashSet::new(),
            selected: None,
            saved: HashMap::new(),
            dirs: HashMap::new(),
            reading: HashSet::new(),
            naming: None,
            name_input,
            name_attempted: false,
            name_busy: false,
            name_error: None,
            clip: None,
            menu: None,
            search: None,
            search_events: None,
            drag_over: None,
            op_error: None,
            lines: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(300.)),
            focus: cx.focus_handle(),
            keyboard: false,
            typeahead: (String::new(), None),
            next_id: 0,
            _subscriptions: subscriptions,
        }
    }
    pub(crate) fn set_show_excluded(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_excluded != show {
            self.show_excluded = show;
            self.rebuild(cx);
        }
    }
    pub(crate) fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let was = self.enabled;
        self.enabled = enabled;
        if enabled {
            self.sync_project(cx);
            if !was {
                self.refresh_loaded(cx);
            }
        }
        if let Some(search) = &self.search {
            search.update(cx, |search, cx| search.set_active(enabled, cx));
        }
    }
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_project(cx);
        self.menu = None;
        if let Some(search) = &self.search {
            search.update(cx, |search, cx| search.focus(window, cx));
            return;
        }
        let search = cx.new(|cx| ProjectSearch::new(self.cwd.clone(), self.enabled, cx));
        self.search_events = Some(cx.subscribe_in(
            &search,
            window,
            |this, _, event: &SearchEvent, window, cx| match event {
                SearchEvent::Close => {
                    this.close_search(window, cx);
                }
                SearchEvent::Open { cwd, matched, pin } => cx.emit(OpenSearchMatch {
                    cwd: cwd.clone(),
                    matched: matched.clone(),
                    pin: *pin,
                }),
            },
        ));
        search.update(cx, |search, cx| search.focus(window, cx));
        self.search = Some(search);
        cx.notify();
    }
    /// Escape also reaches this from the editor after opening a search result.
    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(search) = &self.search else {
            return false;
        };
        if search.read(cx).is_composing(cx) {
            return true;
        }
        self.search = None;
        self.search_events = None;
        window.focus(&self.focus, cx);
        cx.notify();
        true
    }
    /// notifyDirsChanged on window focus.
    pub(crate) fn resume(&mut self, cx: &mut Context<Self>) {
        if self.enabled {
            self.refresh_loaded(cx);
        }
    }
    fn git_changed(&mut self, cx: &mut Context<Self>) {
        if self.enabled {
            if !self.sync_project(cx) {
                self.refresh_loaded(cx);
            }
        } else if self.git.read(cx).root != self.cwd {
            // Keep the hidden tree intact; rebuild it when Files is shown.
        }
    }
    fn sync_project(&mut self, cx: &mut Context<Self>) -> bool {
        let root = self.git.read(cx).root.clone();
        if root == self.cwd {
            return false;
        }
        if !self.cwd.is_empty() {
            self.saved.insert(
                self.cwd.clone(),
                Saved {
                    expanded: std::mem::take(&mut self.expanded),
                    selected: self.selected.take(),
                },
            );
        }
        self.cwd = root.clone();
        if let Some(search) = &self.search {
            search.update(cx, |search, cx| search.set_root(root.clone(), cx));
        }
        let saved = self.saved.get(&root).cloned().unwrap_or_else(|| Saved {
            expanded: HashSet::from([root.clone()]),
            selected: None,
        });
        self.expanded = saved.expanded;
        self.selected = saved.selected;
        self.naming = None;
        self.clip = None;
        self.menu = None;
        self.set_operation_error(None, cx);
        self.list.scroll_to(gpui::ListOffset::default());
        self.read_dir(root, cx);
        self.rebuild(cx);
        true
    }
    fn available(&self) -> bool {
        !self.cwd.is_empty() && self.cwd != "~"
    }
    fn read_dir(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.reading.insert(path.clone()) {
            return;
        }
        let dir = PathBuf::from(&path);
        let work = cx
            .background_executor()
            .spawn(async move { files::list_dir(&dir) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.reading.remove(&path);
                if this.dirs.get(&path) != Some(&result) {
                    this.dirs.insert(path, result);
                    this.rebuild(cx);
                }
            });
        })
        .detach();
    }
    /// Rereads every folder shown, keeping the current rows meanwhile.
    fn refresh_loaded(&mut self, cx: &mut Context<Self>) {
        let mut shown: Vec<String> = self
            .expanded
            .iter()
            .filter(|p| *p == &self.cwd || self.dirs.contains_key(&parent_of(p)))
            .cloned()
            .collect();
        if !shown.contains(&self.cwd) {
            shown.push(self.cwd.clone());
        }
        for path in shown {
            self.read_dir(path, cx);
        }
    }
    fn forget(&mut self, path: &str) {
        let prefix = format!("{path}/");
        self.dirs
            .retain(|k, _| k != path && !k.starts_with(&prefix));
    }
    fn is_dir_at(&self, path: &str) -> bool {
        if path == self.cwd {
            return true;
        }
        match self.dirs.get(&parent_of(path)) {
            Some(Ok(entries)) => entries
                .iter()
                .find(|e| e.path == path)
                .map(|e| e.is_dir)
                .unwrap_or_else(|| self.dirs.contains_key(path)),
            _ => self.dirs.contains_key(path),
        }
    }
    fn create_parent(&self, at: Option<&str>) -> String {
        files::create_parent_of(&self.cwd, at, |p| self.is_dir_at(p))
    }
    fn siblings(&self, parent: &str, except: Option<&str>) -> Vec<String> {
        match self.dirs.get(parent) {
            Some(Ok(entries)) => entries
                .iter()
                .map(|e| e.name.clone())
                .filter(|n| Some(n.as_str()) != except)
                .collect(),
            _ => Vec::new(),
        }
    }
    fn issue(&self, cx: &App) -> Option<files::NameIssue> {
        let value = self.name_input.read(cx).content.to_string();
        let siblings = match self.naming.as_ref()? {
            NameMode::Create { parent, .. } => self.siblings(parent, None),
            NameMode::Rename { path, .. } => self.siblings(&parent_of(path), Some(basename(path))),
        };
        files::validate_file_name(&value, &siblings)
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let mut lines = Vec::new();
        if self.available() && self.expanded.contains(&self.cwd) {
            self.push_children(&self.cwd.clone(), 0, &mut lines, cx);
        }
        let old = self.lines.len();
        self.lines = lines;
        self.list.splice(0..old, self.lines.len());
        cx.notify();
    }
    fn push_children(&self, parent: &str, depth: usize, lines: &mut Vec<Line>, cx: &App) {
        let listing = self.dirs.get(parent);
        let creating = match &self.naming {
            Some(NameMode::Create {
                parent: p, is_dir, ..
            }) if p == parent => Some(*is_dir),
            _ => None,
        };
        if let Some(Err(error)) = listing {
            lines.push(Line::Note {
                depth,
                text: error.clone(),
            });
        }
        if creating == Some(true) {
            self.push_name(depth, true, lines, cx);
        }
        if listing.is_none() {
            lines.push(Line::Note {
                depth,
                text: "…".into(),
            });
        }
        let entries: Vec<&DirEntry> = match listing {
            Some(Ok(entries)) => entries
                .iter()
                .filter(|e| self.show_excluded || !e.ignored)
                .collect(),
            _ => Vec::new(),
        };
        for entry in entries.iter().filter(|e| e.is_dir) {
            self.push_entry(entry, depth, lines, cx);
        }
        if creating == Some(false) {
            self.push_name(depth, false, lines, cx);
        }
        for entry in entries.iter().filter(|e| !e.is_dir) {
            self.push_entry(entry, depth, lines, cx);
        }
    }
    fn push_entry(&self, entry: &DirEntry, depth: usize, lines: &mut Vec<Line>, cx: &App) {
        let open = entry.is_dir && self.expanded.contains(&entry.path);
        if matches!(&self.naming, Some(NameMode::Rename { path, .. }) if path == &entry.path) {
            self.push_name(depth, entry.is_dir, lines, cx);
        } else {
            lines.push(Line::Entry {
                entry: entry.clone(),
                depth,
                open,
            });
        }
        if open {
            self.push_children(&entry.path, depth + 1, lines, cx);
        }
    }
    /// NameRow and its NameIssueView.
    fn push_name(&self, depth: usize, is_dir: bool, lines: &mut Vec<Line>, cx: &App) {
        lines.push(Line::Name { depth, is_dir });
        let value = self.name_input.read(cx).content.to_string();
        let issue = self.issue(cx);
        let show = self.name_error.is_some()
            || issue.as_ref().is_some_and(|i| {
                i.severity == Severity::Warning
                    || (i.kind != NameIssueKind::Empty && !value.is_empty())
                    || (i.kind == NameIssueKind::Empty && self.name_attempted)
            });
        if !show {
            return;
        }
        let text: Vec<(String, bool)> = if let Some(error) = &self.name_error {
            vec![(error.clone(), false)]
        } else if let Some(issue) = &issue {
            match issue.kind {
                NameIssueKind::Empty => {
                    vec![("A file or folder name must be provided.".into(), false)]
                }
                NameIssueKind::Slash => {
                    vec![(
                        "A file or folder name cannot start with a slash.".into(),
                        false,
                    )]
                }
                NameIssueKind::Exists => vec![
                    ("A file or folder ".into(), false),
                    (issue.name.clone(), true),
                    (
                        " already exists at this location. Please choose a different name.".into(),
                        false,
                    ),
                ],
                NameIssueKind::Invalid => vec![
                    ("The name ".into(), false),
                    (issue.name.clone(), true),
                    (
                        " is not valid as a file or folder name. Please choose a different name."
                            .into(),
                        false,
                    ),
                ],
                NameIssueKind::Whitespace => vec![(
                    "Leading or trailing whitespace detected in file or folder name.".into(),
                    false,
                )],
            }
        } else {
            return;
        };
        let error = self.name_error.is_some()
            || issue.as_ref().is_none_or(|i| i.severity == Severity::Error);
        lines.push(Line::Issue { depth, text, error });
    }

    fn select(&mut self, path: String, cx: &mut Context<Self>) {
        self.selected = Some(path);
        cx.notify();
    }
    fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_owned());
            if !self.dirs.contains_key(path) {
                self.read_dir(path.to_owned(), cx);
            }
        }
        self.rebuild(cx);
    }
    fn expand(&mut self, dirs: &[String], cx: &mut Context<Self>) {
        for dir in dirs {
            if self.expanded.insert(dir.clone()) && !self.dirs.contains_key(dir) {
                self.read_dir(dir.clone(), cx);
            }
        }
    }
    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.naming = None;
        self.expanded = HashSet::from([self.cwd.clone()]);
        self.rebuild(cx);
    }

    fn start_create(
        &mut self,
        is_dir: bool,
        at: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parent = self.create_parent(at.as_deref().or(self.selected.as_deref()));
        self.next_id += 1;
        self.expand(&[self.cwd.clone(), parent.clone()], cx);
        self.begin_name(
            NameMode::Create {
                id: self.next_id,
                parent,
                is_dir,
            },
            String::new(),
            window,
            cx,
        );
    }
    fn start_rename(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if path == self.cwd {
            return;
        }
        self.menu = None;
        let is_dir = self.is_dir_at(&path);
        let name = basename(&path).to_owned();
        self.selected = Some(path.clone());
        self.begin_name(NameMode::Rename { path, is_dir }, name.clone(), window, cx);
        // Files select their stem, folders the whole name.
        let end = if is_dir {
            name.len()
        } else {
            name.rfind('.').filter(|&d| d > 0).unwrap_or(name.len())
        };
        self.name_input
            .update(cx, |i, cx| i.select_range(0..end, cx));
    }
    fn begin_name(
        &mut self,
        mode: NameMode,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.naming = Some(mode);
        self.name_attempted = false;
        self.name_busy = false;
        self.name_error = None;
        self.name_input.update(cx, |i, cx| i.set_text(initial, cx));
        let focus = self.name_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.rebuild(cx);
        if let Some(ix) = self
            .lines
            .iter()
            .position(|l| matches!(l, Line::Name { .. }))
        {
            self.list.scroll_to_reveal_item(ix);
        }
    }
    fn cancel_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.naming = None;
        window.focus(&self.focus, cx);
        self.rebuild(cx);
    }
    fn finish_name(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.naming.clone() else {
            return;
        };
        if self.name_busy {
            return;
        }
        if !commit {
            self.cancel_name(window, cx);
            return;
        }
        if self
            .issue(cx)
            .is_some_and(|i| i.severity == Severity::Error)
        {
            self.name_attempted = true;
            self.rebuild(cx);
            return;
        }
        let raw = self.name_input.read(cx).content.to_string();
        let name = files::well_formed_file_name(&raw);
        let cwd = self.cwd.clone();
        match mode {
            NameMode::Rename { path, is_dir } => {
                if name.is_empty() || (name == basename(&path) && !raw.contains(['/', '\\'])) {
                    self.cancel_name(window, cx);
                    return;
                }
                self.name_busy = true;
                let from = path.clone();
                let work = cx.background_executor().spawn({
                    let name = name.clone();
                    async move { files::rename_path(Path::new(&from), &name) }
                });
                cx.spawn_in(window, async move |this, cx| {
                    let result = work.await;
                    let _ = this.update_in(cx, |this, window, cx| match result {
                        Ok(next) => {
                            let parent = parent_of(&path);
                            if is_dir {
                                this.forget(&path);
                            }
                            let touched = files::dirs_touched_by_create(&parent, &name);
                            for dir in touched.iter().chain([&parent]) {
                                this.read_dir(dir.clone(), cx);
                            }
                            this.naming = None;
                            this.expand(&touched, cx);
                            this.remap(&path, &next);
                            this.after_op(window, cx);
                        }
                        Err(error) => this.name_failed(error, cx),
                    });
                })
                .detach();
            }
            NameMode::Create { id, parent, is_dir } => {
                let as_folder = is_dir || raw.ends_with(['/', '\\']);
                self.name_busy = true;
                let at = parent.clone();
                let work = cx.background_executor().spawn({
                    let name = name.clone();
                    async move { files::create_path(Path::new(&at), &name, as_folder) }
                });
                cx.spawn_in(window, async move |this, cx| {
                    let result = work.await;
                    let _ = this.update_in(cx, |this, window, cx| match result {
                        Ok(created) => {
                            let touched = files::dirs_touched_by_create(&parent, &name);
                            for dir in &touched {
                                this.read_dir(dir.clone(), cx);
                            }
                            if matches!(&this.naming, Some(NameMode::Create { id: i, .. }) if *i == id) {
                                this.naming = None;
                            }
                            this.expand(&touched, cx);
                            this.selected = Some(created);
                            let _ = cwd;
                            this.after_op(window, cx);
                        }
                        Err(error) => this.name_failed(error, cx),
                    });
                })
                .detach();
            }
        }
    }
    fn name_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.name_busy = false;
        self.name_error = Some(error);
        self.rebuild(cx);
    }
    fn after_op(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_busy = false;
        window.focus(&self.focus, cx);
        self.git.update(cx, |git, cx| git.changed(cx));
        self.rebuild(cx);
    }
    fn remap(&mut self, from: &str, to: &str) {
        self.expanded = self
            .expanded
            .iter()
            .map(|p| files::rebase_path(p, from, to))
            .collect();
        if let Some(selected) = &self.selected {
            self.selected = Some(files::rebase_path(selected, from, to));
        }
        if let Some(clip) = &mut self.clip
            && (clip.path == from || clip.path.starts_with(&format!("{from}/")))
        {
            clip.path = files::rebase_path(&clip.path, from, to);
        }
    }
    fn set_operation_error(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.op_error = error.clone();
        cx.emit(FileActionError(error));
    }
    fn fail(&mut self, error: String, cx: &mut Context<Self>) {
        self.set_operation_error(Some(error), cx);
        self.rebuild(cx);
    }
    /// `run`: operation errors route to the shared toast until the next action.
    fn run(
        &mut self,
        work: impl FnOnce() -> Result<Option<String>, String> + Send + 'static,
        touched: Vec<String>,
        forget: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_operation_error(None, cx);
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(created) => {
                    if let Some(path) = &forget {
                        this.forget(path);
                    }
                    for dir in &touched {
                        this.read_dir(dir.clone(), cx);
                    }
                    this.expand(&touched, cx);
                    if let Some(created) = created {
                        this.selected = Some(created);
                    }
                    this.after_op(window, cx);
                }
                Err(error) => this.fail(error, cx),
            });
        })
        .detach();
    }
    fn delete(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if path == self.cwd {
            return;
        }
        let is_dir = self.is_dir_at(&path);
        let label = basename(&path).to_owned();
        let answer = window.prompt(
            PromptLevel::Warning,
            &if is_dir {
                format!("Delete folder “{label}” and everything inside it?")
            } else {
                format!("Delete “{label}”?")
            },
            None,
            &["OK", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if !matches!(answer.await, Ok(0)) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let parent = parent_of(&path);
                let target = path.clone();
                this.selected = match &this.selected {
                    Some(s) if s == &path || s.starts_with(&format!("{path}/")) => {
                        Some(parent.clone())
                    }
                    other => other.clone(),
                };
                if this
                    .clip
                    .as_ref()
                    .is_some_and(|c| c.path == path || c.path.starts_with(&format!("{path}/")))
                {
                    this.clip = None;
                }
                this.run(
                    move || files::delete_path(Path::new(&target)).map(|_| None),
                    vec![parent],
                    is_dir.then_some(path),
                    window,
                    cx,
                );
            });
        })
        .detach();
    }
    fn paste(&mut self, at: String, window: &mut Window, cx: &mut Context<Self>) {
        let dest = self.create_parent(Some(&at));
        let Some(clip) = self.clip.clone() else {
            // clipboardFilePaths: files copied in Finder.
            let paths: Vec<PathBuf> = cx
                .read_from_clipboard()
                .map(|item| {
                    item.entries()
                        .iter()
                        .filter_map(|e| match e {
                            ClipboardEntry::ExternalPaths(p) => Some(p.paths().to_vec()),
                            _ => None,
                        })
                        .flatten()
                        .collect()
                })
                .unwrap_or_default();
            self.copy_external(paths, dest, window, cx);
            return;
        };
        if clip.is_dir && (dest == clip.path || dest.starts_with(&format!("{}/", clip.path))) {
            self.fail("Cannot paste a folder into itself.".into(), cx);
            return;
        }
        let from = clip.path.clone();
        if clip.cut {
            let to_parent = dest.clone();
            let task = cx
                .background_executor()
                .spawn(async move { files::move_path(Path::new(&from), Path::new(&to_parent)) });
            let from = clip.path.clone();
            cx.spawn_in(window, async move |this, cx| {
                let result = task.await;
                let _ = this.update_in(cx, |this, window, cx| match result {
                    Ok(created) => {
                        if clip.is_dir {
                            this.forget(&from);
                        }
                        for dir in files::dirs_touched_by_move(&from, &created) {
                            this.read_dir(dir, cx);
                        }
                        this.remap(&from, &created);
                        this.clip = None;
                        this.expand(std::slice::from_ref(&dest), cx);
                        this.selected = Some(created);
                        this.after_op(window, cx);
                    }
                    Err(error) => this.fail(error, cx),
                });
            })
            .detach();
        } else {
            let to = dest.clone();
            self.run(
                move || files::copy_path(Path::new(&from), Path::new(&to)).map(Some),
                vec![dest],
                None,
                window,
                cx,
            );
        }
    }
    fn copy_external(
        &mut self,
        paths: Vec<PathBuf>,
        dest: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() {
            return;
        }
        let to = dest.clone();
        self.run(
            move || {
                let mut created = None;
                for from in &paths {
                    created = Some(files::copy_path(from, Path::new(&to))?);
                }
                Ok(created)
            },
            vec![dest],
            None,
            window,
            cx,
        );
    }
    fn duplicate(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        if path == self.cwd {
            return;
        }
        let parent = parent_of(&path);
        let to = parent.clone();
        self.run(
            move || files::copy_path(Path::new(&path), Path::new(&to)).map(Some),
            vec![parent],
            None,
            window,
            cx,
        );
    }
    fn reveal(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.run(
            move || files::reveal_path(Path::new(&path)).map(|_| None),
            Vec::new(),
            None,
            window,
            cx,
        );
    }

    fn open_menu(
        &mut self,
        target: Target,
        at: Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.naming = None;
        self.selected = Some(target.path.clone());
        let paste_parent = if target.is_dir {
            target.path.clone()
        } else {
            parent_of(&target.path)
        };
        let paste_blocked = self.clip.as_ref().is_some_and(|c| {
            c.is_dir
                && (paste_parent == c.path || paste_parent.starts_with(&format!("{}/", c.path)))
        });
        let root = target.root;
        let items: Vec<MenuItem> = vec![
            MenuAction::new("new-file", "New File").into(),
            MenuAction::new("new-folder", "New Folder").into(),
            MenuItem::Separator,
            MenuAction::new("cut", "Cut")
                .shortcut(format!("{MOD}X"))
                .disabled(root)
                .into(),
            MenuAction::new("copy", "Copy")
                .shortcut(format!("{MOD}C"))
                .disabled(root)
                .into(),
            MenuAction::new("paste", "Paste")
                .shortcut(format!("{MOD}V"))
                .disabled(paste_blocked)
                .into(),
            MenuAction::new("duplicate", "Duplicate")
                .disabled(root)
                .into(),
            MenuItem::Separator,
            MenuAction::new("copy-path", "Copy Path")
                .shortcut(format!("{MOD}{SHIFT}C"))
                .into(),
            MenuAction::new("copy-relative-path", "Copy Relative Path").into(),
            MenuItem::Separator,
            MenuAction::new("rename", "Rename")
                .shortcut("F2")
                .disabled(root)
                .into(),
            MenuAction::new("delete", "Delete")
                .shortcut("⌫")
                .disabled(root)
                .danger()
                .into(),
            MenuItem::Separator,
            MenuAction::new("reveal", REVEAL_LABEL).into(),
        ];
        self.menu = None;
        let menu = cx.new(|cx| Menu::new("File actions", items, window, cx));
        let events = cx.subscribe_in(&menu, window, |this, _, event: &MenuEvent, window, cx| {
            let Some(open) = this.menu.take() else {
                return;
            };
            if let MenuEvent::Picked(id, _) = event {
                this.run_action(id, open.target, window, cx);
            }
            cx.notify();
        });
        self.menu = Some(OpenMenu {
            target,
            anchor: at,
            menu,
            _events: events,
        });
        self.rebuild(cx);
    }
    fn run_action(
        &mut self,
        id: &str,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match id {
            "new-file" => self.start_create(false, Some(target.path), window, cx),
            "new-folder" => self.start_create(true, Some(target.path), window, cx),
            "cut" | "copy" if !target.root => {
                self.clip = Some(Clip {
                    cut: id == "cut",
                    path: target.path,
                    is_dir: target.is_dir,
                });
                self.rebuild(cx);
            }
            "paste" => self.paste(target.path, window, cx),
            "duplicate" => self.duplicate(target.path, window, cx),
            "copy-path" => cx.write_to_clipboard(ClipboardItem::new_string(target.path)),
            "copy-relative-path" => cx.write_to_clipboard(ClipboardItem::new_string(
                files::display_path(&target.path, Some(&self.cwd)),
            )),
            "rename" => self.start_rename(target.path, window, cx),
            "delete" => self.delete(target.path, window, cx),
            "reveal" => self.reveal(target.path, window, cx),
            _ => {}
        }
    }

    /// The rows keyboard navigation walks: the root, then each entry shown.
    fn nav_rows(&self) -> Vec<(String, bool)> {
        let mut rows = vec![(self.cwd.clone(), true)];
        rows.extend(self.lines.iter().filter_map(|l| match l {
            Line::Entry { entry, .. } => Some((entry.path.clone(), entry.is_dir)),
            _ => None,
        }));
        rows
    }
    fn focus_row(&mut self, path: String, cx: &mut Context<Self>) {
        self.keyboard = true;
        if let Some(ix) = self
            .lines
            .iter()
            .position(|l| matches!(l, Line::Entry { entry, .. } if entry.path == path))
        {
            self.list.scroll_to_reveal_item(ix);
        }
        self.select(path, cx);
    }
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.nav_rows();
        let current = self.selected.clone().unwrap_or_else(|| self.cwd.clone());
        let ix = rows.iter().position(|(p, _)| *p == current);
        let next = match ix {
            None if delta > 0 => 0,
            None => rows.len() - 1,
            Some(ix) => (ix as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
        };
        if let Some((path, _)) = rows.get(next).cloned() {
            self.focus_row(path, cx);
        }
    }
    fn page(&self, window: &Window) -> isize {
        let height = self.list.viewport_bounds().size.height.as_f32();
        let _ = window;
        ((height / ROW).floor() as isize - 1).max(1)
    }
    fn current(&self) -> (String, bool) {
        let path = self.selected.clone().unwrap_or_else(|| self.cwd.clone());
        let is_dir = self.is_dir_at(&path);
        (path, is_dir)
    }
    fn activate(&mut self, cx: &mut Context<Self>) {
        let (path, is_dir) = self.current();
        if is_dir {
            self.toggle(&path, cx);
        } else {
            cx.emit(OpenFile(path));
        }
    }
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        let keystroke = &event.keystroke;
        let m = &keystroke.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return;
        }
        if keystroke.key == "space" {
            self.activate(cx);
            cx.stop_propagation();
            return;
        }
        // Type a name prefix to jump to the next matching row; repeating one
        // letter cycles through its matches.
        let Some(ch) = keystroke
            .key_char
            .as_ref()
            .filter(|c| c.chars().count() == 1)
        else {
            return;
        };
        let now = Instant::now();
        let fresh = self
            .typeahead
            .1
            .is_none_or(|at| now.duration_since(at).as_millis() > 700);
        if fresh {
            self.typeahead.0 = ch.clone();
        } else {
            self.typeahead.0.push_str(ch);
        }
        self.typeahead.1 = Some(now);
        let lower = self.typeahead.0.to_lowercase();
        let first = lower.chars().next().unwrap_or_default();
        let cycling = lower.chars().all(|c| c == first);
        let needle = if cycling { first.to_string() } else { lower };
        let rows = self.nav_rows();
        let current = self.selected.clone().unwrap_or_else(|| self.cwd.clone());
        let ix = rows.iter().position(|(p, _)| *p == current);
        let start = if cycling {
            ix.map_or(0, |i| i + 1)
        } else {
            ix.unwrap_or(0)
        };
        let ordered = rows[start.min(rows.len())..]
            .iter()
            .chain(&rows[..start.min(rows.len())]);
        if let Some((path, _)) = ordered
            .filter(|(p, _)| *p != self.cwd)
            .find(|(p, _)| basename(p).to_lowercase().starts_with(&needle))
            .cloned()
        {
            self.focus_row(path, cx);
        }
        cx.stop_propagation();
    }

    fn header_icon(
        id: &'static str,
        label: SharedString,
        path: &'static str,
        enabled: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .group(id)
            .h(px(24.))
            .min_w_0()
            .flex_1()
            .self_center()
            .flex()
            .items_center()
            .justify_center()
            .rounded_md()
            .when(!enabled, |d| d.opacity(0.4))
            .when(enabled, |d| d.hover(|s| s.bg(content(0.05))))
            .child(icon(path, 14.).text_color(content(0.5)).when(enabled, |i| {
                i.group_hover(id, |s| s.text_color(content(1.)))
            }))
    }

    fn line(&self, ix: usize, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let statuses = &self.git.read(cx).statuses;
        let pad = |depth: usize| px(28. + depth as f32 * 12.);
        match self.lines[ix].clone() {
            Line::Note { depth, text } => div()
                .pl(pad(depth))
                .pr_2()
                .truncate()
                .text_size(px(12.))
                .text_color(content(0.5))
                .child(text)
                .into_any_element(),
            Line::Issue { depth, text, error } => div()
                .pl(pad(depth))
                .pr_2()
                .pb_1()
                .text_size(px(12.))
                .line_height(px(16.))
                .text_color(rgb(if error { RED_400 } else { AMBER_400 }))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .children(text.into_iter().map(|(t, bold)| {
                            div()
                                .when(bold, |d| d.font_weight(FontWeight::SEMIBOLD))
                                .child(t)
                        })),
                )
                .into_any_element(),
            Line::Name { depth, is_dir } => {
                let leaf = files::leaf_name(&self.name_input.read(cx).content);
                div()
                    .key_context("ExplorerName")
                    .on_action(cx.listener(|this, _: &CancelName, window, cx| {
                        this.finish_name(false, window, cx)
                    }))
                    .h(px(ROW))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(content(0.1))
                    .pl(px(8. + depth as f32 * 12.))
                    .pr_2()
                    .child(
                        div()
                            .size_4()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(is_dir, |d| {
                                d.child(icon("chevron-right", 14.).text_color(content(0.5)))
                            }),
                    )
                    .child(file_type_icon(&leaf, is_dir, false, 16.))
                    .child(
                        div()
                            .h(px(20.))
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .bg(content(0.1))
                            .border_1()
                            .border_color(accent_color())
                            .px_1()
                            .text_size(px(ENTRY_FONT_SIZE))
                            .text_color(content(1.))
                            .when(self.name_busy, |d| d.opacity(0.5))
                            .child(self.name_input.clone()),
                    )
                    .into_any_element()
            }
            Line::Entry { entry, depth, open } => {
                let selected = self.selected.as_deref() == Some(entry.path.as_str());
                let cut = self
                    .clip
                    .as_ref()
                    .is_some_and(|c| c.cut && c.path == entry.path);
                let over = self.drag_over.as_deref() == Some(entry.path.as_str());
                let status = if entry.is_dir {
                    statuses.dirs.get(&entry.path)
                } else {
                    statuses.files.get(&entry.path)
                };
                let git_color: Option<Hsla> = status.and_then(|s| match s.as_str() {
                    "modified" => Some(rgb(AMBER_400).into()),
                    "added" | "untracked" => Some(rgb(DIFF_ADD).into()),
                    "deleted" => Some(rgb(DIFF_DEL).into()),
                    _ => None,
                });
                let path = entry.path.clone();
                let menu_path = entry.path.clone();
                let drop_path = entry.path.clone();
                let over_path = entry.path.clone();
                let is_dir = entry.is_dir;
                div()
                    .id(("explorer-row", ix))
                    .role(Role::TreeItem)
                    .aria_label(SharedString::from(entry.path.clone()))
                    .when(is_dir, |d| d.aria_expanded(open))
                    .aria_selected(selected)
                    .h(px(ROW))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl(px(8. + depth as f32 * 12.))
                    .pr_2()
                    .text_size(px(ENTRY_FONT_SIZE))
                    .line_height(px(ENTRY_FONT_SIZE))
                    .text_color(content(1.))
                    .when(selected || over, |d| d.bg(selection()))
                    .when(!selected && !over, |d| d.hover(|s| s.bg(content(0.05))))
                    .when(selected && focused && self.keyboard, |d| {
                        d.border_1().border_color(accent_color().opacity(0.4))
                    })
                    .when(cut, |d| d.opacity(0.5))
                    .when(!is_dir, |d| {
                        d.on_drag(DraggedFile(PathBuf::from(&entry.path)), |file, _, _, cx| {
                            cx.new(|_| file.clone())
                        })
                    })
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.keyboard = false;
                        window.focus(&this.focus, cx);
                        this.select(path.clone(), cx);
                        if is_dir {
                            this.toggle(&path, cx);
                        } else {
                            cx.emit(OpenFile(path.clone()));
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.open_menu(
                                Target {
                                    path: menu_path.clone(),
                                    is_dir,
                                    root: false,
                                },
                                event.position,
                                window,
                                cx,
                            );
                        }),
                    )
                    .on_drag_move::<ExternalPaths>(cx.listener(
                        move |this, event: &gpui::DragMoveEvent<ExternalPaths>, _, cx| {
                            if event.bounds.contains(&event.event.position) {
                                let target = this.create_parent(Some(&over_path));
                                if this.drag_over.as_ref() != Some(&target) {
                                    this.drag_over = Some(target);
                                    cx.notify();
                                }
                            }
                        },
                    ))
                    .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
                        this.drag_over = None;
                        let dest = this.create_parent(Some(&drop_path));
                        this.copy_external(paths.paths().to_vec(), dest, window, cx);
                    }))
                    .child(
                        div()
                            .size_4()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(is_dir, |d| {
                                d.child(
                                    icon(
                                        if open {
                                            "chevron-down"
                                        } else {
                                            "chevron-right"
                                        },
                                        14.,
                                    )
                                    .text_color(content(0.5)),
                                )
                            }),
                    )
                    .child(file_type_icon(&entry.name, is_dir, open, 16.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .line_height(px(ENTRY_FONT_SIZE * 1.4))
                            .when(entry.ignored, |d| d.italic().text_color(content(0.5)))
                            .when_some(git_color.filter(|_| !entry.ignored), |d, c| d.text_color(c))
                            .child(entry.name.clone()),
                    )
                    .into_any_element()
            }
        }
    }
}

impl Focusable for Explorer {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search
            .as_ref()
            .map_or_else(|| self.focus.clone(), |search| search.focus_handle(cx))
    }
}

impl Render for Explorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(search) = &self.search {
            return search
                .clone()
                .cached(StyleRefinement::default().flex_1().min_h_0().w_full())
                .into_any_element();
        }
        if !self.available() {
            return div()
                .px_3()
                .py_2()
                .text_size(px(12.))
                .text_color(content(0.5))
                .child("No project folder")
                .into_any_element();
        }
        let root_open = self.expanded.contains(&self.cwd);
        let name: SharedString = basename(&self.cwd).to_uppercase().into();
        let root_selected = self.drag_over.as_deref() == Some(self.cwd.as_str());
        let focused = self.focus.contains_focused(window, cx);
        let entity = cx.entity();
        let cwd = self.cwd.clone();
        let menu = self
            .menu
            .as_ref()
            .map(|open| menu_popover(&open.menu, Anchor::Point(open.anchor), cx).gap(0.));
        div()
            .id("explorer")
            .role(Role::TabPanel)
            .aria_label("Explorer")
            .key_context("Explorer")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &Up, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &PageDown, window, cx| {
                let n = this.page(window);
                this.step(n, cx)
            }))
            .on_action(cx.listener(|this, _: &PageUp, window, cx| {
                let n = this.page(window);
                this.step(-n, cx)
            }))
            .on_action(cx.listener(|this, _: &First, _, cx| this.step(-isize::MAX / 2, cx)))
            .on_action(cx.listener(|this, _: &Last, _, cx| this.step(isize::MAX / 2, cx)))
            .on_action(cx.listener(|this, _: &Expand, _, cx| {
                let (path, is_dir) = this.current();
                if !is_dir {
                    return;
                }
                if !this.expanded.contains(&path) {
                    this.toggle(&path, cx);
                } else {
                    let rows = this.nav_rows();
                    if let Some(ix) = rows.iter().position(|(p, _)| *p == path)
                        && let Some((next, _)) = rows.get(ix + 1).cloned()
                        && parent_of(&next) == path
                    {
                        this.focus_row(next, cx);
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &Collapse, _, cx| {
                let (path, is_dir) = this.current();
                if is_dir && this.expanded.contains(&path) {
                    this.toggle(&path, cx);
                } else if path != this.cwd {
                    this.focus_row(parent_of(&path), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Activate, _, cx| this.activate(cx)))
            .on_action(cx.listener(|this, _: &OpenSelected, _, cx| this.activate(cx)))
            .on_action(cx.listener(|this, _: &Rename, window, cx| {
                let (path, _) = this.current();
                this.start_rename(path, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Delete, window, cx| {
                let (path, _) = this.current();
                this.delete(path, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ClearCut, _, cx| {
                if this.clip.as_ref().is_some_and(|c| c.cut) {
                    this.clip = None;
                    this.rebuild(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, action: &CopyEntry, _, cx| {
                let _ = action;
                let (path, is_dir) = this.current();
                if path != this.cwd {
                    this.clip = Some(Clip {
                        cut: false,
                        path,
                        is_dir,
                    });
                    this.rebuild(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CutEntry, _, cx| {
                let (path, is_dir) = this.current();
                if path != this.cwd {
                    this.clip = Some(Clip {
                        cut: true,
                        path,
                        is_dir,
                    });
                    this.rebuild(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &PasteEntry, window, cx| {
                let (path, _) = this.current();
                this.paste(path, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyPath, _, cx| {
                let (path, _) = this.current();
                cx.write_to_clipboard(ClipboardItem::new_string(path));
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_menu(
                        Target {
                            path: this.cwd.clone(),
                            is_dir: true,
                            root: true,
                        },
                        event.position,
                        window,
                        cx,
                    );
                }),
            )
            .child(
                pane_header("explorer-header", "Explorer")
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .child(
                        Self::header_icon("explorer-new-file", "New File".into(), "file-add", true)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.start_create(false, None, window, cx)
                            })),
                    )
                    .child(
                        Self::header_icon(
                            "explorer-new-folder",
                            "New Folder".into(),
                            "folder-add",
                            true,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_create(true, None, window, cx)
                        })),
                    )
                    .child(
                        Self::header_icon(
                            "explorer-collapse",
                            "Collapse All".into(),
                            "fold-vertical",
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.collapse_all(cx))),
                    )
                    .child(
                        Self::header_icon(
                            "explorer-search",
                            format!("Search in files ({MOD}Shift+F)").into(),
                            "search",
                            true,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.open_search(window, cx))),
                    ),
            )
            .child(
                div()
                    .h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("explorer-root")
                            .role(Role::TreeItem)
                            .aria_label(SharedString::from(cwd.clone()))
                            .tip(SharedString::from(cwd.clone()))
                            .aria_expanded(root_open)
                            .h_full()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap_1()
                            .pl_2()
                            .when(root_selected, |d| d.bg(selection()))
                            .on_mouse_down(MouseButton::Left, |_, window, _| {
                                window.prevent_default()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus, cx);
                                let cwd = this.cwd.clone();
                                this.select(cwd.clone(), cx);
                                this.toggle(&cwd, cx);
                            }))
                            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                                this.drag_over = None;
                                let dest = this.cwd.clone();
                                this.copy_external(paths.paths().to_vec(), dest, window, cx);
                            }))
                            .child(
                                div()
                                    .size_4()
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        icon(
                                            if root_open {
                                                "chevron-down"
                                            } else {
                                                "chevron-right"
                                            },
                                            14.,
                                        )
                                        .text_color(content(0.5)),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(content(0.5))
                                    .child(name),
                            ),
                    ),
            )
            .child(
                div()
                    .id("explorer-tree")
                    .role(Role::Tree)
                    .flex_1()
                    .min_h_0()
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                        this.drag_over = None;
                        let dest = this.cwd.clone();
                        this.copy_external(paths.paths().to_vec(), dest, window, cx);
                    }))
                    .when(root_open, |d| {
                        d.child(
                            list(self.list.clone(), move |ix, _, cx| {
                                entity.update(cx, |this, cx| this.line(ix, focused, cx))
                            })
                            .size_full(),
                        )
                    }),
            )
            .children(menu)
            .into_any_element()
    }
}
