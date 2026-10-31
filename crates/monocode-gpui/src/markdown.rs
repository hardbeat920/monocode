//! GPUI port of AgentMarkdown.tsx and its .agent-markdown CSS.
mod cache;
pub(crate) mod mermaid;
mod search;
use crate::markdown_model::{Block, InlineRun, TableAlign};
use crate::syntax::HighlightedDocument;
use crate::theme::*;
use crate::transcript::selection::TextSelection;
pub(crate) use cache::{PreparedRow, RenderCache};
use gpui::Window;
use gpui::{
    AnyElement, App, ClickEvent, Div, FontStyle, FontWeight, HighlightStyle, InteractiveText,
    MouseButton, Pixels, Point, Role, SharedString, StrikethroughStyle, StyledText, div,
    prelude::*, px, rgb,
};
pub(crate) use search::{mark_row, mark_styles, row_text};

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = markdown, no_json)]
pub(crate) struct LinkMenu {
    pub url: String,
    pub position: Point<Pixels>,
}

pub(crate) fn link_menu(url: &str, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    window.dispatch_action(
        Box::new(LinkMenu {
            url: url.into(),
            position,
        }),
        cx,
    );
}

fn activate_link(event: &ClickEvent, selection: Option<&TextSelection>) -> bool {
    event.click_count() == 1
        && !event.modifiers().shift
        && !selection.is_some_and(TextSelection::has_selection)
}
use std::{
    cell::RefCell,
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    rc::Rc,
    sync::Arc,
    time::Instant,
};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tone {
    Normal,
    Work,
    Reasoning,
}
impl Tone {
    fn prose(self) -> f32 {
        match self {
            Self::Normal => 0.78,
            Self::Work => 0.65,
            Self::Reasoning => 0.48,
        }
    }
    fn strong(self) -> f32 {
        match self {
            Self::Normal => 1.,
            Self::Work => 0.8,
            Self::Reasoning => 0.62,
        }
    }
}
const PART_BYTES: usize = 2048;
const PART_LINES: usize = 24;

#[derive(Clone, PartialEq)]
pub(crate) struct Fence {
    pub code: SharedString,
    pub language: String,
    pub label: String,
    pub path: Option<String>,
    pub start_line: usize,
    pub numbered: bool,
    pub lines: Vec<Range<usize>>,
    pub key: u64,
}
impl Fence {
    fn new(info: &str, code: &str) -> Self {
        let mut words = info.split_whitespace();
        let first = words.next().unwrap_or("");
        let mut language = first.to_owned();
        let mut label = first.to_owned();
        let mut path = None;
        let mut start_line = 1;
        let citation: Vec<_> = first.splitn(3, ':').collect();
        if citation.len() == 3
            && let (Ok(start), Ok(_)) = (citation[0].parse::<usize>(), citation[1].parse::<usize>())
        {
            start_line = start;
            label = citation[2].into();
            path = Some(label.clone());
            language = citation[2].rsplit('.').next().unwrap_or("").into();
        } else if first.contains('/') || first.contains('\\') {
            language = first.rsplit('.').next().unwrap_or("").into();
            path = Some(first.to_owned());
        }

        // Match parseCodeFence: language, optional path/title and startLine.
        for word in words {
            if let Some(value) = word.strip_prefix("startLine=") {
                start_line = value.parse().unwrap_or(1);
            } else if let Some(value) = word
                .strip_prefix("title=")
                .or_else(|| word.strip_prefix("file="))
            {
                label = value.trim_matches(['\'', '"']).to_owned();
                path = Some(label.clone());
            } else if word.contains('/') || word.contains('.') {
                label = word.trim_matches(['\'', '"']).to_owned();
                path = Some(label.clone());
            }
        }
        if matches!(language.as_str(), "" | "text" | "plaintext" | "txt") {
            language = "javascript".into();
        }
        let mut hash = DefaultHasher::new();
        language.hash(&mut hash);
        code.hash(&mut hash);
        let mut at = 0;
        let mut lines = Vec::new();
        for line in code.split_inclusive('\n') {
            let end = at + line.trim_end_matches(['\n', '\r']).len();
            lines.push(at..end);
            at += line.len();
        }
        if lines.is_empty() {
            lines.push(0..0);
        }
        Self {
            code: code.to_owned().into(),
            language,
            label,
            path,
            start_line,
            numbered: !info.contains("noLineNumbers"),
            lines,
            key: hash.finish(),
        }
    }
}

