//! Markdown parsing over pulldown-cmark into a block tree the renderer reads.
//!
//! Replaces the remark (Streamdown) pipeline in
//! `src/features/sessions/ui/AgentMarkdown.tsx`: GFM tables, task lists,
//! strikethrough, and literal autolinks, with raw HTML shown as text.
//!
//! A [`Document`] is a list of top-level blocks with their byte ranges in the
//! source. Inline content is stored flat: one string per paragraph, heading,
//! or table cell, plus style spans that cover it exactly. Each span also
//! records where its text starts in the source, which the word fade uses to
//! find when a word arrived.
//!
//! [`IncrementalParser`] reparses only the tail of a growing source and keeps
//! finished blocks shared, so a streamed reply costs O(tail) per append.

mod autolink;
pub mod fence;
mod incremental;
pub mod mend;

use std::ops::Range;
use std::sync::Arc;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};

pub use fence::CodeFence;
pub use incremental::IncrementalParser;

/// Inline styling for one span of text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineStyle {
    pub strong: bool,
    pub emphasis: bool,
    pub strikethrough: bool,
    /// Inline code.
    pub code: bool,
    /// Link destination, when the span is inside a link.
    pub link: Option<Arc<str>>,
    /// Set when the span is an image. The span text is the image's alt text.
    pub image: Option<Arc<ImageRef>>,
}

/// An image reference from `![alt](url "title")`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRef {
    pub url: String,
    pub alt: String,
    pub title: String,
}

/// A styled byte range of an [`Inline`] text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: InlineStyle,
    /// Byte offset in the source where this span's text starts.
    pub src: usize,
}

/// Inline content: visible text plus style spans that cover it in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<Span>,
}

impl Inline {
    /// Plain unstyled text (tests and fallbacks).
    pub fn plain(text: &str, src: usize) -> Self {
        let mut inline = Inline::default();
        inline.push(text, InlineStyle::default(), src);
        inline
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Append text, merging into the previous span when the style matches and
    /// the source is contiguous.
    pub(crate) fn push(&mut self, text: &str, style: InlineStyle, src: usize) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        if let Some(last) = self.spans.last_mut()
            && last.style == style
            && style.image.is_none()
            && last.src + last.range.len() == src
        {
            last.range.end = end;
            return;
        }
        self.spans.push(Span {
            range: start..end,
            style,
            src,
        });
    }

    fn append(&mut self, other: Inline) {
        let offset = self.text.len();
        self.text.push_str(&other.text);
        for mut span in other.spans {
            span.range = span.range.start + offset..span.range.end + offset;
            self.spans.push(span);
        }
    }

    /// The smallest and largest source offsets this content covers.
    pub fn src_range(&self) -> Option<Range<usize>> {
        let first = self.spans.first()?;
        let last = self.spans.last()?;
        Some(first.src..last.src + last.range.len())
    }

    /// Whether any span is an image.
    pub fn has_images(&self) -> bool {
        self.spans.iter().any(|span| span.style.image.is_some())
    }
}

/// A fenced or indented code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
    /// The info string after the opening fence, trimmed.
    pub info: String,
    /// The code, without the trailing newline.
    pub code: String,
    /// Whether the closing fence has arrived. Indented blocks are always
    /// closed.
    pub closed: bool,
    /// Byte offset in the source of the first code character.
    pub src: usize,
}

impl CodeBlock {
    /// The fence parsed the way `parseCodeFence` in AgentMarkdown.tsx does.
    pub fn fence(&self) -> CodeFence {
        CodeFence::parse(&self.info)
    }
}

/// One list item. `task` is `Some(checked)` for GFM task items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    /// Number of the first item for ordered lists.
    pub start: Option<u64>,
    pub items: Vec<ListItem>,
}

/// GFM column alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub align: Vec<Align>,
    pub header: Vec<Inline>,
    pub rows: Vec<Vec<Inline>>,
}

impl Table {
    /// Number of columns: the widest of the header and every row.
    pub fn columns(&self) -> usize {
        self.rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(self.header.len()))
            .max()
            .unwrap_or(0)
    }
}

/// A block. Quotes and lists nest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Inline),
    Heading { level: u8, content: Inline },
    Code(CodeBlock),
    Quote(Vec<Block>),
    List(List),
    Table(Table),
    Rule,
}

/// A top-level block and its byte range in the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopBlock {
    pub range: Range<usize>,
    pub block: Block,
}

/// A parsed document. Blocks are shared between parses, so a block that did
/// not change keeps its `Arc` (and the renderer keeps its caches).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Arc<TopBlock>>,
}

