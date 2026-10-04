//! Port of src/features/source-control/ui/UnifiedDiffView.tsx.
//!
//! A unified diff for a list of files, drawn with one `uniform_list`, so every
//! row (file header, hunk header, line, folded context) has the same height.
//! The React view used 20px lines, 22px hunk headers, and 32px fold bars; this
//! one uses [`ROW_HEIGHT`] for all of them. Syntax highlighting runs on a
//! background thread per file, for both sides of the diff.
//!
//! Git actions are callbacks: per file (stage, unstage, discard), per hunk
//! (the same three, with a ready-to-apply patch), and per line (comment).

use std::{collections::HashMap, collections::HashSet, ops::Range, rc::Rc, sync::Arc};

use gpui::{
    App, AppContext as _, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ListHorizontalSizingBehavior, ParentElement, Render, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled, StyledText, Task, UniformListScrollHandle, Window,
    div, prelude::FluentBuilder as _, px, uniform_list,
};

use crate::{
    highlighter::{DiffSide, LineStyles, highlight_diff_sides},
    icons::{IconKind, icon},
    language::language_for_path,
    theme::{EditorTheme, SyntaxResolver},
    unified_diff::{
        DiffCommentTarget, DiffHunk, FoldDirection, FoldReveal, PatchFile, PatchStatus,
        UNIFIED_CONTEXT_DEFAULT, UNIFIED_FOLD_STEP, UnifiedBlock, UnifiedFileDiff, UnifiedLineKind,
        blocks_from_lines, build_unified_file, expand_fold, file_hunks, hunk_patch, parse_patch,
        revealed_fold,
    },
};

/// Height of every row.
pub const ROW_HEIGHT: f32 = 22.;
/// Width of the line-number lane (`w-12`).
const GUTTER_WIDTH: f32 = 48.;
/// The +/- column before each line's text (`w-7`).
const MARK_WIDTH: f32 = 28.;

/// Which actions a file offers. `UnifiedDiffFileModel.canStage` and friends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffFileActions {
    pub stage: bool,
    pub unstage: bool,
    pub discard: bool,
    pub stage_hunk: bool,
    pub unstage_hunk: bool,
    pub discard_hunk: bool,
    pub comment: bool,
}

/// `UnifiedDiffFileModel`.
#[derive(Debug, Clone)]
pub struct DiffFile {
    pub id: SharedString,
    pub path: SharedString,
    pub label: SharedString,
    pub previous_path: Option<SharedString>,
    pub status: PatchStatus,
    pub binary: bool,
    pub too_large: bool,
    pub empty_message: Option<SharedString>,
    pub diff: UnifiedFileDiff,
    pub hunks: Vec<DiffHunk>,
    pub actions: DiffFileActions,
}

impl DiffFile {
    /// A file diffed from its old and new text (`buildUnifiedFile`).
    pub fn from_texts(path: impl Into<SharedString>, old: &str, new: &str) -> Self {
        let path: SharedString = path.into();
        let diff = build_unified_file(old, new, UNIFIED_CONTEXT_DEFAULT);
        let hunks = file_hunks(&diff);
        let status = if old.is_empty() && !new.is_empty() {
            PatchStatus::Added
        } else if new.is_empty() && !old.is_empty() {
            PatchStatus::Deleted
        } else {
            PatchStatus::Modified
        };
        Self {
            id: path.clone(),
            label: path.clone(),
            path,
            previous_path: None,
            status,
            binary: false,
            too_large: false,
            empty_message: None,
            diff,
            hunks,
            actions: DiffFileActions::default(),
        }
    }

    /// A file from a parsed patch.
    pub fn from_patch_file(file: PatchFile) -> Self {
        let path: SharedString = file.path.clone().into();
        let label: SharedString = match &file.previous_path {
            Some(previous) => format!("{previous} → {}", file.path).into(),
            None => path.clone(),
        };
        let mut diff = blocks_from_lines(file.lines, UNIFIED_CONTEXT_DEFAULT);
        diff.additions = file.additions;
        diff.deletions = file.deletions;
        let hunks = file_hunks(&diff);
        Self {
            id: path.clone(),
            path,
            label,
            previous_path: file.previous_path.map(Into::into),
            status: file.status,
            binary: file.binary,
            too_large: false,
            empty_message: None,
            diff,
            hunks,
            actions: DiffFileActions::default(),
        }
    }

    pub fn with_actions(mut self, actions: DiffFileActions) -> Self {
        self.actions = actions;
        self
    }
}

