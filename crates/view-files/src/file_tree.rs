//! Port of src/features/files/ui/FileTree.tsx: the explorer. A lazily
//! listed folder tree with git status colors, create and rename rows,
//! cut, copy, paste, duplicate, and delete, files dropped in from the OS,
//! file drags out to other panes, and the explorer context menu.
//!
//! Folder listings, expanded folders, and the selection live in the files
//! model behind [`FilesData`]; the view keeps what each mounted React node
//! kept in its own state (the listing it shows, a listing error).
//!
//! Keys: the React tree read Mod+C, Mod+X, Mod+V, Mod+Shift+C, F2, Delete,
//! Backspace, and Escape from a `keydown` handler, and the browser moved
//! focus between rows. GPUI has no focus traversal between plain rows, so
//! the arrow keys, Enter, and Space move and open the selection instead.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, AsyncWindowContext, ClickEvent, ClipboardItem, Context, DragMoveEvent,
    ElementId, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, Hsla,
    InteractiveElement, IntoElement, KeyBinding, MouseButton, MouseDownEvent, ParentElement,
    Pixels, Point, PromptLevel, Render, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Task, WeakEntity, Window, actions, div, prelude::FluentBuilder as _, px,
};
use gpui_base::input::Input;
use gpui_component::input::{Escape, InputEvent, InputState};
use monocode_core::Platform;
use monocode_ui::{
    IconName, Theme, UiStyled as _, file_type_icon, folder_type_icon, icon, u, widgets::tooltip,
};

use crate::data::{FileOpenOptions, FilesData, FsEntry, GitStatusMap};
use crate::explorer_menu::{
    ExplorerMenu, ExplorerMenuEvent, ExplorerMenuItem, MenuAction, MenuAnchor,
};
use crate::file_name::{
    NameIssue, leaf_name, path_segments, validate_file_name, well_formed_file_name,
};
use crate::paths::{
    REMOTE_PROJECT_PREFIX, basename, display_path, is_same_or_inside, join_path, parent_path,
    rebase_path,
};

const KEY_CONTEXT: &str = "FileTree";

/// How often a remote project's tree re-lists, since no watcher reaches it.
pub const REMOTE_REFRESH_INTERVAL: Duration = Duration::from_millis(5_000);

actions!(
    file_tree,
    [
        /// Copy the selected path (Mod+Shift+C).
        CopyPath,
        /// Copy the selected entry for a paste.
        CopyEntry,
        /// Cut the selected entry for a paste.
        CutEntry,
        /// Paste the clipboard entry, or files a file manager copied.
        PasteEntry,
        /// Rename the selected entry.
        RenameEntry,
        /// Delete the selected entry after a confirmation.
        DeleteEntry,
        /// Drop a pending cut.
        CancelCut,
        /// Select the next row.
        SelectNextEntry,
        /// Select the previous row.
        SelectPreviousEntry,
        /// Expand the selected folder, or step into it.
        ExpandEntry,
        /// Collapse the selected folder, or step out to its parent.
        CollapseEntry,
        /// Open the selected file or toggle the selected folder.
        OpenEntry,
    ]
);

pub(crate) fn init(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-shift-c", CopyPath, context),
        KeyBinding::new("secondary-c", CopyEntry, context),
        KeyBinding::new("secondary-x", CutEntry, context),
        KeyBinding::new("secondary-v", PasteEntry, context),
        KeyBinding::new("f2", RenameEntry, context),
        KeyBinding::new("delete", DeleteEntry, context),
        KeyBinding::new("backspace", DeleteEntry, context),
        KeyBinding::new("escape", CancelCut, context),
        KeyBinding::new("down", SelectNextEntry, context),
        KeyBinding::new("up", SelectPreviousEntry, context),
        KeyBinding::new("right", ExpandEntry, context),
        KeyBinding::new("left", CollapseEntry, context),
        KeyBinding::new("enter", OpenEntry, context),
        KeyBinding::new("space", OpenEntry, context),
    ]);
}

/// What the tree asks its owner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTreeEvent {
    /// `onOpenFile(path, undefined, options)`.
    OpenFile {
        path: String,
        options: FileOpenOptions,
    },
    /// `onOpenTerminal(cwd)`.
    OpenTerminal { cwd: String },
    /// `onFileMoved`: a rename, or a cut and paste.
    FileMoved { from: String, to: String },
    /// `onFileDeleted`.
    FileDeleted { path: String },
    /// `onSearch`: the header's search button.
    Search,
}

/// The drag payload of a file dragged out of the tree. Panes accept it with
/// `on_drop::<DraggedExplorerFile>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraggedExplorerFile {
    pub path: String,
    pub name: String,
}

/// `Clip.mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipMode {
    Copy,
    Cut,
}

/// `Clip`: the entry Mod+C or Mod+X picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub mode: ClipMode,
    pub path: String,
    pub is_dir: bool,
}

/// `MenuTarget`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuTarget {
    pub path: String,
    pub is_dir: bool,
    pub is_root: bool,
}

/// `Creating`: a pending create row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Creating {
    id: u64,
    parent: String,
    is_dir: bool,
}

/// One row of the flattened tree.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeRow {
    /// A folder's listing error, or `…` while it loads.
    Message {
        depth: usize,
        text: String,
        loading: bool,
    },
    Entry {
        depth: usize,
        entry: FsEntry,
    },
    /// The name input of a create or rename.
    NameInput {
        depth: usize,
        is_dir: bool,
    },
}

/// `GIT_STATUS_COLOR`.
fn git_status_color(status: &str, theme: &Theme) -> Option<Hsla> {
    match status {
        "modified" => Some(theme.colors.warning),
        "added" | "untracked" => Some(theme.colors.success),
        "deleted" => Some(theme.colors.danger),
        _ => None,
    }
}

/// `REVEAL_LABEL`.
pub fn reveal_label(platform: Platform) -> &'static str {
    if platform.is_mac() {
        "Reveal in Finder"
    } else if platform.is_windows() {
        "Reveal in File Explorer"
    } else {
        "Open Containing Folder"
    }
}

/// `explorerItems`: the context menu for `target`.
pub fn explorer_items(
    target: &MenuTarget,
    clip: Option<&Clip>,
    can_open_terminal: bool,
    platform: Platform,
) -> Vec<ExplorerMenuItem> {
    let module = platform.mod_label();
    let shift = platform.shift_label();
    let paste_parent = if target.is_dir {
        target.path.clone()
    } else {
        parent_path(&target.path)
    };
    let paste_blocked =
        clip.is_some_and(|clip| clip.is_dir && is_same_or_inside(&paste_parent, &clip.path));
    let mut items = vec![
        MenuAction::new("new-file", "New File").into(),
        MenuAction::new("new-folder", "New Folder").into(),
        ExplorerMenuItem::Separator,
        MenuAction::new("cut", "Cut")
            .shortcut(format!("{module}X"))
            .disabled(target.is_root)
            .into(),
        MenuAction::new("copy", "Copy")
            .shortcut(format!("{module}C"))
            .disabled(target.is_root)
            .into(),
        MenuAction::new("paste", "Paste")
            .shortcut(format!("{module}V"))
            .disabled(paste_blocked)
            .into(),
        MenuAction::new("duplicate", "Duplicate")
            .disabled(target.is_root)
            .into(),
        ExplorerMenuItem::Separator,
        MenuAction::new("copy-path", "Copy Path")
            .shortcut(format!("{module}{shift}C"))
            .into(),
        MenuAction::new("copy-relative-path", "Copy Relative Path").into(),
        ExplorerMenuItem::Separator,
        MenuAction::new("rename", "Rename")
            .shortcut("F2")
            .disabled(target.is_root)
            .into(),
        MenuAction::new("delete", "Delete")
            .shortcut("⌫")
            .disabled(target.is_root)
            .danger()
            .into(),
        ExplorerMenuItem::Separator,
    ];
    if can_open_terminal {
        items.push(MenuAction::new("open-terminal", "Open in Terminal").into());
    }
    items.push(MenuAction::new("reveal", reveal_label(platform)).into());
    items
}

