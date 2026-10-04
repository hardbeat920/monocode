//! Port of `CodeMirrorEditor` and the editing parts of `FileEditor` in
//! src/features/files/ui/FileEditor.tsx.
//!
//! The text engine is gpui-base's `EditorState`, drawn unstyled with colors
//! from [`EditorTheme`]. On top of it this view adds what the CodeMirror
//! extensions did: the git gutter and change navigation (editorGit.ts), the
//! find and replace bar (editorSearch.ts), save with dirty tracking and
//! autosave, and disk reloads that keep the cursor.
//!
//! The view does no IO. The caller passes file contents in, receives save
//! requests through [`CodeEditor::on_save`], and passes the git base text
//! (HEAD or the index) through [`CodeEditor::set_git_base`].

use std::{ops::Range, rc::Rc, sync::Arc, time::Duration};

use gpui::{
    App, AppContext as _, Context, Edges, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyBinding, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Task, Window, actions, div, point,
    prelude::FluentBuilder as _, px,
};
use gpui_base::input::{DocumentColorProvider, EditorState, InputEvent, Rope, RopeExt as _};

use crate::{
    doc::{
        LineEnding, detect_line_ending, map_selection, normalize_line_breaks, restore_line_ending,
    },
    find_bar::FindBar,
    git_diff::{
        ChangeKind, Chunk, Doc, TextChange, TextRange, deleted_line_texts, diff_line_stats,
        marked_lines, revert_chunk_change, stage_chunk_text, widget_pos,
    },
    highlighter::highlighter_factory,
    icons::{IconKind, icon},
    language::{basename, language_for_path},
    search::SearchQuery,
    theme::EditorTheme,
    unified_diff::{DiffCommentTarget, UnifiedLine, UnifiedLineKind},
};

/// `FILE_EDITOR_AUTOSAVE_DELAY_MS`.
pub const AUTOSAVE_DELAY: Duration = Duration::from_millis(1_000);

/// Width of the strip left of the line numbers that holds the git markers.
pub(crate) const GIT_GUTTER_WIDTH: f32 = 14.;

pub(crate) const KEY_CONTEXT: &str = "CodeEditor";

actions!(
    code_editor,
    [
        /// Save the buffer through the save callback.
        Save,
        FormatDocument,
        /// Turn line wrapping on or off.
        ToggleSoftWrap,
        /// Open the find bar with the replace row.
        OpenReplace,
        FindNext,
        FindPrevious,
        /// Jump to the next git change.
        NextChange,
        /// Jump to the previous git change.
        PreviousChange,
    ]
);

/// Bind the editor's keys. Call once at startup, after `gpui_component::init`.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-s", Save, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-z", ToggleSoftWrap, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-i", FormatDocument, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-alt-f", OpenReplace, Some(KEY_CONTEXT)),
        KeyBinding::new("f3", FindNext, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f3", FindPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-g", FindNext, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-shift-g", FindPrevious, Some(KEY_CONTEXT)),
    ]);
    crate::find_bar::init(cx);
}

/// What the save callback receives.
#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub path: SharedString,
    /// The buffer with the file's own line endings restored.
    pub contents: String,
    /// True for an autosave.
    pub automatic: bool,
}

