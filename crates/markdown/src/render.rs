//! Prepared blocks to GPUI elements.
//!
//! Layout follows the `.agent-markdown` rules in index.css: collapsed block
//! margins, list gutters, the code block shell with its header, line numbers
//! and copy button, and the bordered table wrapper. Colors are paint: the
//! fade and syntax colors only change run colors, never fonts or lengths, so
//! they never move text.
//!
//! Every text element registers itself in a per-frame [`Registry`] during
//! prepaint, in document order. Selection, link hover, and link clicks read
//! the registry; paint reads the selection from it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, BorderStyle, Bounds, CursorStyle, ElementId, FontWeight, Hitbox,
    HitboxBehavior, Hsla, Image, ImageFormat, ImageSource, ObjectFit, PathBuilder, Pixels, Point,
    SharedString, StyledImage, StyledText, TextLayout, TextRun, UnderlineStyle, WeakEntity, canvas,
    div, font, img, point, px, quad, size,
};

use crate::fade::RevealTimeline;
use crate::highlight::{self, CodeHighlight};
use crate::parse::{Align, ImageRef};
use crate::prepare::{
    LinkRange, PreparedBlock, PreparedCode, PreparedItem, PreparedNode, PreparedTable,
    PreparedText, Segment,
};
use crate::selection::{Element, ElementKey, Selection};
use crate::style::MarkdownStyle;
use crate::view::MarkdownView;

/// Rounded wash geometry for inline code: the wash spans the padded range
/// and leaves one pixel above and below inside the line box.
const INLINE_CODE_INSET_Y: f32 = 1.0;

/// The text elements of the current frame, in document order. Elements of
/// blocks that were not laid out this frame (see [`Geometry`]) have no
/// layout: they still take part in selection and copy, but not in hit tests.
#[derive(Default)]
pub(crate) struct Registry {
    pub elements: Vec<Element>,
    pub layouts: Vec<Option<TextLayout>>,
    pub links: Vec<Arc<[LinkRange]>>,
    /// The view's selection when it last rendered.
    pub selection: Selection,
    ranges: Option<HashMap<ElementKey, Range<usize>>>,
}

impl Registry {
    pub fn clear(&mut self) {
        self.elements.clear();
        self.layouts.clear();
        self.links.clear();
        self.ranges = None;
    }

    pub fn set_selection(&mut self, selection: Selection) {
        self.selection = selection;
        self.ranges = None;
    }

    fn push(&mut self, element: Element, layout: Option<TextLayout>, links: Arc<[LinkRange]>) {
        self.elements.push(element);
        self.layouts.push(layout);
        self.links.push(links);
        self.ranges = None;
    }

    /// The selected range of `key` this frame.
    pub fn range_for(&mut self, key: ElementKey) -> Option<Range<usize>> {
        if self.ranges.is_none() {
            let ranges = self
                .selection
                .ranges(&self.elements)
                .into_iter()
                .map(|(ix, range)| (self.elements[ix].key, range))
                .collect();
            self.ranges = Some(ranges);
        }
        self.ranges.as_ref()?.get(&key).cloned()
    }

    pub fn keys(&self) -> Vec<ElementKey> {
        self.elements.iter().map(|e| e.key).collect()
    }

    /// The element and byte offset nearest to a window position. Pointers in
    /// a gap between elements land at the end of the element above.
    pub fn hit(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        if self.layouts.is_empty() {
            return None;
        }
        let mut best: Option<(usize, Pixels)> = None;
        for (ix, layout) in self.laid_out() {
            let bounds = layout.bounds();
            if position.y < bounds.top() || position.y > bounds.bottom() {
                continue;
            }
            let dx = if position.x < bounds.left() {
                bounds.left() - position.x
            } else if position.x > bounds.right() {
                position.x - bounds.right()
            } else {
                px(0.)
            };
            if best.is_none_or(|(_, d)| dx < d) {
                best = Some((ix, dx));
            }
        }
        if let Some((ix, _)) = best {
            return Some((ix, self.offset_in(ix, position)));
        }
        // Between or outside elements: the end of the last element above, or
        // the start of the first one.
        let above = self
            .laid_out()
            .filter(|(_, layout)| layout.bounds().bottom() < position.y)
            .max_by(|a, b| {
                a.1.bounds()
                    .bottom()
                    .partial_cmp(&b.1.bounds().bottom())
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            })
            .map(|(ix, _)| ix);
        match above {
            Some(ix) => Some((ix, self.elements[ix].text.len())),
            None => self.laid_out().next().map(|(ix, _)| (ix, 0)),
        }
    }

    fn laid_out(&self) -> impl Iterator<Item = (usize, &TextLayout)> {
        self.layouts
            .iter()
            .enumerate()
            .filter_map(|(ix, layout)| Some((ix, layout.as_ref()?)))
    }