#[derive(Clone, PartialEq)]
pub(crate) enum Part {
    Text {
        runs: Vec<InlineRun>,
        heading: u8,
    },
    Code {
        fence: Arc<Fence>,
        lines: Range<usize>,
    },
    Mermaid {
        fence: Arc<Fence>,
    },
    Table {
        cells: Vec<Vec<Vec<InlineRun>>>,
        columns: Arc<[f32]>,
        align: Vec<TableAlign>,
        header: bool,
        last: bool,
    },
    Rule,
    Math(String),
    Visual {
        id: String,
        caption: String,
    },
}
#[derive(Clone, PartialEq)]
pub(crate) struct MarkdownRow {
    pub part: Part,
    pub indent: usize,
    pub quote: usize,
    pub marker: Option<String>,
    pub gap: f32,
    pub word_start: usize,
}

pub(crate) fn rows(block: &Block) -> Vec<MarkdownRow> {
    let mut rows = Vec::new();
    flatten(block, 0, 0, None, &mut rows);
    rows
}
fn flatten(
    block: &Block,
    indent: usize,
    quote: usize,
    marker: Option<String>,
    out: &mut Vec<MarkdownRow>,
) {
    let first = out.len();
    let mut add = |part, gap| {
        out.push(MarkdownRow {
            part,
            indent,
            quote,
            marker: None,
            gap,
            word_start: 0,
        })
    };
    match block {
        Block::Paragraph { runs } | Block::Heading { runs, .. } => {
            let heading = match block {
                Block::Heading { level, .. } => *level,
                _ => 0,
            };
            for (ix, runs) in split_runs(runs).into_iter().enumerate() {
                add(
                    Part::Text { runs, heading },
                    if ix == 0 {
                        if heading > 0 { 24. } else { 16. }
                    } else {
                        0.
                    },
                );
            }
        }
        Block::CodeBlock { language, code } => {
            let fence = Arc::new(Fence::new(language.as_deref().unwrap_or(""), code));
            if fence.language.eq_ignore_ascii_case("mermaid") {
                add(Part::Mermaid { fence }, 16.);
            } else {
                for at in (0..fence.lines.len()).step_by(PART_LINES) {
                    add(
                        Part::Code {
                            fence: fence.clone(),
                            lines: at..(at + PART_LINES).min(fence.lines.len()),
                        },
                        if at == 0 { 16. } else { 0. },
                    );
                }
            }
        }
        Block::List {
            ordered_start,
            items,
        } => {
            for (ix, children) in items.iter().enumerate() {
                let start = out.len();
                let bullet =
                    ordered_start.map_or_else(|| "•".into(), |n| format!("{}.", n + ix as u64));
                for child in children {
                    flatten(child, indent + 1, quote, None, out);
                }
                if let Some(row) = out.get_mut(start) {
                    row.marker = Some(bullet);
                    row.gap = if ix == 0 { 8. } else { 4. };
                }
            }
        }
        Block::BlockQuote { children } => {
            for child in children {
                flatten(child, indent, quote + 1, None, out);
            }
        }
        Block::Table {
            header,
            rows,
            align,
        } => {
            let all: Vec<_> = std::iter::once(header.clone())
                .chain(rows.iter().cloned())
                .collect();
            let columns = table_column_widths(&all);
            for (ix, cells) in all.chunks(PART_LINES).enumerate() {
                add(
                    Part::Table {
                        cells: cells.to_vec(),
                        columns: columns.clone(),
                        align: align.clone(),
                        header: ix == 0,
                        last: (ix + 1) * PART_LINES >= all.len(),
                    },
                    if ix == 0 { 16. } else { 0. },
                );
            }
        }
        Block::Rule => add(Part::Rule, 24.),
        Block::Math { tex } => add(Part::Math(tex.clone()), 16.),
        Block::Visual { id, caption } => add(
            Part::Visual {
                id: id.clone(),
                caption: caption.clone(),
            },
            16.,
        ),
    }
    if let Some(row) = out.get_mut(first)
        && marker.is_some()
    {
        row.marker = marker;
    }
}