/// Every file in `git diff` output.
pub fn parse_diff(patch: &str) -> Vec<DiffFile> {
    parse_patch(patch)
        .into_iter()
        .map(DiffFile::from_patch_file)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffAction {
    Stage,
    Unstage,
    Discard,
}

/// What a hunk action callback receives.
#[derive(Debug, Clone)]
pub struct HunkActionRequest {
    pub action: DiffAction,
    pub file_id: SharedString,
    pub path: SharedString,
    pub hunk: DiffHunk,
    /// A one-hunk patch for `git apply`: `--cached` stages it, `--cached
    /// --reverse` unstages it, `--reverse` discards it.
    pub patch: String,
}

/// Action callbacks run on the next effect cycle, so they may update the view.
pub type HunkActionHandler = Rc<dyn Fn(HunkActionRequest, &mut Window, &mut App)>;
pub type FileActionHandler = Rc<dyn Fn(DiffAction, SharedString, &mut Window, &mut App)>;
pub type DiffCommentHandler = Rc<dyn Fn(DiffCommentTarget, &mut Window, &mut App)>;

/// `InitialExpansion`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InitialExpansion {
    #[default]
    All,
    First,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Truncated,
    FileHeader {
        file: usize,
    },
    Empty {
        file: usize,
        message: SharedString,
    },
    Hunk {
        file: usize,
        hunk: usize,
    },
    Line {
        file: usize,
        line: usize,
    },
    Fold {
        file: usize,
        block: usize,
        hidden: usize,
    },
}

/// The unified diff view.
pub struct DiffView {
    files: Vec<DiffFile>,
    theme: EditorTheme,
    open: HashSet<usize>,
    reveals: HashMap<(usize, String), FoldReveal>,
    rows: Vec<Row>,
    widest_row: usize,
    tokens: HashMap<usize, Arc<Vec<Option<LineStyles>>>>,
    highlight_tasks: Vec<Task<()>>,
    #[cfg(test)]
    highlight_delay: Option<std::time::Duration>,
    scroll: UniformListScrollHandle,
    hovered_row: Option<usize>,
    busy: Option<SharedString>,
    truncated: bool,
    file_count: Option<usize>,
    on_hunk_action: Option<HunkActionHandler>,
    on_file_action: Option<FileActionHandler>,
    on_comment: Option<DiffCommentHandler>,
}