    fn offset_in(&self, ix: usize, position: Point<Pixels>) -> usize {
        let Some(layout) = &self.layouts[ix] else {
            return 0;
        };
        let bounds = layout.bounds();
        let clamped = point(
            position.x.clamp(bounds.left(), bounds.right()),
            position
                .y
                .clamp(bounds.top(), (bounds.bottom() - px(1.)).max(bounds.top())),
        );
        let offset = match layout.index_for_position(clamped) {
            Ok(ix) | Err(ix) => ix,
        };
        let text = &self.elements[ix].text;
        let mut offset = offset.min(text.len());
        while offset > 0 && !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// The clickable link under a window position: `(element key, link
    /// index)`.
    pub fn link_at(&self, position: Point<Pixels>) -> Option<(ElementKey, usize, Arc<str>)> {
        for (ix, layout) in self.laid_out() {
            let links = &self.links[ix];
            if links.is_empty() || !layout.bounds().contains(&position) {
                continue;
            }
            for (link_ix, link) in links.iter().enumerate() {
                if link.pending {
                    continue;
                }
                if range_rects(layout, &link.range, 0., 0.)
                    .iter()
                    .any(|rect| rect.contains(&position))
                {
                    return Some((self.elements[ix].key, link_ix, link.url.clone()));
                }
            }
        }
        None
    }
}

/// One box per visual line that `range` covers, in window coordinates.
/// `pad_x` widens each box, `inset_y` shrinks it vertically.
pub(crate) fn range_rects(
    layout: &TextLayout,
    range: &Range<usize>,
    pad_x: f32,
    inset_y: f32,
) -> Vec<Bounds<Pixels>> {
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let position = |ix: usize| layout.position_for_index(ix);
    let mut rects = Vec::new();
    let mut cur = range.start;
    let mut guard = range.len() + 2;
    while cur < range.end && guard > 0 {
        guard -= 1;
        let Some(mut start) = position(cur) else {
            break;
        };
        // GPUI reports a soft-wrap boundary at the end of the row above; when
        // the next byte sits lower, `cur` starts that lower row.
        if let Some(after) = position(cur + 1)
            && after.y > start.y
        {
            start = point(bounds.left(), after.y);
        }
        let (row_end, next) = match position(range.end) {
            Some(end) if end.y == start.y => (range.end, range.end),
            _ => {
                let (mut lo, mut hi) = (cur, range.end);
                while hi - lo > 1 {
                    let mid = lo + (hi - lo) / 2;
                    match position(mid) {
                        Some(p) if p.y == start.y => lo = mid,
                        _ => hi = mid,
                    }
                }
                (lo, if lo == cur { lo + 1 } else { lo })
            }
        };
        if let Some(end) = position(row_end)
            && end.x > start.x
            && end.y == start.y
        {
            rects.push(Bounds::new(
                point(start.x - px(pad_x), start.y + px(inset_y)),
                size(
                    end.x - start.x + px(2. * pad_x),
                    line_height - px(2. * inset_y),
                ),
            ));
        }
        if next <= cur {
            break;
        }
        cur = next;
    }
    rects
}

/// What image sources resolve to.
pub type ImageResolver = Rc<dyn Fn(&str) -> Option<ImageSource>>;

/// Highlight state for the code blocks of one message.
///
/// A few new lines (a block streaming in) highlight on the UI thread within
/// the frame. Anything bigger, such as a finished reply opened from history,
/// goes to a background job; the block shows its earlier colors, plain text
/// after them, until the job returns.
#[derive(Default)]
pub(crate) struct CodeState {
    entries: HashMap<ElementKey, CodeEntry>,
    /// Lines that may still be highlighted on the UI thread this frame.
    pub budget: usize,
    /// The grammar set is not loaded yet.
    pub needs_syntaxes: bool,
    /// The block whose copy button shows the check mark.
    pub copied: Option<ElementKey>,
    /// Highlight work for the view to run off the UI thread.
    pub jobs: Vec<HighlightJob>,
    diagrams: HashMap<ElementKey, DiagramEntry>,
    pub diagram_jobs: Vec<DiagramJob>,
}

struct DiagramEntry {
    source: SharedString,
    dark: bool,
    image: Option<Arc<Image>>,
    size: (f32, f32),
    source_visible: bool,
    in_flight: bool,
    attempted: Option<(SharedString, bool)>,
}

pub(crate) struct DiagramJob {
    key: ElementKey,
    source: SharedString,
    dark: bool,
    svg: Option<String>,
}

impl DiagramJob {
    pub fn run(mut self) -> Self {
        self.svg = render_diagram(&self.source, self.dark);
        self
    }
}

fn render_diagram(source: &str, dark: bool) -> Option<String> {
    if source.len() > 512 * 1024 {
        return None;
    }
    let mut theme = if dark {
        mermaid_rs_renderer::Theme::dark()
    } else {
        mermaid_rs_renderer::Theme::modern()
    };
    theme.font_family = "Helvetica, Arial, sans-serif".into();
    let options = mermaid_rs_renderer::RenderOptions {
        theme,
        ..Default::default()
    };
    mermaid_rs_renderer::render_with_options(source, options).ok()
}

fn diagram_size(svg: &str) -> (f32, f32) {
    let dimensions = svg
        .split_once("viewBox=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(value, _)| {
            value
                .split_whitespace()
                .filter_map(|v| v.parse::<f32>().ok())
                .collect::<Vec<_>>()
        });
    match dimensions.as_deref() {
        Some([_, _, width, height])
            if width.is_finite() && height.is_finite() && *width > 0. && *height > 0. =>
        {
            (*width, *height)
        }
        _ => (640., 320.),
    }
}

/// One background highlight: bring `highlight` up to date with `code`.
pub(crate) struct HighlightJob {
    pub key: ElementKey,
    pub highlight: CodeHighlight,
    pub code: SharedString,
}

impl HighlightJob {
    /// Run the job. Call it off the UI thread.
    pub fn run(mut self) -> Self {
        if let Some(syntaxes) = highlight::syntaxes() {
            self.highlight.update(syntaxes, &self.code);
        }
        self
    }

    /// Wrap the job to move it to a background thread.
    pub fn into_send(self) -> SendJob {
        SendJob(self)
    }
}

/// A [`HighlightJob`] that may cross to a worker thread.
pub(crate) struct SendJob(HighlightJob);

// SAFETY: the only part of a job that is not `Send` is the Oniguruma match
// region (`onig::Region`) that syntect's `ParseState` keeps for capture
// back-references. A region is plain heap memory owned by the state, with no
// thread affinity and no shared references, and the job is used by one
// thread at a time: it moves to the worker, runs, and moves back.
unsafe impl Send for SendJob {}

impl SendJob {
    pub fn run(self) -> SendJob {
        SendJob(self.0.run())
    }

    pub fn into_inner(self) -> HighlightJob {
        self.0
    }
}

/// Lines a block may add in one frame and still highlight on the UI thread.
const SYNC_LINES: usize = 12;

struct CodeEntry {
    language: String,
    /// `None` while a background job holds the state.
    highlight: Option<CodeHighlight>,
    /// The code `highlight` is up to date with.
    highlighted: SharedString,
    in_flight: bool,
    /// No grammar matches the language.
    plain_only: bool,
    /// The code the cached runs color, and the runs.
    runs: Option<(SharedString, Rc<Vec<TextRun>>)>,
}

impl CodeEntry {
    fn new(language: String) -> Self {
        Self {
            language,
            highlight: None,
            highlighted: SharedString::default(),
            in_flight: false,
            plain_only: false,
            runs: None,
        }
    }
}