fn table_column_widths(rows: &[Vec<Vec<InlineRun>>]) -> Arc<[f32]> {
    let count = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let mut widths = vec![160_f32; count];
    // These are preferred flex widths, not fixed cell widths. Sample the whole
    // table once so virtualized chunks use the same columns. Short labels keep
    // the stylesheet's 160px minimum; prose receives more of the available room.
    for row in rows {
        for (col, cell) in row.iter().enumerate() {
            let chars = runs_text(cell)
                .lines()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0);
            widths[col] = widths[col].max((chars as f32 * 6. + 20.).clamp(160., 480.));
        }
    }
    widths.into()
}

/// Split text while keeping every byte and its inline style; storage boundaries
/// never become paragraph, list, or code-fence boundaries.
fn split_runs(runs: &[InlineRun]) -> Vec<Vec<InlineRun>> {
    let mut parts = Vec::new();
    let mut part = Vec::new();
    let mut bytes = 0;
    let mut lines = 0;
    for run in runs {
        let mut text = run.text.as_str();
        while !text.is_empty() {
            if bytes >= PART_BYTES || lines >= 12 {
                parts.push(std::mem::take(&mut part));
                bytes = 0;
                lines = 0;
            }
            let mut end = text.len().min(PART_BYTES - bytes);
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            if end == 0 {
                parts.push(std::mem::take(&mut part));
                bytes = 0;
                lines = 0;
                continue;
            }
            if let Some((at, _)) = text[..end].match_indices('\n').nth(11 - lines) {
                end = at + 1;
            }
            let slice = &text[..end];
            part.push(InlineRun {
                text: slice.to_owned(),
                style: run.style.clone(),
            });
            bytes += end;
            lines += slice.matches('\n').count();
            text = &text[end..];
        }
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

pub(crate) fn word_count(row: &MarkdownRow) -> usize {
    let count = |runs: &[InlineRun]| {
        runs.iter()
            .filter(|r| !r.style.code && !r.style.math && r.style.link.is_none())
            .map(|r| r.text.split_whitespace().count())
            .sum::<usize>()
    };
    match &row.part {
        Part::Text { runs, .. } => count(runs),
        Part::Table { cells, .. } => cells.iter().flatten().map(|r| count(r)).sum(),
        _ => 0,
    }
}
#[allow(clippy::too_many_arguments)]
fn inline_selected(
    runs: &[InlineRun],
    id: SharedString,
    tone: Tone,
    fade: Option<&crate::reveal::WordFades>,
    now: Instant,
    word_base: usize,
    font_size: f32,
    selection: Option<TextSelection>,
    prepared: Option<&cache::PreparedInline>,
) -> AnyElement {
    if runs.iter().any(|r| r.style.code) {
        return rich_inline(runs, id, tone, fade, now, word_base, font_size, selection);
    }
    let uncached;
    let prepared = if let Some(prepared) = prepared {
        prepared
    } else {
        uncached = cache::PreparedInline::new(runs, false);
        &uncached
    };
    let mut highlights = Vec::new();
    for run in &prepared.runs {
        let style = &run.style;
        let color = if style.link.is_some() {
            accent_color()
        } else if style.bold || style.italic || style.code {
            content(tone.strong())
        } else {
            content(tone.prose())
        };
        let base_opacity = color.a;
        let mut opaque_color = color;
        opaque_color.a = 1.;
        let highlight = HighlightStyle {
            color: Some(opaque_color),
            background_color: style.search.map(search::search_background),
            fade_out: Some(1. - base_opacity),
            font_weight: style.bold.then_some(FontWeight::SEMIBOLD),
            font_style: style.italic.then_some(FontStyle::Italic),
            strikethrough: style.strikethrough.then_some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            }),
            ..Default::default()
        };
        if fade.is_some_and(|fade| {
            fade.intersects(word_base + run.word_start..word_base + run.word_start + run.words)
        }) {
            for (range, word) in &run.pieces {
                let mut highlight = highlight;
                if let Some(word) = word {
                    let alpha = fade.map_or(1., |fade| fade.opacity(word_base + *word));
                    highlight.fade_out = Some(1. - (1. - highlight.fade_out.unwrap_or(0.)) * alpha);
                }
                highlights.push((range.clone(), highlight));
            }
        } else {
            highlights.push((run.range.clone(), highlight));
        }
    }
    let text = prepared.text.clone();
    let links = prepared.links.clone();
    let urls = prepared.urls.clone();
    let styled = StyledText::new(text.clone())
        .with_highlights(highlights)
        .with_font_family_overrides(prepared.fonts.clone());
    let layout = styled.layout().clone();
    let clicked_selection = selection.clone();
    let clicked_layout = layout.clone();
    let menu_layout = layout.clone();
    let clicked_links = links.clone();
    let menu_links = links.clone();
    let clicked_urls = urls.clone();
    let intercept = link_intercept();
    let text_id: SharedString = format!("{id}-text").into();
    let child = div()
        .id(id)
        .child(InteractiveText::new(text_id, styled).on_click(links, |_, _, _| {}))
        .on_click(move |event, window, cx| {
            if !activate_link(event, clicked_selection.as_ref()) {
                return;
            }
            let Ok(byte) = clicked_layout.index_for_position(event.position()) else {
                return;
            };
            if let Some(ix) = clicked_links.iter().position(|range| range.contains(&byte)) {
                // A drag starting on another link must not activate this one.
                if let ClickEvent::Mouse(mouse) = event {
                    let Ok(down) = clicked_layout.index_for_position(mouse.down.position) else {
                        return;
                    };
                    if !clicked_links[ix].contains(&down) {
                        return;
                    }
                }
                open_link(
                    &clicked_urls[ix],
                    !event.modifiers().secondary(),
                    intercept.as_ref(),
                    window,
                    cx,
                );
            }
        })
        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
            if let Ok(byte) = menu_layout.index_for_position(event.position)
                && let Some(ix) = menu_links.iter().position(|range| range.contains(&byte))
            {
                link_menu(&urls[ix], event.position, window, cx);
                cx.stop_propagation();
            }
        })
        .into_any_element();
    if let Some(selection) = selection {
        selection.wrap(text, layout, child)
    } else {
        child
    }
}
#[allow(clippy::too_many_arguments)]
fn rich_inline(
    runs: &[InlineRun],
    id: SharedString,
    tone: Tone,
    fade: Option<&crate::reveal::WordFades>,
    _now: Instant,
    mut word: usize,
    font_size: f32,
    selection: Option<TextSelection>,
) -> AnyElement {
    let mut children = Vec::new();
    let mut offset = 0;
    for (run_ix, run) in runs.iter().enumerate() {
        let style = &run.style;
        if style.code {
            let text = run.text.clone();
            let file = is_file_chip(&text);
            let styled = StyledText::new(text.clone());
            let layout = styled.layout().clone();
            let selected = selection.as_ref().map(|s| {
                let selected = s.offset(offset);
                if file {
                    selected.with_cursor(gpui::CursorStyle::PointingHand)
                } else {
                    selected
                }
            });
            let clicked_selection = selected.clone();
            let child = styled.into_any_element();
            let child = if let Some(selected) = selected {
                selected.wrap(text.clone().into(), layout, child)
            } else {
                child
            };
            children.push(
                div()
                    .id(SharedString::from(format!("{id}-code-{run_ix}")))
                    .flex()
                    .items_center()
                    .gap_1()
                    .min_h(px(24.))
                    .max_w_full()
                    .px(px(6.))
                    .rounded(px(6.))
                    .bg(style
                        .search
                        .map(search::search_background)
                        .unwrap_or_else(|| content(0.08)))
                    .font_family(crate::fonts::mono())
                    .text_size(px(font_size * 0.8))
                    .line_height(px(24.))
                    .text_color(content(1.))
                    .when(file, |d| {
                        let path = style.link.clone().unwrap_or_else(|| text.clone());
                        let menu_path = path.clone();
                        d.cursor(gpui::CursorStyle::PointingHand)
                            .hover(|s| s.bg(content(0.13)))
                            .on_click(move |event, window, cx| {
                                if activate_link(event, clicked_selection.as_ref()) {
                                    open_link(&path, false, None, window, cx)
                                }
                            })
                            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                link_menu(&menu_path, event.position, window, cx);
                                cx.stop_propagation();
                            })
                            .child(crate::activity::file_icon(&text, 14.))
                    })
                    .child(child)
                    .into_any_element(),
            );
            offset += run.text.len();
            continue;
        }
        for (part_ix, part) in run.text.split_inclusive(char::is_whitespace).enumerate() {
            let text = part.trim_end_matches(char::is_whitespace);
            if !text.is_empty() {
                let alpha = if style.link.is_some() || style.math {
                    1.
                } else {
                    let value = fade.map_or(1., |fade| fade.opacity(word));
                    word += 1;
                    value
                };
                let color = if style.link.is_some() {
                    accent_color()
                } else if style.bold || style.italic {
                    content(tone.strong())
                } else {
                    content(tone.prose())
                };
                let base_opacity = color.a;
                let mut opaque_color = color;
                opaque_color.a = 1.;
                let highlight = HighlightStyle {
                    color: Some(opaque_color),
                    background_color: style.search.map(search::search_background),
                    fade_out: Some(1. - base_opacity),
                    font_weight: style.bold.then_some(FontWeight::SEMIBOLD),
                    font_style: style.italic.then_some(FontStyle::Italic),
                    strikethrough: style.strikethrough.then_some(StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    }),
                    ..Default::default()
                };
                let link = style.link.clone();
                let menu_link = link.clone();
                let intercept = link_intercept();
                let styled =
                    StyledText::new(text.to_owned()).with_highlights([(0..text.len(), highlight)]);
                let layout = styled.layout().clone();
                let child = styled.into_any_element();
                let selected = selection.as_ref().map(|s| {
                    let selected = s.offset(offset);
                    if link.is_some() {
                        selected.with_cursor(gpui::CursorStyle::PointingHand)
                    } else {
                        selected
                    }
                });
                let clicked_selection = selected.clone();
                let child = if let Some(selected) = selected {
                    selected.wrap(text.to_owned().into(), layout, child)
                } else {
                    child
                };
                children.push(
                    div()
                        .id(SharedString::from(format!("{id}-{run_ix}-{part_ix}")))
                        .text_size(px(font_size))
                        .line_height(px(24.))
                        .opacity(alpha)
                        .when(link.is_some(), |d| {
                            d.cursor(gpui::CursorStyle::PointingHand)
                        })
                        .child(child)
                        .on_click(move |event, window, cx| {
                            if let Some(url) = &link
                                && activate_link(event, clicked_selection.as_ref())
                            {
                                open_link(
                                    url,
                                    !event.modifiers().secondary(),
                                    intercept.as_ref(),
                                    window,
                                    cx,
                                );
                            }
                        })
                        .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                            if let Some(url) = &menu_link {
                                link_menu(url, event.position, window, cx);
                                cx.stop_propagation();
                            }
                        })
                        .into_any_element(),
                );
            }
            offset += part.len();
            if part.ends_with('\n') {
                children.push(div().w_full().h(px(0.)).into_any_element());
            } else if part.len() > text.len() {
                children.push(div().w(px(font_size * 0.25)).h(px(1.)).into_any_element());
            }
        }
    }
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(runs.iter().map(|run| run.text.as_str()).collect::<String>())
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .children(children)
        .into_any_element()
}
fn is_file_chip(text: &str) -> bool {
    !text.chars().any(char::is_whitespace)
        && text
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
}
/// Bind preview links to their document, independent of the focused pane.
pub(crate) fn resolve_file_links(row: &mut MarkdownRow, base: &str) {
    fn resolve(runs: &mut [InlineRun], base: &str) {
        for run in runs {
            let target =
                run.style.link.clone().or_else(|| {
                    (run.style.code && is_file_chip(&run.text)).then(|| run.text.clone())
                });
            if let Some(target) = target
                && !target.starts_with('#')
                && !target.starts_with("mailto:")
                && (!target.contains("://") || target.starts_with("file://"))
            {
                let (path, location) = crate::file_editor::file_location(&target, base);
                run.style.link = Some(location.map_or_else(
                    || path.clone(),
                    |(line, col)| format!("{path}:{line}:{col}"),
                ));
            }
        }
    }
    match &mut row.part {
        Part::Text { runs, .. } => resolve(runs, base),
        Part::Table { cells, .. } => {
            for runs in cells.iter_mut().flatten() {
                resolve(runs, base);
            }
        }
        _ => {}
    }
}

