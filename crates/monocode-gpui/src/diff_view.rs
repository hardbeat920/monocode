//! UnifiedDiffView.tsx and unifiedDiffDocument.ts: every file's hunks in one
//! virtualized review, with sticky file headers, a fixed gutter beside code
//! lanes that scroll horizontally together, hunk navigation, per-file and
//! per-hunk staging, and line comments for the chat. Highlighting runs off
//! the UI thread, one file at a time, only for files that are on screen.
use crate::{
    file_icons::file_type_icon,
    input::{self, Input},
    popover::{AnchorSlot, Side, popover},
    right_panel::{DIFF_ADD, DIFF_DEL, format_integer, stroke, tabular},
    syntax::{HighlightRequest, HighlightSpan},
    theme::*,
};
use gpui::{
    AnyElement, App, Bounds, Context, EventEmitter, FocusHandle, Focusable, FontWeight,
    HighlightStyle, Hsla, KeyBinding, ListAlignment, ListOffset, ListState, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Role, ScrollWheelEvent, SharedString, StyledText,
    Subscription, Task, Window, actions, canvas, div, list, prelude::*, px, rgb,
};
use monocode_core::diff::{self, Block, Line, LineKind};
use std::{
    borrow::Cow,
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

pub(crate) const FILE_HEADER: f32 = 40.;
const LINE: f32 = 20.;
const HUNK: f32 = 28.;
const NOTICE: f32 = 44.;
/// `--code-font-size`.
const CODE_SIZE: f32 = 12.5;
const MAX_HIGHLIGHT_CHARS: usize = 250_000;

actions!(diff_review, [PreviousHunk, NextHunk]);

pub(crate) fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-up", PreviousHunk, Some("DiffReview")),
        KeyBinding::new("alt-down", NextHunk, Some("DiffReview")),
    ]);
}

/// UnifiedDiffFileModel.
#[derive(Clone)]
pub(crate) struct DiffFile {
    pub id: SharedString,
    pub path: String,
    pub label: String,
    pub binary: bool,
    pub too_large: bool,
    pub empty_message: Option<String>,
    pub additions: u32,
    pub deletions: u32,
    pub blocks: Arc<Vec<Block>>,
    pub can_stage: bool,
    pub can_discard: bool,
    pub can_stage_hunk: bool,
}
impl DiffFile {
    fn message(&self) -> Option<String> {
        if self.binary {
            Some("Binary file changed".into())
        } else if self.too_large {
            Some("Diff is too large to display".into())
        } else if let Some(message) = &self.empty_message {
            Some(message.clone())
        } else if self.blocks.is_empty() {
            Some("No textual diff".into())
        } else {
            None
        }
    }
    fn same_content(&self, other: &DiffFile) -> bool {
        Arc::ptr_eq(&self.blocks, &other.blocks)
            && self.label == other.label
            && self.binary == other.binary
            && self.too_large == other.too_large
            && self.empty_message == other.empty_message
            && self.additions == other.additions
            && self.deletions == other.deletions
            && (self.can_stage, self.can_discard, self.can_stage_hunk)
                == (other.can_stage, other.can_discard, other.can_stage_hunk)
    }
}

pub(crate) enum DiffEvent {
    StageFile(SharedString),
    DiscardFile(SharedString),
    StageHunk(SharedString, usize),
    /// Text for the active chat's composer.
    Comment(String),
}

#[derive(Clone, PartialEq)]
enum RowKind {
    File,
    Line {
        block: usize,
        line: usize,
        stage: Option<usize>,
    },
    Hunk(SharedString),
    Notice(SharedString),
}
#[derive(Clone)]
struct Row {
    file: usize,
    kind: RowKind,
    height: f32,
}
#[derive(Clone, PartialEq, Eq, Hash)]
struct RowKey(SharedString, u8, usize, usize);

struct Body {
    blocks: Arc<Vec<Block>>,
    message: Option<String>,
    stageable: bool,
    rows: Vec<Row>,
    columns: usize,
    digits: usize,
}
fn build_body(file: &DiffFile, message: Option<String>, stageable: bool) -> Body {
    let mut rows = Vec::new();
    let (mut columns, mut digits) = (40, 3);
    if let Some(text) = &message {
        rows.push(Row {
            file: 0,
            kind: RowKind::Notice(text.clone().into()),
            height: NOTICE,
        });
    } else {
        for (b, block) in file.blocks.iter().enumerate() {
            let Block::Hunk { lines, chunk } = block else {
                continue;
            };
            let changes = lines
                .iter()
                .any(|l| matches!(l.kind, LineKind::Add | LineKind::Del));
            if changes && !lines.iter().any(|l| l.kind == LineKind::Hunk) {
                rows.push(Row {
                    file: 0,
                    kind: RowKind::Hunk(hunk_range(lines).into()),
                    height: HUNK,
                });
            }
            for (i, line) in lines.iter().enumerate() {
                if line.kind == LineKind::Hunk {
                    rows.push(Row {
                        file: 0,
                        kind: RowKind::Hunk(line.text.clone().into()),
                        height: HUNK,
                    });
                    continue;
                }
                let stage = (stageable && matches!(line.kind, LineKind::Add | LineKind::Del))
                    .then(|| line.chunk.or(*chunk))
                    .flatten();
                rows.push(Row {
                    file: 0,
                    kind: RowKind::Line {
                        block: b,
                        line: i,
                        stage,
                    },
                    height: LINE,
                });
                columns = columns.max(diff::visual_columns(&line.text));
                digits = digits
                    .max(line.old_number.unwrap_or(0).to_string().len())
                    .max(line.new_number.unwrap_or(0).to_string().len());
            }
        }
    }
    Body {
        blocks: file.blocks.clone(),
        message,
        stageable,
        rows,
        columns,
        digits,
    }
}
fn hunk_range(lines: &[Line]) -> String {
    let old: Vec<u32> = lines.iter().filter_map(|l| l.old_number).collect();
    let new: Vec<u32> = lines.iter().filter_map(|l| l.new_number).collect();
    format!(
        "@@ -{},{} +{},{} @@",
        old.first().unwrap_or(&0),
        old.len(),
        new.first().unwrap_or(&0),
        new.len()
    )
}
fn expand_tabs(text: &str) -> Cow<'_, str> {
    if !text.contains('\t') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    let mut columns = 0;
    for ch in text.chars() {
        if ch == '\t' {
            let n = 4 - columns % 4;
            out.extend(std::iter::repeat_n(' ', n));
            columns += n;
        } else {
            out.push(ch);
            columns += 1;
        }
    }
    Cow::Owned(out)
}

type Tokens = Arc<Vec<Vec<Vec<HighlightSpan>>>>;