/// Runs while the editor is being updated, so it must not update the editor
/// itself; do the work in the returned task.
pub type SaveHandler = Rc<dyn Fn(SaveRequest, &mut Window, &mut App) -> Task<anyhow::Result<()>>>;
/// Receives the new index contents (LF line endings) for a staged hunk. Like
/// [`SaveHandler`], it must not update the editor synchronously.
pub type StageHandler = Rc<dyn Fn(String, &mut Window, &mut App) -> Task<anyhow::Result<()>>>;
/// Comment and revert callbacks run on the next effect cycle.
pub type CommentHandler = Rc<dyn Fn(DiffCommentTarget, &mut Window, &mut App)>;
pub type RevertHandler = Rc<dyn Fn(TextChange, &mut Window, &mut App)>;
/// `formatText`: `(path, text, cursor)` to `(formatted, cursor)`, or `None`
/// when the file type has no formatter.
pub type Formatter = Rc<dyn Fn(&str, &str, usize) -> Option<(String, usize)>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeEditorEvent {
    /// The buffer changed (`onDocChange`).
    Changed,
    DirtyChanged(bool),
    Saved,
    SaveFailed(String),
    /// A disk change arrived while the buffer was dirty, and the buffer is
    /// clean again. Read the file and call [`CodeEditor::reload`].
    ReloadRequested,
    /// A hunk was reverted in the buffer.
    HunkReverted(TextChange),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveState {
    Idle,
    Saving,
    Saved,
    Error(String),
}

/// Hands the search matches to the editor as document colors.
struct SearchColors(Rc<std::cell::RefCell<Vec<lsp_types::ColorInformation>>>);

impl DocumentColorProvider for SearchColors {
    fn document_colors(
        &self,
        _: &Rope,
        _: &mut Window,
        _: &mut App,
    ) -> Task<anyhow::Result<Vec<lsp_types::ColorInformation>>> {
        Task::ready(Ok(self.0.borrow().clone()))
    }
}

/// Hunks between the git base and the buffer, computed off the main thread.
#[derive(Debug, Default)]
pub(crate) struct GitSnapshot {
    pub chunks: Vec<Chunk>,
    /// Removed lines of each chunk.
    pub deleted: Vec<Vec<String>>,
    /// Line number in the base of each chunk's first removed line.
    pub first_old_line: Vec<usize>,
    /// Buffer lines (0-based, end exclusive) each chunk marks in the gutter.
    pub marked: Vec<Range<usize>>,
    /// `widgetPos` of each chunk.
    pub positions: Vec<usize>,
    pub additions: usize,
    pub deletions: usize,
}

impl GitSnapshot {
    fn compute(base: &str, text: &str) -> Self {
        let chunks = crate::git_diff::chunks_for(base, text);
        let doc = Doc::new(text);
        let original = Doc::new(base);
        let deleted = chunks
            .iter()
            .map(|chunk| deleted_line_texts(&original, chunk))
            .collect();
        let first_old_line = chunks
            .iter()
            .map(|chunk| original.line_at(chunk.from_a.min(original.len())).number)
            .collect();
        let marked = chunks
            .iter()
            .map(|chunk| {
                let lines = marked_lines(&doc, chunk);
                lines.start - 1..lines.end - 1
            })
            .collect();
        let positions = chunks.iter().map(|chunk| widget_pos(&doc, chunk)).collect();
        let (additions, deletions) = diff_line_stats(&doc, &chunks, Some(&original));
        Self {
            chunks,
            deleted,
            first_old_line,
            marked,
            positions,
            additions,
            deletions,
        }
    }

    pub fn kind(&self, index: usize) -> ChangeKind {
        self.chunks[index].kind()
    }
}

/// The source editor pane.
pub struct CodeEditor {
    path: SharedString,
    language: Option<&'static str>,
    pub(crate) state: Entity<EditorState>,
    pub(crate) theme: EditorTheme,
    saved: Rope,
    dirty: bool,
    line_ending: LineEnding,
    read_only: bool,
    soft_wrap: bool,
    show_footer: bool,
    relative_path: Option<SharedString>,
    save_state: SaveState,
    save_generation: u64,
    autosave: bool,
    autosave_task: Option<Task<()>>,
    pending_disk: bool,
    format_on_save: bool,
    formatter: Option<Formatter>,
    on_save: Option<SaveHandler>,
    // Git gutter.
    git_base: Option<Arc<str>>,
    pub(crate) git: Arc<GitSnapshot>,
    git_task: Option<Task<()>>,
    pub(crate) peek: Option<usize>,
    pub(crate) stage_busy: bool,
    chunk_nav_pinned: Option<usize>,
    last_scroll_y: Option<gpui::Pixels>,
    on_stage_hunk: Option<StageHandler>,
    on_revert_hunk: Option<RevertHandler>,
    on_comment: Option<CommentHandler>,
    // Find and replace.
    pub(crate) find: FindBar,
    /// Search match backgrounds, painted behind the text by the editor.
    search_colors: Rc<std::cell::RefCell<Vec<lsp_types::ColorInformation>>>,
    /// The selection the current-match highlight was drawn for.
    decorated_selection: Option<Range<usize>>,
    /// A reveal requested before the editor's first layout.
    pending_reveal: Option<(Range<usize>, bool)>,
    /// A reveal to move to the middle of the viewport on the next frame.
    pending_center: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CodeEditorEvent> for CodeEditor {}

impl CodeEditor {
    /// Open `text` (as read from disk) for `path`.
    pub fn new(
        path: impl Into<SharedString>,
        text: &str,
        theme: EditorTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path: SharedString = path.into();
        let language = language_for_path(&path);
        let line_ending = detect_line_ending(text);
        let content = normalize_line_breaks(text);
        let state = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language.unwrap_or("text"))
                .line_number(true)
                .folding(true)
                .indent_guides(false)
                .soft_wrap(true)
                .searchable(false)
                .default_value(content.clone())
        });
        let search_colors = Rc::new(std::cell::RefCell::new(Vec::new()));
        state.update(cx, |state, cx| {
            state.set_highlighter_factory(highlighter_factory(), cx);
            state.set_editor_style(theme.input_style(language.unwrap_or("text")));
            state.set_editor_paddings(Edges {
                top: px(8.),
                right: px(12.),
                bottom: px(8.),
                left: px(GIT_GUTTER_WIDTH),
            });
            state.lsp_mut().document_color_provider =
                Some(Rc::new(SearchColors(search_colors.clone())));
        });
        let saved = state.read(cx).text().clone();
        let find = FindBar::new(&theme, window, cx);
        let mut subscriptions = vec![cx.subscribe_in(&state, window, Self::on_editor_event)];
        subscriptions.extend(find.subscribe(window, cx));
        Self {
            path,
            language,
            state,
            theme,
            saved,
            dirty: false,
            line_ending,
            read_only: false,
            soft_wrap: true,
            show_footer: false,
            relative_path: None,
            save_state: SaveState::Idle,
            save_generation: 0,
            autosave: false,
            autosave_task: None,
            pending_disk: false,
            format_on_save: true,
            formatter: None,
            on_save: None,
            git_base: None,
            git: Arc::new(GitSnapshot::default()),
            git_task: None,
            peek: None,
            stage_busy: false,
            chunk_nav_pinned: None,
            last_scroll_y: None,
            on_stage_hunk: None,
            on_revert_hunk: None,
            on_comment: None,
            find,
            search_colors,
            decorated_selection: None,
            pending_reveal: None,
            pending_center: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn path(&self) -> &SharedString {
        &self.path
    }

    /// The gpui-component language name, `None` for plain text.
    pub fn language(&self) -> Option<&'static str> {
        self.language
    }

    /// The underlying gpui-base editor state.
    pub fn editor_state(&self) -> &Entity<EditorState> {
        &self.state
    }

    /// The buffer text, LF line endings.
    pub fn text(&self, cx: &App) -> String {
        self.state.read(cx).text().to_string()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn save_state(&self) -> &SaveState {
        &self.save_state
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn soft_wrap(&self) -> bool {
        self.soft_wrap
    }

    pub fn on_save(&mut self, handler: SaveHandler) {
        self.on_save = Some(handler);
    }

    pub fn on_stage_hunk(&mut self, handler: Option<StageHandler>) {
        self.on_stage_hunk = handler;
    }

    pub fn on_revert_hunk(&mut self, handler: Option<RevertHandler>) {
        self.on_revert_hunk = handler;
    }

    pub fn on_comment(&mut self, handler: Option<CommentHandler>) {
        self.on_comment = handler;
    }

    /// `formatText` for save. Formatting is skipped when `None`.
    pub fn set_formatter(&mut self, formatter: Option<Formatter>) {
        self.formatter = formatter;
    }

    /// `loadFormatOnSave`.
    pub fn set_format_on_save(&mut self, enabled: bool) {
        self.format_on_save = enabled;
    }

    /// `loadAutosave`.
    pub fn set_autosave(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.autosave = enabled;
        if !enabled {
            self.autosave_task = None;
        } else if self.dirty {
            self.schedule_autosave(window, cx);
        }
    }

    /// Show the footer with the relative path and the save status.
    pub fn set_footer(&mut self, relative_path: Option<SharedString>, cx: &mut Context<Self>) {
        self.show_footer = relative_path.is_some();
        self.relative_path = relative_path;
        cx.notify();
    }

    pub fn set_theme(&mut self, theme: EditorTheme, cx: &mut Context<Self>) {
        let style = theme.input_style(self.language.unwrap_or("text"));
        self.state.update(cx, |state, cx| {
            state.set_editor_style(style);
            cx.notify();
        });
        self.find.set_theme(&theme, cx);
        self.theme = theme;
        self.refresh_search_decorations(cx);
        cx.notify();
    }

    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        self.state
            .update(cx, |state, cx| state.set_readonly(read_only, cx));
        cx.notify();
    }

    pub fn set_soft_wrap(&mut self, wrap: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.soft_wrap = wrap;
        self.state
            .update(cx, |state, cx| state.set_soft_wrap(wrap, window, cx));
        cx.notify();
    }

    pub fn toggle_soft_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_soft_wrap(!self.soft_wrap, window, cx);
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn undo(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.dispatch_to_editor(Box::new(gpui_base::input::Undo), window, cx);
    }

    pub fn redo(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.dispatch_to_editor(Box::new(gpui_base::input::Redo), window, cx);
    }

    fn dispatch_to_editor(
        &self,
        action: Box<dyn gpui::Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = self.state.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        window.dispatch_action(action, cx);
    }

    /// `revealNavigation`: put the cursor on 1-based `line` and `column` and
    /// scroll it to the middle.
    pub fn reveal_position(
        &mut self,
        line: usize,
        column: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let anchor = {
            let text = self.state.read(cx).text();
            let lines = text.lines_len().max(1);
            let row = line.clamp(1, lines) - 1;
            let start = text.line_start_offset(row);
            let end = text.line_end_offset(row);
            (start + column.unwrap_or(1).max(1) - 1).min(end)
        };
        self.select_and_reveal(anchor..anchor, true, cx);
        self.focus(window, cx);
    }

    /// Select `range` and scroll it into view, centered when it is off screen
    /// (`scrollToSearchMatch`).
    pub(crate) fn select_and_reveal(
        &mut self,
        range: Range<usize>,
        center: bool,
        cx: &mut Context<Self>,
    ) {
        let (laid_out, offscreen) = {
            let state = self.state.read(cx);
            let row = state.text().offset_to_point(range.start).row;
            let offscreen = state
                .visible_row_range()
                .is_none_or(|visible| row <= visible.start || row + 2 >= visible.end);
            (state.line_height().is_some(), offscreen)
        };
        if !laid_out {
            // `set_selected_range` scrolls only once there is a layout.
            self.pending_reveal = Some((range.clone(), center));
        }
        // This scrolls the range to the nearest edge, wrap-aware.
        self.state
            .update(cx, |state, cx| state.set_selected_range(range.clone(), cx));
        if center && offscreen && laid_out {
            // After that scroll lands, move the range to the middle.
            self.pending_center = Some(range.start);
        }
    }

    /// The second half of a centered reveal, once the nearest-edge scroll
    /// has been painted.
    fn apply_pending_center(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(offset) = self.pending_center.take() else {
            return;
        };
        let target = {
            let state = self.state.read(cx);
            let (Some(found), Some(line_height)) = (
                state.range_to_bounds(&(offset..offset)),
                state.line_height(),
            ) else {
                return;
            };
            let input = state.input_bounds();
            let delta = found.top() - (input.top() + input.size.height / 2.);
            if delta.abs() <= line_height {
                return;
            }
            let scroll = state.scroll_offset();
            point(scroll.x, (scroll.y - delta).min(px(0.)))
        };
        self.state
            .update(cx, |state, cx| state.set_scroll_offset(target, cx));
        window.refresh();
    }

    /// Replace the buffer with new disk contents, keeping the cursor and the
    /// scroll position. While the buffer is dirty this only notes that the
    /// disk changed, unless `force` is set.
    pub fn reload(&mut self, text: &str, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.dirty && !force {
            self.pending_disk = true;
            return;
        }
        self.pending_disk = false;
        self.line_ending = detect_line_ending(text);
        let content = normalize_line_breaks(text);
        let current = self.text(cx);
        if current != content {
            let selection = self.state.read(cx).selected_range();
            let mapped = map_selection(&current, &content, selection);
            self.state.update(cx, |state, cx| {
                let scroll = state.scroll_offset();
                state.set_value(content.clone(), window, cx);
                state.set_selected_range(mapped, cx);
                state.set_scroll_offset(scroll, cx);
            });
        }
        self.saved = self.state.read(cx).text().clone();
        self.set_dirty(false, cx);
        self.after_text_change(cx);
    }

    /// The text to compare against (HEAD or the index), or `None` to hide
    /// the git gutter. `setGitOriginal`.
    pub fn set_git_base(&mut self, base: Option<&str>, cx: &mut Context<Self>) {
        let next = base.map(normalize_line_breaks);
        if self.git_base.as_deref() == next.as_deref() {
            return;
        }
        self.git_base = next.map(Arc::from);
        self.peek = None;
        self.chunk_nav_pinned = None;
        if self.git_base.is_none() {
            self.git = Arc::new(GitSnapshot::default());
            self.git_task = None;
            cx.notify();
            return;
        }
        self.recompute_git(cx);
    }

    pub fn git_base(&self) -> Option<&str> {
        self.git_base.as_deref()
    }

    /// Added and removed line counts against the git base.
    pub fn diff_stats(&self) -> (usize, usize) {
        (self.git.additions, self.git.deletions)
    }

    /// Hunks against the git base as of the last background diff.
    pub fn chunks(&self) -> &[Chunk] {
        &self.git.chunks
    }

    fn on_editor_event(
        &mut self,
        _: &Entity<EditorState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::Change = event {
            self.mark_dirty(cx);
            self.schedule_autosave(window, cx);
            self.after_text_change(cx);
            cx.emit(CodeEditorEvent::Changed);
        }
    }

    fn after_text_change(&mut self, cx: &mut Context<Self>) {
        self.peek = None;
        self.recompute_git(cx);
        if self.find.open {
            self.refresh_matches(cx);
        }
        cx.notify();
    }

    fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        let dirty = *self.state.read(cx).text() != self.saved;
        self.set_dirty(dirty, cx);
    }

    /// `setDirty` plus `dirtyChange` in FileEditor.
    fn set_dirty(&mut self, dirty: bool, cx: &mut Context<Self>) {
        if self.dirty == dirty {
            return;
        }
        self.dirty = dirty;
        cx.emit(CodeEditorEvent::DirtyChanged(dirty));
        if dirty {
            if self.save_state != SaveState::Saving {
                self.save_state = SaveState::Idle;
            }
            return;
        }
        if self.pending_disk {
            self.pending_disk = false;
            cx.emit(CodeEditorEvent::ReloadRequested);
        }
    }

    fn schedule_autosave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autosave_task = None;
        if !self.autosave || self.on_save.is_none() {
            return;
        }
        self.autosave_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.autosave_task = None;
                if this.dirty && this.autosave && !this.pending_disk {
                    this.save(true, window, cx);
                }
            });
        }));
    }

    /// Format the current buffer without writing it to disk.
    pub fn format_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(formatter) = self.formatter.clone() else {
            return;
        };
        let before = self.text(cx);
        let cursor = self.state.read(cx).cursor();
        let Some((formatted, cursor)) = formatter(&self.path, &before, cursor) else {
            return;
        };
        if formatted == before {
            return;
        }
        let cursor = cursor.min(formatted.len());
        self.state.update(cx, |state, cx| {
            let scroll = state.scroll_offset();
            state.replace_all(formatted, window, cx);
            state.set_selected_range(cursor..cursor, cx);
            state.set_scroll_offset(scroll, cx);
        });
    }

    /// `save` in CodeMirrorEditor: format, then hand the text to the save
    /// callback with the file's line endings restored.
    pub fn save(&mut self, automatic: bool, window: &mut Window, cx: &mut Context<Self>) {
        let retry_pending_autosave = self.autosave_task.is_some() && self.autosave;
        self.autosave_task = None;
        self.save_generation += 1;
        let generation = self.save_generation;

        if self.format_on_save
            && let Some(formatter) = self.formatter.clone()
        {
            let before = self.text(cx);
            let cursor = self.state.read(cx).cursor();
            if let Some((formatted, cursor)) = formatter(&self.path, &before, cursor)
                && formatted != before
            {
                let cursor = cursor.min(formatted.len());
                self.state.update(cx, |state, cx| {
                    let scroll = state.scroll_offset();
                    state.replace_all(formatted.clone(), window, cx);
                    state.set_selected_range(cursor..cursor, cx);
                    state.set_scroll_offset(scroll, cx);
                });
            }
        }

        if automatic && self.pending_disk {
            return;
        }
        let Some(on_save) = self.on_save.clone() else {
            return;
        };
        let document = self.state.read(cx).text().clone();
        self.save_state = SaveState::Saving;
        cx.notify();
        let request = SaveRequest {
            path: self.path.clone(),
            contents: restore_line_ending(&document.to_string(), self.line_ending),
            automatic,
        };
        let task = on_save(request, window, cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.save_generation {
                    return;
                }
                match result {
                    Ok(()) => {
                        this.save_state = SaveState::Saved;
                        this.saved = document;
                        this.mark_dirty(cx);
                        cx.emit(CodeEditorEvent::Saved);
                    }
                    Err(error) => {
                        let message = error.to_string();
                        this.save_state = SaveState::Error(message.clone());
                        cx.emit(CodeEditorEvent::SaveFailed(message));
                        if retry_pending_autosave && this.dirty {
                            this.schedule_autosave(window, cx);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn recompute_git(&mut self, cx: &mut Context<Self>) {
        let Some(base) = self.git_base.clone() else {
            return;
        };
        let text = self.state.read(cx).text().clone();
        let job =
            cx.background_spawn(async move { GitSnapshot::compute(&base, &text.to_string()) });
        self.git_task = Some(cx.spawn(async move |this, cx| {
            let snapshot = job.await;
            let _ = this.update(cx, |this, cx| {
                this.git = Arc::new(snapshot);
                this.git_task = None;
                if this
                    .peek
                    .is_some_and(|index| index >= this.git.chunks.len())
                {
                    this.peek = None;
                }
                cx.notify();
            });
        }));
    }

    /// The selection, when it is not empty (`actionRange`).
    fn action_range(&self, cx: &App) -> Option<TextRange> {
        let range = self.state.read(cx).selected_range();
        (!range.is_empty()).then(|| range.into())
    }

    /// Open the removed-lines panel for hunk `index`, as a click on its
    /// gutter marker does. `None` closes it.
    pub fn show_hunk(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        self.peek = index.filter(|index| *index < self.git.chunks.len());
        cx.notify();
    }

    /// Open or close the removed-lines panel for chunk `index`.
    pub(crate) fn toggle_peek(&mut self, index: usize, cx: &mut Context<Self>) {
        self.peek = if self.peek == Some(index) {
            None
        } else {
            Some(index)
        };
        cx.notify();
    }

    /// `revertChunkAt`: put chunk `index` back to the git base, as one
    /// undoable edit.
    pub fn revert_hunk(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base) = self.git_base.clone() else {
            return;
        };
        let Some(pos) = self.git.positions.get(index).copied() else {
            return;
        };
        let text = self.text(cx);
        let Some(change) = revert_chunk_change(&base, &text, pos, self.action_range(cx)) else {
            return;
        };
        self.apply_change(&change, window, cx);
        self.peek = None;
        if let Some(handler) = self.on_revert_hunk.clone() {
            let change = change.clone();
            window.defer(cx, move |window, cx| handler(change, window, cx));
        }
        cx.emit(CodeEditorEvent::HunkReverted(change));
    }

    fn apply_change(&mut self, change: &TextChange, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| {
            let len = state.text().len();
            state.set_selected_range(change.from.min(len)..change.to.min(len), cx);
            state.replace(change.insert.clone(), window, cx);
        });
    }

    /// `stageChunkAt`: hand the index contents with chunk `index` applied to
    /// the stage callback, then diff against them.
    pub fn stage_hunk(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(base), Some(handler)) = (self.git_base.clone(), self.on_stage_hunk.clone())
        else {
            return;
        };
        if self.stage_busy {
            return;
        }
        let Some(pos) = self.git.positions.get(index).copied() else {
            return;
        };
        let text = self.text(cx);
        let Some(contents) = stage_chunk_text(&base, &text, pos, self.action_range(cx)) else {
            return;
        };
        self.stage_busy = true;
        let task = handler(contents.clone(), window, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.stage_busy = false;
                if result.is_ok() && this.git_base.as_deref() != Some(contents.as_str()) {
                    this.set_git_base(Some(&contents), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// `commentLineAt`: the cursor line when it is one of the chunk's added
    /// lines, else the chunk's first removed line, else its first added line.
    pub fn comment_hunk(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(handler) = self.on_comment.clone() else {
            return;
        };
        let Some(target) = self.comment_target(index, cx) else {
            return;
        };
        window.defer(cx, move |window, cx| handler(target, window, cx));
    }

    fn comment_target(&self, index: usize, cx: &App) -> Option<DiffCommentTarget> {
        let chunk = self.git.chunks.get(index)?;
        let marked = self.git.marked.get(index)?.clone();
        let state = self.state.read(cx);
        let text = state.text();
        let cursor_row = text.offset_to_point(state.cursor()).row;
        let added_line = |row: usize| UnifiedLine {
            kind: UnifiedLineKind::Add,
            text: text.slice_line(row).to_string(),
            old_number: None,
            new_number: Some(row + 1),
            pos: None,
        };
        let line = if chunk.is_insertion() && marked.contains(&cursor_row) {
            added_line(cursor_row)
        } else if let Some(first) = self.git.deleted.get(index).and_then(|lines| lines.first()) {
            UnifiedLine {
                kind: UnifiedLineKind::Del,
                text: first.clone(),
                old_number: Some(self.git.first_old_line[index]),
                new_number: None,
                pos: None,
            }
        } else {
            added_line(marked.start)
        };
        let path = self
            .relative_path
            .clone()
            .unwrap_or_else(|| self.path.clone())
            .to_string();
        Some(DiffCommentTarget { path, line })
    }

    pub(crate) fn can_stage(&self) -> bool {
        self.on_stage_hunk.is_some()
    }

    pub(crate) fn can_comment(&self) -> bool {
        self.on_comment.is_some()
    }

    /// `diffActiveChunkIndex`: the last change at or above the line 35% down
    /// the viewport.
    fn active_chunk_index(&self, cx: &App) -> Option<usize> {
        let positions = &self.git.positions;
        if positions.is_empty() {
            return None;
        }
        let state = self.state.read(cx);
        let visible = state.visible_row_range().unwrap_or(0..1);
        let line_height = state.line_height().unwrap_or(px(20.));
        let viewport_rows = (state.input_bounds().size.height / line_height).max(1.);
        let center_row = visible.start + (viewport_rows * 0.35) as usize;
        let text = state.text();
        let pos = text.line_start_offset(center_row.min(text.lines_len().saturating_sub(1)));
        let mut index = 0;
        for (i, position) in positions.iter().enumerate() {
            if *position <= pos {
                index = i;
            } else {
                break;
            }
        }
        Some(index)
    }

    fn chunk_nav_index(&mut self, cx: &App) -> Option<usize> {
        let scroll_y = self.state.read(cx).scroll_offset().y;
        if self.last_scroll_y != Some(scroll_y) {
            self.last_scroll_y = Some(scroll_y);
            self.chunk_nav_pinned = None;
        }
        let total = self.git.positions.len();
        match self.chunk_nav_pinned {
            Some(index) if index < total => Some(index),
            _ => self.active_chunk_index(cx),
        }
    }

    /// `stepChunkNav`.
    pub fn step_change(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let total = self.git.positions.len();
        let Some(current) = self.chunk_nav_index(cx) else {
            return;
        };
        let next = (current as isize + delta).clamp(0, total as isize - 1) as usize;
        if next == current {
            return;
        }
        let pos = self.git.positions[next];
        self.select_and_reveal(pos..pos, true, cx);
        self.focus(window, cx);
        // Pin after the scroll this causes, so the counter shows `next`.
        self.last_scroll_y = None;
        self.chunk_nav_index(cx);
        self.chunk_nav_pinned = Some(next);
        cx.notify();
    }

    // Find bar.

    pub(crate) fn search_query(&self) -> &SearchQuery {
        &self.find.query
    }

    /// Recount matches and redraw their highlights (`refreshCount`).
    pub(crate) fn refresh_matches(&mut self, cx: &mut Context<Self>) {
        let (matches, capped) = match self.find.query.compile() {
            Some(compiled) if self.find.open => compiled.count_matches(&self.text(cx)),
            _ => (Vec::new(), false),
        };
        self.find.matches = Arc::new(matches);
        self.find.capped = capped;
        self.refresh_search_decorations(cx);
    }

    /// Redraw the match backgrounds (`.cm-searchMatch`, `.cm-searchMatch-selected`).
    ///
    /// gpui-base drops the background of text decorations, but paints LSP
    /// document colors behind the text, so the matches go through a document
    /// color provider. The editor picks them up on its next language refresh.
    pub(crate) fn refresh_search_decorations(&mut self, cx: &mut Context<Self>) {
        let selection = self.state.read(cx).selected_range();
        let matches = self.find.matches.clone();
        let color = |color: gpui::Hsla| {
            let rgba = gpui::Rgba::from(color);
            lsp_types::Color {
                red: rgba.r,
                green: rgba.g,
                blue: rgba.b,
                alpha: rgba.a,
            }
        };
        let (current, other) = (
            color(self.theme.search_match_selected),
            color(self.theme.search_match),
        );
        let colors = {
            let text = self.state.read(cx).text();
            matches
                .iter()
                .filter(|range| !range.is_empty())
                .map(|range| lsp_types::ColorInformation {
                    range: lsp_types::Range {
                        start: text.offset_to_position(range.start),
                        end: text.offset_to_position(range.end),
                    },
                    color: if *range == selection { current } else { other },
                })
                .collect::<Vec<_>>()
        };
        let changed = *self.search_colors.borrow() != colors;
        if changed {
            *self.search_colors.borrow_mut() = colors;
            self.state.update(cx, |state, cx| state.refresh(cx));
        }
        self.decorated_selection = Some(selection);
    }

    fn render_chunk_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let total = self.git.positions.len();
        let index = self.chunk_nav_index(cx).unwrap_or(0);
        let (additions, deletions) = (self.git.additions, self.git.deletions);
        let label = if total == 0 {
            "0/0".to_string()
        } else {
            format!("{}/{}", index + 1, total)
        };
        let nav_button = |id: &'static str, kind: IconKind, disabled: bool| {
            div()
                .id(id)
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .when(disabled, |this| this.opacity(0.35))
                .when(!disabled, |this| this.hover(|this| this.bg(theme.hover)))
                .child(icon(kind, px(14.), theme.content(0.7)))
        };
        div()
            .flex()
            .flex_none()
            .h(px(32.))
            .items_center()
            .justify_between()
            .gap(px(12.))
            .border_b_1()
            .border_color(theme.stroke)
            .pl(px(12.))
            .pr(px(4.))
            .font_family(theme.ui_font.clone())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .when(additions > 0, |this| {
                        this.child(
                            div()
                                .text_color(theme.git_added)
                                .child(format!("+{additions}")),
                        )
                    })
                    .when(deletions > 0, |this| {
                        this.child(
                            div()
                                .text_color(theme.git_deleted)
                                .child(format!("-{deletions}")),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        nav_button(
                            "previous-change",
                            IconKind::ChevronUp,
                            total == 0 || index == 0,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.step_change(-1, window, cx)),
                        ),
                    )
                    .child(
                        div()
                            .min_w(px(40.))
                            .px(px(2.))
                            .flex()
                            .justify_center()
                            .font_family(theme.mono_font.clone())
                            .text_size(px(10.5))
                            .text_color(theme.content(0.55))
                            .child(label),
                    )
                    .child(
                        nav_button(
                            "next-change",
                            IconKind::ChevronDown,
                            total == 0 || index + 1 >= total,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.step_change(1, window, cx)),
                        ),
                    ),
            )
    }

    fn render_footer(&self) -> impl IntoElement {
        let theme = &self.theme;
        let status = match &self.save_state {
            SaveState::Saving => Some(("Saving…".to_string(), theme.content(0.4))),
            SaveState::Saved => Some(("Saved".to_string(), theme.content(0.4))),
            SaveState::Error(message) => Some((format!("Save failed: {message}"), theme.danger)),
            SaveState::Idle => None,
        };
        let path = self
            .relative_path
            .clone()
            .unwrap_or_else(|| basename(&self.path).to_string().into());
        div()
            .flex()
            .flex_none()
            .h(px(24.))
            .items_center()
            .border_t_1()
            .border_color(theme.stroke)
            .px(px(10.))
            .font_family(theme.mono_font.clone())
            .text_size(px(10.5))
            .text_color(theme.content(0.4))
            .child(div().flex_1().min_w_0().truncate().child(path))
            .when_some(status, |this, (text, color)| {
                this.child(
                    div()
                        .max_w(px(256.))
                        .truncate()
                        .text_color(color)
                        .child(text),
                )
            })
    }
}

impl Focusable for CodeEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }
}