impl DiffView {
    pub fn new(files: Vec<DiffFile>, theme: EditorTheme, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            files: Vec::new(),
            theme,
            open: HashSet::new(),
            reveals: HashMap::new(),
            rows: Vec::new(),
            widest_row: 0,
            tokens: HashMap::new(),
            highlight_tasks: Vec::new(),
            #[cfg(test)]
            highlight_delay: None,
            scroll: UniformListScrollHandle::new(),
            hovered_row: None,
            busy: None,
            truncated: false,
            file_count: None,
            on_hunk_action: None,
            on_file_action: None,
            on_comment: None,
        };
        view.set_files(files, InitialExpansion::All, cx);
        view
    }

    /// Replace the files. Expansion and revealed folds reset, as they do
    /// when React loads a new set of files.
    pub fn set_files(
        &mut self,
        files: Vec<DiffFile>,
        expansion: InitialExpansion,
        cx: &mut Context<Self>,
    ) {
        self.open = match expansion {
            InitialExpansion::None => HashSet::new(),
            InitialExpansion::First if !files.is_empty() => HashSet::from([0]),
            InitialExpansion::First => HashSet::new(),
            InitialExpansion::All => (0..files.len()).collect(),
        };
        self.files = files;
        self.reveals.clear();
        self.tokens.clear();
        self.highlight_tasks.clear();
        self.rebuild_rows();
        self.highlight_open_files(cx);
        cx.notify();
    }

    pub fn files(&self) -> &[DiffFile] {
        &self.files
    }

    /// "Diff is too large to display in full".
    pub fn set_truncated(
        &mut self,
        truncated: bool,
        file_count: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.truncated = truncated;
        self.file_count = file_count;
        self.rebuild_rows();
        cx.notify();
    }

    /// The file whose action is running; its buttons are disabled.
    pub fn set_busy(&mut self, file_id: Option<SharedString>, cx: &mut Context<Self>) {
        self.busy = file_id;
        cx.notify();
    }

    pub fn set_theme(&mut self, theme: EditorTheme, cx: &mut Context<Self>) {
        self.theme = theme;
        self.tokens.clear();
        self.highlight_tasks.clear();
        self.highlight_open_files(cx);
        cx.notify();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn theme(&self) -> &EditorTheme {
        &self.theme
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn expanded_files(&self) -> &HashSet<usize> {
        &self.open
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn revealed_folds(&self) -> &HashMap<(usize, String), FoldReveal> {
        &self.reveals
    }

    pub fn on_hunk_action(&mut self, handler: Option<HunkActionHandler>) {
        self.on_hunk_action = handler;
    }

    pub fn on_file_action(&mut self, handler: Option<FileActionHandler>) {
        self.on_file_action = handler;
    }

    pub fn on_comment(&mut self, handler: Option<DiffCommentHandler>) {
        self.on_comment = handler;
    }

    /// Scroll so the file with `id` (or `path`) is at the top (`focusPath`).
    pub fn scroll_to_file(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(file) = self
            .files
            .iter()
            .position(|file| file.id.as_ref() == id || file.path.as_ref() == id)
        else {
            return;
        };
        if let Some(row) = self
            .rows
            .iter()
            .position(|row| *row == Row::FileHeader { file })
        {
            self.scroll.scroll_to_item(row, ScrollStrategy::Top);
            cx.notify();
        }
    }

    pub fn toggle_file(&mut self, file: usize, cx: &mut Context<Self>) {
        if !self.open.remove(&file) {
            self.open.insert(file);
        }
        self.rebuild_rows();
        self.highlight_open_files(cx);
        cx.notify();
    }

    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.open = (0..self.files.len()).collect();
        self.rebuild_rows();
        self.highlight_open_files(cx);
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.open.clear();
        self.rebuild_rows();
        cx.notify();
    }

    /// `revealFold`.
    pub fn reveal_fold(
        &mut self,
        file: usize,
        block: usize,
        direction: FoldDirection,
        cx: &mut Context<Self>,
    ) {
        let Some(UnifiedBlock::Fold { id, lines }) = self
            .files
            .get(file)
            .and_then(|file| file.diff.blocks.get(block))
        else {
            return;
        };
        let key = (file, id.clone());
        let next = expand_fold(
            self.reveals.get(&key).copied(),
            lines.len(),
            direction,
            UNIFIED_FOLD_STEP,
        );
        self.reveals.insert(key, next);
        self.rebuild_rows();
        cx.notify();
    }

    /// `flattenVisibleRows` for every open file, plus the file headers.
    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        if self.truncated {
            rows.push(Row::Truncated);
        }
        let mut widest = (0usize, 0usize);
        for (index, file) in self.files.iter().enumerate() {
            rows.push(Row::FileHeader { file: index });
            if !self.open.contains(&index) {
                continue;
            }
            let empty = if file.binary {
                Some("Binary file changed".into())
            } else if file.too_large {
                Some("Diff is too large to display".into())
            } else if let Some(message) = &file.empty_message {
                Some(message.clone())
            } else if file.diff.blocks.is_empty() {
                Some("No textual diff".into())
            } else {
                None
            };
            if let Some(message) = empty {
                rows.push(Row::Empty {
                    file: index,
                    message,
                });
                continue;
            }
            let mut push_line = |rows: &mut Vec<Row>, line: usize| {
                let width = file.diff.lines[line].text.chars().count();
                if width > widest.0 {
                    widest = (width, rows.len());
                }
                rows.push(Row::Line { file: index, line });
            };
            for (block_index, block) in file.diff.blocks.iter().enumerate() {
                match block {
                    UnifiedBlock::Fold { id, lines } => {
                        let split = revealed_fold(
                            lines.len(),
                            self.reveals.get(&(index, id.clone())).copied(),
                        );
                        for line in lines.start..lines.start + split.head {
                            push_line(&mut rows, line);
                        }
                        if split.hidden > 0 {
                            rows.push(Row::Fold {
                                file: index,
                                block: block_index,
                                hidden: split.hidden,
                            });
                        }
                        for line in lines.end - split.tail..lines.end {
                            push_line(&mut rows, line);
                        }
                    }
                    UnifiedBlock::Hunk { lines, .. } => {
                        // A diff built from texts has no `@@` lines: give each
                        // hunk block a header row for its actions.
                        if let Some(hunk) = file
                            .hunks
                            .iter()
                            .position(|hunk| !hunk.parsed && hunk.lines.start == lines.start)
                        {
                            rows.push(Row::Hunk { file: index, hunk });
                        }
                        for line in lines.clone() {
                            if file.diff.lines[line].kind == UnifiedLineKind::Hunk {
                                if let Some(hunk) = file
                                    .hunks
                                    .iter()
                                    .position(|hunk| hunk.parsed && hunk.lines.start == line + 1)
                                {
                                    rows.push(Row::Hunk { file: index, hunk });
                                }
                                continue;
                            }
                            push_line(&mut rows, line);
                        }
                    }
                }
            }
        }
        self.widest_row = widest.1;
        self.rows = rows;
        self.hovered_row = None;
    }

    /// `highlightDiffFile` for every open file that has no tokens yet.
    fn highlight_open_files(&mut self, cx: &mut Context<Self>) {
        let pending: Vec<usize> = self
            .open
            .iter()
            .copied()
            .filter(|file| !self.tokens.contains_key(file))
            .collect();
        for index in pending {
            let file = &self.files[index];
            if file.binary || file.too_large {
                continue;
            }
            let language = language_for_path(&file.path);
            let resolver = SyntaxResolver::new(self.theme.syntax, language.unwrap_or("text"));
            let lines: Vec<(UnifiedLineKind, String)> = file
                .diff
                .lines
                .iter()
                .map(|line| (line.kind, line.text.clone()))
                .collect();
            // Mark as requested so a second call does not start another job.
            self.tokens.insert(index, Arc::new(Vec::new()));
            #[cfg(test)]
            let delay = self
                .highlight_delay
                .take()
                .map(|delay| (cx.background_executor().clone(), delay));
            let job = cx.background_spawn(async move {
                #[cfg(test)]
                if let Some((executor, delay)) = delay {
                    executor.timer(delay).await;
                }
                let mut old_index = Vec::new();
                let mut new_index = Vec::new();
                let mut old = DiffSide { lines: Vec::new() };
                let mut new = DiffSide { lines: Vec::new() };
                for (line, (kind, text)) in lines.iter().enumerate() {
                    if *kind == UnifiedLineKind::Hunk {
                        continue;
                    }
                    if *kind != UnifiedLineKind::Add {
                        old_index.push(line);
                        old.lines.push(text.as_str());
                    }
                    if *kind != UnifiedLineKind::Del {
                        new_index.push(line);
                        new.lines.push(text.as_str());
                    }
                }
                let mut out: Vec<Option<LineStyles>> = vec![None; lines.len()];
                if let Some((old_styles, new_styles)) =
                    highlight_diff_sides(&old, &new, language, &resolver)
                {
                    // `assignLineTokens`: removed lines from the old side, the
                    // rest from the new side.
                    for (side_index, line) in old_index.iter().enumerate() {
                        if lines[*line].0 == UnifiedLineKind::Del {
                            out[*line] = old_styles.get(side_index).cloned();
                        }
                    }
                    for (side_index, line) in new_index.iter().enumerate() {
                        out[*line] = new_styles.get(side_index).cloned();
                    }
                }
                out
            });
            self.highlight_tasks.push(cx.spawn(async move |this, cx| {
                let tokens = job.await;
                let _ = this.update(cx, |this, cx| {
                    this.tokens.insert(index, Arc::new(tokens));
                    cx.notify();
                });
            }));
        }
    }

    /// Run `action` on hunk `hunk` of file `file` through the hunk callback,
    /// as its header button does.
    pub fn run_hunk_action(
        &mut self,
        file: usize,
        hunk: usize,
        action: DiffAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(request), Some(handler)) = (
            self.hunk_request(file, hunk, action),
            self.on_hunk_action.clone(),
        ) else {
            return;
        };
        if self.busy.as_ref() == Some(&request.file_id) {
            return;
        }
        // Deferred, so the callback can update this view.
        window.defer(cx, move |window, cx| handler(request, window, cx));
    }

    fn hunk_request(
        &self,
        file: usize,
        hunk: usize,
        action: DiffAction,
    ) -> Option<HunkActionRequest> {
        let file = self.files.get(file)?;
        let hunk = file.hunks.get(hunk)?.clone();
        let patch = hunk_patch(&file.path, file.previous_path.as_deref(), &file.diff, &hunk);
        Some(HunkActionRequest {
            action,
            file_id: file.id.clone(),
            path: file.path.clone(),
            hunk,
            patch,
        })
    }

    fn render_row(
        &self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = &self.theme;
        let row_height = px(ROW_HEIGHT);
        let scroll = self.scroll.0.borrow().base_handle.clone();
        let scroll_x = -scroll.offset().x;
        let viewport_width = scroll.bounds().size.width;
        let _ = window;
        match &self.rows[index] {
            Row::Truncated => div()
                .h(row_height)
                .w(viewport_width)
                .relative()
                .left(scroll_x)
                .flex()
                .items_center()
                .px(px(12.))
                .text_size(px(12.))
                .text_color(theme.content(0.45))
                .child("Diff is too large to display in full. File list is shown without patches.")
                .into_any_element(),
            Row::FileHeader { file } => {
                self.render_file_header(*file, scroll_x, viewport_width, cx)
            }
            Row::Empty { message, .. } => div()
                .h(row_height)
                .w(viewport_width)
                .relative()
                .left(scroll_x)
                .flex()
                .items_center()
                .px(px(12.))
                .text_size(px(12.))
                .text_color(theme.content(0.45))
                .child(message.clone())
                .into_any_element(),
            Row::Hunk { file, hunk } => {
                self.render_hunk_header(index, *file, *hunk, scroll_x, viewport_width, cx)
            }
            Row::Fold {
                file,
                block,
                hidden,
            } => {
                let (file, block, hidden) = (*file, *block, *hidden);
                let button = |id: &'static str, kind: IconKind| {
                    div()
                        .id(id)
                        .size(px(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .hover(|this| this.bg(theme.hover))
                        .child(icon(kind, px(12.), theme.content(0.4)))
                };
                div()
                    .id(("fold", index))
                    .h(row_height)
                    .w(viewport_width)
                    .relative()
                    .left(scroll_x)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .bg(theme.content(0.08))
                    .child(button("up", IconKind::ChevronUp).on_click(cx.listener(
                        move |this, _, _, cx| this.reveal_fold(file, block, FoldDirection::Up, cx),
                    )))
                    .child(button("down", IconKind::ChevronDown).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.reveal_fold(file, block, FoldDirection::Down, cx)
                        },
                    )))
                    .child(
                        div()
                            .id("all")
                            .flex_1()
                            .font_family(theme.mono_font.clone())
                            .text_size(px(11.))
                            .text_color(theme.content(0.45))
                            .hover(|this| this.text_color(theme.content(0.7)))
                            .child(format!(
                                "{hidden} unmodified {}",
                                if hidden == 1 { "line" } else { "lines" }
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.reveal_fold(file, block, FoldDirection::All, cx)
                            })),
                    )
                    .into_any_element()
            }
            Row::Line { file, line } => self.render_line(index, *file, *line, scroll_x, cx),
        }
    }

    fn render_file_header(
        &self,
        file_index: usize,
        scroll_x: gpui::Pixels,
        width: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = &self.theme;
        let file = &self.files[file_index];
        let expanded = self.open.contains(&file_index);
        let busy = self.busy.as_ref() == Some(&file.id);
        let has_handler = self.on_file_action.is_some();
        let action_button = |id: &'static str, kind: IconKind, action: DiffAction| {
            let file_id = file.id.clone();
            div()
                .id(id)
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .when(busy, |this| this.opacity(0.4))
                .when(!busy, |this| this.hover(|this| this.bg(theme.hover)))
                .child(icon(kind, px(13.), theme.content(0.55)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.busy.as_ref() == Some(&file_id) {
                        return;
                    }
                    if let Some(handler) = this.on_file_action.clone() {
                        let file_id = file_id.clone();
                        window.defer(cx, move |window, cx| handler(action, file_id, window, cx));
                    }
                }))
        };
        div()
            .id(("file", file_index))
            .h(px(ROW_HEIGHT))
            .w(width)
            .relative()
            .left(scroll_x)
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .bg(theme.panel_background)
            .border_b_1()
            .border_color(theme.stroke)
            .child(
                div()
                    .id("toggle")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .child(icon(
                        if expanded {
                            IconKind::ChevronDown
                        } else {
                            IconKind::ChevronRight
                        },
                        px(14.),
                        theme.content(0.45),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(theme.mono_font.clone())
                            .text_size(px(12.))
                            .text_color(theme.content(0.85))
                            .child(file.label.clone()),
                    )
                    .child(diff_counts(theme, file.diff.additions, file.diff.deletions))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_file(file_index, cx))),
            )
            .when(has_handler && file.actions.discard, |this| {
                this.child(action_button(
                    "discard",
                    IconKind::Undo,
                    DiffAction::Discard,
                ))
            })
            .when(has_handler && file.actions.unstage, |this| {
                this.child(action_button(
                    "unstage",
                    IconKind::Minus,
                    DiffAction::Unstage,
                ))
            })
            .when(has_handler && file.actions.stage, |this| {
                this.child(action_button("stage", IconKind::Check, DiffAction::Stage))
            })
            .into_any_element()
    }

    fn render_hunk_header(
        &self,
        row: usize,
        file_index: usize,
        hunk_index: usize,
        scroll_x: gpui::Pixels,
        width: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = &self.theme;
        let file = &self.files[file_index];
        let hunk = &file.hunks[hunk_index];
        let busy = self.busy.as_ref() == Some(&file.id);
        let has_handler = self.on_hunk_action.is_some();
        let text_button = |id: &'static str, label: &'static str, action: DiffAction| {
            div()
                .id(id)
                .h(px(18.))
                .px(px(6.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_size(px(11.))
                .text_color(theme.content(0.6))
                .when(busy, |this| this.opacity(0.4))
                .when(!busy, |this| {
                    this.hover(|this| this.bg(theme.hover).text_color(theme.foreground))
                })
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_hunk_action(file_index, hunk_index, action, window, cx)
                }))
        };
        div()
            .id(("hunk", row))
            .h(px(ROW_HEIGHT))
            .w(width)
            .relative()
            .left(scroll_x)
            .flex()
            .items_center()
            .bg(theme.content(0.05))
            .child(div().w(px(GUTTER_WIDTH)).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .px(px(12.))
                    .font_family(theme.mono_font.clone())
                    .text_size(px(11.))
                    .text_color(theme.content(0.4))
                    .child(hunk.header.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .pr(px(8.))
                    .when(has_handler && file.actions.discard_hunk, |this| {
                        this.child(text_button("discard", "Discard", DiffAction::Discard))
                    })
                    .when(has_handler && file.actions.unstage_hunk, |this| {
                        this.child(text_button("unstage", "Unstage", DiffAction::Unstage))
                    })
                    .when(has_handler && file.actions.stage_hunk, |this| {
                        this.child(text_button("stage", "Stage", DiffAction::Stage))
                    }),
            )
            .into_any_element()
    }

    fn render_line(
        &self,
        row: usize,
        file_index: usize,
        line_index: usize,
        scroll_x: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let theme = &self.theme;
        let file = &self.files[file_index];
        let line = &file.diff.lines[line_index];
        let added = line.kind == UnifiedLineKind::Add;
        let deleted = line.kind == UnifiedLineKind::Del;
        let number = if deleted {
            line.old_number
        } else {
            line.new_number
        };
        let row_bg = if added {
            Some(theme.diff_added_row)
        } else if deleted {
            Some(theme.diff_deleted_row)
        } else {
            None
        };
        let gutter_tint = if added {
            theme.diff_added_gutter
        } else if deleted {
            theme.diff_deleted_gutter
        } else {
            gpui::transparent_black()
        };
        let number_color = if added {
            theme.diff_added_number
        } else if deleted {
            theme.diff_deleted_number
        } else {
            theme.content(0.35)
        };
        let hovered = self.hovered_row == Some(row);
        let can_comment = self.on_comment.is_some() && file.actions.comment;

        let styles = self
            .tokens
            .get(&file_index)
            .and_then(|tokens| tokens.get(line_index).cloned().flatten());
        let text: SharedString = line.text.replace('\t', "    ").into();
        let styled = match styles {
            // Tabs widen the text, so runs only apply when none were expanded.
            Some(styles) if !line.text.contains('\t') => StyledText::new(text)
                .with_highlights(styles)
                .into_any_element(),
            _ => StyledText::new(text).into_any_element(),
        };

        let comment_target = DiffCommentTarget {
            path: file.path.to_string(),
            line: line.clone(),
        };
        let gutter = div()
            .absolute()
            .top_0()
            .left(scroll_x)
            .w(px(GUTTER_WIDTH))
            .h(px(ROW_HEIGHT))
            .bg(theme.panel_background)
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .pr(px(8.))
                    .when_some(row_bg, |this, bg| this.bg(bg))
                    .bg(gutter_tint)
                    .font_family(theme.mono_font.clone())
                    .text_size(px(11.))
                    .text_color(number_color)
                    .children(number.map(|number| number.to_string())),
            )
            .when(can_comment && hovered, |this| {
                this.child(
                    div()
                        .id("comment")
                        .absolute()
                        .top(px(3.))
                        .left(px(2.))
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .bg(theme.foreground)
                        .child(icon(IconKind::Comment, px(11.), theme.panel_background))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(handler) = this.on_comment.clone() {
                                let target = comment_target.clone();
                                window.defer(cx, move |window, cx| handler(target, window, cx));
                            }
                        })),
                )
            });

        div()
            .id(("line", row))
            .relative()
            .h(px(ROW_HEIGHT))
            .min_w_full()
            .flex()
            .items_center()
            .when_some(row_bg, |this, bg| this.bg(bg))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let next = if *hovered { Some(row) } else { None };
                if this.hovered_row != next && (*hovered || this.hovered_row == Some(row)) {
                    this.hovered_row = next;
                    cx.notify();
                }
            }))
            .child(div().w(px(GUTTER_WIDTH)).flex_none())
            // The +/- mark, so added and removed lines do not rely on color.
            .child(
                div()
                    .w(px(MARK_WIDTH))
                    .flex_none()
                    .pl(px(12.))
                    .font_family(theme.mono_font.clone())
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(number_color)
                    .child(line_mark(line.kind)),
            )
            .child(
                div()
                    .flex_none()
                    .pr(px(12.))
                    .whitespace_nowrap()
                    .font_family(theme.mono_font.clone())
                    .text_size(px(12.))
                    .text_color(theme.content(0.8))
                    .when(line.kind == UnifiedLineKind::Context, |this| {
                        this.opacity(0.7)
                    })
                    .child(styled),
            )
            .child(gutter)
            .into_any_element()
    }
}