/// A surface that owns some web links (the Inbox owns GitHub work items) can
/// claim them before they reach the browser. Return true when handled.
pub(crate) type LinkIntercept = Rc<dyn Fn(&str, &mut Window, &mut App) -> bool>;

thread_local! {
    static INTERCEPT: RefCell<Option<LinkIntercept>> = const { RefCell::new(None) };
}

/// Elements built inside `build` route plain link clicks through `intercept`.
/// Modified clicks still open the browser.
pub(crate) fn with_link_intercept<R>(
    intercept: Option<LinkIntercept>,
    build: impl FnOnce() -> R,
) -> R {
    let previous = INTERCEPT.replace(intercept);
    let built = build();
    INTERCEPT.set(previous);
    built
}

fn link_intercept() -> Option<LinkIntercept> {
    INTERCEPT.with_borrow(Clone::clone)
}

/// `route` is false for modified clicks, which always mean the browser.
fn open_link(
    url: &str,
    route: bool,
    intercept: Option<&LinkIntercept>,
    window: &mut Window,
    cx: &mut App,
) {
    if route
        && (intercept.is_some_and(|intercept| intercept(url, window, cx))
            || (monocode_core::inbox::link_target(url).is_some()
                && crate::inbox::open_in_inbox(url, window, cx)))
    {
        return;
    }
    if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:") {
        cx.open_url(url);
    } else if !url.starts_with('#') && (!url.contains("://") || url.starts_with("file://")) {
        window.dispatch_action(Box::new(crate::file_editor::OpenPath(url.into())), cx);
    }
}

