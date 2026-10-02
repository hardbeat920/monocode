//! Turns parsed blocks into render-ready data: display strings, text runs,
//! link and inline-code ranges, and element keys.
//!
//! Preparing depends on the style (fonts and colors), not on time, so the
//! view caches one [`PreparedBlock`] per top-level block and reuses it for as
//! long as the parser keeps that block's `Arc`.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    FontStyle, FontWeight, Hsla, Pixels, SharedString, StrikethroughStyle, TextRun, font, px,
};

use crate::parse::mend::PENDING_LINK_URL;
use crate::parse::{Align, Block, CodeBlock, CodeFence, ImageRef, Inline, InlineStyle, TopBlock};
use crate::selection::{Element, ElementKey, Separator};
use crate::style::{BlockMargins, MarkdownStyle};

/// Padding around inline code: a no-break space on each side, so the chip
/// keeps room around its text (`px-1.5`). Copy leaves it out.
const CODE_PAD: &str = "\u{a0}";

/// A link inside a text element.
#[derive(Clone, Debug)]
pub(crate) struct LinkRange {
    pub range: Range<usize>,
    pub url: Arc<str>,
    /// The URL is still streaming; the text looks like a link but does not
    /// respond to clicks.
    pub pending: bool,
}

/// One selectable text element.
pub(crate) struct PreparedText {
    pub key: ElementKey,
    pub text: SharedString,
    pub runs: Vec<TextRun>,
    /// Source offset of each run's first character, or `None` for runs that
    /// never fade (links, inline code, padding).
    pub run_src: Vec<Option<usize>>,
    /// Links by display byte range.
    pub links: Arc<[LinkRange]>,
    pub code_ranges: Vec<Range<usize>>,
    pub hidden: Vec<Range<usize>>,
    pub separator: Separator,
    pub src_range: Option<Range<usize>>,
    pub size: Pixels,
    pub line_height: Pixels,
    /// Keep the text on one line (table header cells).
    pub nowrap: bool,
}

/// Paragraph content: text with images between.
pub(crate) enum Segment {
    Text(PreparedText),
    Image(Arc<ImageRef>, ElementKey),
}

pub(crate) struct PreparedCode {
    pub key: ElementKey,
    pub fence: CodeFence,
    pub code: SharedString,
    pub line_count: usize,
}

pub(crate) struct PreparedTable {
    pub align: Vec<Align>,
    /// Header row first when there is one.
    pub rows: Vec<Vec<PreparedText>>,
    pub has_header: bool,
    pub columns: usize,
    /// Unwrapped width of each column, measured on first render.
    pub naturals: std::cell::OnceCell<Vec<f32>>,
}

pub(crate) struct PreparedItem {
    pub task: Option<bool>,
    pub blocks: Vec<PreparedNode>,
}

pub(crate) enum PreparedNode {
    Text {
        segments: Vec<Segment>,
        heading: Option<u8>,
    },
    Code(PreparedCode),
    Quote(Vec<PreparedNode>),
    List {
        start: Option<u64>,
        items: Vec<PreparedItem>,
    },
    Table(PreparedTable),
    Rule,
}

impl PreparedNode {
    /// CSS block margins for this node at the top level of a message.
    pub fn margins(&self, style: &MarkdownStyle) -> BlockMargins {
        match self {
            PreparedNode::Text {
                heading: Some(_), ..
            } => style.heading_margins,
            PreparedNode::Text { .. } => style.paragraph_margins,
            PreparedNode::Code(_) => style.code_margins,
            PreparedNode::Quote(_) => style.quote_margins,
            PreparedNode::List { .. } => style.list_margins,
            PreparedNode::Table(_) => style.table_margins,
            PreparedNode::Rule => style.rule_margins,
        }
    }
}

