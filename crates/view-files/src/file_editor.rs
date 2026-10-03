//! Port of the `FileEditor` component in src/features/files/ui/FileEditor.tsx:
//! the surface around [`CodeEditor`] that reads the file, writes saves in
//! order, reloads when the file changes on disk, loads the git base of a
//! review tab, follows source navigation, and switches Markdown and SVG
//! files between Preview and Source.
//!
//! The editing itself (line endings, dirty state, autosave, the git gutter,
//! find and replace) is `monocode_editor::CodeEditor`, which ports the
//! `CodeMirrorEditor` part of the same file.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::future::Shared;
use gpui::{
    Anchor, AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, EventEmitter,
    FocusHandle, Focusable, Image, ImageFormat, InteractiveElement, IntoElement, ParentElement,
    Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Task, WeakEntity, Window, anchored, deferred, div, img, point, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{EditorState, InputEvent};
use monocode_editor::doc::{
    LineEnding, detect_line_ending, normalize_line_breaks, restore_line_ending,
};
use monocode_editor::{CodeEditor, CodeEditorEvent, SaveHandler, SaveRequest, StageHandler};
use monocode_layout::GitFileDiffKind;
use monocode_markdown::{LinkClick, MarkdownView};
use monocode_ui::widgets::{PopoverSide, popover_frame};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use crate::data::{EditorNavigation, FilesData};
use crate::markdown_shell::{
    MarkdownViewMode, markdown_view_shell, remember_mode, remembered_mode,
    split_markdown_frontmatter,
};
use crate::paths::{basename, display_path};
use crate::preview_search::{FilePreviewSearch, RunsSource};

/// How long a disk change waits before the reload, like the 50 ms timers.
pub const DISK_RELOAD_DELAY: Duration = Duration::from_millis(50);

/// `isMarkdownPath`.
pub fn is_markdown_path(path: &str) -> bool {
    let name = basename(path).to_lowercase();
    let extension = name.rfind('.').map_or("", |dot| &name[dot..]);
    matches!(extension, ".md" | ".mdx" | ".markdown")
}

/// `isSvgPath`.
pub fn is_svg_path(path: &str) -> bool {
    basename(path).to_lowercase().ends_with(".svg")
}

/// The editor settings the surface follows (`loadAutosave`,
/// `loadFormatOnSave`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorSettings {
    pub autosave: bool,
    pub format_on_save: bool,
}

impl Default for EditorSettings {
    /// `AUTOSAVE_DEFAULT` and `FORMAT_ON_SAVE_DEFAULT`.
    fn default() -> Self {
        Self {
            autosave: false,
            format_on_save: true,
        }
    }
}

/// `LoadState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    Loading,
    Ready,
    Error(String),
}

/// The git side of a review tab (`gitBase`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBase {
    pub path: String,
    /// HEAD or the index, LF line endings.
    pub original: String,
    pub kind: GitFileDiffKind,
    pub line_ending: LineEnding,
    /// The only change is line endings.
    pub eol_only: bool,
}

/// What the surface reports to its pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileEditorEvent {
    /// `onDirtyChange`.
    DirtyChanged(bool),
    /// A link in the Markdown preview asked to open a file.
    OpenFile(String),
    /// `requestAddToChat(editorSelectionContext(selection))`.
    AddToChat(EditorCodeSelection),
}

/// `EditorCodeSelection` from src/features/files/model/editorSelection.ts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCodeSelection {
    /// The path relative to the project (`commentPath`).
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// `EditorSelectionTarget`: a selection and where its head is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionTarget {
    pub selection: EditorCodeSelection,
    pub anchor: Bounds<Pixels>,
    range: Range<usize>,
    scroll_y: Pixels,
}