impl CodeState {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.jobs.clear();
        self.diagrams.clear();
        self.diagram_jobs.clear();
    }

    /// Drop entries for blocks past the end of the document.
    pub fn retain_blocks(&mut self, blocks: usize) {
        self.entries.retain(|key, _| key.block() < blocks);
        self.diagrams.retain(|key, _| key.block() < blocks);
    }

    pub fn finish_diagram(&mut self, job: DiagramJob) {
        let Some(entry) = self.diagrams.get_mut(&job.key) else {
            return;
        };
        entry.in_flight = false;
        if entry.source != job.source || entry.dark != job.dark {
            return;
        }
        if let Some(svg) = job.svg {
            entry.size = diagram_size(&svg);
            entry.image = Some(Arc::new(Image::from_bytes(
                ImageFormat::Svg,
                svg.into_bytes(),
            )));
        }
    }

    pub fn toggle_diagram_source(&mut self, key: ElementKey) {
        if let Some(entry) = self.diagrams.get_mut(&key) {
            entry.source_visible = !entry.source_visible;
        }
    }

    fn diagram(&mut self, code: &PreparedCode, dark: bool) -> Option<(Arc<Image>, (f32, f32))> {
        let entry = self
            .diagrams
            .entry(code.key)
            .or_insert_with(|| DiagramEntry {
                source: code.code.clone(),
                dark,
                image: None,
                size: (640., 320.),
                source_visible: false,
                in_flight: false,
                attempted: None,
            });
        if entry.source != code.code || entry.dark != dark {
            entry.source = code.code.clone();
            entry.dark = dark;
            entry.image = None;
        }
        let desired = (code.code.clone(), dark);
        if !entry.in_flight && entry.attempted.as_ref() != Some(&desired) {
            entry.in_flight = true;
            entry.attempted = Some(desired);
            self.diagram_jobs.push(DiagramJob {
                key: code.key,
                source: code.code.clone(),
                dark,
                svg: None,
            });
        }
        if entry.source_visible {
            return None;
        }
        Some((entry.image.clone()?, entry.size))
    }

    /// Take back the state from a finished background job.
    pub fn finish(&mut self, job: HighlightJob) {
        let Some(entry) = self.entries.get_mut(&job.key) else {
            return;
        };
        if entry.language != job.highlight.language() {
            return;
        }
        entry.in_flight = false;
        entry.highlighted = job.code;
        entry.highlight = Some(job.highlight);
        entry.runs = None;
    }

    /// Text runs for a code block.
    fn runs(&mut self, code: &PreparedCode, style: &MarkdownStyle) -> Rc<Vec<TextRun>> {
        let language = code.fence.highlight_language().to_string();
        let mono = font(style.mono_font_family.clone());
        let plain = |len: usize| TextRun {
            len,
            font: mono.clone(),
            color: style.syntax.plain,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let entry = self
            .entries
            .entry(code.key)
            .or_insert_with(|| CodeEntry::new(language.clone()));
        if entry.language != language {
            *entry = CodeEntry::new(language.clone());
        }
        if let Some((text, runs)) = &entry.runs
            && *text == code.code
        {
            return runs.clone();
        }
        let Some(syntaxes) = highlight::syntaxes() else {
            self.needs_syntaxes = true;
            return Rc::new(vec![plain(code.code.len())]);
        };
        if entry.plain_only {
            let runs = Rc::new(vec![plain(code.code.len())]);
            entry.runs = Some((code.code.clone(), runs.clone()));
            return runs;
        }
        if entry.highlight.is_none() && !entry.in_flight {
            match CodeHighlight::new(syntaxes, &language) {
                Some(highlight) => entry.highlight = Some(highlight),
                None => {
                    entry.plain_only = true;
                    let runs = Rc::new(vec![plain(code.code.len())]);
                    entry.runs = Some((code.code.clone(), runs.clone()));
                    return runs;
                }
            }
        }
        if !entry.in_flight
            && let Some(highlight) = entry.highlight.as_mut()
        {
            let new_lines = new_line_count(&entry.highlighted, &code.code);
            if new_lines <= SYNC_LINES.min(self.budget) {
                self.budget -= new_lines;
                highlight.update(syntaxes, &code.code);
                entry.highlighted = code.code.clone();
                let runs = Rc::new(code_runs(&code.code, highlight, &mono, style));
                entry.runs = Some((code.code.clone(), runs.clone()));
                return runs;
            }
            // Too much for this frame: hand the state to a background job.
            if let Some(highlight) = entry.highlight.take() {
                entry.in_flight = true;
                self.jobs.push(HighlightJob {
                    key: code.key,
                    highlight,
                    code: code.code.clone(),
                });
            }
        }
        // Show the colors that still apply, then plain text.
        match &entry.runs {
            Some((old, runs)) if code.code.starts_with(old.as_ref()) => {
                let mut runs = runs.as_ref().clone();
                runs.push(plain(code.code.len() - old.len()));
                runs.retain(|run| run.len > 0);
                if runs.is_empty() {
                    runs.push(plain(0));
                }
                Rc::new(runs)
            }
            _ => Rc::new(vec![plain(code.code.len())]),
        }
    }
}

fn new_line_count(highlighted: &str, code: &str) -> usize {
    let fresh = code.strip_prefix(highlighted).unwrap_or(code);
    fresh.bytes().filter(|b| *b == b'\n').count() + 1
}