/// A prepared top-level block, tied to the parsed block it came from.
pub(crate) struct PreparedBlock {
    pub source: Arc<TopBlock>,
    /// Unique per preparation, so a cached measurement never outlives the
    /// block it measured.
    pub id: u64,
    pub node: PreparedNode,
    /// The block's text elements for selection when it is not laid out.
    pub elements: std::cell::OnceCell<Rc<[Element]>>,
}

impl PreparedBlock {
    pub fn selection_elements(&self) -> Rc<[Element]> {
        self.elements
            .get_or_init(|| {
                let mut out = Vec::new();
                collect_elements(&self.node, &mut out);
                out.into()
            })
            .clone()
    }
}

fn text_element(text: &PreparedText) -> Element {
    Element {
        key: text.key,
        text: text.text.clone(),
        hidden: text.hidden.clone(),
        separator: text.separator,
        line_mode: false,
    }
}

/// Text elements in render order.
fn collect_elements(node: &PreparedNode, out: &mut Vec<Element>) {
    match node {
        PreparedNode::Text { segments, .. } => {
            for segment in segments {
                if let Segment::Text(text) = segment {
                    out.push(text_element(text));
                }
            }
        }
        PreparedNode::Code(code) => out.push(Element {
            key: code.key,
            text: code.code.clone(),
            hidden: Vec::new(),
            separator: Separator::Paragraph,
            line_mode: true,
        }),
        PreparedNode::Quote(children) => children.iter().for_each(|c| collect_elements(c, out)),
        PreparedNode::List { items, .. } => items
            .iter()
            .flat_map(|item| &item.blocks)
            .for_each(|c| collect_elements(c, out)),
        PreparedNode::Table(table) => {
            for text in table.rows.iter().flatten() {
                out.push(text_element(text));
            }
        }
        PreparedNode::Rule => {}
    }
}

/// Where text sits, which decides its base font, color, and size.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TextRole {
    Body,
    Quote,
    Heading(u8),
    TableHeader,
    TableCell,
}

struct Preparer<'a> {
    style: &'a MarkdownStyle,
    reasoning: bool,
    block_ix: usize,
    next_sub: usize,
    /// Separator for the next text element.
    separator: Separator,
}

impl Preparer<'_> {
    fn key(&mut self) -> ElementKey {
        let key = ElementKey::new(self.block_ix, self.next_sub);
        self.next_sub += 1;
        key
    }

    fn take_separator(&mut self, next: Separator) -> Separator {
        std::mem::replace(&mut self.separator, next)
    }
}

/// Prepare top-level block `block_ix`.
pub(crate) fn prepare_block(
    top: &Arc<TopBlock>,
    block_ix: usize,
    style: &MarkdownStyle,
    reasoning: bool,
) -> PreparedBlock {
    let mut preparer = Preparer {
        style,
        reasoning,
        block_ix,
        next_sub: 0,
        separator: Separator::Paragraph,
    };
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let node = prepare_node(&mut preparer, &top.block, false);
    PreparedBlock {
        source: top.clone(),
        id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        node,
        elements: std::cell::OnceCell::new(),
    }
}