/// `dirsTouchedByCreate`: folders whose children change when creating
/// `name` under `parent`.
pub fn dirs_touched_by_create(parent: &str, name: &str) -> Vec<String> {
    let segments = path_segments(name);
    let mut out = vec![parent.to_string()];
    let mut cur = parent.to_string();
    for segment in segments.iter().take(segments.len().saturating_sub(1)) {
        cur = join_path(&cur, segment);
        out.push(cur.clone());
    }
    out
}

/// `dirsTouchedByMove`.
pub fn dirs_touched_by_move(from: &str, to: &str) -> Vec<String> {
    let from_parent = parent_path(from);
    let to_parent = parent_path(to);
    if from_parent == to_parent {
        vec![from_parent]
    } else {
        vec![from_parent, to_parent]
    }
}

/// The message a name issue shows, with the name to set in bold.
fn issue_text(issue: &NameIssue) -> (String, Option<String>, String) {
    match issue {
        NameIssue::Empty => (
            "A file or folder name must be provided.".into(),
            None,
            String::new(),
        ),
        NameIssue::Slash => (
            "A file or folder name cannot start with a slash.".into(),
            None,
            String::new(),
        ),
        NameIssue::Exists { name } => (
            "A file or folder ".into(),
            Some(name.clone()),
            " already exists at this location. Please choose a different name.".into(),
        ),
        NameIssue::Invalid { name } => (
            "The name ".into(),
            Some(name.clone()),
            " is not valid as a file or folder name. Please choose a different name.".into(),
        ),
        NameIssue::Whitespace => (
            "Leading or trailing whitespace detected in file or folder name.".into(),
            None,
            String::new(),
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NameTarget {
    Create(u64),
    Rename(String),
}

/// `NameRow`'s state.
struct NameRow {
    target: NameTarget,
    input: Entity<InputState>,
    siblings: Vec<String>,
    attempted: bool,
    busy: bool,
    submit_error: Option<String>,
    finished: bool,
    _subscription: Subscription,
}

impl NameRow {
    fn value(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    fn issue(&self, cx: &App) -> Option<NameIssue> {
        validate_file_name(&self.value(cx), self.siblings.iter().map(String::as_str))
    }

    /// `showIssue`: the message under the row, if any.
    fn shown_issue(&self, cx: &App) -> Option<Result<NameIssue, String>> {
        if let Some(error) = &self.submit_error {
            return Some(Err(error.clone()));
        }
        let issue = self.issue(cx)?;
        let value = self.value(cx);
        let show = !issue.is_error()
            || (issue != NameIssue::Empty && !value.is_empty())
            || (issue == NameIssue::Empty && self.attempted);
        show.then_some(Ok(issue))
    }
}

struct OpenMenu {
    target: MenuTarget,
    menu: Entity<ExplorerMenu>,
    _subscription: Subscription,
}

/// The explorer view.
pub struct FileTree {
    data: Rc<dyn FilesData>,
    cwd: String,
    root_label: Option<String>,
    expanded: HashSet<String>,
    selected_path: Option<String>,
    /// The listing each mounted folder shows.
    children: HashMap<String, Vec<FsEntry>>,
    errors: HashMap<String, String>,
    /// The epoch each mounted folder last synced at.
    synced: HashMap<String, u64>,
    loads: HashMap<String, (u64, Task<()>)>,
    next_load: u64,
    creating: Option<Creating>,
    next_create: u64,
    renaming: Option<String>,
    name_row: Option<NameRow>,
    clip: Option<Clip>,
    menu: Option<OpenMenu>,
    drag_over_path: Option<String>,
    dragging_path: Option<String>,
    op_error: Option<String>,
    epoch: u64,
    show_excluded_files: bool,
    git_statuses: GitStatusMap,
    can_search: bool,
    can_open_terminal: bool,
    platform: Platform,
    animate_menus: bool,
    focus_handle: FocusHandle,
    scroll: ScrollHandle,
    remote_poll: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FileTreeEvent> for FileTree {}

impl Focusable for FileTree {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The tree's entity went away during an operation.
fn closed<E>(_: E) -> String {
    "The file tree closed.".into()
}

impl FileTree {
    pub fn new(
        data: Rc<dyn FilesData>,
        cwd: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = cwd.into();
        let weak = cx.entity().downgrade();
        let dirs = data.subscribe_dirs_changed(
            Box::new(move |cx| {
                weak.update(cx, |this, cx| this.dirs_changed(cx)).ok();
            }),
            cx,
        );
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.data.notify_dirs_changed(cx);
            }
        });
        let mut this = Self {
            expanded: data.load_expanded(&cwd, cx),
            selected_path: data.load_selected(&cwd, cx),
            data,
            cwd,
            root_label: None,
            children: HashMap::new(),
            errors: HashMap::new(),
            synced: HashMap::new(),
            loads: HashMap::new(),
            next_load: 0,
            creating: None,
            next_create: 0,
            renaming: None,
            name_row: None,
            clip: None,
            menu: None,
            drag_over_path: None,
            dragging_path: None,
            op_error: None,
            epoch: 0,
            show_excluded_files: false,
            git_statuses: GitStatusMap::default(),
            can_search: false,
            can_open_terminal: false,
            platform: Platform::current(),
            animate_menus: true,
            focus_handle: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            remote_poll: None,
            _subscriptions: vec![dirs, activation],
        };
        if let Some(hit) = this.data.peek_dir(&this.cwd, cx) {
            this.children.insert(this.cwd.clone(), hit);
        }
        this.start_remote_poll(cx);
        this.sync(cx);
        this
    }

    // Owner settings.

    /// `rootLabel`: the name to show for the root, such as a worktree's
    /// branch.
    pub fn set_root_label(&mut self, label: Option<String>, cx: &mut Context<Self>) {
        self.root_label = label;
        cx.notify();
    }

    /// `gitStatuses`.
    pub fn set_git_statuses(&mut self, statuses: GitStatusMap, cx: &mut Context<Self>) {
        if self.git_statuses != statuses {
            self.git_statuses = statuses;
            cx.notify();
        }
    }

    /// The `showExcludedFiles` appearance setting.
    pub fn set_show_excluded_files(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_excluded_files != show {
            self.show_excluded_files = show;
            self.sync(cx);
            cx.notify();
        }
    }

    /// Shows the header's search button (`onSearch`).
    pub fn set_search_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.can_search = enabled;
        cx.notify();
    }

    /// Adds "Open in Terminal" to the menu (`onOpenTerminal`).
    pub fn set_open_terminal_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.can_open_terminal = enabled;
        cx.notify();
    }

    /// Turns menu animations off, for screenshots.
    pub fn set_animate_menus(&mut self, animate: bool) {
        self.animate_menus = animate;
    }

    /// The labels follow this platform (Finder or File Explorer, ⌘ or Ctrl).
    pub fn set_platform(&mut self, platform: Platform, cx: &mut Context<Self>) {
        self.platform = platform;
        cx.notify();
    }

    /// Show another folder, as if the tree mounted again.
    pub fn set_cwd(&mut self, cwd: impl Into<String>, cx: &mut Context<Self>) {
        let cwd = cwd.into();
        if cwd == self.cwd {
            return;
        }
        self.expanded = self.data.load_expanded(&cwd, cx);
        self.selected_path = self.data.load_selected(&cwd, cx);
        self.cwd = cwd;
        self.children.clear();
        self.errors.clear();
        self.synced.clear();
        self.loads.clear();
        self.creating = None;
        self.renaming = None;
        self.name_row = None;
        self.clip = None;
        self.menu = None;
        self.op_error = None;
        if let Some(hit) = self.data.peek_dir(&self.cwd, cx) {
            self.children.insert(self.cwd.clone(), hit);
        }
        self.start_remote_poll(cx);
        self.sync(cx);
        cx.notify();
    }

    // Reads, for owners and tests.

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn selected_path(&self) -> Option<&str> {
        self.selected_path.as_deref()
    }

    pub fn expanded(&self) -> &HashSet<String> {
        &self.expanded
    }

    pub fn clip(&self) -> Option<&Clip> {
        self.clip.as_ref()
    }

    /// `opError`: the last failed operation's message.
    pub fn op_error(&self) -> Option<&str> {
        self.op_error.as_deref()
    }

    /// The folder files dropped from the OS would land in.
    pub fn drag_over_path(&self) -> Option<&str> {
        self.drag_over_path.as_deref()
    }

    /// The open context menu's target and items.
    pub fn menu(&self) -> Option<(&MenuTarget, &Entity<ExplorerMenu>)> {
        self.menu.as_ref().map(|menu| (&menu.target, &menu.menu))
    }

    /// The name input of a pending create or rename.
    pub fn name_input(&self) -> Option<&Entity<InputState>> {
        self.name_row.as_ref().map(|row| &row.input)
    }

    /// The root folder's display name.
    pub fn root_name(&self) -> String {
        self.root_label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| basename(&self.cwd))
    }

    /// The rows the tree draws below the root row.
    pub fn rows(&self) -> Vec<TreeRow> {
        let mut out = Vec::new();
        if self.expanded.contains(&self.cwd) {
            self.push_children(&self.cwd, 0, &mut out);
        }
        out
    }

    fn visible<'a>(&self, entries: &'a [FsEntry]) -> impl Iterator<Item = &'a FsEntry> {
        let show = self.show_excluded_files;
        entries.iter().filter(move |entry| show || !entry.ignored)
    }

    /// `TreeChildren`.
    fn push_children(&self, parent: &str, depth: usize, out: &mut Vec<TreeRow>) {
        let entries = self.children.get(parent);
        let error = self.errors.get(parent);
        let creating = self
            .creating
            .as_ref()
            .filter(|creating| creating.parent == parent);
        if let Some(error) = error {
            out.push(TreeRow::Message {
                depth,
                text: error.clone(),
                loading: false,
            });
        }
        if creating.is_some_and(|creating| creating.is_dir) {
            out.push(TreeRow::NameInput {
                depth,
                is_dir: true,
            });
        }
        if entries.is_none() && error.is_none() {
            out.push(TreeRow::Message {
                depth,
                text: "…".into(),
                loading: true,
            });
        }
        let entries = entries.map(Vec::as_slice).unwrap_or_default();
        for entry in self.visible(entries).filter(|entry| entry.is_dir) {
            self.push_node(entry, depth, out);
        }
        if creating.is_some_and(|creating| !creating.is_dir) {
            out.push(TreeRow::NameInput {
                depth,
                is_dir: false,
            });
        }
        for entry in self.visible(entries).filter(|entry| !entry.is_dir) {
            self.push_node(entry, depth, out);
        }
    }

    /// `TreeNode`.
    fn push_node(&self, entry: &FsEntry, depth: usize, out: &mut Vec<TreeRow>) {
        if self.renaming.as_deref() == Some(entry.path.as_str()) {
            out.push(TreeRow::NameInput {
                depth,
                is_dir: entry.is_dir,
            });
        } else {
            out.push(TreeRow::Entry {
                depth,
                entry: entry.clone(),
            });
        }
        if entry.is_dir && self.expanded.contains(&entry.path) {
            self.push_children(&entry.path, depth + 1, out);
        }
    }

    // Listing.

    /// Runs the listing effect of every mounted, open folder that has not
    /// run for the current epoch.
    fn sync(&mut self, cx: &mut Context<Self>) {
        if !self.expanded.contains(&self.cwd) {
            return;
        }
        let mut stack = vec![self.cwd.clone()];
        while let Some(dir) = stack.pop() {
            if self.synced.get(&dir) != Some(&self.epoch) {
                self.synced.insert(dir.clone(), self.epoch);
                self.sync_dir(&dir, cx);
            }
            if let Some(children) = self.children.get(&dir) {
                for child in self.visible(children) {
                    if child.is_dir && self.expanded.contains(&child.path) {
                        stack.push(child.path.clone());
                    }
                }
            }
        }
    }

    /// The root's and each `TreeNode`'s listing effect.
    fn sync_dir(&mut self, dir: &str, cx: &mut Context<Self>) {
        if let Some(hit) = self.data.peek_dir(dir, cx) {
            self.children.insert(dir.to_string(), hit);
            self.errors.remove(dir);
            self.loads.remove(dir);
            return;
        }
        if dir == self.cwd {
            self.children.remove(dir);
            self.errors.remove(dir);
        }
        self.next_load += 1;
        let id = self.next_load;
        let listing = self.data.list_cached_dir(dir, cx);
        let owned = dir.to_string();
        let task = cx.spawn(async move |this, cx| {
            let result = listing.await;
            this.update(cx, |this, cx| {
                if this
                    .loads
                    .get(&owned)
                    .is_none_or(|(current, _)| *current != id)
                {
                    return;
                }
                match result {
                    Ok(entries) => {
                        this.children.insert(owned.clone(), entries);
                        this.errors.remove(&owned);
                    }
                    Err(error) => {
                        this.errors.insert(owned.clone(), error);
                        this.children.insert(owned.clone(), Vec::new());
                    }
                }
                this.sync(cx);
                cx.notify();
            })
            .ok();
        });
        self.loads.insert(dir.to_string(), (id, task));
    }

    /// `subscribeDirsChanged` fired.
    fn dirs_changed(&mut self, cx: &mut Context<Self>) {
        self.epoch += 1;
        self.sync(cx);
        cx.notify();
    }

    fn start_remote_poll(&mut self, cx: &mut Context<Self>) {
        self.remote_poll = None;
        if !self.cwd.starts_with(REMOTE_PROJECT_PREFIX) {
            return;
        }
        self.remote_poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(REMOTE_REFRESH_INTERVAL)
                    .await;
                if this
                    .update(cx, |this, cx| this.data.notify_dirs_changed(cx))
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    /// `isDirAt`.
    fn is_dir_at(&self, path: &str, cx: &App) -> bool {
        if path == self.cwd {
            return true;
        }
        self.data
            .peek_dir(&parent_path(path), cx)
            .and_then(|entries| entries.into_iter().find(|entry| entry.path == path))
            .map(|entry| entry.is_dir)
            .unwrap_or_else(|| self.data.peek_dir(path, cx).is_some())
    }

    // Selection and expansion.

    /// `toggle`.
    pub fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_string());
        } else {
            self.unmount_below(path);
        }
        self.data
            .save_expanded(&self.cwd, self.expanded.clone(), cx);
        self.sync(cx);
        cx.notify();
    }

    /// A collapsed folder's own effect runs again when it reopens; the
    /// nodes below it unmount and lose their listings.
    fn unmount_below(&mut self, path: &str) {
        self.synced.remove(path);
        let below = |key: &String| key != path && is_same_or_inside(key, path);
        self.synced.retain(|key, _| !below(key));
        self.children.retain(|key, _| !below(key));
        self.errors.retain(|key, _| !below(key));
    }

    /// `onSelect`.
    pub fn select(&mut self, path: &str, cx: &mut Context<Self>) {
        self.set_selected(Some(path.to_string()), cx);
    }

    fn set_selected(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        self.selected_path = path.clone();
        self.data.save_selected(&self.cwd, path, cx);
        cx.notify();
    }

    /// `expandDirs`.
    fn expand_dirs(&mut self, dirs: &[String], cx: &mut Context<Self>) {
        for dir in dirs {
            self.expanded.insert(dir.clone());
        }
        self.data
            .save_expanded(&self.cwd, self.expanded.clone(), cx);
        self.sync(cx);
    }

    /// Collapse every folder but the root (the header's Collapse All).
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.creating = None;
        self.renaming = None;
        self.sync_name_row();
        let cwd = self.cwd.clone();
        for path in self.expanded.clone() {
            if path != cwd {
                self.unmount_below(&path);
            }
        }
        self.expanded = HashSet::from([cwd]);
        self.data
            .save_expanded(&self.cwd, self.expanded.clone(), cx);
        self.sync(cx);
        cx.notify();
    }

    /// `remapTreePaths`: follow a rename or move.
    fn remap_tree_paths(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        self.expanded = self
            .expanded
            .iter()
            .map(|path| rebase_path(path, from, to))
            .collect();
        self.data
            .save_expanded(&self.cwd, self.expanded.clone(), cx);
        let selected = self
            .selected_path
            .as_deref()
            .map(|path| rebase_path(path, from, to));
        self.set_selected(selected, cx);
        if let Some(clip) = self.clip.as_mut()
            && is_same_or_inside(&clip.path, from)
        {
            clip.path = rebase_path(&clip.path, from, to);
        }
    }

    /// A click on a row: select it, then toggle a folder or open a file.
    pub fn click_entry(&mut self, path: &str, is_dir: bool, cx: &mut Context<Self>) {
        self.select(path, cx);
        if is_dir {
            self.toggle(path, cx);
        } else {
            cx.emit(FileTreeEvent::OpenFile {
                path: path.to_string(),
                options: FileOpenOptions::EXACT,
            });
        }
    }

    /// A click on the root row.
    pub fn click_root(&mut self, cx: &mut Context<Self>) {
        let cwd = self.cwd.clone();
        self.select(&cwd, cx);
        self.toggle(&cwd, cx);
    }

    // Create and rename.

    /// `startCreate`: a name row in the folder the selection (or `at_path`)
    /// points into.
    pub fn start_create(
        &mut self,
        is_dir: bool,
        at_path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = at_path.or_else(|| self.selected_path.clone());
        let parent = self.data.create_parent_of(&self.cwd, at.as_deref(), cx);
        self.renaming = None;
        self.expand_dirs(&[self.cwd.clone(), parent.clone()], cx);
        self.next_create += 1;
        let id = self.next_create;
        self.creating = Some(Creating {
            id,
            parent: parent.clone(),
            is_dir,
        });
        let siblings = self
            .children
            .get(&parent)
            .map(|entries| entries.iter().map(|entry| entry.name.clone()).collect())
            .unwrap_or_default();
        self.open_name_row(NameTarget::Create(id), "", false, siblings, window, cx);
        cx.notify();
    }

    /// `startRename`.
    pub fn start_rename(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if path == self.cwd {
            return;
        }
        self.creating = None;
        self.menu = None;
        self.select(path, cx);
        self.renaming = Some(path.to_string());
        let name = basename(path);
        let is_dir = self.is_dir_at(path, cx);
        let siblings = self
            .data
            .peek_dir(&parent_path(path), cx)
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.name)
            .filter(|sibling| *sibling != name)
            .collect();
        self.open_name_row(
            NameTarget::Rename(path.to_string()),
            &name,
            !is_dir,
            siblings,
            window,
            cx,
        );
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn open_name_row(
        &mut self,
        target: NameTarget,
        initial: &str,
        select_stem: bool,
        siblings: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial.to_string()));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    if let Some(row) = this.name_row.as_mut() {
                        row.submit_error = None;
                    }
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.finish_name(true, window, cx),
                InputEvent::Blur => {
                    let commit = this
                        .name_row
                        .as_ref()
                        .is_some_and(|row| row.issue(cx).is_none_or(|issue| !issue.is_error()));
                    this.finish_name(commit, window, cx);
                }
                InputEvent::Focus => {}
            },
        );
        let len = initial.len();
        let stem = initial.rfind('.').filter(|dot| *dot > 0).unwrap_or(len);
        input.update(cx, |state, cx| {
            state.focus(window, cx);
            if select_stem {
                state.set_selected_range(0..stem, cx);
            }
        });
        self.name_row = Some(NameRow {
            target,
            input,
            siblings,
            attempted: false,
            busy: false,
            submit_error: None,
            finished: false,
            _subscription: subscription,
        });
    }

    /// Drop the name row when its create or rename ended.
    fn sync_name_row(&mut self) {
        let stale = match self.name_row.as_ref().map(|row| &row.target) {
            Some(NameTarget::Create(id)) => self.creating.as_ref().is_none_or(|c| c.id != *id),
            Some(NameTarget::Rename(path)) => self.renaming.as_deref() != Some(path.as_str()),
            None => false,
        };
        if stale {
            self.name_row = None;
        }
    }

    /// `finish` in `NameRow`.
    fn finish_name(&mut self, success: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.name_row.as_mut() else {
            return;
        };
        if row.finished {
            return;
        }
        let target = row.target.clone();
        if !success {
            row.finished = true;
            match target {
                NameTarget::Create(id) => {
                    if self
                        .creating
                        .as_ref()
                        .is_some_and(|creating| creating.id == id)
                    {
                        self.creating = None;
                    }
                }
                NameTarget::Rename(_) => self.renaming = None,
            }
            self.sync_name_row();
            window.focus(&self.focus_handle, cx);
            cx.notify();
            return;
        }
        let value = row.input.read(cx).value().to_string();
        let issue = validate_file_name(&value, row.siblings.iter().map(String::as_str));
        if issue.is_some_and(|issue| issue.is_error()) {
            row.attempted = true;
            cx.notify();
            return;
        }
        row.finished = true;
        row.busy = true;
        row.submit_error = None;
        let input = row.input.clone();
        input.update(cx, |state, cx| state.set_disabled(true, cx));
        let commit = match &target {
            NameTarget::Create(id) => self.commit_create(*id, value, window, cx),
            NameTarget::Rename(path) => self.commit_rename(path.clone(), value, window, cx),
        };
        cx.spawn_in(window, async move |this, cx| {
            let result = commit.await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => {
                        if this.name_row.is_none() {
                            window.focus(&this.focus_handle, cx);
                        }
                    }
                    Err(error) => {
                        if let Some(row) = this.name_row.as_mut().filter(|row| row.target == target)
                        {
                            row.finished = false;
                            row.busy = false;
                            row.submit_error = Some(error);
                            row.input
                                .update(cx, |state, cx| state.set_disabled(false, cx));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// `onCreateCommit`.
    fn commit_create(
        &mut self,
        id: u64,
        raw: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let Some(session) = self.creating.clone().filter(|creating| creating.id == id) else {
            return Task::ready(Ok(()));
        };
        let as_folder = session.is_dir || raw.ends_with('/') || raw.ends_with('\\');
        let file_name = well_formed_file_name(&raw);
        let create = self
            .data
            .create_path(&session.parent, &file_name, as_folder, cx);
        cx.spawn_in(window, async move |this, cx| {
            let created = create.await?;
            let touched = dirs_touched_by_create(&session.parent, &file_name);
            refresh_touched(&this, touched.clone(), Vec::new(), cx).await?;
            this.update(cx, |this, cx| {
                if this
                    .creating
                    .as_ref()
                    .is_some_and(|creating| creating.id == id)
                {
                    this.creating = None;
                }
                this.sync_name_row();
                this.expand_dirs(&touched, cx);
                this.set_selected(Some(created.clone()), cx);
                if !as_folder {
                    cx.emit(FileTreeEvent::OpenFile {
                        path: created,
                        options: FileOpenOptions::EXACT,
                    });
                }
            })
            .map_err(closed)
        })
    }

    /// `onRenameCommit`.
    fn commit_rename(
        &mut self,
        path: String,
        raw: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let file_name = well_formed_file_name(&raw);
        if file_name.is_empty()
            || (file_name == basename(&path) && !raw.contains('/') && !raw.contains('\\'))
        {
            self.renaming = None;
            self.sync_name_row();
            cx.notify();
            return Task::ready(Ok(()));
        }
        let rename = self.data.rename_path(&path, &file_name, cx);
        let was_dir = self.is_dir_at(&path, cx);
        cx.spawn_in(window, async move |this, cx| {
            let next = rename.await?;
            let parent = parent_path(&path);
            let mut touched = dirs_touched_by_create(&parent, &file_name);
            touched.push(parent.clone());
            let forget = if was_dir {
                vec![path.clone()]
            } else {
                Vec::new()
            };
            refresh_touched(&this, touched, forget, cx).await?;
            this.update(cx, |this, cx| {
                this.renaming = None;
                this.sync_name_row();
                this.expand_dirs(&dirs_touched_by_create(&parent, &file_name), cx);
                this.remap_tree_paths(&path, &next, cx);
                cx.emit(FileTreeEvent::FileMoved {
                    from: path,
                    to: next,
                });
            })
            .map_err(closed)
        })
    }

    // Clipboard, delete, paste, and drops.

    fn copy_text(&self, text: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    }

    /// `run`: an operation whose failure shows above the tree.
    fn run(
        &mut self,
        work: Task<Result<(), String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        self.op_error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            if let Err(error) = work.await {
                this.update(cx, |this, cx| {
                    this.op_error = Some(error);
                    cx.notify();
                })
                .ok();
            }
        })
    }

    /// `removeEntry`: confirm, delete, and move the selection up.
    pub fn remove_entry(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if path == self.cwd {
            return Task::ready(());
        }
        let is_dir = self.is_dir_at(path, cx);
        let label = basename(path);
        let message = if is_dir {
            format!("Delete folder “{label}” and everything inside it?")
        } else {
            format!("Delete “{label}”?")
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            None,
            &["Delete", "Cancel"],
            cx,
        );
        let path = path.to_string();
        let work = cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return Ok(());
            }
            let delete = this
                .update(cx, |this, cx| this.data.delete_path(&path, cx))
                .map_err(closed)?;
            delete.await?;
            let parent = parent_path(&path);
            let forget = if is_dir {
                vec![path.clone()]
            } else {
                Vec::new()
            };
            refresh_touched(&this, vec![parent.clone()], forget, cx).await?;
            this.update(cx, |this, cx| {
                let reset = this
                    .selected_path
                    .as_deref()
                    .is_none_or(|selected| is_same_or_inside(selected, &path));
                if reset {
                    this.set_selected(Some(parent), cx);
                }
                if this
                    .clip
                    .as_ref()
                    .is_some_and(|clip| is_same_or_inside(&clip.path, &path))
                {
                    this.clip = None;
                }
                cx.emit(FileTreeEvent::FileDeleted { path });
            })
            .map_err(closed)
        });
        self.run(work, window, cx)
    }

    /// `pasteAt`: paste the cut or copied entry, or the files a file manager
    /// put on the clipboard, into the folder `target` points into.
    pub fn paste_at(
        &mut self,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let dest = self.data.create_parent_of(&self.cwd, Some(target), cx);
        let work = match self.clip.clone() {
            None => {
                let paths = self.data.clipboard_file_paths(cx);
                cx.spawn_in(window, async move |this, cx| {
                    let paths = paths.await?;
                    copy_external_files(&this, paths, dest, cx).await
                })
            }
            Some(clip) if clip.is_dir && is_same_or_inside(&dest, &clip.path) => {
                Task::ready(Err("Cannot paste a folder into itself.".to_string()))
            }
            Some(clip) => {
                let operation = match clip.mode {
                    ClipMode::Cut => self.data.move_path(&clip.path, &dest, cx),
                    ClipMode::Copy => self.data.copy_path(&clip.path, &dest, cx),
                };
                cx.spawn_in(window, async move |this, cx| {
                    let created = operation.await?;
                    if clip.mode == ClipMode::Cut {
                        let forget = if clip.is_dir {
                            vec![clip.path.clone()]
                        } else {
                            Vec::new()
                        };
                        refresh_touched(
                            &this,
                            dirs_touched_by_move(&clip.path, &created),
                            forget,
                            cx,
                        )
                        .await?;
                        this.update(cx, |this, cx| {
                            this.remap_tree_paths(&clip.path, &created, cx);
                            cx.emit(FileTreeEvent::FileMoved {
                                from: clip.path.clone(),
                                to: created.clone(),
                            });
                            this.clip = None;
                        })
                        .map_err(closed)?;
                    } else {
                        refresh_touched(&this, vec![dest.clone()], Vec::new(), cx).await?;
                    }
                    this.update(cx, |this, cx| {
                        this.expand_dirs(std::slice::from_ref(&dest), cx);
                        this.set_selected(Some(created), cx);
                    })
                    .map_err(closed)
                })
            }
        };
        self.run(work, window, cx)
    }

    /// `duplicateAt`.
    pub fn duplicate_at(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        if path == self.cwd {
            return Task::ready(());
        }
        let dest = parent_path(path);
        let copy = self.data.copy_path(path, &dest, cx);
        let work = cx.spawn_in(window, async move |this, cx| {
            let created = copy.await?;
            refresh_touched(&this, vec![dest], Vec::new(), cx).await?;
            this.update(cx, |this, cx| this.set_selected(Some(created), cx))
                .map_err(closed)
        });
        self.run(work, window, cx)
    }

    /// `dropFiles`: files dropped in from the OS onto `target`.
    pub fn drop_files(
        &mut self,
        paths: Vec<String>,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        self.drag_over_path = None;
        let dest = self.data.create_parent_of(&self.cwd, Some(target), cx);
        let work = cx.spawn_in(window, async move |this, cx| {
            copy_external_files(&this, paths, dest, cx).await
        });
        self.run(work, window, cx)
    }

    /// A file drag from the OS moved to `target` (a row's path, or the
    /// root for the empty area). `None` when it left the tree.
    pub fn drag_over(&mut self, target: Option<&str>, cx: &mut Context<Self>) {
        let next = target.map(|target| self.data.create_parent_of(&self.cwd, Some(target), cx));
        if next != self.drag_over_path {
            self.drag_over_path = next;
            cx.notify();
        }
    }

    fn set_clip(&mut self, mode: ClipMode, path: &str, is_dir: bool, cx: &mut Context<Self>) {
        self.clip = Some(Clip {
            mode,
            path: path.to_string(),
            is_dir,
        });
        cx.notify();
    }

    // The context menu.

    /// `openMenu`.
    pub fn open_menu(
        &mut self,
        target: MenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.creating = None;
        self.renaming = None;
        self.sync_name_row();
        self.select(&target.path, cx);
        let items = explorer_items(
            &target,
            self.clip.as_ref(),
            self.can_open_terminal,
            self.platform,
        );
        let animate = self.animate_menus;
        let menu = cx.new(|cx| {
            ExplorerMenu::new(MenuAnchor::Point(position), items, window, cx).animate(animate)
        });
        let subscription = cx.subscribe_in(
            &menu,
            window,
            |this, _, event: &ExplorerMenuEvent, window, cx| match event {
                ExplorerMenuEvent::Pick(id) => {
                    let Some(open) = this.menu.take() else {
                        return;
                    };
                    window.focus(&this.focus_handle, cx);
                    this.run_action(id, open.target, window, cx);
                    cx.notify();
                }
                ExplorerMenuEvent::Dismiss => {
                    this.menu = None;
                    window.focus(&this.focus_handle, cx);
                    cx.notify();
                }
                ExplorerMenuEvent::Back | ExplorerMenuEvent::Hover(_) => {}
            },
        );
        self.menu = Some(OpenMenu {
            target,
            menu,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Open the context menu for a row, or for the root with `path == cwd`.
    pub fn open_menu_for(
        &mut self,
        path: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_root = path == self.cwd;
        let target = MenuTarget {
            path: path.to_string(),
            is_dir: is_root || self.is_dir_at(path, cx),
            is_root,
        };
        self.open_menu(target, position, window, cx);
    }

    /// `runAction`.
    pub fn run_action(
        &mut self,
        id: &str,
        target: MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match id {
            "new-file" => self.start_create(false, Some(target.path), window, cx),
            "new-folder" => self.start_create(true, Some(target.path), window, cx),
            "cut" if !target.is_root => {
                self.set_clip(ClipMode::Cut, &target.path, target.is_dir, cx)
            }
            "copy" if !target.is_root => {
                self.set_clip(ClipMode::Copy, &target.path, target.is_dir, cx)
            }
            "paste" => self.paste_at(&target.path, window, cx).detach(),
            "duplicate" => self.duplicate_at(&target.path, window, cx).detach(),
            "copy-path" => self.copy_text(&target.path, cx),
            "copy-relative-path" => {
                let relative = display_path(&target.path, Some(&self.cwd));
                self.copy_text(&relative, cx);
            }
            "rename" => self.start_rename(&target.path, window, cx),
            "delete" => self.remove_entry(&target.path, window, cx).detach(),
            "reveal" => {
                let reveal = self.data.reveal_path(&target.path, cx);
                self.run(reveal, window, cx).detach();
            }
            "open-terminal" => {
                let cwd = if target.is_dir {
                    target.path
                } else {
                    parent_path(&target.path)
                };
                cx.emit(FileTreeEvent::OpenTerminal { cwd });
            }
            _ => {}
        }
    }

    // Keys.

    /// Keys typed into the name row belong to the input.
    fn typing(&self, window: &Window, cx: &App) -> bool {
        self.name_row
            .as_ref()
            .is_some_and(|row| row.input.read(cx).focus_handle(cx).is_focused(window))
    }

    fn key_target(&self, cx: &App) -> (String, bool, bool) {
        let path = self
            .selected_path
            .clone()
            .unwrap_or_else(|| self.cwd.clone());
        let is_root = path == self.cwd;
        let is_dir = self.is_dir_at(&path, cx);
        (path, is_root, is_dir)
    }

    fn on_copy_path(&mut self, _: &CopyPath, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, _, _) = self.key_target(cx);
        self.copy_text(&path, cx);
    }

    fn on_copy_entry(&mut self, _: &CopyEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, is_root, is_dir) = self.key_target(cx);
        if !is_root {
            self.set_clip(ClipMode::Copy, &path, is_dir, cx);
        }
    }

    fn on_cut_entry(&mut self, _: &CutEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, is_root, is_dir) = self.key_target(cx);
        if !is_root {
            self.set_clip(ClipMode::Cut, &path, is_dir, cx);
        }
    }

    fn on_paste_entry(&mut self, _: &PasteEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, _, _) = self.key_target(cx);
        self.paste_at(&path, window, cx).detach();
    }

    fn on_rename_entry(&mut self, _: &RenameEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, _, _) = self.key_target(cx);
        self.start_rename(&path, window, cx);
    }

    fn on_delete_entry(&mut self, _: &DeleteEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, _, _) = self.key_target(cx);
        self.remove_entry(&path, window, cx).detach();
    }

    fn on_cancel_cut(&mut self, _: &CancelCut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.typing(window, cx)
            && self
                .clip
                .as_ref()
                .is_some_and(|clip| clip.mode == ClipMode::Cut)
        {
            self.clip = None;
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    /// The entry rows' paths and kinds, in order.
    fn entry_rows(&self) -> Vec<(String, bool)> {
        self.rows()
            .into_iter()
            .filter_map(|row| match row {
                TreeRow::Entry { entry, .. } => Some((entry.path, entry.is_dir)),
                _ => None,
            })
            .collect()
    }

    fn step_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.entry_rows();
        if rows.is_empty() {
            return;
        }
        let current = self
            .selected_path
            .as_deref()
            .and_then(|selected| rows.iter().position(|(path, _)| path == selected));
        let next = match current {
            Some(index) => (index as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
            None if delta < 0 => rows.len() - 1,
            None => 0,
        };
        let path = rows[next].0.clone();
        self.select(&path, cx);
        self.reveal_row(&path);
    }

    /// Scroll so the row for `path` is visible.
    fn reveal_row(&self, path: &str) {
        let offset = usize::from(self.op_error.is_some());
        if let Some(index) = self.rows().iter().position(|row| match row {
            TreeRow::Entry { entry, .. } => entry.path == path,
            _ => false,
        }) {
            self.scroll.scroll_to_item(index + offset);
        }
    }

    fn on_select_next(&mut self, _: &SelectNextEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        self.step_selection(1, cx);
    }

    fn on_select_previous(
        &mut self,
        _: &SelectPreviousEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        self.step_selection(-1, cx);
    }

    fn on_expand(&mut self, _: &ExpandEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, _, is_dir) = self.key_target(cx);
        if !is_dir {
            return;
        }
        if !self.expanded.contains(&path) {
            self.toggle(&path, cx);
        } else {
            self.step_selection(1, cx);
        }
    }

    fn on_collapse(&mut self, _: &CollapseEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, is_root, is_dir) = self.key_target(cx);
        if is_dir && self.expanded.contains(&path) && !is_root {
            self.toggle(&path, cx);
        } else if !is_root {
            let parent = parent_path(&path);
            self.select(&parent, cx);
            self.reveal_row(&parent);
        }
    }

    fn on_open_entry(&mut self, _: &OpenEntry, window: &mut Window, cx: &mut Context<Self>) {
        if self.typing(window, cx) {
            return cx.propagate();
        }
        let (path, is_root, is_dir) = self.key_target(cx);
        if is_root {
            self.click_root(cx);
        } else {
            self.click_entry(&path, is_dir, cx);
        }
    }

    // Rendering.

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let stroke = theme.colors.stroke;
        let header_icon = |id: &'static str, name: IconName, label: SharedString, cx: &App| {
            let theme = Theme::of(cx);
            div()
                .id(id)
                .flex()
                .h(u(24.))
                .min_w_0()
                .flex_1()
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.md))
                .group("explorer-header-icon")
                .hover(|style| style.bg(theme.content(0.05)))
                .tooltip(tooltip(label))
                .child(
                    icon(name)
                        .size(u(14.))
                        .text_color(theme.content(0.50))
                        .group_hover("explorer-header-icon", |style| {
                            style.text_color(theme.colors.content)
                        }),
                )
        };
        let module = self.platform.mod_label();
        div()
            .flex()
            .h(u(36.))
            .flex_none()
            .items_center()
            .gap(px(1.))
            .border_b_1()
            .border_color(stroke)
            .px(u(8.))
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(
                header_icon("new-file", IconName::FilePlus, "New File".into(), cx).on_click(
                    cx.listener(|this, _, window, cx| this.start_create(false, None, window, cx)),
                ),
            )
            .child(
                header_icon("new-folder", IconName::FolderPlus, "New Folder".into(), cx).on_click(
                    cx.listener(|this, _, window, cx| this.start_create(true, None, window, cx)),
                ),
            )
            .child(
                header_icon(
                    "collapse-all",
                    IconName::FoldVertical,
                    "Collapse All".into(),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.collapse_all(cx))),
            )
            .when(self.can_search, |header| {
                header.child(
                    header_icon(
                        "search",
                        IconName::Search,
                        format!("Search in files ({module}Shift+F)").into(),
                        cx,
                    )
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(FileTreeEvent::Search))),
                )
            })
    }

    fn render_root_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let open = self.expanded.contains(&self.cwd);
        let drag_over = self.drag_over_path.as_deref() == Some(self.cwd.as_str());
        let cwd = self.cwd.clone();
        div().flex().h(u(32.)).flex_none().items_center().child(
            div()
                .id("explorer-root")
                .debug_selector(|| "explorer-root".into())
                .flex()
                .min_w_0()
                .flex_1()
                .h_full()
                .items_center()
                .gap(u(4.))
                .pl(u(8.))
                .when(drag_over, |row| row.bg(theme.colors.selection))
                .tooltip(tooltip(self.cwd.clone()))
                .on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus_handle, cx);
                    this.click_root(cx);
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.open_menu_for(&cwd, event.position, window, cx);
                    }),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .size(u(16.))
                        .items_center()
                        .justify_center()
                        .text_color(theme.content(0.50))
                        .child(
                            icon(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(u(14.))
                            .text_color(theme.content(0.50)),
                        ),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_px(theme.text.caption)
                        .semibold()
                        .text_color(theme.content(0.50))
                        .child(self.root_name().to_uppercase()),
                ),
        )
    }

    fn render_message(
        &self,
        depth: usize,
        text: String,
        loading: bool,
        cx: &App,
    ) -> gpui::AnyElement {
        let theme = Theme::of(cx);
        div()
            .pl(u(28. + depth as f32 * 12.))
            .pr(u(8.))
            .text_px(theme.text.label)
            .text_color(theme.content(0.50))
            .when(!loading, |row| row.truncate())
            .child(text)
            .into_any_element()
    }

    fn render_entry(
        &self,
        depth: usize,
        entry: FsEntry,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::of(cx).clone();
        let open = self.expanded.contains(&entry.path);
        let selected = self.selected_path.as_deref() == Some(entry.path.as_str());
        let cut = self
            .clip
            .as_ref()
            .is_some_and(|clip| clip.mode == ClipMode::Cut && clip.path == entry.path);
        let drag_over = self.drag_over_path.as_deref() == Some(entry.path.as_str());
        let dragging = self.dragging_path.as_deref() == Some(entry.path.as_str());
        let status = if entry.is_dir {
            self.git_statuses.dirs.get(&entry.path)
        } else {
            self.git_statuses.files.get(&entry.path)
        };
        let git_color = status.and_then(|status| git_status_color(status, &theme));
        let path = entry.path.clone();
        let is_dir = entry.is_dir;
        let chevron = entry.is_dir.then(|| {
            icon(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(u(14.))
            .text_color(theme.content(0.50))
        });
        let file_icon = if entry.is_dir {
            folder_type_icon(entry.name.clone(), open, false).into_any_element()
        } else {
            file_type_icon(entry.name.clone()).into_any_element()
        };
        let name = div()
            .min_w_0()
            .truncate()
            .map(|label| {
                if entry.ignored {
                    label.italic().text_color(theme.content(0.50))
                } else if let Some(color) = git_color {
                    label.text_color(color)
                } else {
                    label
                }
            })
            .child(entry.name.clone());
        let selection = theme.colors.selection;
        let hover = theme.content(0.05);
        let mut row = div()
            .id(ElementId::Name(format!("tree-row:{}", entry.path).into()))
            .debug_selector({
                let path = entry.path.clone();
                move || format!("tree-row:{path}")
            })
            .flex()
            .w_full()
            .h(u(30.))
            .items_center()
            .gap(u(4.))
            .pl(u(8. + depth as f32 * 12.))
            .pr(u(8.))
            .text_px(theme.text.ui)
            .leading(theme.leading.none)
            .text_color(theme.colors.content)
            .map(|row| {
                if selected || drag_over {
                    row.bg(selection)
                } else {
                    row.hover(move |style| style.bg(hover))
                }
            })
            .when(cut || dragging, |row| row.opacity(0.5))
            .on_click(cx.listener({
                let path = path.clone();
                move |this, event: &ClickEvent, window, cx| {
                    window.focus(&this.focus_handle, cx);
                    this.click_entry(&path, is_dir, cx);
                    if event.click_count() == 2 && !is_dir {
                        cx.emit(FileTreeEvent::OpenFile {
                            path: path.clone(),
                            options: FileOpenOptions::PINNED,
                        });
                    }
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let path = path.clone();
                    move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let target = MenuTarget {
                            path: path.clone(),
                            is_dir,
                            is_root: false,
                        };
                        this.open_menu(target, event.position, window, cx);
                    }
                }),
            )
            .on_drag_move::<ExternalPaths>(cx.listener({
                let path = path.clone();
                move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    if event.bounds.contains(&event.event.position) {
                        this.drag_over(Some(&path), cx);
                    }
                }
            }))
            .on_drop(cx.listener({
                let path = path.clone();
                move |this, paths: &ExternalPaths, window, cx| {
                    let paths = paths
                        .paths()
                        .iter()
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect();
                    this.drop_files(paths, &path, window, cx).detach();
                }
            }));
        if !entry.is_dir {
            let tree = cx.entity().downgrade();
            let dragged = DraggedExplorerFile {
                path: entry.path.clone(),
                name: entry.name.clone(),
            };
            row = row.on_drag(dragged, move |file, offset, _, cx| {
                tree.update(cx, |this, cx| {
                    this.select(&file.path, cx);
                    this.dragging_path = Some(file.path.clone());
                })
                .ok();
                cx.new(|_| ExplorerFileDragPreview {
                    name: file.name.clone(),
                    offset,
                })
            });
        }
        row.child(
            div()
                .flex()
                .flex_none()
                .size(u(16.))
                .items_center()
                .justify_center()
                .text_color(theme.content(0.50))
                .children(chevron),
        )
        .child(div().flex_none().child(file_icon))
        .child(name)
        .into_any_element()
    }

    fn render_name_row(
        &self,
        depth: usize,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = Theme::of(cx).clone();
        let Some(row) = self.name_row.as_ref() else {
            return div().into_any_element();
        };
        let leaf = leaf_name(&row.value(cx));
        let file_icon = if is_dir {
            folder_type_icon(leaf, false, false).into_any_element()
        } else {
            file_type_icon(leaf).into_any_element()
        };
        let issue = row.shown_issue(cx).map(|shown| {
            let (error, (before, name, after)) = match shown {
                Err(message) => (true, (message, None, String::new())),
                Ok(issue) => (issue.is_error(), issue_text(&issue)),
            };
            div()
                .pl(u(28. + depth as f32 * 12.))
                .pr(u(8.))
                .pb(u(4.))
                .text_px(theme.text.label)
                .line_height(u(16.))
                .text_color(if error {
                    theme.colors.danger
                } else {
                    theme.colors.warning
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .child(before)
                        .when_some(name, |line, name| line.child(div().semibold().child(name)))
                        .child(after),
                )
        });
        div()
            .id("name-row")
            .on_action(
                cx.listener(|this, _: &Escape, window, cx| this.finish_name(false, window, cx)),
            )
            .child(
                div()
                    .flex()
                    .w_full()
                    .h(u(30.))
                    .items_center()
                    .gap(u(4.))
                    .pl(u(8. + depth as f32 * 12.))
                    .pr(u(8.))
                    .bg(theme.content(0.10))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .size(u(16.))
                            .items_center()
                            .justify_center()
                            .text_color(theme.content(0.50))
                            .when(is_dir, |chevron| {
                                chevron.child(
                                    icon(IconName::ChevronRight)
                                        .size(u(14.))
                                        .text_color(theme.content(0.50)),
                                )
                            }),
                    )
                    .child(div().flex_none().child(file_icon))
                    .child(
                        div()
                            .flex()
                            .h(u(20.))
                            .min_w_0()
                            .flex_1()
                            .items_center()
                            .rounded(u(theme.radius.sm))
                            .bg(theme.content(0.10))
                            .px(u(4.))
                            .border_1()
                            .border_color(theme.colors.accent)
                            .text_px(theme.text.ui)
                            .text_color(theme.colors.content)
                            .child(
                                div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .child(Input::new(&row.input)),
                            ),
                    ),
            )
            .children(issue)
            .into_any_element()
    }
}