/// Runs that cover `code` exactly, colored by highlight tokens.
fn code_runs(
    code: &str,
    highlight: &CodeHighlight,
    mono: &gpui::Font,
    style: &MarkdownStyle,
) -> Vec<TextRun> {
    let run = |len: usize, color: Hsla| TextRun {
        len,
        font: mono.clone(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs: Vec<TextRun> = Vec::new();
    let mut push = |len: usize, color: Hsla| {
        if len == 0 {
            return;
        }
        match runs.last_mut() {
            Some(last) if last.color == color => last.len += len,
            _ => runs.push(run(len, color)),
        }
    };
    let plain = style.syntax.plain;
    let mut lines = code.split('\n').enumerate().peekable();
    while let Some((ix, line)) = lines.next() {
        let mut at = 0;
        for token in highlight.line(ix) {
            let start = token.range.start.min(line.len());
            let end = token.range.end.min(line.len());
            if start < at || start >= end {
                continue;
            }
            push(start - at, plain);
            push(end - start, token.kind.color(&style.syntax));
            at = end;
        }
        push(line.len() - at, plain);
        if lines.peek().is_some() {
            push(1, plain);
        }
    }
    if runs.is_empty() {
        runs.push(run(0, plain));
    }
    runs
}

/// Per-frame inputs shared by every element builder.
pub(crate) struct Frame<'a> {
    pub style: &'a MarkdownStyle,
    pub now: Instant,
    /// Set while words fade.
    pub timeline: Option<&'a RevealTimeline>,
    pub registry: &'a Rc<RefCell<Registry>>,
    pub hovered_link: Option<(ElementKey, usize)>,
    pub code: &'a mut CodeState,
    pub view: WeakEntity<MarkdownView>,
    pub images: Option<&'a ImageResolver>,
    pub text_system: Arc<gpui::WindowTextSystem>,
}

/// Where each block sat in the last frame, relative to the top of the view.
///
/// A long reply lays out every block on the first frame. After that, blocks
/// far outside the visible area (more than a viewport away) render as empty
/// boxes of their measured height, so a frame costs about one screen of text
/// however long the reply is. A block renders in full again when it changes,
/// when the view's width changes, or when it nears the visible area.
#[derive(Default)]
pub(crate) struct LayoutCache {
    /// The view's window bounds at the last prepaint.
    pub view: Option<Bounds<Pixels>>,
    /// The visible part of the view at the last prepaint, in view
    /// coordinates.
    pub visible: Option<Range<Pixels>>,
    pub blocks: Vec<Option<Geometry>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    /// [`PreparedBlock::id`] of the measured block.
    pub id: u64,
    pub top: Pixels,
    pub height: Pixels,
    /// The view width the block was measured at.
    pub width: Pixels,
}

impl LayoutCache {
    /// The height to stand in for block `ix`, if it may skip layout.
    fn skip_height(&self, ix: usize, block: &PreparedBlock) -> Option<Pixels> {
        let geometry = self.blocks.get(ix).copied().flatten()?;
        let view = self.view?;
        let visible = self.visible.clone()?;
        if geometry.id != block.id || geometry.width != view.size.width {
            return None;
        }
        let margin = (visible.end - visible.start).max(px(600.));
        let far = geometry.top + geometry.height < visible.start - margin
            || geometry.top > visible.end + margin;
        far.then_some(geometry.height)
    }

    fn record(&mut self, ix: usize, id: u64, bounds: Bounds<Pixels>) {
        let Some(view) = self.view else {
            return;
        };
        if self.blocks.len() <= ix {
            self.blocks.resize(ix + 1, None);
        }
        self.blocks[ix] = Some(Geometry {
            id,
            top: bounds.top() - view.top(),
            height: bounds.size.height,
            width: view.size.width,
        });
    }
}

/// Build the elements for a message.
pub(crate) fn render_blocks(
    blocks: &[Rc<PreparedBlock>],
    layout: &Rc<RefCell<LayoutCache>>,
    frame: &mut Frame,
) -> Vec<AnyElement> {
    layout.borrow_mut().blocks.truncate(blocks.len());
    let mut out = Vec::with_capacity(blocks.len());
    let mut previous: Option<Pixels> = None;
    for (ix, block) in blocks.iter().enumerate() {
        let margins = block.node.margins(frame.style);
        let gap = previous.map(|bottom| bottom.max(margins.top));
        let skip = layout.borrow().skip_height(ix, block);
        let measure = {
            let layout = layout.clone();
            let id = block.id;
            canvas(
                move |bounds, _, _| layout.borrow_mut().record(ix, id, bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };
        let inner = match skip {
            Some(height) => {
                // Not laid out: keep the height and the text for selection.
                let registry = frame.registry.clone();
                let elements = block.selection_elements();
                div().relative().w_full().h(height).child(measure).child(
                    canvas(
                        move |_, _, _| {
                            let mut registry = registry.borrow_mut();
                            for element in elements.iter() {
                                registry.push(element.clone(), None, Arc::from(Vec::new()));
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_0(),
                )
            }
            None => div()
                .relative()
                .w_full()
                .min_w_0()
                .child(render_node(&block.node, frame))
                .child(measure),
        };
        out.push(
            div()
                .w_full()
                .min_w_0()
                .when_some(gap, |el, gap| el.mt(gap))
                .child(inner)
                .into_any_element(),
        );
        previous = Some(margins.bottom);
    }
    out
}

fn render_node(node: &PreparedNode, frame: &mut Frame) -> AnyElement {
    match node {
        PreparedNode::Text { segments, .. } => render_segments(segments, frame),
        PreparedNode::Code(code) => render_code(code, frame),
        PreparedNode::Quote(children) => render_quote(children, frame),
        PreparedNode::List { start, items } => render_list(*start, items, frame),
        PreparedNode::Table(table) => render_table(table, frame),
        PreparedNode::Rule => div()
            .w_full()
            .h(px(1.))
            .bg(frame.style.rule)
            .into_any_element(),
    }
}

fn render_segments(segments: &[Segment], frame: &mut Frame) -> AnyElement {
    if let [Segment::Text(text)] = segments {
        return render_text(text, frame);
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .children(segments.iter().map(|segment| match segment {
            Segment::Text(text) => render_text(text, frame),
            Segment::Image(image, key) => render_image(image, *key, frame),
        }))
        .into_any_element()
}

/// Runs for a text element this frame: fade and link hover applied.
fn frame_runs(text: &PreparedText, frame: &Frame) -> Vec<TextRun> {
    let hovered = frame
        .hovered_link
        .filter(|(key, _)| *key == text.key)
        .and_then(|(_, ix)| text.links.get(ix));
    let fading = frame.timeline.filter(|timeline| {
        let fading = timeline.fading_range();
        text.src_range
            .as_ref()
            .is_some_and(|src| src.end > fading.start && src.start < fading.end)
    });
    if hovered.is_none() && fading.is_none() {
        return text.runs.clone();
    }
    let mut out = Vec::with_capacity(text.runs.len() + 4);
    let mut at = 0;
    for (run, src) in text.runs.iter().zip(&text.run_src) {
        let start = at;
        at += run.len;
        let mut pieces: Vec<(usize, usize, f32)> = Vec::new();
        if let (Some(timeline), Some(src)) = (fading, src) {
            pieces = timeline.pieces(*src, run.len, frame.now);
        }
        // Split at fade pieces; each piece keeps the run's font.
        let mut offset = 0;
        let emit = |len: usize, opacity: f32, out: &mut Vec<TextRun>, from: usize| {
            if len == 0 {
                return;
            }
            let mut piece = run.clone();
            piece.len = len;
            if let Some(link) = hovered
                && from >= link.range.start
                && from + len <= link.range.end
            {
                piece.color = frame.style.link_hover;
                piece.underline = Some(UnderlineStyle {
                    color: Some(frame.style.link_hover),
                    thickness: px(1.),
                    wavy: false,
                });
            }
            if opacity < 1. {
                piece.color = piece.color.opacity(opacity);
                if let Some(strike) = &mut piece.strikethrough {
                    strike.color = strike.color.map(|c| c.opacity(opacity));
                }
            }
            out.push(piece);
        };
        for (piece_offset, len, opacity) in pieces {
            emit(piece_offset - offset, 1., &mut out, start + offset);
            emit(len, opacity, &mut out, start + piece_offset);
            offset = piece_offset + len;
        }
        emit(run.len - offset, 1., &mut out, start + offset);
    }
    out
}

fn render_text(text: &PreparedText, frame: &mut Frame) -> AnyElement {
    let runs = frame_runs(text, frame);
    let styled = StyledText::new(text.text.clone()).with_runs(runs);
    let layout = styled.layout().clone();

    // Under the text: inline code chips and the selection wash.
    let under_layout = layout.clone();
    let registry = frame.registry.clone();
    let key = text.key;
    let code_ranges = text.code_ranges.clone();
    let code_wash = frame.style.inline_code_background;
    let code_radius = frame.style.inline_code_radius;
    let selection_wash = frame.style.selection;
    let underlay = canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            for range in &code_ranges {
                for rect in range_rects(&under_layout, range, 0., INLINE_CODE_INSET_Y) {
                    window.paint_quad(quad(
                        rect,
                        code_radius,
                        code_wash,
                        px(0.),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            let selected = registry.borrow_mut().range_for(key);
            if let Some(range) = selected {
                for rect in range_rects(&under_layout, &range, 0., 0.) {
                    window.paint_quad(gpui::fill(rect, selection_wash));
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();

    // Over the text: register for selection and give links a hand cursor.
    let element = Element {
        key,
        text: text.text.clone(),
        hidden: text.hidden.clone(),
        separator: text.separator,
        line_mode: false,
    };
    let overlay = register_overlay(frame.registry.clone(), element, layout, text.links.clone());

    div()
        .relative()
        .min_w_0()
        .text_size(text.size)
        .line_height(text.line_height)
        .when(text.nowrap, |el| el.whitespace_nowrap())
        .cursor_text()
        .child(underlay)
        .child(styled)
        .child(overlay)
        .into_any_element()
}

/// A canvas that registers a text element during prepaint and sets the
/// pointer cursor over its links.
fn register_overlay(
    registry: Rc<RefCell<Registry>>,
    element: Element,
    layout: TextLayout,
    links: Arc<[LinkRange]>,
) -> impl IntoElement {
    let paint_links = links.clone();
    let paint_layout = layout.clone();
    canvas(
        move |_, window, _| {
            let mut hitboxes: Vec<Hitbox> = Vec::new();
            for link in paint_links.iter().filter(|link| !link.pending) {
                for rect in range_rects(&paint_layout, &link.range, 0., 0.) {
                    hitboxes.push(window.insert_hitbox(rect, HitboxBehavior::Normal));
                }
            }
            registry.borrow_mut().push(element, Some(layout), links);
            hitboxes
        },
        |_, hitboxes, window, _| {
            for hitbox in &hitboxes {
                window.set_cursor_style(CursorStyle::PointingHand, hitbox);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn render_image(image: &Arc<ImageRef>, key: ElementKey, frame: &mut Frame) -> AnyElement {
    let style = frame.style;
    let alt: SharedString = if image.alt.is_empty() {
        image.url.clone().into()
    } else {
        image.alt.clone().into()
    };
    let resolved = if image.url == crate::parse::mend::PENDING_LINK_URL {
        None
    } else {
        match frame.images {
            Some(resolve) => resolve(&image.url),
            None => default_image_source(&image.url),
        }
    };
    let Some(source) = resolved else {
        return div()
            .text_color(style.code_label)
            .italic()
            .child(alt)
            .into_any_element();
    };
    let fallback_alt = alt.clone();
    let fallback_color = style.code_label;
    let loading_bg = style.code_background;
    let max_h = style.image_max_height;
    div()
        .id(("md-image", key.0))
        .max_w_full()
        .child(
            img(source)
                .max_w_full()
                .max_h(max_h)
                .rounded(style.image_radius)
                .object_fit(ObjectFit::Contain)
                .with_loading(move || {
                    div()
                        .w(px(160.))
                        .h(px(96.))
                        .rounded(px(8.))
                        .bg(loading_bg)
                        .into_any_element()
                })
                .with_fallback(move || {
                    div()
                        .text_color(fallback_color)
                        .italic()
                        .child(fallback_alt.clone())
                        .into_any_element()
                }),
        )
        .into_any_element()
}

/// The default resolver: web URLs load over HTTP, absolute paths and
/// `file://` URLs load from disk, and `data:image/…;base64,` URLs decode in
/// place. Relative paths and other schemes do not render.
pub fn default_image_source(url: &str) -> Option<ImageSource> {
    let url = url.trim();
    if url.starts_with("https://") || url.starts_with("http://") {
        return Some(ImageSource::from(url));
    }
    if let Some(path) = url.strip_prefix("file://") {
        return Some(ImageSource::from(std::path::PathBuf::from(path)));
    }
    if url.starts_with('/') {
        return Some(ImageSource::from(std::path::PathBuf::from(url)));
    }
    if let Some(home) = url.strip_prefix("~/")
        && let Some(dir) = std::env::var_os("HOME")
    {
        return Some(ImageSource::from(std::path::PathBuf::from(dir).join(home)));
    }
    if let Some(rest) = url.strip_prefix("data:image/") {
        let (meta, data) = rest.split_once(',')?;
        let (kind, encoding) = meta.split_once(';')?;
        if encoding != "base64" {
            return None;
        }
        let format = match kind {
            "png" => gpui::ImageFormat::Png,
            "jpeg" | "jpg" => gpui::ImageFormat::Jpeg,
            "gif" => gpui::ImageFormat::Gif,
            "webp" => gpui::ImageFormat::Webp,
            "svg+xml" => gpui::ImageFormat::Svg,
            "bmp" => gpui::ImageFormat::Bmp,
            _ => return None,
        };
        let bytes = decode_base64(data)?;
        return Some(ImageSource::from(Arc::new(gpui::Image::from_bytes(
            format, bytes,
        ))));
    }
    None
}

fn decode_base64(data: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for byte in data.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | value(byte)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn render_quote(children: &[PreparedNode], frame: &mut Frame) -> AnyElement {
    let style = frame.style;
    let mut previous: Option<Pixels> = None;
    let mut elements = Vec::with_capacity(children.len());
    for child in children {
        let margins = nested_margins(child, style);
        let gap = previous.map(|bottom: Pixels| bottom.max(margins.top));
        elements.push(
            div()
                .when_some(gap, |el, gap| el.mt(gap))
                .child(render_node(child, frame))
                .into_any_element(),
        );
        previous = Some(margins.bottom);
    }
    div()
        .w_full()
        .border_l(style.quote_border_width)
        .border_color(style.quote_border)
        .pl(style.quote_padding)
        .flex()
        .flex_col()
        .children(elements)
        .into_any_element()
}

/// Margins for blocks inside quotes and list items: paragraphs after the
/// first get `1rem`, lists none, and the rest keep their own.
fn nested_margins(node: &PreparedNode, style: &MarkdownStyle) -> crate::style::BlockMargins {
    match node {
        PreparedNode::List { .. } => crate::style::BlockMargins::new(0., 0.),
        _ => node.margins(style),
    }
}

fn render_list(start: Option<u64>, items: &[PreparedItem], frame: &mut Frame) -> AnyElement {
    let style = frame.style;
    let mut rows = Vec::with_capacity(items.len());
    for (ix, item) in items.iter().enumerate() {
        let marker = list_marker(start, ix, item.task, style);
        let mut previous: Option<Pixels> = None;
        let mut children = Vec::with_capacity(item.blocks.len());
        for block in &item.blocks {
            // Paragraphs in list items sit inline (`[&>p]:inline`), so they
            // add no margin of their own.
            let margins = match block {
                PreparedNode::Text { heading: None, .. } => crate::style::BlockMargins::new(0., 0.),
                other => nested_margins(other, style),
            };
            let gap = previous.map(|bottom: Pixels| bottom.max(margins.top));
            children.push(
                div()
                    .min_w_0()
                    .when_some(gap, |el, gap| el.mt(gap))
                    .child(render_node(block, frame))
                    .into_any_element(),
            );
            previous = Some(margins.bottom);
        }
        rows.push(
            div()
                .flex()
                .flex_row()
                .py(style.list_item_padding)
                .child(marker)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .children(children),
                )
                .into_any_element(),
        );
    }
    div()
        .w_full()
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

fn list_marker(
    start: Option<u64>,
    ix: usize,
    task: Option<bool>,
    style: &MarkdownStyle,
) -> AnyElement {
    let gutter = div()
        .flex_none()
        .w(style.list_indent)
        .h(style.line_height)
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .pr(px(8.));
    if let Some(checked) = task {
        return gutter.child(checkbox(checked, style)).into_any_element();
    }
    let label: SharedString = match start {
        Some(first) => format!("{}.", first + ix as u64).into(),
        None => "•".into(),
    };
    gutter
        .text_size(style.text_size)
        .line_height(style.line_height)
        .text_color(style.body_text)
        .child(label)
        .into_any_element()
}

fn checkbox(checked: bool, style: &MarkdownStyle) -> AnyElement {
    let check = style.checkbox_check;
    div()
        .size(px(14.))
        .rounded(px(3.))
        .border_1()
        .border_color(if checked {
            style.checkbox_checked
        } else {
            style.checkbox_border
        })
        .when(checked, |el| {
            el.bg(style.checkbox_checked).child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        let o = bounds.origin;
                        let mut path = PathBuilder::stroke(px(1.6));
                        path.move_to(point(o.x + px(2.6), o.y + px(6.4)));
                        path.line_to(point(o.x + px(5.2), o.y + px(9.)));
                        path.line_to(point(o.x + px(9.8), o.y + px(3.6)));
                        if let Ok(path) = path.build() {
                            window.paint_path(path, check);
                        }
                    },
                )
                .size_full(),
            )
        })
        .into_any_element()
}

fn render_table(table: &PreparedTable, frame: &mut Frame) -> AnyElement {
    let style = frame.style;
    let columns = table.columns.max(1);
    let naturals = table
        .naturals
        .get_or_init(|| measure_columns(table, columns, &frame.text_system));
    let pad_x = f32::from(style.table_cell_padding_x);
    // Column floors, the way the web table shrinks to its min-content.
    let minimums: Vec<f32> = naturals.iter().map(|n| n.min(96.) + 2. * pad_x).collect();
    let min_width: f32 = minimums.iter().sum();
    let row_count = table.rows.len();
    let mut rows = Vec::with_capacity(row_count);
    for (r, row) in table.rows.iter().enumerate() {
        let header = table.has_header && r == 0;
        let mut cells = Vec::with_capacity(columns);
        for c in 0..columns {
            let grow = naturals.get(c).copied().unwrap_or(1.).max(1.);
            let mut cell = div()
                .flex_grow(1.)
                .flex_basis(px(0.))
                .min_w(px(minimums.get(c).copied().unwrap_or(48.)))
                .px(style.table_cell_padding_x)
                .py(style.table_cell_padding_y)
                .text_size(style.table_text_size)
                .line_height(style.table_line_height);
            cell.style().flex_grow = Some(grow);
            cell.style().flex_shrink = Some(grow);
            cell = match table.align.get(c).copied().unwrap_or_default() {
                Align::Center => cell.text_center(),
                Align::Right => cell.text_right(),
                Align::Left | Align::None => cell,
            };
            if let Some(text) = row.get(c) {
                cell = cell.child(render_text(text, frame));
            }
            cells.push(cell);
        }
        rows.push(
            div()
                .flex()
                .flex_row()
                .when(r + 1 < row_count, |el| {
                    el.border_b_1().border_color(style.table_row_border)
                })
                .when(header, |el| el.font_weight(FontWeight::SEMIBOLD))
                .children(cells),
        );
    }
    let first_key = table
        .rows
        .first()
        .and_then(|row| row.first())
        .map_or(0, |text| text.key.0);
    let mut scroller = div()
        .id(ElementId::NamedInteger("md-table".into(), first_key))
        .w_full()
        .overflow_x_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .min_w(px(min_width))
                .children(rows),
        );
    scroller.style().restrict_scroll_to_axis = Some(true);
    div()
        .w_full()
        .rounded(style.table_radius)
        .border_1()
        .border_color(style.table_border)
        .bg(style.table_background)
        .overflow_hidden()
        .child(scroller)
        .into_any_element()
}

/// Unwrapped content width per column, shaped once per prepared table.
fn measure_columns(
    table: &PreparedTable,
    columns: usize,
    text_system: &gpui::WindowTextSystem,
) -> Vec<f32> {
    let mut widths = vec![24f32; columns];
    for row in &table.rows {
        for (c, text) in row.iter().enumerate().take(columns) {
            if text.text.is_empty() {
                continue;
            }
            // `shape_line` takes one line; a newline and a space are both
            // one byte, so the runs still fit.
            let line: SharedString = if text.text.contains('\n') {
                text.text.replace('\n', " ").into()
            } else {
                text.text.clone()
            };
            let width = text_system
                .shape_line(line, text.size, &text.runs, None)
                .width;
            widths[c] = widths[c].max(f32::from(width));
        }
    }
    widths
}

fn render_code(code: &PreparedCode, frame: &mut Frame) -> AnyElement {
    let style = frame.style;
    if code.fence.is_mermaid()
        && let Some((image, (width, height))) = frame.code.diagram(code, style.text.l > 0.5)
    {
        frame.registry.borrow_mut().push(
            Element {
                key: code.key,
                text: code.code.clone(),
                hidden: Vec::new(),
                separator: crate::selection::Separator::Paragraph,
                line_mode: true,
            },
            None,
            Arc::from(Vec::new()),
        );
        let scale = (600. / height).min(1.);
        return div()
            .w_full()
            .min_w_0()
            .rounded(style.code_radius)
            .border_1()
            .border_color(style.code_border)
            .bg(style.code_background)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .min_h(style.code_header_height)
                    .text_size(style.code_label_size)
                    .text_color(style.code_label)
                    .child("Mermaid")
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(diagram_toggle(code.key, "Source", frame))
                            .child(copy_button(
                                code,
                                frame.code.copied == Some(code.key),
                                frame,
                            )),
                    ),
            )
            .child(
                div().flex().justify_center().p(px(12.)).child(
                    img(image)
                        .w(px(width * scale))
                        .h(px(height * scale))
                        .max_w_full()
                        .object_fit(ObjectFit::Contain),
                ),
            )
            .into_any_element();
    }
    let runs = frame.code.runs(code, style);
    let copied = frame.code.copied == Some(code.key);

    let styled = StyledText::new(code.code.clone()).with_runs(runs.as_ref().clone());
    let layout = styled.layout().clone();
    let registry = frame.registry.clone();
    let key = code.key;
    let selection_wash = style.selection;
    let under_layout = layout.clone();
    let underlay = canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            let selected = registry.borrow_mut().range_for(key);
            if let Some(range) = selected {
                for rect in range_rects(&under_layout, &range, 0., 0.) {
                    window.paint_quad(gpui::fill(rect, selection_wash));
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let overlay = register_overlay(
        frame.registry.clone(),
        Element {
            key,
            text: code.code.clone(),
            hidden: Vec::new(),
            separator: crate::selection::Separator::Paragraph,
            line_mode: true,
        },
        layout,
        Arc::from(Vec::new()),
    );

    let fence = &code.fence;
    let line_numbers = fence.line_numbers && !fence.is_mermaid();
    let numbers = line_numbers.then(|| {
        let first = fence.start_line.unwrap_or(1).max(1) as usize;
        let mut text = String::with_capacity(code.line_count * 4);
        for n in 0..code.line_count {
            if n > 0 {
                text.push('\n');
            }
            text.push_str(&(first + n).to_string());
        }
        div()
            .flex_none()
            .w(style.line_number_width)
            .mr(px(8.))
            .text_right()
            .font_family(style.mono_font_family.clone())
            .text_size(style.line_number_size)
            .line_height(style.code_line_height)
            .text_color(style.line_number)
            .child(SharedString::from(text))
    });

    let code_text = div()
        .relative()
        .flex_none()
        .whitespace_nowrap()
        .font_family(style.mono_font_family.clone())
        .text_size(style.code_size)
        .line_height(style.code_line_height)
        .cursor_text()
        .child(underlay)
        .child(styled)
        .child(overlay);

    let mut body = div()
        .id(ElementId::NamedInteger("md-code".into(), key.0))
        .w_full()
        .overflow_x_scroll()
        .border_t_1()
        .border_color(style.code_border)
        .child(
            div()
                .flex()
                .flex_row()
                .min_w_full()
                .pt(px(10.))
                .pb(px(10.))
                .pr(px(8.))
                .when(!line_numbers, |el| el.pl(px(12.)))
                .children(numbers)
                .child(code_text),
        );
    body.style().restrict_scroll_to_axis = Some(true);

    let label = fence.label().to_string();
    let mono_label = fence.file_path.is_some() || fence.is_plaintext();
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .min_h(style.code_header_height)
        .pl(px(12.))
        .pr(px(6.))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(style.code_label_size)
                .font_weight(FontWeight::MEDIUM)
                .text_color(style.code_label)
                .when(mono_label, |el| {
                    el.font_family(style.mono_font_family.clone())
                })
                .child(SharedString::from(if mono_label {
                    label.to_lowercase()
                } else {
                    label
                })),
        )
        .child(
            div()
                .flex()
                .items_center()
                .when(
                    code.fence.is_mermaid()
                        && frame
                            .code
                            .diagrams
                            .get(&key)
                            .is_some_and(|entry| entry.image.is_some()),
                    |el| el.child(diagram_toggle(key, "Diagram", frame)),
                )
                .child(copy_button(code, copied, frame)),
        );

    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .rounded(style.code_radius)
        .border_1()
        .border_color(style.code_border)
        .bg(style.code_background)
        .overflow_hidden()
        .child(header)
        .child(body)
        .into_any_element()
}

fn diagram_toggle(key: ElementKey, label: &'static str, frame: &Frame) -> AnyElement {
    let view = frame.view.clone();
    div()
        .id(ElementId::NamedInteger("md-diagram-source".into(), key.0))
        .cursor_pointer()
        .px(px(8.))
        .py(px(4.))
        .text_size(frame.style.code_label_size)
        .text_color(frame.style.code_label)
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let _ = view.update(cx, |view, cx| view.toggle_diagram_source(key, cx));
        })
        .child(label)
        .into_any_element()
}

fn copy_button(code: &PreparedCode, copied: bool, frame: &Frame) -> AnyElement {
    let style = frame.style;
    let view = frame.view.clone();
    let key = code.key;
    let text = code.code.clone();
    let group: SharedString = format!("md-copy-{}", key.0).into();
    let icon_color = style.copy_icon;
    let icon_hover = style.copy_icon_hover;
    div()
        .id(ElementId::NamedInteger("md-copy".into(), key.0))
        .group(group.clone())
        .flex_none()
        .size(px(24.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|el| el.bg(style.copy_hover_background))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx: &mut App| {
            cx.stop_propagation();
            let copy = text.trim_end_matches(['\n', '\r']).to_string();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy));
            let _ = view.update(cx, |view, cx| view.mark_copied(key, cx));
        })
        .child(if copied {
            check_icon(icon_hover)
        } else {
            copy_icon(icon_color, icon_hover, group)
        })
        .into_any_element()
}

/// The two-page copy icon from AgentMarkdown's `CodeCopyButton`, drawn at
/// 15px inside the 24px button.
fn copy_icon(color: Hsla, hover: Hsla, group: SharedString) -> AnyElement {
    let stroke = px(1.2);
    div()
        .relative()
        .size(px(24.))
        .child(
            // Back page: an L from the top right to the bottom left.
            div()
                .absolute()
                .left(px(4.5 + 2.4))
                .top(px(4.5 + 2.4))
                .w(px(6.6))
                .h(px(6.6))
                .border_t(stroke)
                .border_l(stroke)
                .rounded_tl(px(1.5))
                .border_color(color)
                .group_hover(group.clone(), |el| el.border_color(hover)),
        )
        .child(
            // Front page.
            div()
                .absolute()
                .left(px(4.5 + 4.65))
                .top(px(4.5 + 4.65))
                .size(px(7.95))
                .border(stroke)
                .rounded(px(1.5))
                .border_color(color)
                .group_hover(group, |el| el.border_color(hover)),
        )
        .into_any_element()
}

fn check_icon(color: Hsla) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let o = bounds.origin + point(px(4.5), px(4.5));
            let mut path = PathBuilder::stroke(px(1.2));
            path.move_to(point(o.x + px(3.375), o.y + px(7.875)));
            path.line_to(point(o.x + px(6.15), o.y + px(10.5)));
            path.line_to(point(o.x + px(11.625), o.y + px(4.875)));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(px(24.))
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_mermaid_diagrams_with_native_svg_text_and_geometry() {
        for source in [
            "flowchart LR\n A[Start] --> B[Done]",
            "sequenceDiagram\n Alice->>Bob: Hello",
            "classDiagram\n Animal <|-- Duck",
        ] {
            let svg = render_diagram(source, true).unwrap();
            assert!(svg.contains("<svg"));
            assert!(svg.contains("<text"));
            let (width, height) = diagram_size(&svg);
            assert!(width > 0. && height > 0.);
        }
        assert!(render_diagram("this is not a diagram", false).is_none());
    }

    #[test]
    fn coalesces_streaming_diagram_updates_until_the_worker_finishes() {
        let mut state = CodeState::default();
        let mut code = PreparedCode {
            key: ElementKey(0),
            fence: crate::parse::CodeFence::parse("mermaid"),
            code: "flowchart LR; A-->B".into(),
            line_count: 1,
        };
        assert!(state.diagram(&code, true).is_none());
        let first = state.diagram_jobs.pop().unwrap();
        code.code = "flowchart LR; A-->B-->C".into();
        assert!(state.diagram(&code, true).is_none());
        assert!(state.diagram_jobs.is_empty());
        state.finish_diagram(first.run());
        assert!(state.diagram(&code, true).is_none());
        let next = state.diagram_jobs.pop().unwrap();
        state.finish_diagram(next.run());
        assert!(state.diagram(&code, true).is_some());
        state.toggle_diagram_source(code.key);
        assert!(state.diagram(&code, true).is_none());
        assert!(state.diagram_jobs.is_empty());
    }

    /// codeHighlightPlugin.test.ts: the Shiki plugin cached every partial
    /// version of a streaming fence in a global map. Here each block keeps
    /// only its latest highlight, keyed by its place in the message and
    /// checked against its exact code.
    #[test]
    fn a_streaming_fence_keeps_one_highlight_per_block() {
        highlight::syntaxes_blocking();
        let style = MarkdownStyle::default();
        let mut state = CodeState::default();
        let source = (0..40)
            .map(|i| format!("const value{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut code = PreparedCode {
            key: ElementKey(0),
            fence: crate::parse::CodeFence::parse("ts"),
            code: SharedString::default(),
            line_count: 0,
        };
        let mut end = 20;
        while end <= source.len() {
            state.budget = usize::MAX;
            code.code = source[..end].to_string().into();
            let runs = state.runs(&code, &style);
            assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), end);
            end += 20;
        }
        assert_eq!(state.entries.len(), 1);

        // Two blocks of the same length, head, and tail keep their own runs.
        let head = "const a = 1;\n".repeat(10);
        let tail = "\nconst z = 26;".repeat(10);
        let first: SharedString = format!("{head}let middle = \"one\";{tail}").into();
        let second: SharedString = format!("{head}let middle = 2.000;{tail}").into();
        assert_eq!(first.len(), second.len());
        for (ix, text) in [(1, &first), (2, &second)] {
            state.budget = usize::MAX;
            let block = PreparedCode {
                key: ElementKey(ix),
                fence: crate::parse::CodeFence::parse("ts"),
                code: text.clone(),
                line_count: 21,
            };
            // 21 lines is past the frame's sync limit, so the block goes to a
            // background job. Finish it the way the view does, then cache.
            state.runs(&block, &style);
            let jobs = std::mem::take(&mut state.jobs);
            assert_eq!(jobs.len(), 1);
            for job in jobs {
                state.finish(job.run());
            }
            state.runs(&block, &style);
        }
        let cached = |ix| {
            state.entries[&ElementKey(ix)]
                .runs
                .as_ref()
                .map(|(text, runs)| (text.clone(), runs.clone()))
                .unwrap()
        };
        let (first_text, first_runs) = cached(1);
        let (second_text, second_runs) = cached(2);
        assert_eq!(first_text, first);
        assert_eq!(second_text, second);
        assert_ne!(first_runs, second_runs);
    }

    #[test]
    fn base64_decodes() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("aGk").unwrap(), b"hi");
        assert!(decode_base64("a*b").is_none());
    }

    #[test]
    fn default_images_resolve_known_sources() {
        assert!(default_image_source("https://x.dev/a.png").is_some());
        assert!(default_image_source("/tmp/a.png").is_some());
        assert!(default_image_source("file:///tmp/a.png").is_some());
        assert!(default_image_source("data:image/png;base64,aGk=").is_some());
        assert!(default_image_source("relative/a.png").is_none());
        assert!(default_image_source("javascript:alert(1)").is_none());
    }
}