fn prepare_node(p: &mut Preparer, block: &Block, quoted: bool) -> PreparedNode {
    let role = if quoted {
        TextRole::Quote
    } else {
        TextRole::Body
    };
    match block {
        Block::Paragraph(inline) => PreparedNode::Text {
            segments: prepare_segments(p, inline, role),
            heading: None,
        },
        Block::Heading { level, content } => PreparedNode::Text {
            segments: prepare_segments(p, content, TextRole::Heading(*level)),
            heading: Some(*level),
        },
        Block::Code(code) => PreparedNode::Code(prepare_code(p, code)),
        Block::Quote(children) => PreparedNode::Quote(
            children
                .iter()
                .map(|child| {
                    let node = prepare_node(p, child, true);
                    p.separator = Separator::Paragraph;
                    node
                })
                .collect(),
        ),
        Block::List(list) => {
            let items = list
                .items
                .iter()
                .map(|item| {
                    let blocks = item
                        .blocks
                        .iter()
                        .map(|child| prepare_node(p, child, quoted))
                        .collect();
                    p.separator = Separator::Line;
                    PreparedItem {
                        task: item.task,
                        blocks,
                    }
                })
                .collect();
            p.separator = Separator::Paragraph;
            PreparedNode::List {
                start: list.start,
                items,
            }
        }
        Block::Table(table) => {
            let columns = table.columns();
            let mut rows = Vec::new();
            let has_header = !table.header.is_empty();
            if has_header {
                rows.push(prepare_row(p, &table.header, TextRole::TableHeader));
            }
            for row in &table.rows {
                rows.push(prepare_row(p, row, TextRole::TableCell));
            }
            p.separator = Separator::Paragraph;
            PreparedNode::Table(PreparedTable {
                align: table.align.clone(),
                rows,
                has_header,
                columns,
                naturals: std::cell::OnceCell::new(),
            })
        }
        Block::Rule => PreparedNode::Rule,
    }
}

fn prepare_row(p: &mut Preparer, cells: &[Inline], role: TextRole) -> Vec<PreparedText> {
    let row = cells
        .iter()
        .map(|cell| {
            let text = prepare_text(p, cell, role);
            p.separator = Separator::Tab;
            text
        })
        .collect();
    p.separator = Separator::Line;
    row
}

fn prepare_code(p: &mut Preparer, code: &CodeBlock) -> PreparedCode {
    let key = p.key();
    p.separator = Separator::Paragraph;
    PreparedCode {
        key,
        fence: code.fence(),
        line_count: code.code.split('\n').count(),
        code: code.code.clone().into(),
    }
}

fn prepare_segments(p: &mut Preparer, inline: &Inline, role: TextRole) -> Vec<Segment> {
    if !inline.has_images() {
        return vec![Segment::Text(prepare_text(p, inline, role))];
    }
    // Split at images; each image sits on its own line between text runs.
    let mut segments = Vec::new();
    let mut part = Inline::default();
    for span in &inline.spans {
        if let Some(image) = &span.style.image {
            if !part.text.trim().is_empty() {
                segments.push(Segment::Text(prepare_text(p, &part, role)));
            }
            part = Inline::default();
            segments.push(Segment::Image(image.clone(), p.key()));
        } else {
            part.push(
                &inline.text[span.range.clone()],
                span.style.clone(),
                span.src,
            );
        }
    }
    if !part.text.trim().is_empty() {
        segments.push(Segment::Text(prepare_text(p, &part, role)));
    }
    segments
}

struct RoleStyle {
    size: Pixels,
    line_height: Pixels,
    color: Hsla,
    emphasis_color: Hsla,
    weight: FontWeight,
    italic: bool,
}

fn role_style(p: &Preparer, role: TextRole) -> RoleStyle {
    let style = p.style;
    let (body, emphasis) = if p.reasoning {
        (style.reasoning_text, style.reasoning_emphasis)
    } else {
        (style.body_text, style.text)
    };
    match role {
        TextRole::Body => RoleStyle {
            size: style.text_size,
            line_height: style.line_height,
            color: body,
            emphasis_color: emphasis,
            weight: FontWeight::NORMAL,
            italic: false,
        },
        TextRole::Quote => RoleStyle {
            size: style.text_size,
            line_height: style.line_height,
            color: body,
            emphasis_color: emphasis,
            weight: FontWeight::NORMAL,
            italic: true,
        },
        TextRole::Heading(level) => {
            let (size, line_height) = style.heading_metrics(level);
            RoleStyle {
                size,
                line_height,
                color: style.heading,
                emphasis_color: style.heading,
                weight: FontWeight::SEMIBOLD,
                italic: false,
            }
        }
        TextRole::TableHeader => RoleStyle {
            size: style.table_text_size,
            line_height: style.table_line_height,
            color: style.text,
            emphasis_color: style.text,
            weight: FontWeight::SEMIBOLD,
            italic: false,
        },
        TextRole::TableCell => RoleStyle {
            size: style.table_text_size,
            line_height: style.table_line_height,
            color: style.text,
            emphasis_color: style.text,
            weight: FontWeight::NORMAL,
            italic: false,
        },
    }
}