pub(crate) fn body(
    row: &MarkdownRow,
    id: SharedString,
    tone: Tone,
    fade: Option<&crate::reveal::WordFades>,
    now: Instant,
) -> AnyElement {
    body_selected(row, id, tone, fade, now, None, None)
}
pub(crate) fn body_selected(
    row: &MarkdownRow,
    id: SharedString,
    tone: Tone,
    fade: Option<&crate::reveal::WordFades>,
    now: Instant,
    selection: Option<TextSelection>,
    prepared: Option<&PreparedRow>,
) -> AnyElement {
    // Share one logical row across its styled fragments. Selection boundaries
    // then span inline code and formatting without scanning transcript history.
    let selection = selection.map(|selection| {
        selection.with_text(prepared.map_or_else(
            || selection_text(row).into(),
            |prepared| prepared.selection.clone(),
        ))
    });
    let mut body = div()
        .w_full()
        .min_w_0()
        .text_size(px(14.))
        .line_height(px(24.));
    match &row.part {
        Part::Text { runs, heading } => {
            let mut runs = std::borrow::Cow::Borrowed(runs.as_slice());
            if *heading > 0 && prepared.and_then(|p| p.inline.as_ref()).is_none() {
                for run in runs.to_mut() {
                    run.style.bold = true;
                }
            }
            let heading = if tone == Tone::Work { 0 } else { *heading };
            if heading > 0 {
                body = body
                    .text_size(px(match heading {
                        1 => 22.,
                        2 => 18.,
                        _ => 16.,
                    }))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(px(if heading == 1 { 30. } else { 27. }));
            }
            body.child(inline_selected(
                &runs,
                id,
                tone,
                fade,
                now,
                row.word_start,
                match heading {
                    1 => 22.,
                    2 => 18.,
                    0 => 14.,
                    _ => 16.,
                },
                selection,
                prepared.and_then(|prepared| prepared.inline.as_ref()),
            ))
            .into_any_element()
        }
        Part::Rule => body
            .py_2()
            .child(div().w_full().h(px(1.)).bg(content(0.1)))
            .into_any_element(),
        // Only the transcript can draw a session's visuals; elsewhere the
        // caption stands in for it.
        Part::Visual { caption, .. } => body
            .text_color(content(0.5))
            .child(if caption.is_empty() {
                "Visual".to_owned()
            } else {
                caption.clone()
            })
            .into_any_element(),
        Part::Math(tex) => {
            let styled = StyledText::new(tex.clone());
            let layout = styled.layout().clone();
            let child = styled.into_any_element();
            let child = if let Some(selection) = selection {
                selection.wrap(tex.clone().into(), layout, child)
            } else {
                child
            };
            body.font_family(crate::fonts::mono())
                .text_color(content(0.78))
                .child(child)
                .into_any_element()
        }
        Part::Table {
            cells,
            columns,
            align,
            header,
            last,
        } => body
            .id(id.clone())
            .overflow_x_scroll()
            .rounded_t(px(if *header { 10. } else { 0. }))
            .rounded_b(px(if *last { 10. } else { 0. }))
            .border_1()
            .border_color(content(0.1))
            .bg(content(0.06))
            .child(
                div()
                    .w_full()
                    .min_w(px(columns.len() as f32 * 160.))
                    .flex()
                    .flex_col()
                    .children(cells.iter().enumerate().map(|(ix, cells)| {
                        div()
                            .w_full()
                            .flex()
                            .when(ix > 0 || !*header, |d| {
                                d.border_t_1().border_color(content(0.05))
                            })
                            .children(columns.iter().enumerate().map(|(col, width)| {
                                let cell = cells.get(col).map(Vec::as_slice).unwrap_or_default();
                                div()
                                    .w(px(*width))
                                    .min_w(px(160.))
                                    .flex_grow(*width)
                                    .flex_shrink_1()
                                    .px(px(10.))
                                    .py_2()
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .when(*header && ix == 0, |d| {
                                        d.font_weight(FontWeight::SEMIBOLD)
                                    })
                                    .when(align.get(col) == Some(&TableAlign::Center), |d| {
                                        d.text_center()
                                    })
                                    .when(align.get(col) == Some(&TableAlign::Right), |d| {
                                        d.text_right()
                                    })
                                    .child(inline_selected(
                                        cell,
                                        format!("{id}-{ix}-{col}").into(),
                                        tone,
                                        None,
                                        now,
                                        0,
                                        12.,
                                        selection.as_ref().map(|selection| {
                                            selection.offset(table_offset(&row.part, ix, col))
                                        }),
                                        None,
                                    ))
                            }))
                    })),
            )
            .into_any_element(),
        Part::Code { .. } => body.into_any_element(),
        Part::Mermaid { fence } => body
            .child(mermaid::Diagram {
                id,
                fence: fence.clone(),
                selection,
            })
            .into_any_element(),
    }
}