impl Document {
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

pub(crate) fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

/// Parse a whole source.
pub fn parse(source: &str) -> Document {
    parse_at(source, 0)
}

/// Parse `text` as if it started at byte `offset` of a larger source. Ranges
/// and source offsets in the result include the offset.
pub(crate) fn parse_at(text: &str, offset: usize) -> Document {
    let events: Vec<(Event, Range<usize>)> = Parser::new_ext(text, options())
        .into_offset_iter()
        .map(|(event, range)| (event, range.start + offset..range.end + offset))
        .collect();
    let mut cursor = Cursor {
        events: &events,
        ix: 0,
        text,
        offset,
    };
    let mut blocks = Vec::new();
    while let Some((event, range)) = cursor.peek() {
        let range = range.clone();
        match event {
            Event::Rule => {
                cursor.bump();
                blocks.push(Arc::new(TopBlock {
                    range,
                    block: Block::Rule,
                }));
            }
            Event::Start(_) => {
                for block in parse_started_block(&mut cursor) {
                    blocks.push(Arc::new(TopBlock {
                        range: range.clone(),
                        block,
                    }));
                }
            }
            _ => cursor.bump(),
        }
    }
    Document { blocks }
}

struct Cursor<'a, 'e> {
    events: &'a [(Event<'e>, Range<usize>)],
    ix: usize,
    /// The parsed text, for checks that read raw source (closing fences).
    text: &'a str,
    /// Offset of `text` in the full source.
    offset: usize,
}

impl<'a, 'e> Cursor<'a, 'e> {
    fn peek(&self) -> Option<&(Event<'e>, Range<usize>)> {
        self.events.get(self.ix)
    }

    fn peek_event(&self) -> Option<&Event<'e>> {
        self.peek().map(|(event, _)| event)
    }

    fn bump(&mut self) {
        self.ix += 1;
    }

    /// The raw source text of an event range.
    fn raw(&self, range: &Range<usize>) -> &'a str {
        &self.text[range.start - self.offset..range.end - self.offset]
    }

    fn next(&mut self) -> Option<(Event<'e>, Range<usize>)> {
        let item = self.events.get(self.ix).cloned();
        if item.is_some() {
            self.ix += 1;
        }
        item
    }
}

fn is_block_tag(tag: &Tag) -> bool {
    matches!(
        tag,
        Tag::Paragraph
            | Tag::Heading { .. }
            | Tag::CodeBlock(_)
            | Tag::BlockQuote(_)
            | Tag::List(_)
            | Tag::Item
            | Tag::Table(_)
            | Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
    )
}

/// Consume a `Start(tag)` through its matching `End` and produce blocks.
/// Containers this crate does not model splice their children in.
fn parse_started_block(cursor: &mut Cursor) -> Vec<Block> {
    let Some((Event::Start(tag), range)) = cursor.next() else {
        return Vec::new();
    };
    match tag {
        Tag::Paragraph => {
            let content = parse_inline_container(cursor, &InlineStyle::default());
            if content.is_empty() {
                Vec::new()
            } else {
                vec![Block::Paragraph(content)]
            }
        }
        Tag::Heading { level, .. } => vec![Block::Heading {
            level: heading_level(level),
            content: parse_inline_container(cursor, &InlineStyle::default()),
        }],
        Tag::CodeBlock(kind) => {
            let (info, fenced) = match kind {
                CodeBlockKind::Fenced(info) => (info.trim().to_string(), true),
                CodeBlockKind::Indented => (String::new(), false),
            };
            let mut code = String::new();
            let mut src = None;
            loop {
                match cursor.next() {
                    Some((Event::Text(text), text_range)) => {
                        src.get_or_insert(text_range.start);
                        code.push_str(&text);
                    }
                    Some((Event::End(_), _)) | None => break,
                    Some(_) => {}
                }
            }
            if code.ends_with('\n') {
                code.pop();
            }
            let closed = !fenced || fence::is_closed(cursor.raw(&range));
            vec![Block::Code(CodeBlock {
                info,
                code,
                closed,
                src: src.unwrap_or(range.end),
            })]
        }
        Tag::BlockQuote(_) => vec![Block::Quote(parse_block_sequence(cursor))],
        Tag::List(start) => {
            let mut items = Vec::new();
            loop {
                match cursor.peek_event() {
                    Some(Event::Start(Tag::Item)) => {
                        cursor.bump();
                        items.push(parse_list_item(cursor));
                    }
                    Some(Event::End(_)) | None => {
                        cursor.bump();
                        break;
                    }
                    Some(_) => cursor.bump(),
                }
            }
            vec![Block::List(List { start, items })]
        }
        Tag::Table(align) => vec![parse_table(cursor, &align)],
        Tag::HtmlBlock => {
            // Raw HTML shows as its source text.
            let mut inline = Inline::default();
            loop {
                match cursor.next() {
                    Some((Event::Html(text) | Event::Text(text), text_range)) => {
                        inline.push(&text, InlineStyle::default(), text_range.start);
                    }
                    Some((Event::End(_), _)) | None => break,
                    Some(_) => {}
                }
            }
            trim_trailing_newlines(&mut inline);
            if inline.is_empty() {
                Vec::new()
            } else {
                vec![Block::Paragraph(inline)]
            }
        }
        Tag::MetadataBlock(_) => {
            skip_to_end(cursor);
            Vec::new()
        }
        _ => parse_block_sequence(cursor),
    }
}

