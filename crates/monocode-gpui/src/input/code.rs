//! The editor shares native selection/IME handling with Input and retains glyph
//! runs for visible lines. Scroll position is independent of the caret.
use super::*;
use crate::{
    editor_analysis::Analysis,
    editor_scrollbar::{Metrics, WIDTH},
};
use gpui::{AnyElement, Task};
use std::{cell::Cell, sync::Arc};

const TOP_PADDING: f32 = 8.;
const BOTTOM_PADDING: f32 = 32.;

#[derive(Clone)]
struct MarkerHit {
    bounds: Bounds<Pixels>,
    pos: usize,
    diagnostic: bool,
    message: String,
}

pub(super) struct Code {
    hovered_change: Option<usize>,
    can_stage: bool,
    suppress_completion: bool,
    diff: bool,
    deleted_rows: Vec<(f32, f32, String, usize, Vec<crate::editor_analysis::Token>)>,
    path: String,
    completions: Vec<crate::editor_analysis::Completion>,
    completion_index: usize,
    completion_task: Option<Task<()>>,
    completion_source: Option<(SharedString, usize)>,
    folded: Vec<crate::editor_analysis::Fold>,
    search_matches: Vec<Range<usize>>,
    offsets: Vec<usize>,
    visible: Vec<(usize, WrappedLine)>,
    reveal: bool,
    max_width: Pixels,
    undo: Vec<(SharedString, Range<usize>)>,
    redo: Vec<(SharedString, Range<usize>)>,
    wrap: bool,
    layout_width: Option<Pixels>,
    tops: Vec<f32>,
    analysis: Arc<Analysis>,
    /// The buffer as of the last change, which `analysis` offsets follow.
    text: SharedString,
    rail: Option<Bounds<Pixels>>,
    drag_offset: Option<f32>,
    hovered: bool,
    markers: Vec<MarkerHit>,
    hovered_marker: Option<usize>,
    affinity: Cell<Option<(usize, bool)>>,
}
impl Code {
    fn new() -> Self {
        Self {
            hovered_change: None,
            can_stage: false,
            suppress_completion: false,
            diff: false,
            deleted_rows: Vec::new(),
            path: String::new(),
            completions: Vec::new(),
            completion_index: 0,
            completion_task: None,
            completion_source: None,
            folded: Vec::new(),
            search_matches: Vec::new(),
            offsets: vec![0],
            visible: Vec::new(),
            reveal: true,
            max_width: px(0.),
            undo: Vec::new(),
            redo: Vec::new(),
            wrap: true,
            layout_width: None,
            tops: vec![TOP_PADDING, TOP_PADDING + 20.],
            analysis: Arc::new(Analysis::default()),
            text: SharedString::default(),
            rail: None,
            drag_offset: None,
            hovered: false,
            markers: Vec::new(),
            hovered_marker: None,
            affinity: Cell::new(None),
        }
    }
    fn row(&self, offset: usize) -> usize {
        self.offsets
            .partition_point(|start| *start <= offset)
            .saturating_sub(1)
    }
    fn fold_at(&self, row: usize) -> Option<&crate::editor_analysis::Fold> {
        self.folded.iter().find(|f| self.row(f.from) == row)
    }
    fn hidden(&self, row: usize) -> bool {
        self.folded
            .iter()
            .any(|f| row > self.row(f.from) && row <= self.row(f.to))
    }
    fn display_row(&self, at: usize) -> usize {
        let row = self.row(at);
        self.folded
            .iter()
            .find(|f| row > self.row(f.from) && row <= self.row(f.to))
            .map_or(row, |f| self.row(f.from))
    }
    fn display_offset(&self, row: usize, at: usize) -> usize {
        let start = self.offsets[row];
        if let Some(f) = self.fold_at(row) {
            if at >= f.to {
                return f.from - start + "…".len() + at - f.to;
            }
            if at > f.from {
                return f.from - start;
            }
        }
        at.saturating_sub(start)
    }
    fn source_offset(&self, row: usize, at: usize) -> usize {
        let start = self.offsets[row];
        if let Some(f) = self.fold_at(row) {
            let prefix = f.from - start;
            if at > prefix {
                return f.to + at.saturating_sub(prefix + "…".len());
            }
        }
        start + at
    }
    fn source_end(&self, row: usize, len: usize) -> usize {
        let row = self.fold_at(row).map_or(row, |f| self.row(f.to));
        self.offsets.get(row + 1).map_or(len, |s| s - 1)
    }
    fn gutter(&self) -> f32 {
        let digits = self.offsets.len().to_string().len() as f32;
        if self.diff {
            4. + 12. + digits * 7.5 + 5. + 16. + 1. + 6.
        } else {
            4. + (digits * 7.5 + 12.).max(36.) + 12. + 1. + 6.
        }
    }
    fn top(&self, row: usize, line_height: f32) -> f32 {
        self.tops
            .get(row)
            .copied()
            .unwrap_or(TOP_PADDING + row as f32 * line_height)
    }
    fn total(&self) -> f32 {
        self.tops.last().copied().unwrap_or(TOP_PADDING) + BOTTOM_PADDING
    }
    fn row_at_y(&self, y: f32) -> usize {
        self.tops
            .partition_point(|top| *top <= y)
            .saturating_sub(1)
            .min(self.offsets.len() - 1)
    }
}
impl Input {
    pub(crate) fn editor(cx: &mut Context<Self>) -> Self {
        let mut input = Self::new(cx);
        input.code = Some(Code::new());
        input.code.as_mut().unwrap().wrap = cx
            .try_global::<crate::settings::Preferences>()
            .is_none_or(|p| p.file_word_wrap);
        input.label = "File editor".into();
        input.placeholder = "".into();
        input.font_size = 12.5;
        input.line_height = 20.;
        input.secondary_submit = true;
        input
    }
    pub(crate) fn set_editor_diff(&mut self, diff: bool, can_stage: bool) {
        if let Some(code) = &mut self.code {
            code.can_stage = can_stage;
        }
        if let Some(code) = &mut self.code
            && code.diff != diff
        {
            code.diff = diff;
            code.layout_width = None;
        }
    }
    pub(crate) fn set_editor_path(&mut self, path: String) {
        if let Some(code) = &mut self.code {
            code.path = path;
        }
    }
    pub(crate) fn selection_range(&self) -> Range<usize> {
        self.selection.clone()
    }
    pub(crate) fn replace_bytes(
        &mut self,
        range: Range<usize>,
        text: &str,
        cursor: usize,
        cx: &mut Context<Self>,
    ) {
        if self.read_only || self.is_composing() || !self.accept_code_edit(&range, text, cx) {
            return;
        }
        self.record_undo();
        self.map_code_folds(&range, text.len());
        self.content = format!(
            "{}{}{}",
            &self.content[..range.start],
            text,
            &self.content[range.end..]
        )
        .into();
        let at = range.start + cursor.min(text.len());
        self.selection = at..at;
        self.reversed = false;
        self.code_changed();
        if let Some(code) = &mut self.code {
            code.suppress_completion = true;
        }
        cx.emit(Changed);
        cx.notify();
    }
    pub(super) fn unfold_range(&mut self, range: &Range<usize>) {
        if let Some(code) = &mut self.code {
            let before = code.folded.len();
            code.folded.retain(|f| {
                !(range.start > f.from && range.start < f.to
                    || range.end > f.from && range.end < f.to)
            });
            if code.folded.len() != before {
                code.layout_width = None;
            }
        }
    }
    pub(crate) fn set_search_matches(
        &mut self,
        matches: Vec<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(code) = &mut self.code {
            code.search_matches = matches;
        }
        cx.notify();
    }
    pub(crate) fn schedule_completion(&mut self, explicit: bool, cx: &mut Context<Self>) {
        if self.is_composing() || !self.selection.is_empty() {
            return;
        }
        let at = self.cursor();
        if !explicit
            && !self.content[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
        {
            return;
        }
        let Some(code) = &mut self.code else {
            return;
        };
        if !explicit && std::mem::take(&mut code.suppress_completion) {
            return;
        }
        let source = self.content.clone();
        let path = code.path.clone();
        code.completion_source = Some((source.clone(), at));
        code.completion_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(if explicit {
                    0
                } else {
                    100
                }))
                .await;
            let text = source.clone();
            let result = cx
                .background_executor()
                .spawn(async move { crate::editor_analysis::complete(&path, &text, at, explicit) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.content == source
                    && this.cursor() == at
                    && !this.is_composing()
                    && let Some(code) = &mut this.code
                    && code.completion_source.as_ref() == Some(&(source, at))
                {
                    code.completions = result.unwrap_or_default();
                    code.completion_index = 0;
                    cx.notify();
                }
            });
        }));
    }
    pub(super) fn clear_completion(&mut self) {
        if let Some(code) = &mut self.code {
            code.completions.clear();
            code.completion_task = None;
            code.completion_source = None;
        }
    }
    pub(super) fn start_completion(
        &mut self,
        _: &Complete,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.schedule_completion(true, cx);
    }
    pub(super) fn accept_completion(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(code) = &self.code else {
            return false;
        };
        if self.is_composing()
            || code.completion_source.as_ref() != Some(&(self.content.clone(), self.cursor()))
        {
            return false;
        }
        let Some(option) = code.completions.get(code.completion_index).cloned() else {
            return false;
        };
        self.replace_bytes(option.from..option.to, &option.insert, option.cursor, cx);
        true
    }
    pub(super) fn completion_step(&mut self, backwards: bool, cx: &mut Context<Self>) -> bool {
        let Some(code) = &mut self.code else {
            return false;
        };
        if code.completions.is_empty() {
            return false;
        }
        code.completion_index = (code.completion_index
            + code.completions.len()
            + if backwards {
                code.completions.len() - 1
            } else {
                1
            })
            % code.completions.len();
        cx.notify();
        true
    }
    pub(super) fn dismiss_completion(
        &mut self,
        _: &DismissCompletion,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(code) = &mut self.code else {
            cx.propagate();
            return;
        };
        if code.completions.is_empty() && code.completion_task.is_none() {
            cx.propagate();
            return;
        }
        code.completions.clear();
        code.completion_task = None;
        code.completion_source = None;
        cx.notify();
    }
    fn expand_emmet(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(code) = &self.code else {
            return false;
        };
        let path = code.path.clone();
        // Expansion is requested explicitly with Tab. The same bounded worker
        // service is used here, and stale results never replace newer typing.
        let source = self.content.clone();
        let at = self.cursor();
        if !matches!(
            std::path::Path::new(&path)
                .extension()
                .and_then(|s| s.to_str()),
            Some("html" | "htm" | "jsx" | "tsx" | "md" | "mdx" | "markdown")
        ) {
            return false;
        }
        let token = source[..at].split_whitespace().last().unwrap_or("");
        if token.is_empty() || token.contains(['<', '=', ';', '"', '\'']) {
            return false;
        }
        cx.spawn(async move |this, cx| {
            let text = source.clone();
            let result = cx
                .background_executor()
                .spawn(async move { crate::editor_analysis::emmet(&path, &text, at) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.content == source && this.cursor() == at && !this.is_composing() {
                    if let Ok(Some(result)) = result {
                        this.replace_bytes(0..source.len(), &result.formatted, result.cursor, cx);
                    } else {
                        this.replace_bytes(at..at, "  ", 2, cx);
                    }
                }
            });
        })
        .detach();
        true
    }
    pub(super) fn completion_popup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let code = self.code.as_ref()?;
        if code.completions.is_empty()
            || code.completion_source.as_ref() != Some(&(self.content.clone(), self.cursor()))
        {
            return None;
        }
        let p = self.code_position(self.cursor());
        let gutter = code.gutter();
        let height = (code.completions.len().min(10) as f32 * 22.).min(240.);
        let viewport = self.bounds.map_or(400., |b| b.size.height.as_f32());
        let y = (p.y - self.scroll_y).as_f32();
        let top = if y + self.line_height + height > viewport {
            (y - height).max(0.)
        } else {
            y + self.line_height
        };
        let first = code.completion_index.saturating_sub(9);
        let viewport_width = self.bounds.map_or(400., |b| b.size.width.as_f32() + gutter);
        let width = code
            .completions
            .iter()
            .map(|o| (o.label.chars().count() + o.detail.chars().count()) as f32 * 7.2 + 44.)
            .fold(180., f32::max)
            .min(400.)
            .min((viewport_width - 8.).max(1.));
        let left = (gutter + (p.x - self.scroll_x).as_f32())
            .clamp(4., (viewport_width - width - 4.).max(4.));
        Some(
            div()
                .id("editor-completions")
                .role(Role::List)
                .aria_label("Autocomplete")
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .rounded(px(6.))
                .border_1()
                .border_color(crate::theme::content(0.12))
                .bg(crate::theme::shell_fill().opacity(1.))
                .shadow_md()
                .overflow_hidden()
                .py_1()
                .children(
                    code.completions
                        .iter()
                        .enumerate()
                        .skip(first)
                        .take(10)
                        .map(|(n, option)| {
                            div()
                                .id(("completion", n))
                                .role(Role::ListItem)
                                .aria_label(option.label.clone())
                                .px_2()
                                .h(px(22.))
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_size(px(12.))
                                .text_color(crate::theme::content(1.))
                                .when(n == code.completion_index, |d| {
                                    d.bg(crate::theme::accent_color().opacity(0.25))
                                })
                                .child(div().text_color(crate::theme::content(0.5)).child(
                                    match option.kind.as_str() {
                                        "function" => "ƒ",
                                        "keyword" => "k",
                                        "variable" => "v",
                                        _ => "·",
                                    },
                                ))
                                .child(option.label.clone())
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .text_color(crate::theme::content(0.5))
                                        .child(option.detail.clone()),
                                )
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(code) = &mut this.code {
                                        code.completion_index = n;
                                    }
                                    this.accept_completion(cx);
                                }))
                        }),
                )
                .into_any_element(),
        )
    }
    fn toggle_fold_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(code) = &mut self.code else {
            return;
        };
        if let Some(n) = code.folded.iter().position(|f| code.row(f.from) == row) {
            code.folded.remove(n);
        } else if let Some(fold) = code
            .analysis
            .folds
            .iter()
            .find(|f| code.row(f.from) == row)
            .cloned()
        {
            if self.selection.end > fold.from && self.selection.start < fold.to {
                self.selection = fold.from..fold.from;
            }
            code.folded.push(fold);
        }
        code.layout_width = None;
        cx.notify();
    }
    pub(super) fn fold(&mut self, _: &FoldCode, _: &mut Window, cx: &mut Context<Self>) {
        let Some(code) = &self.code else {
            cx.propagate();
            return;
        };
        let row = code.row(self.cursor());
        if !code.folded.iter().any(|f| code.row(f.from) == row) {
            self.toggle_fold_row(row, cx);
        }
    }
    pub(super) fn unfold(&mut self, _: &UnfoldCode, _: &mut Window, cx: &mut Context<Self>) {
        let Some(code) = &self.code else {
            cx.propagate();
            return;
        };
        let row = code.display_row(self.cursor());
        if code.fold_at(row).is_some() {
            self.toggle_fold_row(row, cx);
        }
    }
    pub(super) fn fold_all(&mut self, _: &FoldAll, _: &mut Window, cx: &mut Context<Self>) {
        let Some(code) = &mut self.code else {
            cx.propagate();
            return;
        };
        code.folded = code.analysis.folds.clone();
        code.folded.sort_by_key(|f| f.from);
        let mut end = 0;
        code.folded.retain(|f| {
            if f.from < end {
                false
            } else {
                end = f.to;
                true
            }
        });
        code.layout_width = None;
        self.selection = 0..0;
        cx.notify();
    }
    pub(super) fn unfold_all(&mut self, _: &UnfoldAll, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(code) = &mut self.code {
            code.folded.clear();
            code.layout_width = None;
            cx.notify();
        }
    }
    pub(super) fn code_fold_down(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        let (Some(code), Some(bounds)) = (&self.code, self.bounds) else {
            return false;
        };
        if e.position.x < bounds.left() - px(14.)
            || e.position.x >= bounds.left()
            || e.position.y < bounds.top()
            || e.position.y > bounds.bottom()
        {
            return false;
        }
        let row = code.row_at_y((e.position.y - bounds.top() + self.scroll_y).as_f32());
        self.toggle_fold_row(row, cx);
        true
    }
    pub(crate) fn word_wrap(&self) -> bool {
        self.code.as_ref().is_some_and(|c| c.wrap)
    }
    pub(super) fn code_rail_hovered(&self) -> bool {
        self.code
            .as_ref()
            .is_some_and(|code| code.hovered || code.drag_offset.is_some())
    }
    pub(crate) fn editor_analysis(&self) -> &Analysis {
        &self.code.as_ref().unwrap().analysis
    }
    pub(crate) fn set_word_wrap(&mut self, wrap: bool, cx: &mut Context<Self>) {
        if let Some(code) = &mut self.code
            && code.wrap != wrap
        {
            code.wrap = wrap;
            code.layout_width = None;
            code.reveal = true;
            if wrap {
                self.scroll_x = px(0.);
            }
            cx.notify();
        }
    }
    pub(crate) fn set_editor_analysis(&mut self, analysis: Analysis, cx: &mut Context<Self>) {
        if let Some(code) = &mut self.code {
            code.folded.retain(|fold| analysis.folds.contains(fold));
            code.analysis = Arc::new(analysis);
            code.layout_width = None;
            cx.notify();
        }
    }
    pub(crate) fn cursor_location(&self) -> (usize, usize) {
        let at = self.cursor();
        let row = self.code.as_ref().map_or(0, |c| c.row(at));
        let start = self.code.as_ref().map_or(0, |c| c.offsets[row]);
        (row + 1, self.content[start..at].chars().count() + 1)
    }
    pub(crate) fn editor_line_count(&self) -> usize {
        self.code.as_ref().map_or(1, |code| code.offsets.len())
    }
    pub(crate) fn reload_file(
        &mut self,
        text: String,
        preserve_viewport: bool,
        cx: &mut Context<Self>,
    ) {
        let selection = self.selection.clone();
        let scroll = point(self.scroll_x, self.scroll_y);
        self.set_text(text, cx);
        self.reset_editor_history();
        if preserve_viewport {
            let clamp = |at: usize| {
                let mut at = at.min(self.content.len());
                while !self.content.is_char_boundary(at) {
                    at -= 1;
                }
                at
            };
            self.selection = clamp(selection.start)..clamp(selection.end);
            self.scroll_x = scroll.x;
            self.scroll_y = scroll.y;
            if let Some(code) = &mut self.code {
                code.reveal = false;
            }
        } else {
            self.select_range(0..0, cx);
        }
    }
    pub(crate) fn reset_editor_history(&mut self) {
        if let Some(code) = &mut self.code {
            code.undo.clear();
            code.redo.clear();
        }
    }
    pub(super) fn accept_code_edit(
        &self,
        range: &Range<usize>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        use monocode_core::files::document::{MAX_LINE_BYTES, MAX_TEXT_BYTES};
        if self.content.len() - range.len() + text.len() > MAX_TEXT_BYTES {
            cx.emit(Rejected("Edit exceeds the 2 MB file limit."));
            return false;
        }
        let start = self.content[..range.start].rfind('\n').map_or(0, |n| n + 1);
        let end = self.content[range.end..]
            .find('\n')
            .map_or(self.content.len(), |n| range.end + n);
        let changed = format!(
            "{}{}{}",
            &self.content[start..range.start],
            text,
            &self.content[range.end..end]
        );
        if changed.split('\n').any(|line| line.len() > MAX_LINE_BYTES) {
            cx.emit(Rejected("Edit exceeds the 16 KB line limit."));
            return false;
        }
        true
    }
    pub(super) fn map_code_folds(&mut self, range: &Range<usize>, inserted: usize) {
        if let Some(code) = &mut self.code {
            code.folded.retain_mut(|f| {
                if range.end <= f.from {
                    f.from = f.from + inserted - range.len();
                    f.to = f.to + inserted - range.len();
                    true
                } else {
                    range.start >= f.to
                }
            });
        }
    }
    pub(super) fn clear_folds(&mut self) {
        if let Some(code) = &mut self.code {
            code.folded.clear();
            code.layout_width = None;
        }
    }
    pub(super) fn folded_boundary(&self, at: usize, backwards: bool) -> Option<usize> {
        self.code.as_ref()?.folded.iter().find_map(|f| {
            if backwards && at == f.to {
                Some(f.from)
            } else if !backwards && at == f.from {
                Some(f.to)
            } else {
                None
            }
        })
    }
    pub(super) fn code_changed(&mut self) {
        if let Some(code) = &mut self.code {
            code.completions.clear();
            code.completion_task = None;
            code.completion_source = None;
            code.deleted_rows.clear();
            code.hovered_change = None;
            code.offsets = std::iter::once(0)
                .chain(self.content.match_indices('\n').map(|(at, _)| at + 1))
                .collect();
            code.visible.clear();
            code.reveal = true;
            code.max_width = px(0.);
            code.layout_width = None;
            code.tops = (0..=code.offsets.len())
                .map(|row| TOP_PADDING + row as f32 * self.line_height)
                .collect();
            // Carry the last results through the edit until fresh analysis
            // lands; clearing them would flash the whole buffer uncoloured.
            let previous = std::mem::replace(&mut code.text, self.content.clone());
            if let Some(edit) = crate::editor_analysis::Edit::between(&previous, &self.content) {
                code.analysis = Arc::new(code.analysis.remapped(edit));
            }
            code.markers.clear();
            code.affinity.set(None);
        }
    }
    pub(super) fn reveal_code(&mut self) {
        if let Some(code) = &mut self.code {
            code.reveal = true;
        }
    }
    pub(super) fn record_undo(&mut self) {
        if let Some(code) = &mut self.code
            && self.marked.is_none()
        {
            code.undo
                .push((self.content.clone(), self.selection.clone()));
            let mut bytes = 0;
            let mut keep = 0;
            for (text, _) in code.undo.iter().rev().take(100) {
                bytes += text.len();
                if bytes > 8 * 1024 * 1024 {
                    break;
                }
                keep += 1;
            }
            let discard = code.undo.len().saturating_sub(keep.max(1));
            code.undo.drain(..discard);
            code.redo.clear();
        }
    }
    pub(super) fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.history(false, cx);
    }
    pub(super) fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.history(true, cx);
    }
    fn history(&mut self, redo: bool, cx: &mut Context<Self>) {
        let Some(code) = &mut self.code else {
            cx.propagate();
            return;
        };
        if self.marked.is_some() {
            return;
        }
        let source = if redo { &mut code.redo } else { &mut code.undo };
        let Some((text, selection)) = source.pop() else {
            return;
        };
        let target = if redo { &mut code.undo } else { &mut code.redo };
        target.push((self.content.clone(), self.selection.clone()));
        self.clear_folds();
        self.content = text;
        self.selection = selection;
        self.reversed = false;
        self.code_changed();
        cx.emit(Changed);
        cx.notify();
    }
    pub(super) fn indent(&mut self, _: &Indent, w: &mut Window, cx: &mut Context<Self>) {
        if self.code.is_none() {
            cx.propagate();
            return;
        }
        if self.accept_completion(cx) {
            return;
        }
        if self.selection.is_empty() {
            if self.expand_emmet(cx) {
                return;
            }
            self.replace_text_in_range(None, "  ", w, cx);
        } else {
            self.indent_lines(false, w, cx);
        }
    }
    pub(super) fn outdent(&mut self, _: &Outdent, w: &mut Window, cx: &mut Context<Self>) {
        if self.code.is_none() {
            cx.propagate();
            return;
        }
        self.indent_lines(true, w, cx);
    }
    fn indent_lines(&mut self, outdent: bool, w: &mut Window, cx: &mut Context<Self>) {
        if self.marked.is_some() {
            return;
        }
        let selection = self.selection.clone();
        let start = self.content[..selection.start]
            .rfind('\n')
            .map_or(0, |at| at + 1);
        let last = if selection.end > selection.start
            && self.content.as_bytes().get(selection.end - 1) == Some(&b'\n')
        {
            selection.end - 1
        } else {
            selection.end
        };
        let end = self.content[last..]
            .find('\n')
            .map_or(self.content.len(), |n| last + n);
        let replacement = self.content[start..end]
            .split('\n')
            .map(|line| {
                if outdent {
                    let remove = if line.starts_with('\t') {
                        1
                    } else {
                        line.bytes().take(4).take_while(|c| *c == b' ').count()
                    };
                    line[remove..].to_owned()
                } else {
                    format!("    {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let removed = (end - start).saturating_sub(replacement.len());
        let inserted = replacement.len();
        self.replace_text_in_range(Some(self.range16(start..end)), &replacement, w, cx);
        if selection.is_empty() {
            self.move_to(selection.start.saturating_sub(removed).max(start), cx);
        } else {
            self.select_range(start..start + inserted, cx);
        }
    }
    pub(super) fn code_position(&self, at: usize) -> Point<Pixels> {
        let code = self.code.as_ref().unwrap();
        let row = code.display_row(at);
        let p = code
            .visible
            .iter()
            .find(|(ix, _)| *ix == row)
            .map(|(_, line)| {
                line_position(
                    line,
                    code.display_offset(row, at),
                    self.line_height(),
                    code.affinity.get() == Some((at, true)),
                )
            })
            .unwrap_or_default();
        p + point(px(0.), px(code.top(row, self.line_height)))
    }
    pub(super) fn code_index(&self, p: Point<Pixels>) -> usize {
        let code = self.code.as_ref().unwrap();
        let row = code.row_at_y(p.y.as_f32());
        let start = code.offsets[row];
        let at = code
            .visible
            .iter()
            .find(|(ix, _)| *ix == row)
            .map_or(start, |(_, line)| {
                code.source_offset(
                    row,
                    line.closest_index_for_position(
                        point(p.x, p.y - px(code.top(row, self.line_height))),
                        self.line_height(),
                    )
                    .unwrap_or_else(|ix| ix),
                )
            });
        if let Some((_, line)) = code.visible.iter().find(|(ix, _)| *ix == row) {
            let local_y = (p.y.as_f32() - code.top(row, self.line_height)).max(0.);
            let downstream = line_position(
                line,
                code.display_offset(row, at),
                self.line_height(),
                false,
            );
            code.affinity.set(Some((
                at,
                (local_y / self.line_height).floor()
                    < (downstream.y.as_f32() / self.line_height).floor(),
            )));
        }
        at
    }
    pub(super) fn code_vertical(&self, down: bool) -> usize {
        let code = self.code.as_ref().unwrap();
        if code.wrap {
            let p = self.code_position(self.cursor());
            return self.code_index(point(
                p.x,
                (p.y + self.line_height() * if down { 1. } else { -1. }).max(px(0.)),
            ));
        }
        let row = code.row(self.cursor());
        let col = self.content[code.offsets[row]..self.cursor()]
            .graphemes(true)
            .count();
        let mut row = if down {
            (row + 1).min(code.offsets.len() - 1)
        } else {
            row.saturating_sub(1)
        };
        while code.hidden(row) && row > 0 && row + 1 < code.offsets.len() {
            if down {
                row += 1;
            } else {
                row -= 1;
            }
        }
        let start = code.offsets[row];
        let end = code
            .offsets
            .get(row + 1)
            .map_or(self.content.len(), |s| s - 1);
        start
            + self.content[start..end]
                .grapheme_indices(true)
                .nth(col)
                .map_or(end - start, |(at, _)| at)
    }
    pub(super) fn code_scroll(
        &mut self,
        e: &gpui::ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(code) = &mut self.code {
            let delta = e.delta.pixel_delta(px(self.line_height));
            let height = self.bounds.map_or(px(0.), |b| b.size.height);
            self.scroll_y = (self.scroll_y - delta.y)
                .max(px(0.))
                .min((px(code.total()) - height).max(px(0.)));
            self.scroll_x = (self.scroll_x - delta.x).max(px(0.)).min(code.max_width);
            code.reveal = false;
            cx.stop_propagation();
            cx.notify();
        }
    }
    pub(super) fn code_scrollbar_down(
        &mut self,
        e: &MouseDownEvent,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(code) = &mut self.code else {
            return false;
        };
        let Some(rail) = code.rail.filter(|b| b.contains(&e.position)) else {
            return false;
        };
        cx.stop_propagation();
        if let Some(marker) = code
            .markers
            .iter()
            .rev()
            .find(|m| m.bounds.contains(&e.position))
            .cloned()
        {
            if marker.diagnostic {
                self.move_to(marker.pos, cx);
            }
            let code = self.code.as_mut().unwrap();
            let row = code.row(marker.pos);
            self.scroll_y =
                px((code.top(row, self.line_height) - rail.size.height.as_f32() / 2.).max(0.));
            code.reveal = false;
            w.focus(&self.focus, cx);
        } else {
            let metrics = Metrics::new(
                self.scroll_y.as_f32(),
                code.total(),
                rail.size.height.as_f32(),
                rail.size.height.as_f32(),
            );
            if !metrics.scrollable {
                return true;
            }
            let y = (e.position.y - rail.top()).as_f32();
            let offset = if e.position.x >= rail.left() + px(3.)
                && e.position.x <= rail.right() - px(3.)
                && y >= metrics.thumb_top
                && y <= metrics.thumb_top + metrics.thumb_height
            {
                y - metrics.thumb_top
            } else {
                metrics.thumb_height / 2.
            };
            code.drag_offset = Some(offset);
            self.scroll_y = px(metrics.scroll_from_pointer(y, offset));
            code.reveal = false;
        }
        cx.notify();
        true
    }
    pub(super) fn code_scrollbar_move(
        &mut self,
        e: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(code) = &mut self.code else {
            return false;
        };
        let Some(rail) = code.rail else {
            return false;
        };
        let hovered = rail.contains(&e.position);
        if code.hovered != hovered {
            code.hovered = hovered;
            cx.notify();
        }
        let marker = code
            .markers
            .iter()
            .rposition(|m| m.bounds.contains(&e.position) && !m.message.is_empty());
        if code.hovered_marker != marker {
            code.hovered_marker = marker;
            cx.notify();
        }
        if let Some(offset) = code.drag_offset {
            if e.pressed_button != Some(MouseButton::Left) {
                code.drag_offset = None;
                return false;
            }
            let metrics = Metrics::new(
                self.scroll_y.as_f32(),
                code.total(),
                rail.size.height.as_f32(),
                rail.size.height.as_f32(),
            );
            self.scroll_y =
                px(metrics.scroll_from_pointer((e.position.y - rail.top()).as_f32(), offset));
            code.reveal = false;
            cx.notify();
            return true;
        }
        false
    }
    pub(super) fn code_max_scroll_y(&self) -> Pixels {
        let height = self.bounds.map_or(px(0.), |b| b.size.height);
        self.code
            .as_ref()
            .map_or(px(0.), |code| (px(code.total()) - height).max(px(0.)))
    }
    pub(super) fn code_scrollbar_up(&mut self) {
        if let Some(code) = &mut self.code {
            code.drag_offset = None;
        }
    }
    pub(super) fn code_hover(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let (Some(code), Some(bounds)) = (&mut self.code, self.bounds) else {
            return;
        };
        if !code.diff {
            return;
        }
        let y = (e.position.y - bounds.top() + self.scroll_y).as_f32();
        let row = code.row_at_y(y);
        let hovered = if e.position.x >= bounds.left() - px(code.gutter())
            && e.position.x <= bounds.right()
            && e.position.y >= bounds.top()
            && e.position.y <= bounds.bottom()
        {
            code.analysis
                .changes
                .iter()
                .position(|c| code.row(c.from) <= row && code.row(c.to) >= row)
        } else {
            None
        };
        if hovered != code.hovered_change {
            code.hovered_change = hovered;
            cx.notify();
        }
    }
    pub(super) fn hunk_actions(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let code = self.code.as_ref()?;
        let index = code.hovered_change?;
        let change = code.analysis.changes.get(index)?;
        let row = code.row(change.from);
        let top = (code.top(row, self.line_height) - self.scroll_y.as_f32()).max(0.);
        Some(
            div()
                .id("editor-hunk-actions")
                .absolute()
                .left(px(code.gutter() - 18.))
                .top(px(top))
                .w(px(18.))
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(px(4.))
                .border_1()
                .border_color(crate::theme::content(0.14))
                .bg(crate::theme::shell_fill().opacity(0.92))
                .shadow_md()
                .when(code.can_stage, |d| {
                    d.child(
                        crate::right_panel::icon_action(
                            "stage-change",
                            "Stage change",
                            "plus",
                            true,
                        )
                        .size(px(18.))
                        .on_click(
                            cx.listener(move |_, _, _, cx| cx.emit(CodeAction::Stage(index))),
                        ),
                    )
                })
                .child(
                    crate::right_panel::icon_action(
                        "revert-change",
                        "Revert change",
                        "rotate-ccw",
                        true,
                    )
                    .size(px(18.))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(CodeAction::Revert(index)))),
                )
                .into_any_element(),
        )
    }
    pub(super) fn code_tooltip(&self) -> Option<AnyElement> {
        let code = self.code.as_ref()?;
        let marker = code.markers.get(code.hovered_marker?)?;
        let rail = code.rail?;
        Some(
            div()
                .absolute()
                .right(px(WIDTH + 6.))
                .top(
                    (marker.bounds.top() - rail.top())
                        .min((rail.size.height - px(32.)).max(px(0.))),
                )
                .max_w(px(360.))
                .px_2()
                .py_1()
                .rounded_md()
                .bg(crate::theme::pane_fill())
                .border_1()
                .border_color(crate::theme::content(0.14))
                .text_size(px(12.))
                .text_color(crate::theme::content(1.))
                .child(marker.message.clone())
                .into_any_element(),
        )
    }
}

// GPUI resolves an index at a soft boundary to the end of the previous row.
// Keep the clicked/navigation side of that boundary for the caret and IME.
fn line_position(line: &WrappedLine, at: usize, lh: Pixels, upstream: bool) -> Point<Pixels> {
    if !upstream {
        for (row, boundary) in line.wrap_boundaries().iter().enumerate() {
            let glyph = &line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix];
            if glyph.index == at {
                return point(px(0.), lh * (row + 1));
            }
        }
    }
    line.position_for_index(at, lh).unwrap_or_default()
}