/// highlightDiffFile: the old and new sides as two documents, mapped back
/// to their lines. Indexed by block, then line.
fn highlight_file(path: &str, blocks: &[Block]) -> Option<Vec<Vec<Vec<HighlightSpan>>>> {
    let mut old: Vec<(usize, usize, String)> = Vec::new();
    let mut new: Vec<(usize, usize, String)> = Vec::new();
    let (mut old_chars, mut new_chars) = (0, 0);
    for (b, block) in blocks.iter().enumerate() {
        for (i, line) in block.lines().iter().enumerate() {
            if line.kind == LineKind::Hunk {
                continue;
            }
            let text = expand_tabs(&line.text).into_owned();
            if line.kind != LineKind::Add {
                old_chars += text.len() + 1;
                old.push((b, i, text.clone()));
            }
            if line.kind != LineKind::Del {
                new_chars += text.len() + 1;
                new.push((b, i, text));
            }
            if old_chars > MAX_HIGHLIGHT_CHARS || new_chars > MAX_HIGHLIGHT_CHARS {
                return None;
            }
        }
    }
    let mut out: Vec<Vec<Vec<HighlightSpan>>> = blocks
        .iter()
        .map(|b| (0..b.lines().len()).map(|_| Vec::new()).collect())
        .collect();
    let ext = path.rsplit('.').next().unwrap_or("");
    for (side, take_del) in [(old, true), (new, false)] {
        let source = side
            .iter()
            .map(|(_, _, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let Ok(doc) = crate::syntax::highlight(HighlightRequest {
            source: &source,
            path: Some(ext),
            fence_tag: None,
        }) else {
            continue;
        };
        for ((b, i, _), spans) in side.into_iter().zip(doc.lines) {
            let is_del = blocks[b].lines()[i].kind == LineKind::Del;
            if is_del == take_del {
                out[b][i] = spans;
            }
        }
    }
    Some(out)
}

struct Comment {
    path: String,
    line: Line,
    anchor: AnchorSlot,
    input: gpui::Entity<Input>,
    _events: Vec<Subscription>,
}

pub(crate) struct DiffView {
    files: Vec<DiffFile>,
    by_id: HashMap<SharedString, usize>,
    open: HashSet<SharedString>,
    known: HashSet<SharedString>,
    bodies: HashMap<SharedString, Body>,
    rows: Vec<Row>,
    offsets: Vec<f32>,
    total: f32,
    /// Hunk rows in order: (file, row index when expanded).
    hunks: Vec<(usize, Option<usize>)>,
    file_rows: Vec<usize>,
    max_columns: usize,
    number_width: f32,
    list: ListState,
    scroll_x: f32,
    viewport: Rc<Cell<Option<Bounds<Pixels>>>>,
    char_width: Cell<Option<f32>>,
    hovered: Option<usize>,
    pending: Option<usize>,
    focus_id: Option<SharedString>,
    pub(crate) busy_id: Option<SharedString>,
    pub(crate) file_count: Option<usize>,
    pub(crate) totals: Option<(u32, u32)>,
    highlights: HashMap<SharedString, (Arc<Vec<Block>>, Option<Tokens>)>,
    highlighting: Option<Task<()>>,
    thumb_drag: Option<(Pixels, f32)>,
    overview_drag: Option<f32>,
    overview_bounds: AnchorSlot,
    overview_hovered: bool,
    /// The axis this wheel gesture scrolls, and when it last moved.
    wheel_axis: Option<(bool, std::time::Instant)>,
    comment: Option<Comment>,
    action_slots: HashMap<usize, AnchorSlot>,
    focus: FocusHandle,
}
impl EventEmitter<DiffEvent> for DiffView {}
impl Focusable for DiffView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DiffView {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let list = ListState::new(0, ListAlignment::Top, px(600.));
        let weak = cx.weak_entity();
        list.set_scroll_handler(move |_, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                // A wheel or drag ends a pending jump, as React's capture does.
                this.pending = None;
                cx.notify();
            });
        });
        Self {
            files: Vec::new(),
            by_id: HashMap::new(),
            open: HashSet::new(),
            known: HashSet::new(),
            bodies: HashMap::new(),
            rows: Vec::new(),
            offsets: vec![0.],
            total: 0.,
            hunks: Vec::new(),
            file_rows: Vec::new(),
            max_columns: 40,
            number_width: 36.,
            list,
            scroll_x: 0.,
            viewport: Rc::default(),
            char_width: Cell::new(None),
            hovered: None,
            pending: None,
            focus_id: None,
            busy_id: None,
            file_count: None,
            totals: None,
            highlights: HashMap::new(),
            highlighting: None,
            thumb_drag: None,
            overview_drag: None,
            overview_bounds: AnchorSlot::default(),
            overview_hovered: false,
            wheel_axis: None,
            comment: None,
            action_slots: HashMap::new(),
            focus: cx.focus_handle(),
        }
    }

    /// New file models. Folds a user made survive; files that appear open.
    pub(crate) fn set_files(&mut self, files: Vec<DiffFile>, cx: &mut Context<Self>) {
        let unchanged = files.len() == self.files.len()
            && files
                .iter()
                .zip(&self.files)
                .all(|(a, b)| a.id == b.id && a.same_content(b));
        if unchanged {
            return;
        }
        let ids: HashSet<SharedString> = files.iter().map(|f| f.id.clone()).collect();
        self.open.retain(|id| ids.contains(id));
        for id in &ids {
            if !self.known.contains(id) {
                self.open.insert(id.clone());
            }
        }
        self.known = ids;
        self.files = files;
        self.rebuild(cx);
    }
    /// The file a review was opened on: expanded and scrolled to the top.
    pub(crate) fn focus_file(&mut self, id: Option<SharedString>, cx: &mut Context<Self>) {
        if id == self.focus_id {
            return;
        }
        self.focus_id = id.clone();
        if let Some(id) = id {
            if self.open.insert(id.clone()) {
                self.rebuild(cx);
            }
            if let Some(&ix) = self.by_id.get(&id) {
                self.pending = Some(ix);
                self.apply_pending(cx);
            }
        }
    }

    fn row_key(&self, ix: usize) -> Option<RowKey> {
        let row = self.rows.get(ix)?;
        let id = self.files.get(row.file)?.id.clone();
        Some(match &row.kind {
            RowKind::File => RowKey(id, 0, 0, 0),
            RowKind::Line { block, line, .. } => RowKey(id, 1, *block, *line),
            RowKind::Hunk(_) => RowKey(id, 2, ix, 0),
            RowKind::Notice(_) => RowKey(id, 3, 0, 0),
        })
    }
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        // Data changes above the reader stay anchored to the row they read.
        let anchor = {
            let top = self.list.logical_scroll_top();
            self.row_key(top.item_ix)
                .map(|key| (key, top.offset_in_item))
        };
        self.by_id = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.id.clone(), i))
            .collect();
        let mut rows = Vec::new();
        let mut hunks = Vec::new();
        let mut file_rows = Vec::with_capacity(self.files.len());
        let (mut columns, mut digits) = (40, 3);
        let keep: HashSet<_> = self.files.iter().map(|f| f.id.clone()).collect();
        self.bodies.retain(|id, _| keep.contains(id));
        for (index, file) in self.files.iter().enumerate() {
            let message = file.message();
            let stageable = file.can_stage_hunk;
            let stale = self.bodies.get(&file.id).is_none_or(|b| {
                !Arc::ptr_eq(&b.blocks, &file.blocks)
                    || b.message != message
                    || b.stageable != stageable
            });
            if stale {
                self.bodies
                    .insert(file.id.clone(), build_body(file, message, stageable));
            }
            let body = &self.bodies[&file.id];
            let expanded = self.open.contains(&file.id);
            file_rows.push(rows.len());
            rows.push(Row {
                file: index,
                kind: RowKind::File,
                height: FILE_HEADER,
            });
            let start = rows.len();
            for (i, row) in body.rows.iter().enumerate() {
                if matches!(row.kind, RowKind::Hunk(_)) {
                    hunks.push((index, expanded.then_some(start + i)));
                }
            }
            if expanded {
                rows.extend(body.rows.iter().map(|row| Row {
                    file: index,
                    ..row.clone()
                }));
                columns = columns.max(body.columns);
                digits = digits.max(body.digits);
            }
        }
        let mut offsets = Vec::with_capacity(rows.len() + 1);
        let mut at = 0.;
        for row in &rows {
            offsets.push(at);
            at += row.height;
        }
        offsets.push(at);
        self.rows = rows;
        self.offsets = offsets;
        self.total = at;
        self.hunks = hunks;
        self.file_rows = file_rows;
        self.max_columns = columns;
        self.number_width = (digits as f32 * 8. + 12.).max(36.);
        self.hovered = None;
        self.list.reset(self.rows.len());
        if self.pending.is_some() {
            self.apply_pending(cx);
        } else if let Some((key, offset)) = anchor {
            let target = (0..self.rows.len()).find(|&i| self.row_key(i).as_ref() == Some(&key));
            let target = target.or_else(|| self.by_id.get(&key.0).map(|&f| self.file_rows[f]));
            if let Some(item_ix) = target {
                let offset_in_item = if self.row_key(item_ix).as_ref() == Some(&key) {
                    offset
                } else {
                    px(0.)
                };
                self.list.scroll_to(ListOffset {
                    item_ix,
                    offset_in_item,
                });
            }
        }
        self.clamp_scroll_x();
        cx.notify();
    }
    /// A jump holds until its file has loaded, then lets go.
    fn apply_pending(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.pending else {
            return;
        };
        let Some(&row) = self.file_rows.get(file) else {
            return;
        };
        self.list.scroll_to(ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
        if self.files[file].empty_message.as_deref() != Some("Loading…") {
            self.pending = None;
        }
        cx.notify();
    }
    fn scroll_to_px(&self, y: f32) {
        let y = y.clamp(0., self.total.max(0.));
        let ix = self.offsets.partition_point(|o| *o <= y).saturating_sub(1);
        let ix = ix.min(self.rows.len().saturating_sub(1));
        self.list.scroll_to(ListOffset {
            item_ix: ix,
            offset_in_item: px(y - self.offsets.get(ix).copied().unwrap_or(0.)),
        });
    }
    fn top_px(&self) -> f32 {
        let top = self.list.logical_scroll_top();
        self.offsets.get(top.item_ix).copied().unwrap_or(0.) + top.offset_in_item.as_f32()
    }
    fn overview_metrics(&self, height: f32) -> crate::editor_scrollbar::Metrics {
        crate::editor_scrollbar::Metrics::new(self.top_px(), self.total, height, height)
    }
    fn overview_scroll(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let Some(bounds) = self.overview_bounds.get() else {
            return;
        };
        let Some(offset) = self.overview_drag else {
            return;
        };
        let m = self.overview_metrics(bounds.size.height.as_f32());
        self.pending = None;
        self.scroll_to_px(m.scroll_from_pointer((y - bounds.top()).as_f32(), offset));
        cx.notify();
    }
    fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::editor_scrollbar::WIDTH;
        let total = self.total.max(1.);
        let top = self.top_px();
        let active = self.overview_drag.is_some() || self.overview_hovered;
        div()
            .id("diff-overview")
            .debug_selector(|| "diff-overview".into())
            .absolute()
            .right_0()
            .top_0()
            .bottom_0()
            .w(px(WIDTH))
            .cursor(gpui::CursorStyle::Arrow)
            .child(self.overview_bounds.probe())
            .child(
                canvas(
                    move |bounds, _, _| bounds,
                    move |bounds, _, window, _| {
                        let m = crate::editor_scrollbar::Metrics::new(
                            top,
                            total,
                            bounds.size.height.as_f32(),
                            bounds.size.height.as_f32(),
                        );
                        window.paint_quad(gpui::fill(
                            Bounds::new(bounds.origin, gpui::size(px(1.), bounds.size.height)),
                            content(0.07),
                        ));
                        if m.scrollable {
                            window.paint_quad(
                                gpui::fill(
                                    Bounds::new(
                                        bounds.origin + gpui::point(px(3.), px(m.thumb_top)),
                                        gpui::size(px(WIDTH - 6.), px(m.thumb_height)),
                                    ),
                                    content(if active { 0.38 * 0.88 } else { 0.3 * 0.72 }),
                                )
                                .corner_radii(px(2.)),
                            );
                        }
                    },
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let Some(bounds) = this.overview_bounds.get() else {
                        return;
                    };
                    let y = (event.position.y - bounds.top()).as_f32();
                    let height = bounds.size.height.as_f32();
                    let m = this.overview_metrics(height);
                    if !m.scrollable {
                        return;
                    }
                    this.pending = None;
                    this.overview_drag = Some(
                        if event.position.x >= bounds.left() + px(3.)
                            && event.position.x <= bounds.right() - px(3.)
                            && y >= m.thumb_top
                            && y <= m.thumb_top + m.thumb_height
                        {
                            y - m.thumb_top
                        } else {
                            m.thumb_height / 2.
                        },
                    );
                    this.overview_scroll(event.position.y, cx);
                    cx.notify();
                }),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.overview_hovered = *hovered;
                cx.notify();
            }))
            .into_any_element()
    }
    /// currentHunkIndex.
    fn current_hunk(&self) -> Option<usize> {
        let top = self.top_px() + FILE_HEADER + 8.;
        let mut found = None;
        let mut first = None;
        for (index, (_, row)) in self.hunks.iter().enumerate() {
            let Some(row) = row else { continue };
            first.get_or_insert(index);
            if self.offsets[*row] > top + 0.5 {
                break;
            }
            found = Some(index);
        }
        found.or(first)
    }
    fn navigate(&mut self, direction: isize, cx: &mut Context<Self>) {
        if self.hunks.is_empty() {
            return;
        }
        let current = self.current_hunk().map_or(-1, |i| i as isize);
        let index = (current + direction).clamp(0, self.hunks.len() as isize - 1) as usize;
        let (file, _) = self.hunks[index];
        let id = self.files[file].id.clone();
        if self.open.insert(id) {
            self.rebuild(cx);
        }
        if let Some((_, Some(row))) = self.hunks.get(index) {
            self.pending = None;
            self.scroll_to_px(self.offsets[*row] - FILE_HEADER - 8.);
        }
        cx.notify();
    }
    fn toggle(&mut self, file: usize, cx: &mut Context<Self>) {
        let id = self.files[file].id.clone();
        if !self.open.remove(&id) {
            self.open.insert(id);
        }
        self.rebuild(cx);
    }

    fn gutter_width(&self) -> f32 {
        self.number_width * 2. + 24. + 3.
    }
    fn char_width(&self, window: &mut Window) -> f32 {
        if let Some(width) = self.char_width.get() {
            return width;
        }
        let mut font = window.text_style().font();
        font.family = crate::fonts::mono().into();
        let run = gpui::TextRun {
            len: 10,
            font,
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let width = window
            .text_system()
            .shape_line("0000000000".into(), px(CODE_SIZE), &[run], None)
            .width
            .as_f32()
            / 10.;
        self.char_width.set(Some(width));
        width
    }
    /// `--diff-code-min-width`: `calc((maxColumns + 2)ch + 24px)`.
    fn code_width(&self) -> f32 {
        (self.max_columns as f32 + 2.) * self.char_width.get().unwrap_or(7.5) + 24.
    }
    fn lane_width(&self) -> f32 {
        self.viewport
            .get()
            .map_or(0., |b| b.size.width.as_f32() - self.gutter_width())
    }
    fn max_scroll_x(&self) -> f32 {
        (self.code_width() - self.lane_width()).max(0.)
    }
    fn clamp_scroll_x(&mut self) {
        self.scroll_x = self.scroll_x.clamp(0., self.max_scroll_x());
    }
    /// Runs before the list sees the wheel. Any wheel ends a pending jump,
    /// as React's `stopJump`. Each gesture keeps to the axis it started on:
    /// a sideways one scrolls the code lanes and never the list, a vertical
    /// one never drifts the lanes. Returns whether it took the event.
    fn wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) -> bool {
        use gpui::TouchPhase;
        self.pending = None;
        let delta = event.delta.pixel_delta(px(LINE));
        let (dx, dy) = if event.modifiers.shift && delta.x.abs() < px(0.5) {
            (delta.y, px(0.))
        } else {
            (delta.x, delta.y)
        };
        let now = std::time::Instant::now();
        let fresh = matches!(event.touch_phase, TouchPhase::Started)
            || self.wheel_axis.is_none_or(|(_, at)| {
                now.duration_since(at) > std::time::Duration::from_millis(150)
            });
        let horizontal = if fresh {
            if dx.abs() < px(0.01) && dy.abs() < px(0.01) {
                self.wheel_axis = None;
                return false;
            }
            dx.abs() > dy.abs()
        } else {
            self.wheel_axis.is_some_and(|(h, _)| h)
        };
        self.wheel_axis =
            (!matches!(event.touch_phase, TouchPhase::Ended)).then_some((horizontal, now));
        if !horizontal {
            return false;
        }
        let next = (self.scroll_x - dx.as_f32()).clamp(0., self.max_scroll_x());
        if next != self.scroll_x {
            self.scroll_x = next;
            cx.notify();
        }
        true
    }

    /// Highlights files on screen that have no tokens yet, one at a time.
    fn request_highlights(&mut self, visible: &[usize], cx: &mut Context<Self>) {
        if self.highlighting.is_some() {
            return;
        }
        let Some(&file) = visible.iter().find(|&&f| {
            let file = &self.files[f];
            !file.blocks.is_empty()
                && self
                    .highlights
                    .get(&file.id)
                    .is_none_or(|(blocks, _)| !Arc::ptr_eq(blocks, &file.blocks))
        }) else {
            return;
        };
        let (id, path, blocks) = {
            let f = &self.files[file];
            (f.id.clone(), f.path.clone(), f.blocks.clone())
        };
        let work_blocks = blocks.clone();
        let work = cx
            .background_executor()
            .spawn(async move { highlight_file(&path, &work_blocks).map(Arc::new) });
        self.highlighting = Some(cx.spawn(async move |this, cx| {
            let tokens = work.await;
            let _ = this.update(cx, |this, cx| {
                this.highlights.insert(id, (blocks, tokens));
                this.highlighting = None;
                let keep: HashSet<_> = this.files.iter().map(|f| f.id.clone()).collect();
                this.highlights.retain(|id, _| keep.contains(id));
                cx.notify();
            });
        }));
    }
    fn tokens(&self, file: &DiffFile, block: usize, line: usize) -> Option<&[HighlightSpan]> {
        let (blocks, tokens) = self.highlights.get(&file.id)?;
        if !Arc::ptr_eq(blocks, &file.blocks) {
            return None;
        }
        tokens.as_ref()?.get(block)?.get(line).map(Vec::as_slice)
    }

    fn open_comment(
        &mut self,
        file: usize,
        line: Line,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.files[file].path.clone();
        let anchor = self.action_slots.get(&row).cloned().unwrap_or_default();
        let input = cx.new(|cx| {
            let mut input = Input::commit_message(cx);
            input.set_placeholder("Leave a comment…", cx);
            input
        });
        let focus = input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        let events = vec![
            cx.subscribe(&input, |_, _, _: &input::Changed, cx| cx.notify()),
            cx.subscribe_in(
                &input,
                window,
                |this, _, _: &input::Submitted, window, cx| this.add_comment(window, cx),
            ),
        ];
        self.comment = Some(Comment {
            path,
            line,
            anchor,
            input,
            _events: events,
        });
        cx.notify();
    }
    fn add_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(comment) = &self.comment else {
            return;
        };
        let text = diff::format_comment(
            &comment.path,
            &comment.line,
            &comment.input.read(cx).content,
        );
        if text.is_empty() {
            return;
        }
        self.comment = None;
        window.focus(&self.focus, cx);
        cx.emit(DiffEvent::Comment(text));
        cx.notify();
    }
    fn dismiss_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.comment.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.file_count.unwrap_or(self.files.len());
        let (additions, deletions) = self.totals.unwrap_or_else(|| {
            self.files
                .iter()
                .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions))
        });
        let current = self.current_hunk();
        let total = self.hunks.len();
        let position = if total > 0 {
            format!("{}/{total}", current.map_or(0, |c| c + 1))
        } else {
            "0/0".into()
        };
        let can_prev = total > 0 && current.is_some_and(|c| c > 0);
        let can_next = total > 0 && current.is_none_or(|c| c + 1 < total);
        div()
            .id("diff-tools")
            .role(Role::Toolbar)
            .aria_label("Diff tools")
            .h(px(32.))
            .flex_shrink_0()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(stroke())
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(content(0.85))
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(content(0.65))
                    .child(if count == 1 {
                        "1 file".to_owned()
                    } else {
                        format!("{count} files")
                    }),
            )
            .child(diff_counts(additions, deletions))
            .child(
                div()
                    .ml_auto()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        tool_button("previous-hunk", "Previous hunk", "chevron-up", can_prev)
                            .when(can_prev, |d| {
                                d.on_click(cx.listener(|this, _, _, cx| this.navigate(-1, cx)))
                            }),
                    )
                    .child(
                        div()
                            .min_w(px(36.))
                            .text_center()
                            .text_size(px(10.))
                            .font_weight(FontWeight::NORMAL)
                            .font_features(tabular())
                            .text_color(content(0.4))
                            .child(position),
                    )
                    .child(
                        tool_button("next-hunk", "Next hunk", "chevron-down", can_next)
                            .when(can_next, |d| {
                                d.on_click(cx.listener(|this, _, _, cx| this.navigate(1, cx)))
                            }),
                    )
                    .child(div().mx_1().h(px(12.)).w(px(1.)).bg(stroke()))
                    .child(
                        tool_button("expand-all", "Expand all files", "unfold-vertical", true)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open = this.files.iter().map(|f| f.id.clone()).collect();
                                this.rebuild(cx);
                            })),
                    )
                    .child(
                        tool_button(
                            "collapse-all",
                            "Collapse all files",
                            "fold-vertical",
                            !self.open.is_empty(),
                        )
                        .when(!self.open.is_empty(), |d| {
                            d.on_click(cx.listener(|this, _, _, cx| {
                                this.open.clear();
                                this.rebuild(cx);
                            }))
                        }),
                    ),
            )
    }

    fn file_header(&self, file: usize, sticky: bool, cx: &mut Context<Self>) -> AnyElement {
        let f = &self.files[file];
        let expanded = self.open.contains(&f.id);
        let busy = self.busy_id.as_ref() == Some(&f.id);
        let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_owned();
        let directory = f
            .label
            .strip_suffix(&name)
            .map(|d| d.trim_end_matches('/').to_owned())
            .unwrap_or_default();
        let id = f.id.clone();
        let (discard_id, stage_id) = (id.clone(), id.clone());
        div()
            .id(("diff-file", file * 2 + usize::from(sticky)))
            .h(px(FILE_HEADER))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(stroke())
            .hover(|s| s.bg(content(0.08)))
            .child(
                div()
                    .id(("diff-file-toggle", file * 2 + usize::from(sticky)))
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!(
                        "{} {}",
                        if expanded { "Collapse" } else { "Expand" },
                        f.label
                    )))
                    .aria_expanded(expanded)
                    .h_full()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        icon(
                            if expanded {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            14.,
                        )
                        .text_color(content(0.4)),
                    )
                    .child(file_type_icon(&name, false, false, 15.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(content(0.9))
                            .child(if directory.is_empty() {
                                f.label.clone()
                            } else {
                                name.clone()
                            }),
                    )
                    .when(!directory.is_empty(), |d| {
                        d.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(content(0.4))
                                .child(directory),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(file, cx))),
            )
            .child(diff_counts(f.additions, f.deletions))
            .when(f.can_discard, |d| {
                d.child(
                    tool_button(("discard-file", file), "Discard file", "undo", !busy).when(
                        !busy,
                        |d| {
                            d.on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(DiffEvent::DiscardFile(discard_id.clone()))
                            }))
                        },
                    ),
                )
            })
            .when(f.can_stage, |d| {
                d.child(
                    tool_button(("stage-file", file), "Stage file", "check", !busy).when(
                        !busy,
                        |d| {
                            d.on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(DiffEvent::StageFile(stage_id.clone()))
                            }))
                        },
                    ),
                )
            })
            .into_any_element()
    }

    fn row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(ix).cloned() else {
            return div().into_any_element();
        };
        let gutter = self.gutter_width();
        let number_width = self.number_width;
        let accent: Hsla = accent_color();
        match row.kind {
            RowKind::File => self.file_header(row.file, false, cx),
            RowKind::Notice(text) => div()
                .h(px(NOTICE))
                .w_full()
                .flex()
                .items_center()
                .child(div().w(px(gutter)).flex_shrink_0())
                .child(
                    div()
                        .px_3()
                        .text_size(px(12.))
                        .text_color(content(0.45))
                        .child(text),
                )
                .into_any_element(),
            RowKind::Hunk(text) => div()
                .h(px(HUNK))
                .w_full()
                .flex()
                .items_center()
                .bg(accent.opacity(0.06))
                .border_t_1()
                .border_b_1()
                .border_color(accent.opacity(0.1))
                .child(
                    div()
                        .w(px(gutter))
                        .flex_shrink_0()
                        .flex()
                        .pl(px(3.))
                        .text_size(px(9.))
                        .text_color(content(0.35))
                        .children(["Old", "New"].map(|label| {
                            div().w(px(number_width)).pr_2().text_right().child(label)
                        })),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .px_3()
                        .font_family(crate::fonts::mono())
                        .text_size(px(11.))
                        .text_color(content(0.5))
                        .child(text),
                )
                .into_any_element(),
            RowKind::Line { block, line, stage } => {
                let file = &self.files[row.file];
                let Some(l) = file.blocks.get(block).and_then(|b| b.lines().get(line)) else {
                    return div().h(px(LINE)).into_any_element();
                };
                let (added, removed) = (l.kind == LineKind::Add, l.kind == LineKind::Del);
                let ink: Option<Hsla> = if added {
                    Some(rgb(DIFF_ADD).into())
                } else if removed {
                    Some(rgb(DIFF_DEL).into())
                } else {
                    None
                };
                let hovered = self.hovered == Some(ix);
                let busy = self.busy_id.as_ref() == Some(&file.id);
                let text = expand_tabs(&l.text).into_owned();
                let styles: Vec<_> = self
                    .tokens(file, block, line)
                    .unwrap_or(&[])
                    .iter()
                    .filter(|span| span.range.end <= text.len())
                    .map(|span| {
                        (
                            span.range.clone(),
                            HighlightStyle {
                                color: Some(rgb(span.color).into()),
                                ..Default::default()
                            },
                        )
                    })
                    .collect();
                let number = if removed { l.old_number } else { l.new_number };
                let comment_line = l.clone();
                let file_ix = row.file;
                let file_id = file.id.clone();
                let slot = self.action_slots.entry(ix).or_default().clone();
                let gutter_ink = ink.unwrap_or(content(0.4));
                let numbers = |n: Option<u32>, hide: bool| {
                    div()
                        .w(px(number_width))
                        .flex_shrink_0()
                        .pr_2()
                        .text_right()
                        .text_size(px(11.))
                        .font_features(tabular())
                        .when(hide, |d| d.invisible())
                        .child(n.map(|n| n.to_string()).unwrap_or_default())
                };
                let scroll_x = self.scroll_x;
                let _ = window;
                div()
                    .id(("diff-line", ix))
                    .h(px(LINE))
                    .w_full()
                    .flex()
                    .line_height(px(LINE))
                    .when_some(ink, |d, ink| d.bg(ink.opacity(0.06)))
                    .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                        let next = if *hovering {
                            Some(ix)
                        } else if this.hovered == Some(ix) {
                            None
                        } else {
                            this.hovered
                        };
                        if next != this.hovered {
                            this.hovered = next;
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .relative()
                            .w(px(gutter))
                            .flex_shrink_0()
                            .flex()
                            .text_color(gutter_ink)
                            .child(
                                div()
                                    .w(px(3.))
                                    .flex_shrink_0()
                                    .when_some(ink, |d, ink| d.bg(ink.opacity(0.55))),
                            )
                            .child(numbers(l.old_number, hovered))
                            .child(numbers(l.new_number, false))
                            .child(
                                div()
                                    .w(px(24.))
                                    .flex_shrink_0()
                                    .text_center()
                                    .text_size(px(12.))
                                    .when(hovered && stage.is_some(), |d| d.invisible())
                                    .child(if added {
                                        "+"
                                    } else if removed {
                                        "−"
                                    } else {
                                        ""
                                    }),
                            )
                            .when(hovered, |d| {
                                d.child(
                                    div()
                                        .id(("diff-comment", ix))
                                        .role(Role::Button)
                                        .aria_label(SharedString::from(format!(
                                            "Comment on line {}",
                                            number.map(|n| n.to_string()).unwrap_or_default()
                                        )))
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left(px(3.))
                                        .w(px(number_width))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(3.))
                                        .hover(|s| s.bg(content(0.12)))
                                        .child(slot.probe())
                                        .child(icon("comment-add", 12.).text_color(content(1.)))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_comment(
                                                file_ix,
                                                comment_line.clone(),
                                                ix,
                                                window,
                                                cx,
                                            )
                                        })),
                                )
                                .when_some(stage, |d, chunk| {
                                    d.child(
                                        div()
                                            .id(("diff-stage-hunk", ix))
                                            .role(Role::Button)
                                            .aria_label("Stage hunk")
                                            .absolute()
                                            .top_0()
                                            .bottom_0()
                                            .right_0()
                                            .w(px(24.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(px(3.))
                                            .when(busy, |d| d.opacity(0.35))
                                            .when(!busy, |d| d.hover(|s| s.bg(content(0.12))))
                                            .child(icon("check", 12.).text_color(content(1.)))
                                            .when(!busy, |d| {
                                                d.on_click(cx.listener(move |_, _, _, cx| {
                                                    cx.emit(DiffEvent::StageHunk(
                                                        file_id.clone(),
                                                        chunk,
                                                    ))
                                                }))
                                            }),
                                    )
                                })
                            }),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left(px(-scroll_x))
                                    .pl_3()
                                    .pr_6()
                                    .whitespace_nowrap()
                                    .font_family(crate::fonts::mono())
                                    .text_size(px(CODE_SIZE))
                                    .text_color(content(0.92))
                                    .child(StyledText::new(text).with_highlights(styles)),
                            ),
                    )
                    .into_any_element()
            }
        }
    }

    /// The header of the file whose body is at the top, pinned until the
    /// next file's header pushes it away.
    fn sticky_header(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let top = self.list.logical_scroll_top();
        let row = self.rows.get(top.item_ix)?;
        if matches!(row.kind, RowKind::File) && top.offset_in_item <= px(0.) {
            return None;
        }
        let file = row.file;
        // A collapsed file has no body for its header to stick over.
        if !self.open.contains(&self.files[file].id) {
            return None;
        }
        let top_px = self.top_px();
        let next = self
            .file_rows
            .get(file + 1)
            .map(|&r| self.offsets[r] - top_px - FILE_HEADER)
            .unwrap_or(0.)
            .min(0.);
        Some(
            div()
                .absolute()
                .top(px(next))
                .left_0()
                .right_0()
                .occlude()
                .child(blur())
                .child(self.file_header(file, true, cx))
                .into_any_element(),
        )
    }

    fn horizontal_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let max = self.max_scroll_x();
        let lane = self.lane_width();
        let code = self.code_width();
        let track = lane.max(1.);
        let thumb = if code > 0. {
            (lane / code * track).max(24.)
        } else {
            track
        };
        let left = if max > 0. {
            self.scroll_x / max * (track - thumb)
        } else {
            0.
        };
        let gutter = self.gutter_width();
        div()
            .id("diff-horizontal-scroll")
            .role(Role::ScrollBar)
            .aria_label("Scroll diff horizontally")
            .h(px(14.))
            .flex_shrink_0()
            .relative()
            .when(max > 0., |d| {
                d.child(
                    div()
                        .id("diff-horizontal-thumb")
                        .absolute()
                        .top(px(4.))
                        .h(px(6.))
                        .left(px(gutter + left))
                        .w(px(thumb))
                        .rounded_full()
                        .bg(content(if self.thumb_drag.is_some() { 0.35 } else { 0.2 }))
                        .hover(|s| s.bg(content(0.35)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                this.thumb_drag = Some((event.position.x, this.scroll_x));
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        ),
                )
            })
    }
}