/// `refreshTouched`: forget and re-list the folders an operation changed,
/// then let every mounted folder read the new listings.
async fn refresh_touched(
    this: &WeakEntity<FileTree>,
    touched: Vec<String>,
    forget: Vec<String>,
    cx: &mut AsyncWindowContext,
) -> Result<(), String> {
    let refreshes = this
        .update(cx, |this, cx| {
            for path in &forget {
                this.data.forget_dir(path, cx);
            }
            let mut seen = HashSet::new();
            touched
                .iter()
                .filter(|path| seen.insert(path.as_str()))
                .map(|path| this.data.refresh_dir(path, cx))
                .collect::<Vec<_>>()
        })
        .map_err(closed)?;
    for refresh in refreshes {
        refresh.await?;
    }
    this.update(cx, |this, cx| {
        this.epoch += 1;
        this.sync(cx);
        cx.notify();
    })
    .map_err(closed)
}

/// `copyExternalFiles`: copy each path into `dest`, then select the last
/// copy. A failure part way still refreshes what was copied.
async fn copy_external_files(
    this: &WeakEntity<FileTree>,
    paths: Vec<String>,
    dest: String,
    cx: &mut AsyncWindowContext,
) -> Result<(), String> {
    let mut created = None;
    let mut failure = None;
    for from in paths {
        let copy = this
            .update(cx, |this, cx| this.data.copy_path(&from, &dest, cx))
            .map_err(closed)?;
        match copy.await {
            Ok(path) => created = Some(path),
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    if let Some(created) = created {
        refresh_touched(this, vec![dest.clone()], Vec::new(), cx).await?;
        this.update(cx, |this, cx| {
            this.expand_dirs(std::slice::from_ref(&dest), cx);
            this.set_selected(Some(created), cx);
        })
        .map_err(closed)?;
    }
    failure.map_or(Ok(()), Err)
}

impl Render for FileTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.drag_over_path = None;
            self.dragging_path = None;
        }
        let theme = Theme::of(cx).clone();
        let rows = self.rows();
        let mut list = div()
            .id("explorer-scroll")
            .flex()
            .flex_col()
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                if this.menu.take().is_some() {
                    cx.notify();
                }
            }));
        if let Some(error) = self.op_error.clone() {
            list = list.child(
                div()
                    .flex_none()
                    .px(u(12.))
                    .py(u(4.))
                    .text_px(theme.text.label)
                    .line_height(u(16.))
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        }
        for row in rows {
            let element = match row {
                TreeRow::Message {
                    depth,
                    text,
                    loading,
                } => self.render_message(depth, text, loading, cx),
                TreeRow::Entry { depth, entry } => self.render_entry(depth, entry, cx),
                TreeRow::NameInput { depth, is_dir } => self.render_name_row(depth, is_dir, cx),
            };
            // Rows keep their height; the list scrolls instead of shrinking them.
            list = list.child(div().flex_none().child(element));
        }
        let cwd = self.cwd.clone();
        let menu = self.menu.as_ref().map(|open| open.menu.clone());
        let _ = window;
        div()
            .id("file-tree")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .text_color(theme.colors.content)
            .on_action(cx.listener(Self::on_copy_path))
            .on_action(cx.listener(Self::on_copy_entry))
            .on_action(cx.listener(Self::on_cut_entry))
            .on_action(cx.listener(Self::on_paste_entry))
            .on_action(cx.listener(Self::on_rename_entry))
            .on_action(cx.listener(Self::on_delete_entry))
            .on_action(cx.listener(Self::on_cancel_cut))
            .on_action(cx.listener(Self::on_select_next))
            .on_action(cx.listener(Self::on_select_previous))
            .on_action(cx.listener(Self::on_expand))
            .on_action(cx.listener(Self::on_collapse))
            .on_action(cx.listener(Self::on_open_entry))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let cwd = cwd.clone();
                    move |this, event: &MouseDownEvent, window, cx| {
                        if this.typing(window, cx) {
                            return;
                        }
                        this.open_menu_for(&cwd, event.position, window, cx);
                    }
                }),
            )
            .on_drag_move::<ExternalPaths>(cx.listener({
                let cwd = cwd.clone();
                move |this, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    let inside = event.bounds.contains(&event.event.position);
                    this.drag_over(inside.then_some(cwd.as_str()), cx);
                }
            }))
            .on_drop(cx.listener({
                let cwd = cwd.clone();
                move |this, paths: &ExternalPaths, window, cx| {
                    let paths = paths
                        .paths()
                        .iter()
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect();
                    this.drop_files(paths, &cwd, window, cx).detach();
                }
            }))
            .child(self.render_header(cx))
            .child(self.render_root_row(cx))
            .child(list)
            .children(menu)
    }
}