fn paint_rail(
    i: &Input,
    rail: Bounds<Pixels>,
    scroll: Pixels,
    w: &mut Window,
    cx: &App,
) -> Vec<MarkerHit> {
    let code = i.code.as_ref().unwrap();
    let height = rail.size.height.as_f32();
    let total = code.total().max(1.);
    let metrics = Metrics::new(scroll.as_f32(), total, height, height);
    let mut markers = Vec::new();
    w.with_content_mask(Some(ContentMask { bounds: rail }), |w| {
        w.paint_quad(fill(
            Bounds::new(rail.origin, size(px(1.), rail.size.height)),
            crate::theme::content(0.07),
        ));
        if metrics.scrollable {
            w.paint_quad(
                fill(
                    Bounds::new(
                        rail.origin + point(px(3.), px(metrics.thumb_top)),
                        size(px(WIDTH - 6.), px(metrics.thumb_height)),
                    ),
                    crate::theme::content(if code.hovered || code.drag_offset.is_some() {
                        0.38 * 0.88
                    } else {
                        0.30 * 0.72
                    }),
                )
                .corner_radii(px(2.)),
            );
        }
        // Exactly one diagnostic per logical line, strongest severity wins.
        let mut diagnostics = std::collections::BTreeMap::new();
        for diagnostic in &code.analysis.diagnostics {
            let row = code.row(diagnostic.from);
            let current = diagnostics.entry(row).or_insert(diagnostic);
            if diagnostic.priority() > current.priority() {
                *current = diagnostic;
            }
        }
        for (row, diagnostic) in diagnostics {
            let bounds = Bounds::new(
                rail.origin
                    + point(
                        px(3.),
                        px(row as f32 / code.offsets.len().max(1) as f32 * height),
                    ),
                size(px(WIDTH - 5.), px(3.)),
            );
            let color = match diagnostic.severity.as_str() {
                "error" => 0xf87171,
                "warning" => 0xfbbf24,
                "info" => 0x60a5fa,
                _ => 0x969696,
            };
            let ink = if diagnostic.severity == "hint" {
                crate::theme::content(0.48)
            } else {
                rgb(color).into()
            };
            w.paint_quad(fill(bounds, ink).corner_radii(px(1.)));
            markers.push(MarkerHit {
                bounds,
                pos: diagnostic.from,
                diagnostic: true,
                message: diagnostic.message.clone(),
            });
        }
        let cursor_y = code.row(i.cursor()) as f32 / code.offsets.len().max(1) as f32 * height;
        let mut cursor_color = crate::theme::accent_color();
        cursor_color.a *= if i.focus.is_focused(w) { 1. } else { 0.6 };
        w.paint_quad(fill(
            Bounds::new(
                rail.origin + point(px(0.), px(cursor_y)),
                size(rail.size.width, px(2.)),
            ),
            cursor_color,
        ));
        let (add, del) = crate::editor_scrollbar::diff_colors(cx);
        // Git's layer is above the cursor/diagnostic layer in editorGit.ts.
        for change in &code.analysis.changes {
            let row = code.row(change.from);
            let top = code.top(row, i.line_height);
            let bottom = if change.kind == "del" {
                top + change.deleted.max(1) as f32 * i.line_height
            } else {
                code.top(code.row(change.to.saturating_sub(1)) + 1, i.line_height)
            };
            let bounds = Bounds::new(
                rail.origin + point(px(3.), px(top / total * height)),
                size(
                    px(WIDTH - 5.),
                    px(((bottom - top).max(0.) / total * height).max(3.)),
                ),
            );
            if change.kind == "mod" {
                let half = bounds.size.width / 2.;
                w.paint_quad(
                    fill(
                        Bounds::new(bounds.origin, size(half, bounds.size.height)),
                        rgb(del),
                    )
                    .corner_radii(px(1.)),
                );
                w.paint_quad(
                    fill(
                        Bounds::new(
                            bounds.origin + point(half, px(0.)),
                            size(half, bounds.size.height),
                        ),
                        rgb(add),
                    )
                    .corner_radii(px(1.)),
                );
            } else {
                w.paint_quad(
                    fill(bounds, rgb(if change.kind == "del" { del } else { add }))
                        .corner_radii(px(1.)),
                );
            }
            markers.push(MarkerHit {
                bounds,
                pos: change.from,
                diagnostic: false,
                message: String::new(),
            });
        }
    });
    markers
}