fn blur() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            // Keep the tint on the blur quad itself. GPUI treats a fully
            // transparent fill as border-only and strips its interior.
            let quad = gpui::fill(bounds, content(0.02))
                .border_color(gpui::backdrop_blur_marker(24. * window.scale_factor()));
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let quad = gpui::fill(bounds, rgb(CHAT));
            window.paint_quad(quad);
        },
    )
    .absolute()
    .size_full()
}

/// DiffCounts: `+a −d` at 11px medium, nothing when unchanged.
pub(crate) fn diff_counts(additions: u32, deletions: u32) -> AnyElement {
    if additions == 0 && deletions == 0 {
        return div().into_any_element();
    }
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .font_features(tabular())
        .when(additions > 0, |d| {
            d.child(
                div()
                    .text_color(rgb(DIFF_ADD))
                    .child(format!("+{}", format_integer(additions as i64))),
            )
        })
        .when(deletions > 0, |d| {
            d.child(
                div()
                    .text_color(rgb(DIFF_DEL))
                    .child(format!("−{}", format_integer(deletions as i64))),
            )
        })
        .into_any_element()
}

/// UnifiedDiffView's IconButton: 24px, 45% ink, 30% when disabled.
fn tool_button(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    path: &'static str,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let group: SharedString = format!("diff-tool-{id:?}").into();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .group(group.clone())
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .when(!enabled, |d| d.opacity(0.3))
        .when(enabled, |d| d.hover(|s| s.bg(content(0.05))))
        .child(
            icon(path, 14.)
                .text_color(content(0.45))
                .when(enabled, |i| {
                    i.group_hover(group, |s| s.text_color(content(1.)))
                }),
        )
}