/// `editorSelectionTarget`: a single, non-blank selection whose head is on
/// screen.
fn selection_target(path: &str, state: &EditorState, text: &str) -> Option<SelectionTarget> {
    let range = state.selected_range();
    if range.is_empty() || range.end > text.len() {
        return None;
    }
    if text.get(range.clone())?.trim().is_empty() {
        return None;
    }
    let head = range.end;
    let anchor = state.range_to_bounds(&(head..head))?;
    let viewport = state.input_bounds();
    if anchor.bottom() < viewport.top()
        || anchor.top() > viewport.bottom()
        || anchor.right() < viewport.left()
        || anchor.left() > viewport.right()
    {
        return None;
    }
    let line_of = |offset: usize| text[..offset].matches('\n').count() + 1;
    let last = range.start.max(range.end - 1);
    Some(SelectionTarget {
        selection: EditorCodeSelection {
            path: path.to_string(),
            start_line: line_of(range.start),
            end_line: line_of(last),
        },
        anchor,
        range,
        scroll_y: state.scroll_offset().y,
    })
}

type SaveQueue = Rc<RefCell<Option<Shared<Task<Result<(), String>>>>>>;

/// The editor surface of one file tab.
pub struct FileEditorSurface {
    data: Rc<dyn FilesData>,
    path: String,
    cwd: String,
    active: bool,
    show_diff: bool,
    settings: EditorSettings,
    state: LoadState,
    editor: Option<Entity<CodeEditor>>,
    load_generation: u64,
    load_task: Option<Task<()>>,
    reload_timer: Option<Task<()>>,
    git_base: Option<GitBase>,
    git_generation: u64,
    git_task: Option<Task<()>>,
    mode: MarkdownViewMode,
    preview: Option<Entity<MarkdownView>>,
    preview_scroll: ScrollHandle,
    preview_search: Option<Entity<FilePreviewSearch>>,
    metadata: Option<String>,
    metadata_open: bool,
    draft: String,
    navigation_token: Option<u64>,
    source_navigation_token: Option<u64>,
    pending_navigation: Option<EditorNavigation>,
    navigated_selection: Option<Range<usize>>,
    save_queue: SaveQueue,
    selection_target: Option<SelectionTarget>,
    window: AnyWindowHandle,
    focus_handle: FocusHandle,
    _watch: Option<Subscription>,
    _git: Option<Subscription>,
    _editor_subscriptions: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FileEditorEvent> for FileEditorSurface {}

impl Focusable for FileEditorSurface {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.editor {
            Some(editor) if self.mode == MarkdownViewMode::Source || !self.has_preview() => {
                editor.read(cx).focus_handle(cx)
            }
            _ => self.focus_handle.clone(),
        }
    }
}