pub(crate) fn code_lines(
    fence: &Fence,
    range: Range<usize>,
    highlighted: Option<&HighlightedDocument>,
    query: &str,
    current: bool,
) -> Div {
    code_lines_selected(fence, range, highlighted, query, current, None)
}
pub(crate) fn code_lines_selected(
    fence: &Fence,
    range: Range<usize>,
    highlighted: Option<&HighlightedDocument>,
    query: &str,
    current: bool,
    selection: Option<TextSelection>,
) -> Div {
    let selection = selection.map(|selection| {
        selection.with_text(
            range
                .clone()
                .map(|ix| fence.code[fence.lines[ix].clone()].to_owned())
                .collect::<Vec<_>>()
                .join("\n")
                .into(),
        )
    });
    let start = range.start;
    let longest = fence
        .lines
        .iter()
        .map(|r| fence.code[r.clone()].chars().count())
        .max()
        .unwrap_or(0);
    div()
        .w(px(longest as f32 * 7.3 + 48.))
        .min_w_full()
        .flex()
        .flex_col()
        .children(range.map(|ix| {
            let line = &fence.code[fence.lines[ix].clone()];
            let styles = highlighted
                .and_then(|h| h.lines.get(ix))
                .into_iter()
                .flatten()
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
            let styles = mark_styles(line, styles, query, current);
            let text: SharedString = if line.is_empty() {
                " ".into()
            } else {
                line.to_owned().into()
            };
            let styled = StyledText::new(text.clone()).with_highlights(styles);
            let layout = styled.layout().clone();
            let child = styled.into_any_element();
            let child = if let Some(selection) = selection.as_ref() {
                let offset = (start..ix).map(|line| fence.lines[line].len() + 1).sum();
                selection.offset(offset).wrap(text, layout, child)
            } else {
                child
            };
            div()
                .flex()
                .min_w_full()
                .line_height(px(20.))
                .text_size(px(12.))
                .font_family(crate::fonts::mono())
                .when(fence.numbered, |d| {
                    d.child(
                        div()
                            .w(px(24.))
                            .mr_2()
                            .flex_shrink_0()
                            .text_right()
                            .text_size(px(10.))
                            .text_color(content(0.35))
                            .child((ix + fence.start_line).to_string()),
                    )
                })
                .when(!fence.numbered, |d| d.pl(px(10.)))
                .child(child)
        }))
}