/// The mark before a line: `+` added, `−` (U+2212) removed, nothing for
/// context.
pub fn line_mark(kind: UnifiedLineKind) -> &'static str {
    match kind {
        UnifiedLineKind::Add => "+",
        UnifiedLineKind::Del => "\u{2212}",
        _ => "",
    }
}

fn diff_counts(theme: &EditorTheme, additions: usize, deletions: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .when(additions > 0, |this| {
            this.child(
                div()
                    .text_color(theme.diff_added_number)
                    .child(format!("+{additions}")),
            )
        })
        .when(deletions > 0, |this| {
            this.child(
                div()
                    .text_color(theme.diff_deleted_number)
                    .child(format!("-{deletions}")),
            )
        })
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        if self.files.is_empty() {
            return div()
                .size_full()
                .px(px(16.))
                .py(px(24.))
                .font_family(theme.ui_font.clone())
                .text_size(px(13.))
                .text_color(theme.content(0.45))
                .child("No file changes")
                .into_any_element();
        }
        let count = self.file_count.unwrap_or(self.files.len());
        let file_label = if count == 1 {
            "1 file".to_string()
        } else {
            format!("{count} files")
        };
        let additions: usize = self.files.iter().map(|file| file.diff.additions).sum();
        let deletions: usize = self.files.iter().map(|file| file.diff.deletions).sum();
        let collapse_disabled = self.open.is_empty();
        let header_button = |id: &'static str, kind: IconKind, disabled: bool| {
            div()
                .id(id)
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .when(disabled, |this| this.opacity(0.4))
                .when(!disabled, |this| this.hover(|this| this.bg(theme.hover)))
                .child(icon(kind, px(14.), theme.content(0.45)))
        };
        let row_count = self.rows.len();
        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .font_family(theme.ui_font.clone())
            .text_color(theme.foreground)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .h(px(32.))
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(theme.stroke)
                    .text_size(px(12.))
                    .child(div().text_color(theme.content(0.7)).child(file_label))
                    .child(diff_counts(&theme, additions, deletions))
                    .child(div().flex_1())
                    .child(
                        header_button("expand-all", IconKind::ChevronDown, false)
                            .on_click(cx.listener(|this, _, _, cx| this.expand_all(cx))),
                    )
                    .child(
                        header_button("collapse-all", IconKind::ChevronUp, collapse_disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.collapse_all(cx))),
                    ),
            )
            .child(
                uniform_list(
                    "unified-diff",
                    row_count,
                    cx.processor(|this, range: Range<usize>, window, cx| {
                        range
                            .map(|index| this.render_row(index, window, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                .with_width_from_item(Some(self.widest_row))
                .flex_1()
                .min_h_0(),
            )
            .into_any_element()
    }
}

/// The diff of one file as an entity, for callers that only show one.
pub fn single_file_diff(
    path: &str,
    old: &str,
    new: &str,
    theme: EditorTheme,
    cx: &mut App,
) -> Entity<DiffView> {
    let file = DiffFile::from_texts(path.to_owned(), old, new);
    cx.new(|cx| DiffView::new(vec![file], theme, cx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    const PATCH: &str = "diff --git a/src/a.rs b/src/a.rs
index 1..2 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,3 @@
 fn a() {}
-fn b() {}
+fn bee() {}
 fn c() {}
@@ -10,2 +10,3 @@ impl X {
 fn j() {}
+fn k() {}
 fn l() {}
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
";

    // UnifiedDiffView.markers.test.ts
    #[gpui::test]
    fn marks_added_and_removed_lines_with_a_glyph_not_only_color(cx: &mut TestAppContext) {
        let file = DiffFile::from_texts("a.ts", "alpha\nbeta\ngamma\n", "alpha\nBETA\ngamma\n");
        let view = cx.new(|cx| DiffView::new(vec![file], EditorTheme::dark(), cx));
        view.read_with(cx, |view, _| {
            let rows: Vec<(&str, &str)> = view
                .rows
                .iter()
                .filter_map(|row| match row {
                    Row::Line { file, line } => {
                        let line = &view.files[*file].diff.lines[*line];
                        Some((line_mark(line.kind), line.text.as_str()))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                rows,
                vec![
                    ("", "alpha"),
                    ("\u{2212}", "beta"),
                    ("+", "BETA"),
                    ("", "gamma")
                ]
            );
        });
    }

    #[gpui::test]
    fn rows_cover_headers_hunks_and_lines(cx: &mut TestAppContext) {
        let view = cx.new(|cx| DiffView::new(parse_diff(PATCH), EditorTheme::dark(), cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.rows[0], Row::FileHeader { file: 0 });
            assert_eq!(view.rows[1], Row::Hunk { file: 0, hunk: 0 });
            let hunks = view
                .rows
                .iter()
                .filter(|row| matches!(row, Row::Hunk { .. }))
                .count();
            assert_eq!(hunks, 2);
            assert!(view.rows.contains(&Row::FileHeader { file: 1 }));
            assert!(
                view.rows
                    .iter()
                    .any(|row| matches!(row, Row::Empty { file: 1, .. }))
            );
        });
        view.update(cx, |view, cx| view.collapse_all(cx));
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.rows,
                vec![Row::FileHeader { file: 0 }, Row::FileHeader { file: 1 }]
            );
        });
    }

    #[gpui::test]
    fn folds_expand_in_steps(cx: &mut TestAppContext) {
        let old: String = (1..=60).map(|i| format!("line {i}\n")).collect();
        let new = old.replace("line 30\n", "LINE 30\n");
        let view = cx.new(|cx| {
            DiffView::new(
                vec![DiffFile::from_texts("a.txt", &old, &new)],
                EditorTheme::dark(),
                cx,
            )
        });
        let fold = view.read_with(cx, |view, _| {
            view.rows
                .iter()
                .find_map(|row| match row {
                    Row::Fold { block, hidden, .. } => Some((*block, *hidden)),
                    _ => None,
                })
                .unwrap()
        });
        assert_eq!(fold.1, 26);
        view.update(cx, |view, cx| {
            view.reveal_fold(0, fold.0, FoldDirection::Down, cx)
        });
        view.read_with(cx, |view, _| {
            let hidden = view.rows.iter().find_map(|row| match row {
                Row::Fold { block, hidden, .. } if *block == fold.0 => Some(*hidden),
                _ => None,
            });
            assert_eq!(hidden, Some(6));
        });
        view.update(cx, |view, cx| {
            view.reveal_fold(0, fold.0, FoldDirection::All, cx)
        });
        view.read_with(cx, |view, _| {
            assert!(
                !view
                    .rows
                    .iter()
                    .any(|row| matches!(row, Row::Fold { block, .. } if *block == fold.0))
            );
        });
    }

    #[gpui::test]
    fn hunk_requests_carry_a_patch(cx: &mut TestAppContext) {
        let view = cx.new(|cx| DiffView::new(parse_diff(PATCH), EditorTheme::dark(), cx));
        view.read_with(cx, |view, _| {
            let request = view.hunk_request(0, 1, DiffAction::Stage).unwrap();
            assert_eq!(request.path.as_ref(), "src/a.rs");
            assert!(
                request
                    .patch
                    .contains("@@ -10,2 +10,3 @@\n fn j() {}\n+fn k() {}\n fn l() {}\n")
            );
        });
    }

    #[gpui::test]
    fn highlights_both_sides_in_the_background(cx: &mut TestAppContext) {
        let view = cx.new(|cx| DiffView::new(parse_diff(PATCH), EditorTheme::dark(), cx));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let tokens = view.tokens.get(&0).unwrap();
            let file = &view.files[0];
            let deleted = file
                .diff
                .lines
                .iter()
                .position(|line| line.kind == UnifiedLineKind::Del)
                .unwrap();
            let styles = tokens[deleted].as_ref().unwrap();
            let keyword = view.theme.syntax.keyword;
            assert!(styles.iter().any(|(_, style)| style.color == Some(keyword)));
        });
    }

    #[gpui::test]
    fn pending_highlight_cannot_restore_the_previous_theme(cx: &mut TestAppContext) {
        let original = (0..60)
            .map(|line| format!("fn line_{line}() {{}}\n"))
            .collect::<String>();
        let current = original.replace("fn line_30()", "fn changed_30()");
        let file = DiffFile::from_texts("change.rs", &original, &current);
        let expected_diff = file.diff.clone();
        let view = cx.new(|cx| DiffView::new(Vec::new(), EditorTheme::dark(), cx));
        view.update(cx, |view, cx| {
            view.highlight_delay = Some(std::time::Duration::from_secs(1));
            view.set_files(vec![file], InitialExpansion::All, cx);
            let fold = view.files[0]
                .diff
                .blocks
                .iter()
                .position(UnifiedBlock::is_fold)
                .unwrap();
            view.reveal_fold(0, fold, FoldDirection::Down, cx);
            view.scroll.scroll_to_item(10, ScrollStrategy::Top);
        });
        cx.run_until_parked();
        let (open, reveals, rows) = view.read_with(cx, |view, _| {
            assert!(
                view.tokens[&0].is_empty(),
                "the old syntax job must be pending"
            );
            (view.open.clone(), view.reveals.clone(), view.rows.clone())
        });
        view.update(cx, |view, cx| view.set_theme(EditorTheme::light(), cx));
        cx.run_until_parked();
        let assert_light_syntax = |view: &DiffView| {
            let deleted = view.files[0]
                .diff
                .lines
                .iter()
                .position(|line| line.kind == UnifiedLineKind::Del)
                .unwrap();
            let styles = view.tokens[&0][deleted].as_ref().unwrap();
            assert!(
                styles
                    .iter()
                    .any(|(_, style)| { style.color == Some(EditorTheme::light().syntax.keyword) }),
                "a completed old syntax job must not replace the new palette",
            );
        };
        view.read_with(cx, |view, _| assert_light_syntax(view));
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.theme, EditorTheme::light());
            assert_eq!(view.files[0].diff, expected_diff);
            assert_eq!(view.open, open);
            assert_eq!(view.reveals, reveals);
            assert_eq!(view.rows, rows);
            assert_eq!(view.scroll.logical_scroll_top_index(), 10);
            assert_light_syntax(view);
        });
    }
}