impl Render for DiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.files.is_empty() {
            return div()
                .id("diff-review")
                .key_context("DiffReview")
                .track_focus(&self.focus)
                .px_4()
                .py_6()
                .text_size(px(13.))
                .text_color(content(0.45))
                .child("No file changes")
                .into_any_element();
        }
        self.char_width(window);
        self.clamp_scroll_x();
        // Files with lines on screen get highlighted.
        let top = self.list.logical_scroll_top().item_ix;
        let height = self.viewport.get().map_or(800., |b| b.size.height.as_f32());
        let end = self
            .offsets
            .partition_point(|o| *o <= self.offsets.get(top).copied().unwrap_or(0.) + height);
        let mut visible: Vec<usize> = self.rows[top.min(self.rows.len())..end.min(self.rows.len())]
            .iter()
            .filter(|r| matches!(r.kind, RowKind::Line { .. }))
            .map(|r| r.file)
            .collect();
        visible.dedup();
        self.request_highlights(&visible, cx);
        let entity = cx.entity();
        let viewport = self.viewport.clone();
        let view_id = cx.entity_id();
        let weak = cx.weak_entity();
        let sticky = self.sticky_header(cx);
        let comment = self.comment.as_ref().map(|c| {
            let location = diff::comment_location(&c.path, &c.line);
            let can_add = !c.input.read(cx).content.trim().is_empty();
            popover(
                "diff-comment",
                crate::popover::Anchor::Slot(c.anchor.clone()),
            )
            .side(Side::Right)
            .gap(6.)
            .width(320.)
            .padding(8.)
            .on_dismiss({
                let weak = cx.weak_entity();
                move |window, cx| {
                    let _ = weak.update(cx, |this, cx| this.dismiss_comment(window, cx));
                }
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .mb(px(6.))
                            .px(px(2.))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(crate::fonts::mono())
                                    .text_size(px(11.))
                                    .text_color(content(0.55))
                                    .child(location),
                            )
                            .child(
                                tool_button("cancel-comment", "Cancel comment", "x", true)
                                    .size(px(20.))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dismiss_comment(window, cx)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .min_h(px(72.))
                            .rounded_lg()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(crate::theme::background_base().opacity(0.7))
                            .px(px(10.))
                            .py_2()
                            .text_size(px(13.))
                            .text_color(content(1.))
                            .child(c.input.clone()),
                    )
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(div().text_size(px(10.)).text_color(content(0.35)).child(
                                if cfg!(target_os = "macos") {
                                    "⌘↩ to add"
                                } else {
                                    "Ctrl+↩ to add"
                                },
                            ))
                            .child(
                                div()
                                    .id("add-comment")
                                    .role(Role::Button)
                                    .aria_label("Add to chat")
                                    .h(px(28.))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .px(px(10.))
                                    .rounded_md()
                                    .bg(content(1.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(crate::theme::background_base())
                                    .when(!can_add, |d| d.opacity(0.4))
                                    .when(can_add, |d| d.hover(|s| s.opacity(0.8)))
                                    .child(
                                        icon("comment-add", 14.)
                                            .text_color(crate::theme::background_base()),
                                    )
                                    .child("Add to chat")
                                    .when(can_add, |d| {
                                        d.on_click(cx.listener(|this, _, window, cx| {
                                            this.add_comment(window, cx)
                                        }))
                                    }),
                            ),
                    ),
            )
        });
        div()
            .id("diff-review")
            .key_context("DiffReview")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &PreviousHunk, _, cx| this.navigate(-1, cx)))
            .on_action(cx.listener(|this, _: &NextHunk, _, cx| this.navigate(1, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.pending = None;
                    if this.comment.is_none() {
                        window.focus(&this.focus, cx);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.overview_drag.is_some() {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.overview_scroll(event.position.y, cx);
                    } else {
                        this.overview_drag = None;
                        cx.notify();
                    }
                }
                if let Some((start, from)) = this.thumb_drag {
                    if event.pressed_button != Some(MouseButton::Left) {
                        this.thumb_drag = None;
                    } else {
                        let lane = this.lane_width().max(1.);
                        let code = this.code_width().max(1.);
                        let thumb = (lane / code * lane).max(24.);
                        let span = (lane - thumb).max(1.);
                        let dx = (event.position.x - start).as_f32();
                        this.scroll_x =
                            (from + dx / span * this.max_scroll_x()).clamp(0., this.max_scroll_x());
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.overview_drag.take().is_some() {
                        cx.notify();
                    }
                    if this.thumb_drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.overview_drag.take().is_some() | this.thumb_drag.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(self.toolbar(cx))
            .child(
                div()
                    .id("unified-diff")
                    .role(Role::Region)
                    .aria_label("Unified diff")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    // The pinned header slides up under the toolbar, never over it.
                    .overflow_hidden()
                    .pr(px(crate::editor_scrollbar::WIDTH))
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                // A new width moves the scroll limits: lay out again.
                                if viewport.get().is_none_or(|old| old.size != bounds.size) {
                                    cx.notify(view_id);
                                }
                                viewport.set(Some(bounds));
                            },
                            move |bounds, _, window, _| {
                                let weak = weak.clone();
                                window.on_mouse_event(
                                    move |event: &ScrollWheelEvent, phase, _, cx| {
                                        if phase != gpui::DispatchPhase::Capture
                                            || !bounds.contains(&event.position)
                                        {
                                            return;
                                        }
                                        let took = weak
                                            .update(cx, |this, cx| this.wheel(event, cx))
                                            .unwrap_or(false);
                                        if took {
                                            cx.stop_propagation();
                                        }
                                    },
                                );
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        list(self.list.clone(), move |ix, window, cx| {
                            entity.update(cx, |this, cx| this.row(ix, window, cx))
                        })
                        .size_full(),
                    )
                    .children(sticky)
                    .child(self.overview(cx)),
            )
            .child(self.horizontal_bar(cx))
            .children(comment)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    fn review_overview_has_no_change_markers_and_supports_scrolling_and_dragging(
        cx: &mut gpui::TestAppContext,
    ) {
        let original: String = (0..200).map(|n| format!("line {n}\n")).collect();
        let current = original
            .replace("line 80\n", "changed 80\n")
            .replace("line 150\n", "changed 150\n");
        let unified = diff::build_unified_file(&original, &current, usize::MAX);
        let mut model = file("overview", &original, &current);
        model.blocks = Arc::new(unified.blocks);
        let window = cx.update(|cx| {
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::new(
                        gpui::point(px(0.), px(0.)),
                        gpui::size(px(500.), px(300.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(DiffView::new),
            )
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| view.set_files(vec![model], cx))
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let rail = visual.debug_bounds("diff-overview").unwrap();
        assert_eq!(rail.size.width, px(18.));
        window
            .update(visual, |_, window, cx| {
                let (add, del) = crate::editor_scrollbar::diff_colors(cx);
                let scale = window.scale_factor();
                let quads = window.painted_quads();
                let rail_quads: Vec<_> = quads
                    .iter()
                    .filter(|quad| {
                        quad.bounds.left().0 >= rail.left().as_f32() * scale
                            && quad.bounds.right().0 <= rail.right().as_f32() * scale
                    })
                    .collect();
                assert!(!rail_quads.is_empty());
                assert!(rail_quads.iter().all(|quad| {
                    quad.background != gpui::Hsla::from(rgb(add)).into()
                        && quad.background != gpui::Hsla::from(rgb(del)).into()
                }));
            })
            .unwrap();
        let track = rail.origin + gpui::point(px(8.), rail.size.height * 0.75);
        visual.simulate_mouse_down(track, MouseButton::Left, Default::default());
        window
            .update(visual, |view, _, _| {
                assert!(view.overview_drag.is_some());
            })
            .unwrap();
        visual.simulate_mouse_up(track, MouseButton::Left, Default::default());
        visual.run_until_parked();
        let thumb = window
            .update(visual, |view, _, _| {
                assert!(view.top_px() > 2500.);
                let m = view.overview_metrics(rail.size.height.as_f32());
                rail.origin + gpui::point(px(8.), px(m.thumb_top + 4.))
            })
            .unwrap();
        visual.simulate_mouse_down(thumb, MouseButton::Left, Default::default());
        let bottom = gpui::point(thumb.x, rail.bottom() - px(1.));
        visual.simulate_mouse_move(bottom, MouseButton::Left, Default::default());
        visual.simulate_mouse_up(bottom, MouseButton::Left, Default::default());
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                assert!(view.top_px() > 3500.);
                assert!(view.overview_drag.is_none());
            })
            .unwrap();
    }
    fn file(id: &str, original: &str, current: &str) -> DiffFile {
        let unified = diff::build_unified_file(original, current, 3);
        DiffFile {
            id: id.to_owned().into(),
            path: format!("/r/{id}"),
            label: id.into(),
            binary: false,
            too_large: false,
            empty_message: None,
            additions: unified.additions,
            deletions: unified.deletions,
            blocks: Arc::new(unified.blocks),
            can_stage: true,
            can_discard: true,
            can_stage_hunk: true,
        }
    }
    #[gpui::test]
    fn sideways_swipes_end_jumps_and_never_scroll_the_list(cx: &mut gpui::TestAppContext) {
        use gpui::{ScrollDelta, ScrollWheelEvent, TouchPhase, point};
        let wide: String = (0..80)
            .map(|i| format!("{i} {}\n", "x".repeat(300)))
            .collect();
        let edited = wide.replacen("1 x", "1 y", 1);
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(DiffView::new))
                .unwrap()
        });
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(cx, |view, _, cx| {
                let mut loading = file("b.rs", "", "");
                loading.empty_message = Some("Loading…".into());
                view.set_files(vec![file("a.rs", &wide, &edited), loading], cx);
                view.focus_file(Some("b.rs".into()), cx);
                assert!(view.pending.is_some());
            })
            .unwrap();
        cx.run_until_parked();
        let top = window.update(cx, |view, _, _| view.top_px()).unwrap();
        for (phase, dx, dy) in [
            (TouchPhase::Started, -40., -3.),
            (TouchPhase::Moved, -40., 6.),
            (TouchPhase::Moved, -40., 8.),
        ] {
            cx.simulate_event(ScrollWheelEvent {
                position: point(px(300.), px(200.)),
                delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
                modifiers: Default::default(),
                touch_phase: phase,
            });
        }
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(view.pending.is_none());
                assert_eq!(view.scroll_x, 120.);
                assert_eq!(view.top_px(), top);
            })
            .unwrap();
    }
    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn sticky_header_retains_its_translucent_blur_surface(cx: &mut gpui::TestAppContext) {
        let long: String = (0..60).map(|i| format!("line {i}\n")).collect();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(DiffView::new))
                .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                view.set_files(
                    vec![file("a.rs", &long, &long.replace("line 30", "LINE 30"))],
                    cx,
                );
                view.list.scroll_to(ListOffset {
                    item_ix: 3,
                    offset_in_item: px(0.),
                });
            })
            .unwrap();
        cx.refresh().unwrap();
        window
            .update(cx, |_, window, _| {
                let marker = gpui::backdrop_blur_marker(24. * window.scale_factor());
                let quads = window.painted_quads();
                let glass = quads.iter().find(|q| q.border_color == marker).expect(
                    "the sticky header's blur must survive GPUI's transparent-quad culling",
                );
                assert_eq!(glass.background, content(0.02).into());
                assert_eq!(
                    glass.bounds.size.height.0,
                    FILE_HEADER * window.scale_factor()
                );
            })
            .unwrap();
    }
    #[gpui::test]
    fn pinned_header_follows_open_files_only(cx: &mut gpui::TestAppContext) {
        let long: String = (0..60).map(|i| format!("line {i}\n")).collect();
        let edited = long.replace("line 30", "LINE 30");
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(DiffView::new))
                .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                view.set_files(
                    vec![file("a.rs", &long, &edited), file("b.rs", "x\n", "y\n")],
                    cx,
                );
                // Inside a.rs's body, below its header.
                view.list.scroll_to(ListOffset {
                    item_ix: 3,
                    offset_in_item: px(0.),
                });
                assert!(view.sticky_header(cx).is_some());
                view.toggle(0, cx);
                view.list.scroll_to(ListOffset {
                    item_ix: 0,
                    offset_in_item: px(10.),
                });
                assert!(view.sticky_header(cx).is_none());
            })
            .unwrap();
    }
    #[test]
    fn bodies_number_hunks_and_mark_stageable_lines() {
        let f = file("a.rs", "one\ntwo\nthree\n", "one\nTWO\nthree\n");
        let body = build_body(&f, f.message(), true);
        assert!(
            matches!(&body.rows[0].kind, RowKind::Hunk(text) if text.as_ref() == "@@ -1,3 +1,3 @@")
        );
        let stages: Vec<_> = body
            .rows
            .iter()
            .filter_map(|r| match r.kind {
                RowKind::Line { stage, .. } => Some(stage),
                _ => None,
            })
            .collect();
        assert_eq!(stages, [None, Some(0), Some(0), None]);
        let binary = DiffFile {
            binary: true,
            ..f.clone()
        };
        assert!(
            matches!(&build_body(&binary, binary.message(), true).rows[0].kind,
            RowKind::Notice(t) if t.as_ref() == "Binary file changed")
        );
        assert_eq!(expand_tabs("\tab\tc"), "    ab  c");
    }
}