fn table_offset(part: &Part, row: usize, col: usize) -> usize {
    let Part::Table { cells, .. } = part else {
        return 0;
    };
    cells
        .iter()
        .take(row)
        .map(|row| {
            row.iter().map(|cell| runs_text(cell).len()).sum::<usize>()
                + row.len().saturating_sub(1)
                + 1
        })
        .sum::<usize>()
        + cells[row]
            .iter()
            .take(col)
            .map(|cell| runs_text(cell).len() + 1)
            .sum::<usize>()
}
fn runs_text(runs: &[InlineRun]) -> String {
    let rich = runs.iter().any(|run| run.style.code);
    runs.iter()
        .map(|run| {
            format!(
                "{}{}",
                if rich {
                    String::new()
                } else {
                    run.style.task.as_ref().map_or(String::new(), |task| {
                        if task.checked {
                            "☑ ".into()
                        } else {
                            "☐ ".into()
                        }
                    })
                },
                run.text
            )
        })
        .collect()
}
pub(crate) fn selection_text(row: &MarkdownRow) -> String {
    match &row.part {
        Part::Text { runs, .. } => runs_text(runs),
        Part::Table { cells, .. } => cells
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| runs_text(cell))
                    .collect::<Vec<_>>()
                    .join("\t")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => row_text(row),
    }
}