/// `.explorer-file-drag-preview`: the file's icon and name under the
/// pointer while a file drags out of the tree.
pub struct ExplorerFileDragPreview {
    name: String,
    offset: Point<Pixels>,
}

impl Render for ExplorerFileDragPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let rem = window.rem_size();
        // GPUI draws the drag view at the pointer minus the grab offset; the
        // React preview sat 12 px left of and 13 px above the pointer.
        let left = self.offset.x - u(12.).to_pixels(rem);
        let top = self.offset.y - u(13.).to_pixels(rem);
        let background =
            monocode_ui::color::mix(theme.colors.background_base, theme.colors.content, 0.96);
        div().pl(left.max(px(0.))).pt(top.max(px(0.))).child(
            div()
                .flex()
                .h(u(26.))
                .max_w(u(280.))
                .items_center()
                .gap(u(6.))
                .pl(u(7.))
                .pr(u(9.))
                .overflow_hidden()
                .rounded(u(5.))
                .border_1()
                .border_color(theme.content(0.12))
                .bg(background)
                .shadow_md()
                .opacity(0.94)
                .text_px(theme.text.label)
                .leading(theme.leading.none)
                .text_color(theme.colors.content)
                .child(file_type_icon(self.name.clone()))
                .child(div().min_w_0().truncate().child(self.name.clone())),
        )
    }
}

#[cfg(test)]
#[path = "file_tree_tests.rs"]
mod tests;