pub(super) struct CodeElement {
    pub input: Entity<Input>,
}
pub(super) struct CodePaint {
    lines: Vec<(usize, WrappedLine)>,
    deleted: Vec<(f32, WrappedLine, usize)>,
    scroll: Point<Pixels>,
    text_bounds: Bounds<Pixels>,
}
impl IntoElement for CodeElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
fn plain_line(
    text: &str,
    width: Option<Pixels>,
    font_size: f32,
    w: &mut Window,
    color: Hsla,
) -> WrappedLine {
    w.text_system()
        .shape_text(
            text.to_owned().into(),
            px(font_size),
            &[TextRun {
                len: text.len(),
                font: gpui::font(crate::fonts::mono()),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            width,
            None,
        )
        .unwrap()
        .remove(0)
}
fn token_line(
    text: &str,
    tokens: &[crate::editor_analysis::Token],
    width: Option<Pixels>,
    font_size: f32,
    w: &mut Window,
) -> WrappedLine {
    let base = TextRun {
        len: 0,
        font: gpui::font(crate::fonts::mono()),
        color: crate::theme::content(1.),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs = Vec::new();
    let mut at = 0;
    for token in tokens {
        if token.from > at {
            runs.push(TextRun {
                len: token.from - at,
                ..base.clone()
            });
        }
        runs.push(TextRun {
            len: token.to - token.from,
            color: crate::theme::syntax_color(&token.kind),
            ..base.clone()
        });
        at = token.to;
    }
    if at < text.len() || runs.is_empty() {
        runs.push(TextRun {
            len: text.len() - at,
            ..base
        });
    }
    w.text_system()
        .shape_text(text.to_owned().into(), px(font_size), &runs, width, None)
        .unwrap()
        .remove(0)
}
fn shape(i: &Input, row: usize, width: Option<Pixels>, w: &mut Window) -> WrappedLine {
    let code = i.code.as_ref().unwrap();
    let start = code.offsets[row];
    let end = code.source_end(row, i.content.len());
    let text: SharedString = if let Some(fold) = code.fold_at(row) {
        format!(
            "{}…{}",
            &i.content[start..fold.from],
            &i.content[fold.to..end]
        )
        .into()
    } else {
        i.content[start..end].to_string().into()
    };
    let base = TextRun {
        len: text.len(),
        font: gpui::font(crate::fonts::mono()),
        color: crate::theme::content(1.),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs = Vec::new();
    let tokens = &code.analysis.tokens;
    let first = tokens.partition_point(|t| t.to <= start);
    let mut spans = Vec::new();
    for token in tokens.iter().skip(first).take_while(|t| t.from < end) {
        let from = token.from.max(start);
        let to = token.to.min(end);
        let color = crate::theme::syntax_color(&token.kind);
        if let Some(fold) = code.fold_at(row) {
            if from < fold.from {
                spans.push((from - start, to.min(fold.from) - start, color));
            }
            if to > fold.to {
                spans.push((
                    code.display_offset(row, from.max(fold.to)),
                    code.display_offset(row, to),
                    color,
                ));
            }
        } else {
            spans.push((from - start, to - start, color));
        }
    }
    if let Some(fold) = code.fold_at(row) {
        spans.push((
            fold.from - start,
            fold.from - start + "…".len(),
            crate::theme::content(0.4),
        ));
    }
    spans.sort_by_key(|s| s.0);
    let mut at = 0;
    for (from, to, color) in spans {
        if from > at {
            runs.push(TextRun {
                len: from - at,
                ..base.clone()
            });
        }
        if to > from {
            runs.push(TextRun {
                len: to - from,
                color,
                ..base.clone()
            });
        }
        at = to;
    }
    if at < text.len() || runs.is_empty() {
        runs.push(TextRun {
            len: text.len() - at,
            ..base
        });
    }
    if let Some(marked) = &i.marked {
        let marked = marked.start.saturating_sub(start).min(text.len())
            ..marked.end.saturating_sub(start).min(text.len());
        if !marked.is_empty() {
            let mut at = 0;
            let mut decorated = Vec::new();
            for run in runs {
                let end = at + run.len;
                let a = marked.start.clamp(at, end);
                let b = marked.end.clamp(at, end);
                if at < a {
                    decorated.push(TextRun {
                        len: a - at,
                        ..run.clone()
                    });
                }
                if a < b {
                    decorated.push(TextRun {
                        len: b - a,
                        underline: Some(UnderlineStyle {
                            color: Some(run.color),
                            thickness: px(1.),
                            wavy: false,
                        }),
                        ..run.clone()
                    });
                }
                if b < end {
                    decorated.push(TextRun {
                        len: end - b,
                        ..run
                    });
                }
                at = end;
            }
            runs = decorated;
        }
    }
    for diagnostic in code
        .analysis
        .diagnostics
        .iter()
        .filter(|_| code.fold_at(row).is_none())
    {
        let range = diagnostic.range();
        if range.end <= start || range.start >= end {
            continue;
        }
        let from = range.start.saturating_sub(start).min(text.len());
        let to = range.end.saturating_sub(start).min(text.len());
        let mut at = 0;
        let mut decorated = Vec::new();
        for run in runs {
            let end = at + run.len;
            let a = from.clamp(at, end);
            let b = to.clamp(at, end);
            if at < a {
                decorated.push(TextRun {
                    len: a - at,
                    ..run.clone()
                });
            }
            if a < b {
                decorated.push(TextRun {
                    len: b - a,
                    underline: Some(UnderlineStyle {
                        color: Some(rgb(0xf87171).into()),
                        thickness: px(1.),
                        wavy: true,
                    }),
                    ..run.clone()
                });
            }
            if b < end {
                decorated.push(TextRun {
                    len: end - b,
                    ..run
                });
            }
            at = end;
        }
        runs = decorated;
    }
    w.text_system()
        .shape_text(text, px(i.font_size), &runs, width, None)
        .unwrap()
        .remove(0)
}
impl Element for CodeElement {
    type RequestLayoutState = ();
    type PrepaintState = CodePaint;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        w: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (w.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        w: &mut Window,
        cx: &mut App,
    ) -> CodePaint {
        // Measure wrapped block heights only when the document or pane width
        // changes. Short lines use a conservative UTF-8 byte-width bound;
        // offscreen lines do not retain glyph runs.
        self.input.update(cx, |i, _| {
            let code = i.code.as_ref().unwrap();
            let gutter = px(code.gutter());
            let width = (bounds.size.width - gutter - px(WIDTH + 12.)).max(px(1.));
            if code.layout_width != Some(width) {
                let mut tops = Vec::with_capacity(code.offsets.len() + 1);
                let mut y = TOP_PADDING;
                let mut deleted_rows = Vec::new();
                let mut deletion_changes = code
                    .analysis
                    .changes
                    .iter()
                    .filter(|c| c.kind != "add")
                    .peekable();
                let sample = w.text_system().shape_line(
                    "M".into(),
                    px(i.font_size),
                    &[TextRun {
                        len: 1,
                        font: gpui::font(crate::fonts::mono()),
                        color: crate::theme::content(1.),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                );
                for row in 0..code.offsets.len() {
                    while deletion_changes
                        .peek()
                        .is_some_and(|c| code.row(c.from) < row)
                    {
                        deletion_changes.next();
                    }
                    if !code.hidden(row) && code.diff {
                        while deletion_changes
                            .peek()
                            .is_some_and(|c| code.row(c.from) == row)
                        {
                            let change = deletion_changes.next().unwrap();
                            let mut offset = 0;
                            for (n, text) in change
                                .original
                                .strip_suffix('\n')
                                .unwrap_or(&change.original)
                                .split('\n')
                                .enumerate()
                            {
                                let tokens: Vec<_> = change
                                    .tokens
                                    .iter()
                                    .filter(|t| t.to > offset && t.from < offset + text.len())
                                    .map(|t| crate::editor_analysis::Token {
                                        from: t.from.saturating_sub(offset),
                                        to: t.to.min(offset + text.len()) - offset,
                                        kind: t.kind.clone(),
                                    })
                                    .collect();
                                offset += text.len() + 1;
                                let line = token_line(
                                    text,
                                    &tokens,
                                    code.wrap.then_some(width),
                                    i.font_size,
                                    w,
                                );
                                let height = line.size(i.line_height()).height.as_f32();
                                deleted_rows.push((
                                    y,
                                    height,
                                    text.to_owned(),
                                    change.old_line + n,
                                    tokens,
                                ));
                                y += height;
                            }
                        }
                    }
                    tops.push(y);
                    if code.hidden(row) {
                        continue;
                    }
                    let start = code.offsets[row];
                    let end = code.offsets.get(row + 1).map_or(i.content.len(), |n| n - 1);
                    let text = &i.content[start..end];
                    let short = !text.contains('\t') && sample.width * text.len() <= width;
                    y += if !code.wrap || short {
                        i.line_height
                    } else {
                        shape(i, row, Some(width), w)
                            .size(i.line_height())
                            .height
                            .as_f32()
                    };
                }
                tops.push(y);
                let code = i.code.as_mut().unwrap();
                code.tops = tops;
                code.deleted_rows = deleted_rows;
                code.layout_width = Some(width);
            }
        });
        let i = self.input.read(cx);
        let code = i.code.as_ref().unwrap();
        let gutter = px(code.gutter());
        let text_bounds = Bounds::new(
            bounds.origin + point(gutter, px(0.)),
            size(
                (bounds.size.width - gutter - px(WIDTH + 12.)).max(px(1.)),
                bounds.size.height,
            ),
        );
        let wrap = code.wrap.then_some(text_bounds.size.width);
        let mut scroll = point(i.scroll_x, i.scroll_y);
        if code.reveal {
            let row = code.display_row(i.cursor());
            let caret_line = shape(i, row, wrap, w);
            let p = line_position(
                &caret_line,
                code.display_offset(row, i.cursor()),
                i.line_height(),
                code.affinity.get() == Some((i.cursor(), true)),
            );
            let y = px(code.top(row, i.line_height)) + p.y;
            if y < scroll.y {
                scroll.y = y;
            }
            if y + i.line_height() > scroll.y + bounds.size.height {
                scroll.y = y + i.line_height() - bounds.size.height;
            }
            if p.x < scroll.x {
                scroll.x = p.x;
            }
            if p.x + px(8.) > scroll.x + text_bounds.size.width {
                scroll.x = p.x + px(8.) - text_bounds.size.width;
            }
        }
        if code.wrap {
            scroll.x = px(0.);
        }
        scroll.y = scroll
            .y
            .max(px(0.))
            .min((px(code.total()) - bounds.size.height).max(px(0.)));
        let start = code.row_at_y(scroll.y.as_f32());
        let end =
            (code.row_at_y((scroll.y + bounds.size.height).as_f32()) + 2).min(code.offsets.len());
        let lines = (start..end)
            .filter(|row| !code.hidden(*row))
            .map(|row| (row, shape(i, row, wrap, w)))
            .collect();
        let deleted = code
            .deleted_rows
            .iter()
            .filter(|(top, height, _, _, _)| {
                *top + *height >= scroll.y.as_f32()
                    && *top < (scroll.y + bounds.size.height).as_f32()
            })
            .map(|(top, _, text, number, tokens)| {
                (
                    *top,
                    token_line(text, tokens, wrap, i.font_size, w),
                    *number,
                )
            })
            .collect();
        CodePaint {
            lines,
            deleted,
            scroll,
            text_bounds,
        }
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        state: &mut CodePaint,
        w: &mut Window,
        cx: &mut App,
    ) {
        paint_drag_listener(&self.input, w, cx);
        let i = self.input.read(cx);
        let code = i.code.as_ref().unwrap();
        let lh = i.line_height();
        let cursor = i.cursor();
        let selected = i.selection.clone();
        let focused = i.focus.is_focused(w);
        let line_height = i.line_height;
        let code_affinity = code.affinity.get();
        let starts: Vec<_> = state
            .lines
            .iter()
            .map(|(row, _)| code.offsets[*row])
            .collect();
        let tops: Vec<_> = state
            .lines
            .iter()
            .map(|(row, _)| code.top(*row, line_height))
            .collect();
        let rail = Bounds::new(
            point(bounds.right() - px(WIDTH), bounds.top()),
            size(px(WIDTH), bounds.size.height),
        );
        let markers = paint_rail(i, rail, state.scroll.y, w, cx);
        let content_len = i.content.len();
        let mut snapshot = Code::new();
        snapshot.offsets = code.offsets.clone();
        snapshot.tops = code.tops.clone();
        snapshot.folded = code.folded.clone();
        snapshot.analysis = code.analysis.clone();
        snapshot.search_matches = code.search_matches.clone();
        snapshot.diff = code.diff;
        let code = &snapshot;
        w.handle_input(
            &i.focus,
            ElementInputHandler::new(state.text_bounds, self.input.clone()),
            cx,
        );
        w.with_content_mask(Some(ContentMask { bounds }), |w| {
            if code.diff {
                for (top, line, number) in &state.deleted {
                    let y = bounds.top() + px(*top) - state.scroll.y;
                    w.paint_quad(fill(
                        Bounds::new(
                            point(bounds.left(), y),
                            size(bounds.size.width - px(WIDTH), line.size(lh).height),
                        ),
                        crate::theme::diff_background(false, false),
                    ));
                    w.paint_quad(fill(
                        Bounds::new(
                            point(bounds.left(), y),
                            size(px(code.gutter() - 22.), line.size(lh).height),
                        ),
                        crate::theme::diff_background(false, true),
                    ));
                    let number = plain_line(
                        &number.to_string(),
                        None,
                        11.,
                        w,
                        crate::theme::diff_foreground(false),
                    );
                    let _ = number.paint(
                        point(state.text_bounds.left() - px(26.) - number.width(), y),
                        lh,
                        TextAlign::Left,
                        None,
                        w,
                        cx,
                    );
                    let minus = plain_line("−", None, 12., w, crate::theme::diff_foreground(false));
                    let _ = minus.paint(
                        point(state.text_bounds.left() - px(12.), y),
                        lh,
                        TextAlign::Left,
                        None,
                        w,
                        cx,
                    );
                }
                for ((row, line), top) in state.lines.iter().zip(&tops) {
                    let start = code.offsets[*row];
                    if code
                        .analysis
                        .changes
                        .iter()
                        .any(|c| c.kind != "del" && start >= c.from && start <= c.to)
                    {
                        let y = bounds.top() + px(*top) - state.scroll.y;
                        w.paint_quad(fill(
                            Bounds::new(
                                point(bounds.left(), y),
                                size(bounds.size.width - px(WIDTH), line.size(lh).height),
                            ),
                            crate::theme::diff_background(true, false),
                        ));
                        let plus =
                            plain_line("+", None, 12., w, crate::theme::diff_foreground(true));
                        let _ = plus.paint(
                            point(state.text_bounds.left() - px(12.), y),
                            lh,
                            TextAlign::Left,
                            None,
                            w,
                            cx,
                        );
                    }
                }
            }
            for ((row, shaped), top) in state.lines.iter().zip(&tops) {
                if code.analysis.folds.iter().any(|f| code.row(f.from) == *row) {
                    let arrow: SharedString = if code.fold_at(*row).is_some() {
                        "›"
                    } else {
                        "⌄"
                    }
                    .into();
                    let arrow_line = w.text_system().shape_line(
                        arrow,
                        px(12.),
                        &[TextRun {
                            len: if code.fold_at(*row).is_some() {
                                "›".len()
                            } else {
                                "⌄".len()
                            },
                            font: gpui::font(crate::fonts::mono()),
                            color: crate::theme::content(0.45),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    );
                    let _ = arrow_line.paint(
                        point(
                            state.text_bounds.left() - px(12.),
                            bounds.top() + px(*top) - state.scroll.y,
                        ),
                        lh,
                        TextAlign::Left,
                        None,
                        w,
                        cx,
                    );
                }
                let _ = shaped;
                let number: SharedString = (row + 1).to_string().into();
                let len = number.len();
                let line = w.text_system().shape_line(
                    number,
                    px(11.),
                    &[TextRun {
                        len,
                        font: gpui::font(crate::fonts::mono()),
                        color: crate::theme::content(0.3),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                );
                let _ = line.paint(
                    point(
                        state.text_bounds.left() - px(26.) - line.width,
                        bounds.top() + px(*top) - state.scroll.y,
                    ),
                    lh,
                    TextAlign::Left,
                    None,
                    w,
                    cx,
                );
            }
        });
        w.with_content_mask(
            Some(ContentMask {
                bounds: state.text_bounds,
            }),
            |w| {
                for (top, line, _) in &state.deleted {
                    let _ = line.paint(
                        point(
                            state.text_bounds.left() - state.scroll.x,
                            bounds.top() + px(*top) - state.scroll.y,
                        ),
                        lh,
                        TextAlign::Left,
                        None,
                        w,
                        cx,
                    );
                }
                for (((row, line), &start), top) in state.lines.iter().zip(&starts).zip(&tops) {
                    let y = bounds.top() + px(*top) - state.scroll.y;
                    let x = state.text_bounds.left() - state.scroll.x;
                    if cursor >= start && cursor <= code.source_end(*row, content_len) {
                        w.paint_quad(fill(
                            Bounds::new(
                                point(state.text_bounds.left(), y),
                                size(state.text_bounds.size.width, line.size(lh).height),
                            ),
                            crate::theme::content(0.1),
                        ));
                    }
                    for range in &code.search_matches {
                        let a = code.display_offset(*row, range.start).min(line.len());
                        let b = code.display_offset(*row, range.end).min(line.len());
                        if a < b {
                            let a = line_position(line, a, lh, false);
                            let b = line.position_for_index(b, lh).unwrap_or_default();
                            if a.y == b.y {
                                w.paint_quad(fill(
                                    Bounds::new(
                                        point(x + a.x, y + a.y),
                                        size((b.x - a.x).max(px(2.)), lh),
                                    ),
                                    rgb(0xe2c08d).opacity(0.46),
                                ));
                            }
                        }
                    }
                    let a = code.display_offset(*row, selected.start).min(line.len());
                    let b = code.display_offset(*row, selected.end).min(line.len());
                    if a < b
                        || (selected.start <= start + line.len()
                            && selected.end > start + line.len())
                    {
                        let a = line_position(line, a, lh, false);
                        let b = line.position_for_index(b, lh).unwrap_or_default();
                        let mut sy = a.y;
                        while sy <= b.y {
                            let left = if sy == a.y { a.x } else { px(0.) };
                            let right = if sy == b.y && selected.end <= start + line.len() {
                                b.x
                            } else {
                                state.text_bounds.size.width
                            };
                            w.paint_quad(fill(
                                Bounds::new(
                                    point(x + left, y + sy),
                                    size((right - left).max(px(2.)), lh),
                                ),
                                crate::theme::text_selection(),
                            ));
                            sy += lh;
                        }
                    }
                    let _ = line.paint(point(x, y), lh, TextAlign::Left, None, w, cx);
                    if focused
                        && selected.is_empty()
                        && cursor >= start
                        && cursor <= code.source_end(*row, content_len)
                    {
                        let p = line_position(
                            line,
                            code.display_offset(*row, cursor),
                            lh,
                            code_affinity == Some((cursor, true)),
                        );
                        w.paint_quad(fill(
                            Bounds::new(point(x + p.x, y + p.y), size(px(1.5), lh)),
                            crate::theme::content(1.),
                        ));
                    }
                }
            },
        );
        self.input.update(cx, |i, _| {
            i.bounds = Some(state.text_bounds);
            i.scroll_x = state.scroll.x;
            i.scroll_y = state.scroll.y;
            let code = i.code.as_mut().unwrap();
            code.reveal = false;
            code.rail = Some(rail);
            code.markers = markers;
            for (_, line) in &state.lines {
                code.max_width = code
                    .max_width
                    .max(line.width() - state.text_bounds.size.width + px(20.));
            }
            code.visible = std::mem::take(&mut state.lines);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowBounds, WindowOptions};
    #[gpui::test]
    fn wrapped_editor_keeps_unicode_selection_visual_navigation_and_resize_in_sync(
        cx: &mut TestAppContext,
    ) {
        let window = cx.update(|cx| {
            bind(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(400.), px(240.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(Input::editor),
            )
            .unwrap()
        });
        let text = format!("{}🙂漢字\nsecond line\n", "word ".repeat(30));
        window
            .update(cx, |i, w, cx| {
                w.focus(&i.focus, cx);
                i.set_text(text.clone(), cx);
                i.move_to(0, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        let height = window
            .update(cx, |i, _, _| {
                let code = i.code.as_ref().unwrap();
                assert!(code.tops[1] - code.tops[0] > i.line_height * 2.);
                for (at, _) in text
                    .char_indices()
                    .take_while(|(at, _)| *at < text.find('\n').unwrap())
                {
                    assert_eq!(i.code_index(i.code_position(at)), at, "UTF-8 offset {at}");
                }
                code.total()
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "down");
        window
            .update(cx, |i, _, _| {
                assert_eq!(
                    i.cursor_location().0,
                    1,
                    "Down moves within a wrapped logical line"
                );
                assert!(i.cursor() > 0);
                assert_eq!(
                    i.code_position(i.cursor()).y,
                    px(TOP_PADDING + i.line_height)
                );
            })
            .unwrap();
        cx.simulate_window_resize(window.into(), size(px(260.), px(240.)));
        cx.refresh().unwrap();
        window
            .update(cx, |i, _, cx| {
                assert!(i.code.as_ref().unwrap().total() > height);
                i.set_word_wrap(false, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |i, _, _| {
                assert_eq!(i.content.as_ref(), text);
                assert_eq!(
                    i.code.as_ref().unwrap().tops[1] - TOP_PADDING,
                    i.line_height
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn overview_markers_jump_and_the_thumb_drags_without_editing_or_selecting(
        cx: &mut TestAppContext,
    ) {
        use crate::editor_analysis::{Change, Diagnostic};
        let window = cx.update(|cx| {
            bind(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(400.), px(240.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(Input::editor),
            )
            .unwrap()
        });
        let text: String = (0..200).map(|n| format!("line {n}\n")).collect();
        let diagnostic = text.find("line 150").unwrap();
        let change = text.find("line 80").unwrap();
        window
            .update(cx, |i, w, cx| {
                w.focus(&i.focus, cx);
                i.set_text(text.clone(), cx);
                i.set_editor_analysis(
                    Analysis {
                        diagnostics: vec![Diagnostic {
                            from: diagnostic,
                            to: diagnostic + 4,
                            severity: "error".into(),
                            message: "Unexpected token".into(),
                        }],
                        changes: vec![Change {
                            kind: "mod".into(),
                            from: change,
                            to: change + 7,
                            deleted: 1,
                            original: "old line\n".into(),
                            old_line: 80,
                            tokens: Vec::new(),
                        }],
                        folds: Vec::new(),
                        tokens: Vec::new(),
                    },
                    cx,
                );
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let (rail, error_tick, change_tick) = window
            .update(visual, |i, _, _| {
                let code = i.code.as_ref().unwrap();
                (
                    code.rail.unwrap(),
                    code.markers[0].bounds.center(),
                    code.markers[1].bounds.center(),
                )
            })
            .unwrap();
        assert_eq!(rail.size.width, px(18.));
        visual.simulate_click(error_tick, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |i, _, _| {
                assert_eq!(i.cursor(), diagnostic);
                assert!(i.scroll_y > px(2000.));
                assert!(!i.selecting);
            })
            .unwrap();
        visual.simulate_click(change_tick, Default::default());
        visual.run_until_parked();
        let thumb = window
            .update(visual, |i, _, _| {
                assert_eq!(i.cursor(), diagnostic, "Git navigation preserves the caret");
                assert!(i.scroll_y < px(2000.));
                let code = i.code.as_ref().unwrap();
                let m = Metrics::new(
                    i.scroll_y.as_f32(),
                    code.total(),
                    rail.size.height.as_f32(),
                    rail.size.height.as_f32(),
                );
                // Grab the thumb away from the change tick, which deliberately
                // takes priority over dragging in the frontend.
                rail.origin + point(px(8.), px(m.thumb_top + 4.))
            })
            .unwrap();
        visual.simulate_mouse_down(thumb, MouseButton::Left, Default::default());
        let end = point(thumb.x, rail.bottom() - px(1.));
        visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |i, _, _| {
                assert_eq!(i.content.as_ref(), text);
                assert_eq!(i.cursor(), diagnostic);
                assert!(i.scroll_y > px(3500.), "drag ended at {:?}", i.scroll_y);
                assert!(i.code.as_ref().unwrap().drag_offset.is_none());
                assert!(!i.selecting);
            })
            .unwrap();
    }
    #[gpui::test]
    fn drag_selection_keeps_scrolling_past_the_viewport_edge(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            bind(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(400.), px(240.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(Input::editor),
            )
            .unwrap()
        });
        window
            .update(cx, |input, w, cx| {
                w.focus(&input.focus, cx);
                input.set_text((0..1_000).map(|n| format!("line {n}\n")).collect(), cx);
                input.move_to(0, cx);
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let bounds = window.update(visual, |i, _, _| i.bounds.unwrap()).unwrap();
        let start = bounds.origin + point(px(4.), px(10.));
        visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
        let anchor = window.update(visual, |i, _, _| i.cursor()).unwrap();
        // Leave the editor and hold still: the selection must keep growing.
        let below = point(start.x, bounds.bottom() + px(40.));
        visual.simulate_mouse_move(below, MouseButton::Left, Default::default());
        visual
            .executor()
            .advance_clock(std::time::Duration::from_millis(500));
        visual.run_until_parked();
        let (scrolled, row) = window
            .update(visual, |i, _, _| {
                assert!(i.autoscroll.is_some());
                (i.scroll_y, i.code.as_ref().unwrap().row(i.selection.end))
            })
            .unwrap();
        assert!(scrolled > bounds.size.height, "scrolled {scrolled:?}");
        assert!(row > 20, "selected to row {row}");
        visual.simulate_mouse_up(below, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |i, _, _| {
                assert!(!i.selecting);
                assert!(i.autoscroll.is_none());
                assert_eq!(i.selection.start, anchor);
                assert_eq!(i.scroll_y, scrolled);
            })
            .unwrap();
    }
    #[gpui::test]
    fn editor_keyboard_undo_ime_and_viewport_are_native_and_bounded(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            bind(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(400.), px(240.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(Input::editor),
            )
            .unwrap()
        });
        window
            .update(cx, |input, w, cx| {
                w.focus(&input.focus, cx);
                input.set_text((0..10_000).map(|n| format!("line {n}: 🙂\n")).collect(), cx);
                input.move_to(0, cx);
                input.reset_editor_history();
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |input, _, _| {
                assert!(input.code.as_ref().unwrap().visible.len() < 20);
                assert_eq!(input.cursor_location(), (1, 1));
            })
            .unwrap();
        let m = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(window.into(), "tab enter");
        window
            .update(cx, |i, _, _| assert!(i.content.starts_with("  \n  line 0")))
            .unwrap();
        cx.simulate_keystrokes(window.into(), &format!("{m}-z {m}-z"));
        window
            .update(cx, |i, _, _| assert!(i.content.starts_with("line 0")))
            .unwrap();
        cx.simulate_keystrokes(window.into(), &format!("{m}-shift-z"));
        window
            .update(cx, |i, w, cx| {
                assert!(i.content.starts_with("  line 0"));
                i.replace_and_mark_text_in_range(None, "漢", None, w, cx);
                i.replace_text_in_range(None, "漢字", w, cx);
                assert!(i.content.starts_with("  漢字line"));
                i.undo(&Undo, w, cx);
                assert!(i.content.starts_with("  line"));
                i.move_to(i.content.len(), cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |i, _, cx| {
                assert!(i.scroll_y > px(100_000.));
                let code = i.code.as_mut().unwrap();
                assert!(code.visible.len() < 20);
                i.scroll_y = px(0.);
                code.reveal = false;
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |i, w, cx| {
                assert_eq!(
                    i.scroll_y,
                    px(0.),
                    "scrolling away must not snap back to the caret"
                );
                i.move_to(0, cx);
                let before = i.content.clone();
                i.replace_text_in_range(None, &"x".repeat(document_limit()), w, cx);
                assert_eq!(
                    i.content, before,
                    "oversized edits must be rejected in full"
                );
            })
            .unwrap();
    }
    fn document_limit() -> usize {
        monocode_core::files::document::MAX_LINE_BYTES + 1
    }
}

#[cfg(test)]
mod feature_tests {
    use super::*;
    use gpui::{TestAppContext, WindowBounds, WindowOptions};
    fn editor(cx: &mut TestAppContext) -> gpui::WindowHandle<Input> {
        cx.update(|cx| {
            bind(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(500.), px(300.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(Input::editor),
            )
            .unwrap()
        })
    }
    #[gpui::test]
    fn folds_keep_source_offsets_and_expand_when_search_reveals_hidden_text(
        cx: &mut TestAppContext,
    ) {
        let window = editor(cx);
        let source = "function hello() {\n  const emoji = '🙂';\n  return emoji;\n}\n";
        window
            .update(cx, |i, _, cx| {
                i.set_text(source.into(), cx);
                i.set_editor_analysis(
                    crate::editor_analysis::analyze("a.js", source, None).unwrap(),
                    cx,
                );
                i.toggle_fold_row(0, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |i, _, cx| {
                let code = i.code.as_ref().unwrap();
                let fold = code.folded[0].clone();
                assert_eq!(i.content.as_ref(), source);
                assert!(code.hidden(1) && code.hidden(2) && code.hidden(3));
                assert_eq!(i.next(fold.from), fold.to);
                let p = i.code_position(fold.to);
                assert_eq!(i.code_index(p), fold.to);
                i.replace_bytes(0..0, "// header\n", 10, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |i, _, cx| {
                assert_eq!(
                    i.code.as_ref().unwrap().folded.len(),
                    1,
                    "editing before a fold keeps it collapsed"
                );
                let at = i.content.find("emoji").unwrap();
                i.select_range(at..at + 5, cx);
                assert!(i.code.as_ref().unwrap().folded.is_empty());
            })
            .unwrap();
    }
    #[gpui::test]
    fn autocomplete_accepts_with_tab_and_deleted_diff_rows_remain_outside_the_document(
        cx: &mut TestAppContext,
    ) {
        let window = editor(cx);
        window
            .update(cx, |i, _, cx| {
                i.set_editor_path("a.ts".into());
                i.set_text("cons".into(), cx);
                i.schedule_completion(true, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |i, w, cx| {
                let code = i.code.as_mut().unwrap();
                code.completion_index = code
                    .completions
                    .iter()
                    .position(|c| c.label == "const")
                    .unwrap();
                i.indent(&Indent, w, cx);
                assert_eq!(i.content.as_ref(), "const");
                i.undo(&Undo, w, cx);
                assert_eq!(i.content.as_ref(), "cons");
                i.set_text("const value = 2;\n".into(), cx);
                i.set_editor_diff(true, true);
                let analysis =
                    crate::editor_analysis::analyze("a.ts", &i.content, Some("const value = 1;\n"))
                        .unwrap();
                i.set_editor_analysis(analysis, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |i, _, _| {
                let code = i.code.as_ref().unwrap();
                assert_eq!(code.deleted_rows.len(), 1);
                assert_eq!(code.deleted_rows[0].2, "const value = 1;");
                assert_eq!(code.tops[0], TOP_PADDING + i.line_height);
                assert_eq!(i.content.as_ref(), "const value = 2;\n");
            })
            .unwrap();
    }
}