impl Render for CodeEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_diff = self.git_base.is_some();
        if self.state.read(cx).line_height().is_some()
            && let Some((range, center)) = self.pending_reveal.take()
        {
            self.select_and_reveal(range, center, cx);
            window.refresh();
        } else {
            self.apply_pending_center(window, cx);
        }
        if self.find.open
            && self.decorated_selection.as_ref() != Some(&self.state.read(cx).selected_range())
        {
            // The user moved the selection: move the current-match highlight.
            self.refresh_search_decorations(cx);
        }
        let overlay = crate::git_gutter::overlay(self, window, cx).into_any_element();
        let chunk_nav = show_diff.then(|| self.render_chunk_nav(cx).into_any_element());
        let find_bar = self
            .find
            .open
            .then(|| self.render_find_bar(window, cx).into_any_element());
        let footer = self
            .show_footer
            .then(|| self.render_footer().into_any_element());
        div()
            .key_context(KEY_CONTEXT)
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .bg(self.theme.background)
            .text_color(self.theme.foreground)
            .capture_action(
                cx.listener(|this, _: &gpui_base::input::Search, window, cx| {
                    this.open_find(false, window, cx);
                }),
            )
            .capture_action(
                cx.listener(|this, _: &gpui_base::input::Escape, window, cx| {
                    if this.peek.is_some() {
                        this.peek = None;
                        cx.notify();
                    } else if this.find.open {
                        this.close_find(window, cx);
                    } else {
                        cx.propagate();
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save(false, window, cx)))
            .on_action(
                cx.listener(|this, _: &FormatDocument, window, cx| {
                    this.format_document(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ToggleSoftWrap, window, cx| {
                    this.toggle_soft_wrap(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &OpenReplace, window, cx| this.open_find(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &FindNext, window, cx| this.find_next(window, cx)))
            .on_action(
                cx.listener(|this, _: &FindPrevious, window, cx| this.find_previous(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NextChange, window, cx| this.step_change(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PreviousChange, window, cx| {
                    this.step_change(-1, window, cx)
                }),
            )
            .children(chunk_nav)
            .children(find_bar)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .font_family(self.theme.mono_font.clone())
                    .text_size(self.theme.font_size)
                    .line_height(self.theme.line_height_px())
                    .child(gpui_base::input::Editor::new(&self.state))
                    .child(overlay),
            )
            .children(footer)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn open<'a>(
        text: &str,
        cx: &'a mut TestAppContext,
    ) -> (Entity<CodeEditor>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            gpui_base::init(cx);
            init(cx);
        });
        let text = text.to_owned();
        cx.add_window_view(move |window, cx| {
            CodeEditor::new("/repo/src/main.rs", &text, EditorTheme::dark(), window, cx)
        })
    }

    fn edit(
        editor: &Entity<CodeEditor>,
        range: Range<usize>,
        insert: &str,
        cx: &mut VisualTestContext,
    ) {
        let state = editor.read_with(cx, |editor, _| editor.state.clone());
        let insert = insert.to_owned();
        state.update_in(cx, |state, window, cx| {
            state.set_selected_range(range, cx);
            state.replace(insert, window, cx);
        });
        cx.run_until_parked();
    }

    fn text(editor: &Entity<CodeEditor>, cx: &mut VisualTestContext) -> String {
        editor.read_with(cx, |editor, cx| editor.text(cx))
    }

    #[gpui::test]
    fn dirty_follows_the_saved_text(cx: &mut TestAppContext) {
        let (editor, cx) = open("alpha\n", cx);
        assert!(!editor.read_with(cx, |editor, _| editor.is_dirty()));
        edit(&editor, 0..0, "x", cx);
        assert!(editor.read_with(cx, |editor, _| editor.is_dirty()));
        edit(&editor, 0..1, "", cx);
        assert!(!editor.read_with(cx, |editor, _| editor.is_dirty()));
    }

    #[gpui::test]
    fn save_restores_line_endings_and_marks_clean(cx: &mut TestAppContext) {
        let (editor, cx) = open("a\r\nb\r\n", cx);
        assert_eq!(text(&editor, cx), "a\nb\n");
        let saved = Rc::new(RefCell::new(Vec::<String>::new()));
        let sink = saved.clone();
        editor.update(cx, |editor, _| {
            editor.on_save(Rc::new(move |request: SaveRequest, _, _| {
                sink.borrow_mut().push(request.contents);
                Task::ready(Ok(()))
            }));
        });
        edit(&editor, 0..0, "z", cx);
        editor.update_in(cx, |editor, window, cx| editor.save(false, window, cx));
        cx.run_until_parked();
        assert_eq!(saved.borrow().as_slice(), ["za\r\nb\r\n"]);
        editor.read_with(cx, |editor, _| {
            assert!(!editor.is_dirty());
            assert_eq!(editor.save_state(), &SaveState::Saved);
        });
    }

    #[gpui::test]
    fn a_failed_save_keeps_the_buffer_dirty(cx: &mut TestAppContext) {
        let (editor, cx) = open("a\n", cx);
        editor.update(cx, |editor, _| {
            editor.on_save(Rc::new(|_, _, _| {
                Task::ready(Err(anyhow::anyhow!("disk full")))
            }));
        });
        edit(&editor, 0..0, "z", cx);
        editor.update_in(cx, |editor, window, cx| editor.save(false, window, cx));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert!(editor.is_dirty());
            assert_eq!(editor.save_state(), &SaveState::Error("disk full".into()));
        });
    }

    #[gpui::test]
    fn format_on_save_replaces_the_buffer_first(cx: &mut TestAppContext) {
        let (editor, cx) = open("a  b\n", cx);
        let saved = Rc::new(RefCell::new(None::<String>));
        let sink = saved.clone();
        editor.update(cx, |editor, _| {
            editor.set_formatter(Some(Rc::new(|_, text, cursor| {
                Some((text.replace("  ", " "), cursor))
            })));
            editor.on_save(Rc::new(move |request: SaveRequest, _, _| {
                *sink.borrow_mut() = Some(request.contents);
                Task::ready(Ok(()))
            }));
        });
        editor.update_in(cx, |editor, window, cx| editor.save(false, window, cx));
        cx.run_until_parked();
        assert_eq!(saved.borrow().as_deref(), Some("a b\n"));
        assert_eq!(text(&editor, cx), "a b\n");
    }

    #[gpui::test]
    fn reload_keeps_the_cursor_on_the_same_text(cx: &mut TestAppContext) {
        let (editor, cx) = open("a\nb\nc\n", cx);
        let state = editor.read_with(cx, |editor, _| editor.state.clone());
        state.update(cx, |state, cx| state.set_selected_range(4..4, cx));
        editor.update_in(cx, |editor, window, cx| {
            editor.reload("x\ny\na\nb\nc\n", false, window, cx)
        });
        assert_eq!(text(&editor, cx), "x\ny\na\nb\nc\n");
        let cursor = state.read_with(cx, |state, _| state.cursor());
        assert_eq!(&text(&editor, cx)[cursor..cursor + 1], "c");
        assert!(!editor.read_with(cx, |editor, _| editor.is_dirty()));
    }

    #[gpui::test]
    fn reload_while_dirty_waits_and_asks_again_once_clean(cx: &mut TestAppContext) {
        let (editor, cx) = open("a\n", cx);
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&editor, move |_, event: &CodeEditorEvent, _| {
                sink.borrow_mut().push(event.clone());
            })
            .detach();
        });
        edit(&editor, 0..0, "z", cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.reload("disk\n", false, window, cx)
        });
        assert_eq!(text(&editor, cx), "za\n");
        // Undoing back to the saved text makes the buffer clean.
        edit(&editor, 0..1, "", cx);
        assert!(events.borrow().contains(&CodeEditorEvent::ReloadRequested));
    }

    #[gpui::test]
    fn computes_hunks_and_reverts_one(cx: &mut TestAppContext) {
        let (editor, cx) = open("alpha\nBETA\ngamma\ndelta\n", cx);
        editor.update(cx, |editor, cx| {
            editor.set_git_base(Some("alpha\nbeta\ngamma\n"), cx)
        });
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.chunks().len(), 2);
            assert_eq!(editor.diff_stats(), (2, 1));
        });
        editor.update_in(cx, |editor, window, cx| editor.revert_hunk(0, window, cx));
        cx.run_until_parked();
        assert_eq!(text(&editor, cx), "alpha\nbeta\ngamma\ndelta\n");
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.chunks().len(), 1);
            assert!(editor.is_dirty());
        });
    }

    #[gpui::test]
    fn hunks_follow_typing(cx: &mut TestAppContext) {
        let (editor, cx) = open("alpha\nbeta\n", cx);
        editor.update(cx, |editor, cx| {
            editor.set_git_base(Some("alpha\nbeta\n"), cx)
        });
        cx.run_until_parked();
        assert!(editor.read_with(cx, |editor, _| editor.chunks().is_empty()));
        edit(&editor, 6..6, "new\n", cx);
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.chunks().len(), 1);
            assert_eq!(editor.chunks()[0].kind(), ChangeKind::Added);
        });
    }

    #[gpui::test]
    fn stage_hands_the_index_contents_to_the_callback(cx: &mut TestAppContext) {
        let (editor, cx) = open("alpha\nBETA\ngamma\nDELTA\n", cx);
        let staged = Rc::new(RefCell::new(None::<String>));
        let sink = staged.clone();
        editor.update(cx, |editor, cx| {
            editor.set_git_base(Some("alpha\nbeta\ngamma\ndelta\n"), cx);
            editor.on_stage_hunk(Some(Rc::new(move |contents, _, _| {
                *sink.borrow_mut() = Some(contents);
                Task::ready(Ok(()))
            })));
        });
        cx.run_until_parked();
        editor.update_in(cx, |editor, window, cx| editor.stage_hunk(0, window, cx));
        cx.run_until_parked();
        assert_eq!(
            staged.borrow().as_deref(),
            Some("alpha\nBETA\ngamma\ndelta\n")
        );
        // The staged text becomes the new base, so one hunk is left.
        editor.read_with(cx, |editor, _| assert_eq!(editor.chunks().len(), 1));
    }

    #[gpui::test]
    fn find_counts_and_replace_all_edits_the_buffer(cx: &mut TestAppContext) {
        let (editor, cx) = open("cat concat cat\n", cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.set_find_query(
                SearchQuery {
                    search: "cat".into(),
                    replace: "dog".into(),
                    whole_word: true,
                    ..Default::default()
                },
                window,
                cx,
            );
        });
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.find.matches.len(), 2);
            // Revealing selects the first match.
            assert_eq!(
                editor.count_label(cx),
                crate::search::CountLabel::Ok("1 of 2".into())
            );
        });
        editor.update_in(cx, |editor, window, cx| editor.replace_all(window, cx));
        cx.run_until_parked();
        assert_eq!(text(&editor, cx), "dog concat dog\n");
    }

    #[gpui::test]
    fn replace_next_steps_through_matches(cx: &mut TestAppContext) {
        let (editor, cx) = open("ab ab ab\n", cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.set_find_query(
                SearchQuery {
                    search: "ab".into(),
                    replace: "X".into(),
                    ..Default::default()
                },
                window,
                cx,
            );
            editor.replace_next(window, cx);
        });
        cx.run_until_parked();
        assert_eq!(text(&editor, cx), "X ab ab\n");
        let selection = editor.read_with(cx, |editor, cx| editor.state.read(cx).selected_range());
        assert_eq!(selection, 2..4);
        editor.update_in(cx, |editor, window, cx| editor.find_next(window, cx));
        let selection = editor.read_with(cx, |editor, cx| editor.state.read(cx).selected_range());
        assert_eq!(selection, 5..7);
    }

    #[gpui::test]
    fn read_only_blocks_revert_and_replace(cx: &mut TestAppContext) {
        let (editor, cx) = open("ab\n", cx);
        editor.update_in(cx, |editor, window, cx| {
            editor.set_read_only(true, cx);
            editor.set_find_query(
                SearchQuery {
                    search: "ab".into(),
                    replace: "X".into(),
                    ..Default::default()
                },
                window,
                cx,
            );
            editor.replace_all(window, cx);
        });
        assert_eq!(text(&editor, cx), "ab\n");
    }
}