fn prepare_text(p: &mut Preparer, inline: &Inline, role: TextRole) -> PreparedText {
    let style = p.style;
    let base = role_style(p, role);
    let key = p.key();
    let separator = p.take_separator(Separator::Paragraph);

    let mut text = String::with_capacity(inline.text.len() + 8);
    let mut runs: Vec<TextRun> = Vec::with_capacity(inline.spans.len());
    let mut run_src: Vec<Option<usize>> = Vec::with_capacity(inline.spans.len());
    let mut links: Vec<LinkRange> = Vec::new();
    let mut code_ranges: Vec<Range<usize>> = Vec::new();
    let mut hidden: Vec<Range<usize>> = Vec::new();

    let body_font = |weight: FontWeight, italic: bool| {
        let mut f = font(style.font_family.clone());
        f.weight = weight;
        f.style = if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        f
    };

    let spans = &inline.spans;
    for (ix, span) in spans.iter().enumerate() {
        let piece = &inline.text[span.range.clone()];
        let s: &InlineStyle = &span.style;
        let start = text.len();
        let weight = if s.strong {
            FontWeight::SEMIBOLD
        } else {
            base.weight
        };
        let italic = base.italic || s.emphasis;
        let mut color = if s.strong || s.emphasis {
            base.emphasis_color
        } else {
            base.color
        };
        if s.link.is_some() {
            color = style.link;
        }
        let strikethrough = s.strikethrough.then(|| StrikethroughStyle {
            thickness: px(1.),
            color: Some(color),
        });

        if s.code {
            // Pad a code chip unless it continues the previous code span.
            let joins_previous = ix > 0 && spans[ix - 1].style.code;
            if !joins_previous {
                let pad_start = text.len();
                text.push_str(CODE_PAD);
                hidden.push(pad_start..text.len());
                runs.push(TextRun {
                    len: CODE_PAD.len(),
                    font: body_font(base.weight, false),
                    color: style.inline_code_text,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
                run_src.push(None);
            }
            text.push_str(piece);
            let mut f = font(style.mono_font_family.clone());
            f.weight = weight;
            runs.push(TextRun {
                len: piece.len(),
                font: f,
                color: if s.link.is_some() {
                    style.link
                } else {
                    style.inline_code_text
                },
                background_color: None,
                underline: None,
                strikethrough,
            });
            run_src.push(None);
            let joins_next = spans.get(ix + 1).is_some_and(|next| next.style.code);
            if !joins_next {
                let pad_start = text.len();
                text.push_str(CODE_PAD);
                hidden.push(pad_start..text.len());
                runs.push(TextRun {
                    len: CODE_PAD.len(),
                    font: body_font(base.weight, false),
                    color: style.inline_code_text,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
                run_src.push(None);
            }
            match code_ranges.last_mut() {
                Some(last) if last.end == start => last.end = text.len(),
                _ => code_ranges.push(start..text.len()),
            }
        } else {
            text.push_str(piece);
            runs.push(TextRun {
                len: piece.len(),
                font: body_font(weight, italic),
                color,
                background_color: None,
                underline: None,
                strikethrough,
            });
            run_src.push(s.link.is_none().then_some(span.src));
        }

        if let Some(url) = &s.link {
            let pending = url.as_ref() == PENDING_LINK_URL;
            match links.last_mut() {
                Some(last) if last.range.end == start && last.url == *url => {
                    last.range.end = text.len()
                }
                _ => links.push(LinkRange {
                    range: start..text.len(),
                    url: url.clone(),
                    pending,
                }),
            }
        }
    }

    PreparedText {
        key,
        text: text.into(),
        runs,
        run_src,
        links: links.into(),
        code_ranges,
        hidden,
        separator,
        src_range: inline.src_range(),
        size: base.size,
        line_height: base.line_height,
        nowrap: role == TextRole::TableHeader,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn prepared(source: &str) -> Vec<PreparedBlock> {
        let doc = parse(source);
        let style = MarkdownStyle::dark();
        doc.blocks
            .iter()
            .enumerate()
            .map(|(ix, top)| prepare_block(top, ix, &style, false))
            .collect()
    }

    fn texts(node: &PreparedNode) -> Vec<&PreparedText> {
        let mut out = Vec::new();
        fn walk<'a>(node: &'a PreparedNode, out: &mut Vec<&'a PreparedText>) {
            match node {
                PreparedNode::Text { segments, .. } => {
                    for segment in segments {
                        if let Segment::Text(text) = segment {
                            out.push(text);
                        }
                    }
                }
                PreparedNode::Quote(children) => children.iter().for_each(|c| walk(c, out)),
                PreparedNode::List { items, .. } => items
                    .iter()
                    .flat_map(|item| &item.blocks)
                    .for_each(|c| walk(c, out)),
                PreparedNode::Table(table) => table.rows.iter().flatten().for_each(|t| out.push(t)),
                PreparedNode::Code(_) | PreparedNode::Rule => {}
            }
        }
        walk(node, &mut out);
        out
    }

    #[test]
    fn runs_cover_text_and_code_is_padded() {
        let blocks = prepared("run `cargo test` and **check** [docs](https://x.dev)");
        let all = texts(&blocks[0].node);
        let text = all[0];
        let total: usize = text.runs.iter().map(|r| r.len).sum();
        assert_eq!(total, text.text.len());
        assert_eq!(text.runs.len(), text.run_src.len());
        assert_eq!(
            text.text.as_ref(),
            "run \u{a0}cargo test\u{a0} and check docs"
        );
        assert_eq!(text.hidden.len(), 2);
        assert_eq!(text.code_ranges.len(), 1);
        assert_eq!(
            &text.text[text.code_ranges[0].clone()],
            "\u{a0}cargo test\u{a0}"
        );
        assert_eq!(text.links.len(), 1);
        assert_eq!(&text.text[text.links[0].range.clone()], "docs");
        // Links, code, and padding never fade; plain words do.
        let fading: Vec<_> = text
            .runs
            .iter()
            .zip(&text.run_src)
            .scan(0, |at, (run, src)| {
                let start = *at;
                *at += run.len;
                Some((start..*at, src.is_some()))
            })
            .filter(|(_, fades)| *fades)
            .map(|(range, _)| text.text[range].to_string())
            .collect();
        assert_eq!(fading, vec!["run ", " and ", "check", " "]);
    }

    #[test]
    fn keys_are_unique_and_separators_follow_structure() {
        let blocks = prepared("- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        let list = texts(&blocks[0].node);
        assert_eq!(list[0].separator, Separator::Paragraph);
        assert_eq!(list[1].separator, Separator::Line);
        let table = texts(&blocks[1].node);
        let seps: Vec<_> = table.iter().map(|t| t.separator).collect();
        assert_eq!(
            seps,
            vec![
                Separator::Paragraph,
                Separator::Tab,
                Separator::Line,
                Separator::Tab
            ]
        );
        let mut keys: Vec<_> = list.iter().chain(&table).map(|t| t.key).collect();
        let len = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), len);
    }

    #[test]
    fn images_split_paragraphs() {
        let blocks = prepared("before ![shot](a.png) after");
        let PreparedNode::Text { segments, .. } = &blocks[0].node else {
            panic!();
        };
        assert_eq!(segments.len(), 3);
        assert!(matches!(segments[1], Segment::Image(..)));
    }
}