#[cfg(test)]
mod file_link_tests {
    use super::*;
    use crate::markdown_model::InlineStyle;
    #[test]
    fn preview_links_keep_their_document_base_and_display_text() {
        let runs = vec![
            InlineRun {
                text: "source".into(),
                style: InlineStyle {
                    link: Some("../main.rs:5".into()),
                    ..Default::default()
                },
            },
            InlineRun {
                text: "src/lib.rs".into(),
                style: InlineStyle {
                    code: true,
                    ..Default::default()
                },
            },
            InlineRun {
                text: "web".into(),
                style: InlineStyle {
                    link: Some("https://example.com".into()),
                    ..Default::default()
                },
            },
        ];
        let mut row = rows(&Block::Paragraph { runs }).remove(0);
        resolve_file_links(&mut row, "/project/docs");
        let Part::Text { runs, .. } = row.part else {
            panic!()
        };
        assert_eq!(runs[0].text, "source");
        assert_eq!(runs[0].style.link.as_deref(), Some("/project/main.rs:5:1"));
        assert_eq!(runs[1].text, "src/lib.rs");
        assert_eq!(
            runs[1].style.link.as_deref(),
            Some("/project/docs/src/lib.rs")
        );
        assert_eq!(runs[2].style.link.as_deref(), Some("https://example.com"));
    }
    #[test]
    fn link_intercepts_apply_only_while_building_and_restore_the_outer_one() {
        let outer: LinkIntercept = Rc::new(|_, _, _| true);
        assert!(link_intercept().is_none());
        with_link_intercept(Some(outer.clone()), || {
            assert!(link_intercept().is_some_and(|set| Rc::ptr_eq(&set, &outer)));
            with_link_intercept(None, || assert!(link_intercept().is_none()));
            assert!(link_intercept().is_some_and(|set| Rc::ptr_eq(&set, &outer)));
        });
        assert!(link_intercept().is_none());
    }
}