impl FileEditorSurface {
    pub fn new(
        data: Rc<dyn FilesData>,
        path: impl Into<String>,
        cwd: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = path.into();
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() && this.show_diff {
                this.load_git_base(cx);
            }
        });
        let mut this = Self {
            mode: remembered_mode(&path, cx),
            data,
            path,
            cwd: cwd.into(),
            active: false,
            show_diff: false,
            settings: EditorSettings::default(),
            state: LoadState::Loading,
            editor: None,
            load_generation: 0,
            load_task: None,
            reload_timer: None,
            git_base: None,
            git_generation: 0,
            git_task: None,
            preview: None,
            preview_scroll: ScrollHandle::new(),
            preview_search: None,
            metadata: None,
            metadata_open: false,
            draft: String::new(),
            navigation_token: None,
            source_navigation_token: None,
            pending_navigation: None,
            navigated_selection: None,
            save_queue: Rc::default(),
            selection_target: None,
            window: window.window_handle(),
            focus_handle: cx.focus_handle(),
            _watch: None,
            _git: None,
            _editor_subscriptions: Vec::new(),
            _subscriptions: vec![activation],
        };
        this.load(window, cx);
        this
    }

    // Reads.

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn load_state(&self) -> &LoadState {
        &self.state
    }

    pub fn editor(&self) -> Option<&Entity<CodeEditor>> {
        self.editor.as_ref()
    }

    pub fn mode(&self) -> MarkdownViewMode {
        self.mode
    }

    pub fn git_base(&self) -> Option<&GitBase> {
        self.git_base.as_ref().filter(|base| base.path == self.path)
    }

    pub fn preview_search(&self) -> Option<&Entity<FilePreviewSearch>> {
        self.preview_search.as_ref()
    }

    /// `relativePath`: the path under `cwd`, or the whole path.
    pub fn relative_path(&self) -> String {
        self.path
            .strip_prefix(&format!("{}/", self.cwd))
            .map(str::to_string)
            .unwrap_or_else(|| self.path.clone())
    }

    fn markdown(&self) -> bool {
        is_markdown_path(&self.path)
    }

    fn svg(&self) -> bool {
        is_svg_path(&self.path)
    }

    fn has_preview(&self) -> bool {
        self.markdown() || self.svg()
    }

    // Owner settings.

    /// `active`: the tab shows in a focused pane.
    pub fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        self.sync_search_active(cx);
        if active
            && let Some(editor) = self.editor.clone()
            && (!self.has_preview() || self.mode == MarkdownViewMode::Source)
        {
            let handle = editor.read(cx).focus_handle(cx);
            if !handle.contains_focused(window, cx) {
                editor.update(cx, |editor, cx| editor.focus(window, cx));
            }
        }
        cx.notify();
    }

    /// `showDiff`: a review tab shows the git gutter against HEAD or the
    /// index, and reloads it when git or the file changes.
    pub fn set_show_diff(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_diff == show {
            return;
        }
        self.show_diff = show;
        if show {
            let weak = cx.entity().downgrade();
            self._git = Some(self.data.subscribe_git_changed(
                Box::new(move |cx| {
                    weak.update(cx, |this, cx| this.load_git_base(cx)).ok();
                }),
                cx,
            ));
        } else {
            self._git = None;
        }
        self.load_git_base(cx);
    }

    pub fn set_settings(
        &mut self,
        settings: EditorSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings = settings;
        if let Some(editor) = self.editor.clone() {
            editor.update(cx, |editor, cx| {
                editor.set_autosave(settings.autosave, window, cx);
                editor.set_format_on_save(settings.format_on_save);
            });
        }
    }

    /// Show another file in this surface, keeping its save queue, as a
    /// React `FileEditor` did when its `path` changed.
    pub fn set_path(
        &mut self,
        path: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = path.into();
        if path == self.path {
            return;
        }
        self.path = path;
        self.mode = remembered_mode(&self.path, cx);
        self.git_base = None;
        self.load(window, cx);
        if self.show_diff {
            self.load_git_base(cx);
        }
    }

    /// Switch a Markdown or SVG file between Preview and Source.
    pub fn set_mode(
        &mut self,
        mode: MarkdownViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        remember_mode(&self.path, mode, cx);
        self.mode = mode;
        self.sync_search_active(cx);
        if mode == MarkdownViewMode::Source
            && self.active
            && let Some(editor) = self.editor.clone()
        {
            editor.update(cx, |editor, cx| editor.focus(window, cx));
        }
        cx.notify();
    }

    fn sync_search_active(&mut self, cx: &mut Context<Self>) {
        let active = self.active && self.mode == MarkdownViewMode::Preview;
        if let Some(search) = self.preview_search.clone() {
            search.update(cx, |search, cx| search.set_active(active, cx));
        }
    }

    // Loading.

    /// The load effect: read the file, then build a fresh editor.
    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load_generation += 1;
        let generation = self.load_generation;
        self.state = LoadState::Loading;
        self.editor = None;
        self._editor_subscriptions.clear();
        self._watch = None;
        self.reload_timer = None;
        let read = self.data.read_text_file(&self.path, cx);
        self.load_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            this.update_in(cx, |this, window, cx| {
                if generation != this.load_generation {
                    return;
                }
                match result {
                    Ok(text) => this.ready(text, window, cx),
                    Err(error) => this.state = LoadState::Error(error),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Retry after a failed load (`reloadKey`).
    pub fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load(window, cx);
    }

    fn save_handler(&self) -> SaveHandler {
        let queue = self.save_queue.clone();
        let data = self.data.clone();
        Rc::new(move |request: SaveRequest, _: &mut Window, cx: &mut App| {
            let previous = queue.borrow().clone();
            let data = data.clone();
            let write = cx
                .spawn(async move |cx| {
                    if let Some(previous) = previous {
                        let _ = previous.await;
                    }
                    let path = request.path.to_string();
                    let task = cx.update(|cx| data.write_text_file(&path, request.contents, cx));
                    let result = task.await;
                    if result.is_ok() {
                        cx.update(|cx| {
                            data.sync_watched_mtime(&path, cx);
                            data.notify_git_changed(cx);
                        });
                    }
                    result
                })
                .shared();
            *queue.borrow_mut() = Some(write.clone());
            cx.spawn(async move |_| write.await.map_err(anyhow::Error::msg))
        })
    }

    /// `stageGit`: write the hunk's new index contents.
    fn stage_handler(&self) -> Option<StageHandler> {
        let base = self.git_base()?.clone();
        if !self.show_diff || base.kind != GitFileDiffKind::Unstaged {
            return None;
        }
        let data = self.data.clone();
        let cwd = self.cwd.clone();
        let relative = display_path(&self.path, Some(&cwd));
        let valid = !cwd.is_empty() && cwd != "~" && !relative.is_empty() && relative != self.path;
        Some(Rc::new(
            move |contents: String, _: &mut Window, cx: &mut App| {
                if !valid {
                    return Task::ready(Err(anyhow::anyhow!("Can't stage this file")));
                }
                let contents = restore_line_ending(&contents, base.line_ending);
                let stage = data.git_stage_contents(&cwd, &relative, contents, cx);
                let data = data.clone();
                cx.spawn(async move |cx| {
                    stage.await.map_err(anyhow::Error::msg)?;
                    cx.update(|cx| data.notify_git_changed(cx));
                    Ok(())
                })
            },
        ))
    }

    /// `applyDiskContent` and the editor's mount.
    fn ready(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.state = LoadState::Ready;
        let theme = crate::editor_theme(cx);
        let relative: SharedString = self.relative_path().into();
        let save = self.save_handler();
        let settings = self.settings;
        let path = self.path.clone();
        let editor = cx.new(|cx| {
            let mut editor = CodeEditor::new(path, &text, theme, window, cx);
            editor.set_footer(Some(relative), cx);
            editor.on_save(save);
            editor.set_autosave(settings.autosave, window, cx);
            editor.set_format_on_save(settings.format_on_save);
            editor
        });
        let state = editor.read(cx).editor_state().clone();
        self._editor_subscriptions = vec![
            cx.subscribe_in(&editor, window, Self::on_editor_event),
            cx.subscribe(&state, |this, _, event: &InputEvent, _| {
                if let InputEvent::Blur = event {
                    this.pending_navigation = None;
                }
            }),
            cx.observe(&state, |this, state, cx| {
                if this.pending_navigation.is_some()
                    && this.navigated_selection.as_ref() != Some(&state.read(cx).selected_range())
                {
                    this.pending_navigation = None;
                }
                this.sync_selection_target(&state, cx);
            }),
        ];
        self.editor = Some(editor);
        self.apply_git_base(cx);
        self.draft = normalize_line_breaks(&text);
        if self.markdown() && self.preview.is_none() {
            self.build_preview(window, cx);
        } else {
            self.update_preview(cx);
        }
        let weak = cx.entity().downgrade();
        self._watch = Some(self.data.watch_file(
            &self.path,
            Box::new(move |cx| {
                weak.update(cx, |this, cx| this.disk_changed(cx)).ok();
            }),
            cx,
        ));
        if self.active
            && (!self.has_preview() || self.mode == MarkdownViewMode::Source)
            && let Some(editor) = self.editor.clone()
        {
            editor.update(cx, |editor, cx| editor.focus(window, cx));
        }
        self.apply_pending_navigation(window, cx);
    }

    fn on_editor_event(
        &mut self,
        editor: &Entity<CodeEditor>,
        event: &CodeEditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            CodeEditorEvent::Changed => {
                self.pending_navigation = None;
                let text = editor.read(cx).text(cx);
                self.set_draft(text, cx);
            }
            CodeEditorEvent::DirtyChanged(dirty) => cx.emit(FileEditorEvent::DirtyChanged(*dirty)),
            CodeEditorEvent::ReloadRequested => self.reload_from_disk(false, window, cx),
            CodeEditorEvent::Saved
            | CodeEditorEvent::SaveFailed(_)
            | CodeEditorEvent::HunkReverted(_) => {}
        }
    }

    /// `setDraft`: the Markdown preview follows the buffer.
    fn set_draft(&mut self, draft: String, cx: &mut Context<Self>) {
        if draft == self.draft {
            return;
        }
        self.draft = draft;
        self.update_preview(cx);
    }

    fn update_preview(&mut self, cx: &mut Context<Self>) {
        if !self.markdown() {
            return;
        }
        let parts = split_markdown_frontmatter(&self.draft);
        self.metadata = parts.metadata;
        if let Some(preview) = self.preview.clone() {
            preview.update(cx, |preview, cx| preview.set_text(&parts.body, cx));
            if let Some(search) = self.preview_search.clone() {
                search.update(cx, |search, cx| search.content_changed(cx));
            }
        }
    }

    /// The Markdown preview and its find bar, built once per surface.
    fn build_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parts = split_markdown_frontmatter(&self.draft);
        self.metadata = parts.metadata;
        let preview = cx.new(|cx| MarkdownView::with_text(parts.body, cx));
        let weak = cx.entity().downgrade();
        let cwd = self.cwd.clone();
        preview.update(cx, |preview, _| {
            preview.on_link_click(move |link: &LinkClick, _, cx| {
                open_link(&weak, link, &cwd, cx);
            });
        });
        let source_view = preview.clone();
        let source: RunsSource = Rc::new(move |cx: &App| source_view.read(cx).rendered_text());
        let surface = cx.entity().downgrade();
        let scroll = self.preview_scroll.clone();
        let content = cx.new(|_| PreviewContent {
            preview: preview.clone(),
            scroll: scroll.clone(),
            surface,
        });
        let search =
            cx.new(|cx| FilePreviewSearch::new(content.into(), source, Some(scroll), window, cx));
        self.preview = Some(preview);
        self.preview_search = Some(search);
        self.sync_search_active(cx);
    }

    /// A disk change from the file watch.
    fn disk_changed(&mut self, cx: &mut Context<Self>) {
        let dirty = self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.read(cx).is_dirty());
        let delay = if dirty {
            Duration::ZERO
        } else {
            DISK_RELOAD_DELAY
        };
        let window = self.window;
        let timer = cx.background_executor().timer(delay);
        self.reload_timer = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            timer.await;
            window
                .update(cx, |_, window, cx| {
                    this.update(cx, |this, cx| {
                        if this.show_diff {
                            this.load_git_base(cx);
                        }
                        this.reload_from_disk(false, window, cx);
                    })
                    .ok();
                })
                .ok();
        }));
    }

    /// `reloadFromDisk`. The editor keeps a dirty buffer and notes the
    /// disk change; `force` replaces it anyway.
    pub fn reload_from_disk(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.load_generation += 1;
        let generation = self.load_generation;
        let read = self.data.read_text_file(&self.path, cx);
        self.load_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            this.update_in(cx, |this, window, cx| {
                if generation != this.load_generation {
                    return;
                }
                let Some(editor) = this.editor.clone() else {
                    if let Ok(text) = result {
                        this.ready(text, window, cx);
                    }
                    return;
                };
                let dirty = editor.read(cx).is_dirty();
                match result {
                    Ok(text) => {
                        editor.update(cx, |editor, cx| editor.reload(&text, force, window, cx));
                        if !dirty || force {
                            let text = editor.read(cx).text(cx);
                            this.set_draft(text, cx);
                            if this.pending_navigation.is_some() {
                                this.navigated_selection =
                                    Some(editor.read(cx).editor_state().read(cx).selected_range());
                                this.apply_pending_navigation(window, cx);
                            }
                        }
                    }
                    Err(error) => {
                        if !dirty || force {
                            this.state = LoadState::Error(error);
                            this.editor = None;
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    // Git.

    /// The git base effect of a review tab.
    fn load_git_base(&mut self, cx: &mut Context<Self>) {
        self.git_generation += 1;
        let generation = self.git_generation;
        let relative = display_path(&self.path, Some(&self.cwd));
        if !self.show_diff
            || self.cwd.is_empty()
            || self.cwd == "~"
            || relative.is_empty()
            || relative == self.path
        {
            self.git_task = None;
            self.set_git_base(None, cx);
            return;
        }
        let files = self.data.git_diff_files(&self.cwd, cx);
        let data = self.data.clone();
        let cwd = self.cwd.clone();
        let path = self.path.clone();
        self.git_task = Some(cx.spawn(async move |this, cx| {
            let result: Result<Option<GitBase>, String> = async {
                let index = files.await?;
                let file = index
                    .files
                    .iter()
                    .find(|entry| entry.relative == relative)
                    .cloned();
                let kind = if file
                    .as_ref()
                    .is_some_and(|file| file.staged && !file.unstaged)
                {
                    GitFileDiffKind::Staged
                } else {
                    GitFileDiffKind::Unstaged
                };
                let diff = cx
                    .update(|cx| {
                        data.git_file_diff(&cwd, &relative, kind == GitFileDiffKind::Staged, cx)
                    })
                    .await?;
                if diff.binary || diff.too_large {
                    return Ok(None);
                }
                let original = normalize_line_breaks(&diff.original);
                let changed = file
                    .as_ref()
                    .is_some_and(|file| file.staged || file.unstaged);
                let eol_only = changed
                    && diff.original != diff.current
                    && original == normalize_line_breaks(&diff.current);
                let source = if diff.original.is_empty() {
                    &diff.current
                } else {
                    &diff.original
                };
                Ok(Some(GitBase {
                    path,
                    line_ending: detect_line_ending(source),
                    original,
                    kind,
                    eol_only,
                }))
            }
            .await;
            this.update(cx, |this, cx| {
                if generation == this.git_generation {
                    this.set_git_base(result.ok().flatten(), cx);
                }
            })
            .ok();
        }));
    }

    fn set_git_base(&mut self, base: Option<GitBase>, cx: &mut Context<Self>) {
        self.git_base = base;
        self.apply_git_base(cx);
        cx.notify();
    }

    fn apply_git_base(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.clone() else {
            return;
        };
        let original = if self.show_diff {
            self.git_base().map(|base| base.original.clone())
        } else {
            None
        };
        let stage = self.stage_handler();
        editor.update(cx, |editor, cx| {
            editor.set_git_base(original.as_deref(), cx);
            editor.on_stage_hunk(stage);
        });
    }

    // Navigation.

    /// `navigation`: reveal a line and column. A clamped location stays
    /// pending until its line arrives, the user moves the cursor or edits,
    /// or focus leaves the editor. A new token asks again.
    pub fn set_navigation(
        &mut self,
        navigation: Option<EditorNavigation>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(navigation) = navigation else {
            self.pending_navigation = None;
            return;
        };
        if self.has_preview() && self.source_navigation_token != Some(navigation.token) {
            self.source_navigation_token = Some(navigation.token);
            self.set_mode(MarkdownViewMode::Source, window, cx);
        }
        if self.navigation_token != Some(navigation.token) {
            self.navigation_token = Some(navigation.token);
            self.pending_navigation = Some(navigation);
        }
        self.apply_pending_navigation(window, cx);
    }

    /// `revealNavigation` for the pending target.
    fn apply_pending_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(pending), Some(editor)) = (self.pending_navigation.clone(), self.editor.clone())
        else {
            return;
        };
        let lines = editor.read(cx).text(cx).matches('\n').count() + 1;
        editor.update(cx, |editor, cx| {
            editor.reveal_position(pending.line, pending.column, window, cx)
        });
        self.navigated_selection = Some(editor.read(cx).editor_state().read(cx).selected_range());
        if pending.line <= lines {
            self.pending_navigation = None;
        }
    }

    /// The Add to chat target: kept while the selection and the scroll
    /// stay put, dropped when either moves, like the React listener for
    /// scroll and resize.
    fn sync_selection_target(&mut self, state: &Entity<EditorState>, cx: &mut Context<Self>) {
        let state = state.read(cx);
        let range = state.selected_range();
        let scroll_y = state.scroll_offset().y;
        let next = if !self.active || (self.has_preview() && self.mode != MarkdownViewMode::Source)
        {
            None
        } else if let Some(current) = self
            .selection_target
            .as_ref()
            .filter(|current| current.range == range)
        {
            (current.scroll_y == scroll_y).then(|| current.clone())
        } else {
            selection_target(&self.relative_path(), state, &state.value())
        };
        if next != self.selection_target {
            self.selection_target = next;
            cx.notify();
        }
    }

    /// The selection the Add to chat button would send.
    pub fn selection_target(&self) -> Option<&SelectionTarget> {
        self.selection_target.as_ref()
    }

    /// `EditorSelectionMenu`: Add to chat, above the selection's head.
    fn render_selection_menu(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let target = self.selection_target.clone()?;
        let theme = Theme::of(cx).clone();
        let selection = target.selection.clone();
        let button = div()
            .id("add-to-chat")
            .flex()
            .h(u(28.))
            .items_center()
            .gap(u(6.))
            .rounded(u(theme.radius.lg))
            .px(u(8.))
            .font_family(theme.fonts.sans.clone())
            .text_px(theme.text.body)
            .leading(theme.leading.none)
            .text_color(theme.colors.content)
            .hover(|style| style.bg(theme.content(0.05)))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.emit(FileEditorEvent::AddToChat(selection.clone()));
                this.selection_target = None;
                cx.notify();
            }))
            .child(
                icon(IconName::MessageSquarePlus)
                    .size(u(14.))
                    .text_color(theme.colors.content),
            )
            .child("Add to chat");
        let menu = popover_frame("editor-selection-menu")
            .side(PopoverSide::Top)
            .child(div().p(u(4.)).child(button));
        let anchor = target.anchor;
        Some(
            deferred(
                anchored()
                    .position(point(anchor.center().x, anchor.top() - px(6.)))
                    .anchor(Anchor::BottomCenter)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(theme.layer.popover)
            .into_any_element(),
        )
    }

    // Rendering.

    fn render_message(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::of(cx).clone();
        let name = basename(&self.path);
        match &self.state {
            LoadState::Loading | LoadState::Ready => div()
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_px(theme.text.label)
                .text_color(theme.content(0.45))
                .child(format!("Opening {name}…"))
                .into_any_element(),
            LoadState::Error(message) => div()
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .p(u(24.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .max_w(u(448.))
                        .child(
                            icon(IconName::AlertCircle)
                                .size(u(20.))
                                .mb(u(12.))
                                .text_color(theme.colors.danger),
                        )
                        .child(
                            div()
                                .text_px(theme.text.body)
                                .text_color(theme.colors.content)
                                .child(format!("Couldn’t open {name}")),
                        )
                        .child(
                            div()
                                .mt(u(4.))
                                .text_px(theme.text.label)
                                .line_height(u(20.))
                                .text_color(theme.content(0.50))
                                .child(message.clone()),
                        )
                        .child(
                            div()
                                .id("retry")
                                .mt(u(16.))
                                .flex()
                                .h(u(28.))
                                .items_center()
                                .gap(u(6.))
                                .rounded(u(theme.radius.md))
                                .bg(theme.content(0.10))
                                .px(u(10.))
                                .text_px(theme.text.label)
                                .text_color(theme.colors.content)
                                .hover(|style| style.bg(theme.content(0.15)))
                                .on_click(cx.listener(|this, _, window, cx| this.retry(window, cx)))
                                .child(
                                    icon(IconName::RotateCcw)
                                        .size(u(12.))
                                        .text_color(theme.colors.content),
                                )
                                .child("Retry"),
                        ),
                )
                .into_any_element(),
        }
    }

    fn render_svg(&self) -> gpui::AnyElement {
        let image = Arc::new(Image::from_bytes(
            ImageFormat::Svg,
            self.draft.clone().into_bytes(),
        ));
        div()
            .id("svg-preview")
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .overflow_scroll()
            .p(u(24.))
            .child(img(image).max_w_full().max_h_full())
            .into_any_element()
    }
}

/// A link click in the preview: open project files in the editor.
fn open_link(surface: &WeakEntity<FileEditorSurface>, link: &LinkClick, cwd: &str, cx: &mut App) {
    let url = link.url.to_string();
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
        cx.open_url(&url);
        return;
    }
    if let Some(path) = monocode_core::transcript::paths::resolve_workspace_path(&url, Some(cwd)) {
        surface
            .update(cx, |_, cx| cx.emit(FileEditorEvent::OpenFile(path)))
            .ok();
    }
}

/// `MarkdownDocumentPreview`: the scrolling preview with the Properties
/// disclosure for front matter.
struct PreviewContent {
    preview: Entity<MarkdownView>,
    scroll: ScrollHandle,
    surface: WeakEntity<FileEditorSurface>,
}

impl Render for PreviewContent {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let (metadata, open) = self
            .surface
            .upgrade()
            .map(|surface| {
                let surface = surface.read(cx);
                (surface.metadata.clone(), surface.metadata_open)
            })
            .unwrap_or_default();
        let surface = self.surface.clone();
        let header = metadata.map(|metadata| {
            div()
                .mb(u(24.))
                .rounded(u(theme.radius.lg))
                .border_1()
                .border_color(theme.content(0.10))
                .bg(theme.content(0.03))
                .child(
                    div()
                        .id("properties")
                        .flex()
                        .items_center()
                        .gap(u(6.))
                        .rounded(u(theme.radius.lg))
                        .px(u(12.))
                        .py(u(8.))
                        .text_px(theme.text.label)
                        .text_color(theme.content(0.60))
                        .hover(|style| style.text_color(theme.colors.content))
                        .cursor_pointer()
                        .on_click(move |_, _, cx| {
                            surface
                                .update(cx, |surface, cx| {
                                    surface.metadata_open = !surface.metadata_open;
                                    cx.notify();
                                })
                                .ok();
                        })
                        .child(
                            icon(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(u(14.))
                            .text_color(theme.content(0.50)),
                        )
                        .child("Properties"),
                )
                .when(open, |details| {
                    details.child(
                        div()
                            .border_t_1()
                            .border_color(theme.colors.stroke)
                            .px(u(12.))
                            .py(u(8.))
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.label)
                            .line_height(u(20.))
                            .text_color(theme.content(0.70))
                            .child(metadata),
                    )
                })
        });
        div()
            .id("markdown-preview")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(
                div()
                    .px(u(24.))
                    .py(u(32.))
                    .children(header)
                    .child(self.preview.clone()),
            )
    }
}