fn skip_to_end(cursor: &mut Cursor) {
    let mut depth = 0usize;
    while let Some((event, _)) = cursor.next() {
        match event {
            Event::Start(_) => depth += 1,
            Event::End(_) if depth == 0 => break,
            Event::End(_) => depth -= 1,
            _ => {}
        }
    }
}

fn trim_trailing_newlines(inline: &mut Inline) {
    while inline.text.ends_with('\n') {
        inline.text.pop();
        if let Some(last) = inline.spans.last_mut() {
            last.range.end -= 1;
            if last.range.is_empty() {
                inline.spans.pop();
            }
        }
    }
}

fn parse_list_item(cursor: &mut Cursor) -> ListItem {
    // The task marker comes first, either bare (tight items) or as the first
    // event of the item's first paragraph (loose items).
    match cursor.peek_event() {
        Some(Event::TaskListMarker(checked)) => {
            let task = Some(*checked);
            cursor.bump();
            return ListItem {
                task,
                blocks: parse_block_sequence(cursor),
            };
        }
        Some(Event::Start(Tag::Paragraph)) => {
            if let Some((Event::TaskListMarker(checked), _)) = cursor.events.get(cursor.ix + 1) {
                let task = Some(*checked);
                // Skip the paragraph start and the marker, then read the rest
                // of the paragraph.
                cursor.ix += 2;
                let content = parse_inline_container(cursor, &InlineStyle::default());
                let mut blocks = Vec::new();
                if !content.is_empty() {
                    blocks.push(Block::Paragraph(content));
                }
                blocks.extend(parse_block_sequence(cursor));
                return ListItem { task, blocks };
            }
        }
        _ => {}
    }
    ListItem {
        task: None,
        blocks: parse_block_sequence(cursor),
    }
}

/// Parse blocks until the container's `End` (consumed). Bare inline events,
/// which tight list items produce, collect into an implicit paragraph.
fn parse_block_sequence(cursor: &mut Cursor) -> Vec<Block> {
    let mut out = Vec::new();
    let mut pending = Inline::default();
    while let Some(event) = cursor.peek_event() {
        match event {
            Event::End(_) => {
                cursor.bump();
                break;
            }
            Event::Start(tag) if is_block_tag(tag) => {
                flush_paragraph(&mut out, &mut pending);
                out.extend(parse_started_block(cursor));
            }
            Event::Rule => {
                flush_paragraph(&mut out, &mut pending);
                cursor.bump();
                out.push(Block::Rule);
            }
            _ => parse_inline_event(cursor, &mut pending, &InlineStyle::default()),
        }
    }
    flush_paragraph(&mut out, &mut pending);
    out
}

fn flush_paragraph(out: &mut Vec<Block>, pending: &mut Inline) {
    if !pending.is_empty() {
        let inline = autolink::autolink(std::mem::take(pending));
        out.push(Block::Paragraph(inline));
    }
}

fn parse_table(cursor: &mut Cursor, align: &[Alignment]) -> Block {
    let align = align
        .iter()
        .map(|align| match align {
            Alignment::None => Align::None,
            Alignment::Left => Align::Left,
            Alignment::Center => Align::Center,
            Alignment::Right => Align::Right,
        })
        .collect();
    let mut header = Vec::new();
    let mut rows = Vec::new();
    loop {
        match cursor.peek_event() {
            Some(Event::Start(Tag::TableHead)) => {
                cursor.bump();
                header = parse_table_cells(cursor);
            }
            Some(Event::Start(Tag::TableRow)) => {
                cursor.bump();
                rows.push(parse_table_cells(cursor));
            }
            Some(Event::End(_)) | None => {
                cursor.bump();
                break;
            }
            Some(_) => cursor.bump(),
        }
    }
    Block::Table(Table {
        align,
        header,
        rows,
    })
}

fn parse_table_cells(cursor: &mut Cursor) -> Vec<Inline> {
    let mut cells = Vec::new();
    loop {
        match cursor.peek_event() {
            Some(Event::Start(Tag::TableCell)) => {
                cursor.bump();
                cells.push(parse_inline_container(cursor, &InlineStyle::default()));
            }
            Some(Event::End(_)) | None => {
                cursor.bump();
                break;
            }
            Some(_) => cursor.bump(),
        }
    }
    cells
}

/// Parse inline events until the container's `End` (consumed).
fn parse_inline_container(cursor: &mut Cursor, style: &InlineStyle) -> Inline {
    let mut inline = Inline::default();
    collect_inline(cursor, &mut inline, style);
    autolink::autolink(inline)
}

fn collect_inline(cursor: &mut Cursor, inline: &mut Inline, style: &InlineStyle) {
    while let Some(event) = cursor.peek_event() {
        if matches!(event, Event::End(_)) {
            cursor.bump();
            break;
        }
        parse_inline_event(cursor, inline, style);
    }
}

fn is_br(html: &str) -> bool {
    let tag = html.trim().to_ascii_lowercase();
    matches!(tag.as_str(), "<br>" | "<br/>" | "<br />")
}

fn parse_inline_event(cursor: &mut Cursor, inline: &mut Inline, style: &InlineStyle) {
    let Some((event, range)) = cursor.next() else {
        return;
    };
    match event {
        Event::Text(text) => inline.push(&text, style.clone(), range.start),
        Event::Code(text) => {
            let mut code = style.clone();
            code.code = true;
            // The code text starts after the opening backticks.
            let ticks = cursor
                .raw(&range)
                .bytes()
                .take_while(|b| *b == b'`')
                .count();
            inline.push(&text, code, range.start + ticks);
        }
        Event::SoftBreak => inline.push(" ", style.clone(), range.start),
        Event::HardBreak => inline.push("\n", style.clone(), range.start),
        Event::InlineHtml(html) if is_br(&html) => inline.push("\n", style.clone(), range.start),
        Event::Html(text) | Event::InlineHtml(text) => {
            inline.push(&text, style.clone(), range.start)
        }
        Event::InlineMath(text) | Event::DisplayMath(text) => {
            inline.push(&text, style.clone(), range.start)
        }
        Event::FootnoteReference(label) => {
            inline.push(&format!("[{label}]"), style.clone(), range.start)
        }
        // A marker that is not first in an item has no checkbox to become.
        Event::TaskListMarker(checked) => inline.push(
            if checked { "[x] " } else { "[ ] " },
            style.clone(),
            range.start,
        ),
        Event::Start(tag) => {
            let mut inner = style.clone();
            match tag {
                Tag::Emphasis => inner.emphasis = true,
                Tag::Strong => inner.strong = true,
                Tag::Strikethrough => inner.strikethrough = true,
                Tag::Link { dest_url, .. } => inner.link = Some(dest_url.to_string().into()),
                Tag::Image {
                    dest_url, title, ..
                } => {
                    let mut alt = Inline::default();
                    collect_inline(cursor, &mut alt, style);
                    let image = ImageRef {
                        url: dest_url.to_string(),
                        alt: alt.text.clone(),
                        title: title.to_string(),
                    };
                    inner.image = Some(Arc::new(image));
                    let text = if alt.text.is_empty() {
                        // Keep a character so the image owns a span.
                        "\u{FFFC}".to_string()
                    } else {
                        alt.text
                    };
                    inline.push(&text, inner, range.start);
                    return;
                }
                _ => {}
            }
            let mut nested = Inline::default();
            collect_inline(cursor, &mut nested, &inner);
            inline.append(nested);
            merge_adjacent(inline);
        }
        _ => {}
    }
}

/// Merge neighboring spans with equal styles and contiguous sources after an
/// append of nested content.
fn merge_adjacent(inline: &mut Inline) {
    let mut merged: Vec<Span> = Vec::with_capacity(inline.spans.len());
    for span in inline.spans.drain(..) {
        if let Some(last) = merged.last_mut()
            && last.style == span.style
            && span.style.image.is_none()
            && last.range.end == span.range.start
            && last.src + last.range.len() == span.src
        {
            last.range.end = span.range.end;
            continue;
        }
        merged.push(span);
    }
    inline.spans = merged;
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// The visible text of a block, for tests and plain-text fallbacks.
pub fn block_text(block: &Block) -> String {
    fn walk(block: &Block, out: &mut String) {
        match block {
            Block::Paragraph(inline)
            | Block::Heading {
                content: inline, ..
            } => out.push_str(&inline.text),
            Block::Code(code) => out.push_str(&code.code),
            Block::Quote(children) => children.iter().for_each(|child| walk(child, out)),
            Block::List(list) => list
                .items
                .iter()
                .flat_map(|item| item.blocks.iter())
                .for_each(|child| walk(child, out)),
            Block::Table(table) => {
                for cell in table.header.iter().chain(table.rows.iter().flatten()) {
                    out.push_str(&cell.text);
                }
            }
            Block::Rule => {}
        }
    }
    let mut out = String::new();
    walk(block, &mut out);
    out
}

#[cfg(test)]
mod tests;