impl Render for FileEditorSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let Some(editor) = self
            .editor
            .clone()
            .filter(|_| self.state == LoadState::Ready)
        else {
            return div()
                .track_focus(&self.focus_handle)
                .size_full()
                .child(self.render_message(cx));
        };
        let notice = self
            .git_base()
            .filter(|base| self.show_diff && base.eol_only)
            .map(|base| {
                let side = if base.kind == GitFileDiffKind::Staged {
                    "Staged"
                } else {
                    "Unstaged"
                };
                div()
                    .flex_none()
                    .border_b_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .py(u(4.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.60))
                    .child(format!(
                        "{side} line-ending changes. Line breaks are normalized in this view."
                    ))
            });
        let body = if self.has_preview() {
            let preview = if self.markdown() {
                self.preview_search
                    .clone()
                    .map(|search| search.into_any_element())
                    .unwrap_or_else(|| div().into_any_element())
            } else {
                self.render_svg()
            };
            let find_open = self
                .preview_search
                .as_ref()
                .is_some_and(|search| search.read(cx).is_open());
            let weak = cx.entity().downgrade();
            markdown_view_shell(
                self.mode,
                move |mode, window, cx| {
                    weak.update(cx, |this, cx| this.set_mode(mode, window, cx))
                        .ok();
                },
                preview,
                div().flex().flex_col().size_full().child(editor),
            )
            .find_open(find_open && self.mode == MarkdownViewMode::Preview)
            .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .min_h_0()
                .flex_1()
                .child(editor)
                .into_any_element()
        };
        let selection_menu = self.render_selection_menu(cx);
        div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .children(notice)
            .child(body)
            .children(selection_menu)
    }
}

#[cfg(test)]
#[path = "file_editor_tests.rs"]
mod tests;
