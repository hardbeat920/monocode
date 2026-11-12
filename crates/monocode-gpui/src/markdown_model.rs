//! Native Markdown model for AgentMarkdown's Streamdown elements.
use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd,
};
use std::{ops::Range, sync::Arc};

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub link: Option<String>,
    pub task: Option<TaskMarker>,
    pub math: bool,
    pub search: Option<bool>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TaskMarker {
    pub checked: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InlineRun {
    pub text: String,
    pub style: InlineStyle,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TableAlign {
    Left,
    Center,
    Right,
}
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::enum_variant_names)] // Markdown terms: code block and block quote.
pub(crate) enum Block {
    Paragraph {
        runs: Vec<InlineRun>,
    },
    Heading {
        level: u8,
        runs: Vec<InlineRun>,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    List {
        ordered_start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    BlockQuote {
        children: Vec<Block>,
    },
    Table {
        header: Vec<Vec<InlineRun>>,
        rows: Vec<Vec<Vec<InlineRun>>>,
        align: Vec<TableAlign>,
    },
    Rule,
    Math {
        tex: String,
    },
    /// `![caption](visual:id)` on its own line: a published visual placed
    /// in the prose.
    Visual {
        id: String,
        caption: String,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TopBlock {
    range: Range<usize>,
    pub block: Block,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct BlockTree {
    pub blocks: Vec<Arc<TopBlock>>,
}
#[derive(Default)]
pub(crate) struct MarkdownParser {
    source: String,
    tree: BlockTree,
    display: Vec<(Arc<TopBlock>, BlockTree)>,
    full_display: Option<DisplayCache>,
    display_full: bool,
    display_dirty: bool,
    full_only: bool,
    #[cfg(test)]
    parsed_bytes: usize,
}
impl MarkdownParser {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_text(&mut self, text: &str) {
        if self.source == text {
            return;
        }
        let append = text.starts_with(&self.source);
        self.display_dirty = true;
        // The display mender treats raw HTML as prose and can observe fence
        // markers that the canonical parser hides inside HTML. Preserve that
        // whole-source context instead of repairing those blocks separately.
        self.display_full = if append {
            self.display_full || text[self.source.len()..].contains(['<', '`'])
        } else {
            text.contains(['<', '`'])
        };
        // Reference definitions can change links anywhere in the document.
        // Check the new bytes (including a split `]:`) and use a full parse
        // for that uncommon case, including definitions inside containers.
        self.full_only = if append {
            self.full_only
                || text.as_bytes()[self.source.len().saturating_sub(1)..]
                    .windows(2)
                    .any(|pair| pair == b"]:")
        } else {
            text.contains("]:")
        };
        let mut boundary = 0;
        if append && !self.full_only && self.tree.blocks.len() > 2 {
            // Keep two trailing blocks reparsable: an append can turn a
            // paragraph into a table/heading or merge it into a loose list.
            let n = self.tree.blocks.len();
            boundary = self.tree.blocks[n - 2].range.start;
            let previous = self.tree.blocks[n - 3].range.start;
            let between = &text[previous..boundary];
            if matches!(
                self.tree.blocks[n - 3].block,
                Block::List { .. } | Block::BlockQuote { .. }
            ) && between[between.trim_end().len()..].matches('\n').count() < 2
            {
                boundary = previous;
            }
            boundary = text[..boundary].rfind('\n').map_or(0, |at| at + 1);
        }
        self.source
            .truncate(if append { self.source.len() } else { 0 });
        self.source.push_str(&text[self.source.len()..]);
        #[cfg(test)]
        {
            self.parsed_bytes += text.len() - boundary;
        }
        self.tree.blocks.retain(|block| block.range.end <= boundary);
        self.tree
            .blocks
            .extend(parse_at(&text[boundary..], boundary, false).blocks);
        // Display repairs flush at blank lines, not Markdown block borders.
        // Adjacent blocks may therefore share one synthetic closer.
        self.display_full |= self.tree.blocks.windows(2).any(|pair| {
            let between = &text[pair[0].range.start..pair[1].range.start];
            between[between.trim_end().len()..].matches('\n').count() < 2
        });
        self.display.truncate(
            self.tree
                .blocks
                .iter()
                .zip(&self.display)
                .take_while(|(block, (old, _))| Arc::ptr_eq(block, old))
                .count(),
        );
    }
    pub fn tree(&self) -> &BlockTree {
        &self.tree
    }
    pub fn display_tree(&mut self) -> BlockTree {
        if self.full_only || self.display_full {
            if !self.display_dirty {
                return self
                    .full_display
                    .as_ref()
                    .map_or_else(|| self.tree.clone(), |display| display.tree.clone());
            }
            self.display_dirty = false;
            self.display = Vec::new();
            let repaired = mend_stream(&self.source);
            if repaired == self.source {
                self.full_display = None;
                return self.tree.clone();
            }
            let display = self.full_display.get_or_insert_with(|| DisplayCache {
                source: self.source.clone(),
                tree: self.tree.clone(),
            });
            display.set_text(repaired);
            // Unrepaired prefix blocks share canonical storage, including on
            // the next commit when its closing marker has finally arrived.
            for (display, canonical) in display.tree.blocks.iter_mut().zip(&self.tree.blocks) {
                if !Arc::ptr_eq(display, canonical) && display == canonical {
                    *display = canonical.clone();
                }
            }
            return display.tree.clone();
        }
        self.full_display = None;
        for (index, block) in self.tree.blocks.iter().enumerate().skip(self.display.len()) {
            let end = self
                .tree
                .blocks
                .get(index + 1)
                .map_or(self.source.len(), |next| next.range.start);
            let source = &self.source[block.range.start..end];
            let repaired = mend_stream(source);
            let display = if repaired == source {
                BlockTree {
                    blocks: vec![block.clone()],
                }
            } else {
                parse_full(&repaired)
            };
            self.display.push((block.clone(), display));
        }
        BlockTree {
            blocks: self
                .display
                .iter()
                .flat_map(|(_, tree)| tree.blocks.iter().cloned())
                .collect(),
        }
    }
    pub fn finish_display(&mut self) {
        self.display = Vec::new();
        self.full_display = None;
        self.display_dirty = true;
    }
}

/// Synthetic closing markers may change at the tail of a display repair.
/// Keep the prefix before that edit and preserve whole-source fence context.
#[derive(Default)]
struct DisplayCache {
    source: String,
    tree: BlockTree,
}
impl DisplayCache {
    fn set_text(&mut self, text: String) {
        if self.source == text {
            return;
        }
        let common = self
            .source
            .bytes()
            .zip(text.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let prefix = self
            .tree
            .blocks
            .partition_point(|block| block.range.end <= common);
        let boundary = if text.contains("]:") || self.source.contains("]:") || prefix < 3 {
            0
        } else {
            let at = self.tree.blocks[prefix - 3].range.start;
            text[..at].rfind('\n').map_or(0, |at| at + 1)
        };
        self.tree.blocks.retain(|block| block.range.end <= boundary);
        self.tree
            .blocks
            .extend(parse_at(&text[boundary..], boundary, false).blocks);
        self.source = text;
    }
}

// streamingMarkdown.ts / remend: repair prose blocks while leaving literal
// fences untouched. Repairs affect only the display tree, never stored text.
fn mend_stream(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 32);
    let mut prose = String::new();
    let mut fence = None;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied().unwrap_or(0);
        let width = trimmed.bytes().take_while(|b| *b == marker).count();
        let is_fence = indent <= 3 && matches!(marker, b'`' | b'~') && width >= 3;
        if let Some((opened, count)) = fence {
            out.push_str(line);
            if is_fence && marker == opened && width >= count && trimmed[width..].trim().is_empty()
            {
                fence = None;
            }
        } else if is_fence {
            out.push_str(&mend_prose(&prose));
            prose.clear();
            out.push_str(line);
            fence = Some((marker, width));
        } else {
            prose.push_str(line);
            if line.trim().is_empty() {
                out.push_str(&mend_prose(&prose));
                prose.clear();
            }
        }
    }
    out.push_str(&mend_prose(&prose));
    out
}
fn mend_prose(source: &str) -> String {
    let mut text = source.to_owned();
    let mut stack: Vec<String> = Vec::new();
    let mut brackets = Vec::new();
    let mut code = None;
    let mut at = 0;
    while at < text.len() {
        let byte = text.as_bytes()[at];
        if byte == b'\\' {
            at += 1;
            if at < text.len() {
                at += text[at..].chars().next().unwrap().len_utf8();
            }
            continue;
        }
        if byte == b'`' {
            let n = text[at..].bytes().take_while(|b| *b == b'`').count();
            match code {
                Some(width) if width == n => code = None,
                None => code = Some(n),
                _ => {}
            }
            at += n;
            continue;
        }
        if code.is_some() {
            at += text[at..].chars().next().unwrap().len_utf8();
            continue;
        }
        if byte == b'<'
            && !text[at..].contains('>')
            && text[at + 1..]
                .trim_start_matches('/')
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
        {
            text.truncate(at);
            break;
        }
        if byte == b'[' {
            brackets.push(at);
        }
        if byte == b']'
            && let Some(open) = brackets.pop()
            && text.as_bytes().get(at + 1) == Some(&b'(')
        {
            let mut depth = 1;
            let mut end = at + 2;
            while end < text.len() {
                match text.as_bytes()[end] {
                    b'\\' => {
                        end = (end + 2).min(text.len());
                        continue;
                    }
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            if depth != 0 {
                if open > 0 && text.as_bytes()[open - 1] == b'!' {
                    text.truncate(open - 1);
                } else {
                    text.truncate(at + 2);
                    text.push_str("streamdown:incomplete-link)");
                }
                break;
            }
            at = end + 1;
            continue;
        }
        if matches!(byte, b'*' | b'_' | b'~') {
            let n = text[at..].bytes().take_while(|b| *b == byte).count();
            let previous = text[..at].chars().next_back();
            let next = text[at + n..].chars().next();
            let line_start = text[..at]
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .trim()
                .is_empty();
            let line_rest = text[at..].split('\n').next().unwrap_or("");
            let rule = n >= 3
                && line_start
                && line_rest
                    .chars()
                    .all(|c| c == byte as char || c.is_whitespace());
            let list = line_start && n == 1 && byte == b'*' && next.is_none_or(char::is_whitespace);
            let intraword = byte == b'_'
                && previous.is_some_and(char::is_alphanumeric)
                && next.is_some_and(char::is_alphanumeric);
            if !rule && !list && !intraword && (byte != b'~' || n == 2) && n <= 3 {
                let delimiter = text[at..at + n].to_owned();
                if previous.is_some_and(|c| !c.is_whitespace()) && stack.last() == Some(&delimiter)
                {
                    stack.pop();
                } else if next.is_some_and(|c| !c.is_whitespace()) {
                    stack.push(delimiter);
                }
            }
            at += n;
        } else {
            at += text[at..].chars().next().unwrap().len_utf8();
        }
    }
    if code.is_none()
        && let Some(open) = brackets.last().copied()
    {
        if open > 0 && text.as_bytes()[open - 1] == b'!' {
            text.truncate(open - 1);
        } else if !text[open + 1..].trim().is_empty() {
            text.push_str("](streamdown:incomplete-link)");
        }
    }
    let whitespace = text.len() - text.trim_end().len();
    let tail = text.split_off(text.len() - whitespace);
    if let Some(width) = code {
        text.push_str(&"`".repeat(width));
    }
    for delimiter in stack.into_iter().rev() {
        text.push_str(&delimiter);
    }
    text.push_str(&tail);
    let last = text.trim_end().rsplit('\n').next().unwrap_or("");
    if matches!(last.trim(), "-" | "--" | "=" | "==") && text.trim_end().contains('\n') {
        let at = text.trim_end().len();
        text.insert(at, '\u{200b}');
    }
    text
}
/// The events of one top-level block, with the source they were parsed from.
struct Inline<'a> {
    events: Vec<Option<(Event<'a>, Range<usize>)>>,
    at: usize,
    source: &'a str,
    /// Where the last leaf or closed span ended. Container events span their
    /// whole element, so a closing delimiter starts here.
    leaf_end: usize,
    /// The `leaf_end` before the event last taken.
    before: usize,
    /// Enclosing spans a bare link closed, and the source left after it.
    close: usize,
    spill: Option<Range<usize>>,
}
impl<'a> Inline<'a> {
    fn new(events: Vec<(Event<'a>, Range<usize>)>, source: &'a str, start: usize) -> Self {
        Self {
            events: events.into_iter().map(Some).collect(),
            at: 0,
            source,
            leaf_end: start,
            before: start,
            close: 0,
            spill: None,
        }
    }
    fn peek(&self) -> Option<&(Event<'a>, Range<usize>)> {
        self.events.get(self.at)?.as_ref()
    }
    fn next(&mut self) -> Option<(Event<'a>, Range<usize>)> {
        let (event, range) = self.events.get_mut(self.at)?.take()?;
        self.at += 1;
        if !matches!(event, Event::Start(_)) {
            self.before = self.leaf_end;
            self.leaf_end = range.end;
        }
        Some((event, range))
    }
}
pub(crate) fn parse_full(text: &str) -> BlockTree {
    parse_with_breaks(text, false)
}

/// Notes preserve prose line breaks, matching AgentMarkdown's hardBreaks flag.
pub(crate) fn parse_full_hard_breaks(text: &str) -> BlockTree {
    parse_with_breaks(text, true)
}

fn parse_with_breaks(text: &str, hard_breaks: bool) -> BlockTree {
    parse_at(text, 0, hard_breaks)
}

fn parse_at(text: &str, base: usize, hard_breaks: bool) -> BlockTree {
    let parser = Parser::new_ext(
        text,
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_MATH,
    );
    let mut tree = BlockTree::default();
    let mut pending = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (event, range) in parser.into_offset_iter() {
        if pending.is_empty() {
            start = range.start;
        }
        match &event {
            Event::Start(_) => depth += 1,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        let event = if hard_breaks && matches!(event, Event::SoftBreak) {
            Event::HardBreak
        } else {
            event
        };
        pending.push((event, range.clone()));
        if depth == 0 {
            let mut inline = Inline::new(std::mem::take(&mut pending), text, start);
            tree.blocks
                .extend(blocks(&mut inline, None, 0).into_iter().map(|block| {
                    Arc::new(TopBlock {
                        range: base + start..base + range.end,
                        block,
                    })
                }));
        }
    }
    tree
}
fn blocks(cx: &mut Inline<'_>, until: Option<TagEnd>, depth: usize) -> Vec<Block> {
    let mut out = Vec::new();
    let style = InlineStyle::default();
    while let Some((event, range)) = cx.next() {
        match event {
            Event::End(end) if until == Some(end) => break,
            Event::Start(
                tag @ (Tag::Strong
                | Tag::Emphasis
                | Tag::Strikethrough
                | Tag::Link { .. }
                | Tag::Image { .. }),
            ) => {
                let mut runs = Vec::new();
                inline_span(cx, tag, style.clone(), true, &mut runs);
                settle(cx, &style, &mut runs);
                extend_paragraph(&mut out, runs);
            }
            Event::Start(tag) if depth < 32 => match tag {
                Tag::Paragraph => {
                    if let Some(visual) = placed_visual(cx) {
                        out.push(visual);
                        continue;
                    }
                    out.push(Block::Paragraph {
                        runs: inlines(cx, TagEnd::Paragraph),
                    })
                }
                Tag::Heading { level, .. } => {
                    let number = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    out.push(Block::Heading {
                        level: number,
                        runs: inlines(cx, TagEnd::Heading(level)),
                    });
                }
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(info) if !info.is_empty() => Some(info.into_string()),
                        _ => None,
                    };
                    let mut code = String::new();
                    while let Some((event, _)) = cx.next() {
                        match event {
                            Event::End(TagEnd::CodeBlock) => break,
                            Event::Text(text) => code.push_str(&text),
                            _ => {}
                        }
                    }
                    out.push(Block::CodeBlock { language, code });
                }
                Tag::BlockQuote(kind) => out.push(Block::BlockQuote {
                    children: blocks(cx, Some(TagEnd::BlockQuote(kind)), depth + 1),
                }),
                Tag::List(start) => {
                    let mut items = Vec::new();
                    while let Some((event, _)) = cx.next() {
                        match event {
                            Event::End(TagEnd::List(_)) => break,
                            Event::Start(Tag::Item) => {
                                items.push(blocks(cx, Some(TagEnd::Item), depth + 1))
                            }
                            _ => {}
                        }
                    }
                    out.push(Block::List {
                        ordered_start: start,
                        items,
                    });
                }
                Tag::Table(alignment) => {
                    let align = alignment
                        .into_iter()
                        .map(|a| match a {
                            Alignment::Center => TableAlign::Center,
                            Alignment::Right => TableAlign::Right,
                            _ => TableAlign::Left,
                        })
                        .collect();
                    let mut rows = Vec::new();
                    let mut row = Vec::new();
                    let mut header = Vec::new();
                    let mut head = false;
                    while let Some((event, _)) = cx.next() {
                        match event {
                            Event::End(TagEnd::Table) => break,
                            Event::Start(Tag::TableHead) => head = true,
                            Event::Start(Tag::TableCell) => {
                                row.push(inlines(cx, TagEnd::TableCell))
                            }
                            Event::End(TagEnd::TableHead) => {
                                header = std::mem::take(&mut row);
                                head = false;
                            }
                            Event::End(TagEnd::TableRow) if !head => {
                                rows.push(std::mem::take(&mut row))
                            }
                            _ => {}
                        }
                    }
                    out.push(Block::Table {
                        header,
                        rows,
                        align,
                    });
                }
                _ => {}
            },
            Event::Rule => out.push(Block::Rule),
            Event::DisplayMath(tex) => out.push(Block::Math {
                tex: tex.into_string(),
            }),
            // Tight list items have no paragraph event around their inline runs.
            event @ (Event::Text(_)
            | Event::Code(_)
            | Event::TaskListMarker(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::InlineMath(_)) => {
                let mut runs = vec![];
                append_event(cx, event, range, &style, true, &mut runs);
                settle(cx, &style, &mut runs);
                while let Some((next, _)) = cx.peek() {
                    // A closing delimiter whose span a bare link opened stays text.
                    let orphan = matches!(
                        next,
                        Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough)
                    );
                    if !orphan
                        && matches!(
                            next,
                            Event::Start(
                                Tag::List(_)
                                    | Tag::BlockQuote(_)
                                    | Tag::CodeBlock(_)
                                    | Tag::Paragraph
                            ) | Event::End(_)
                        )
                    {
                        break;
                    }
                    let (next, range) = cx.next().unwrap();
                    match next {
                        Event::Start(tag) => inline_span(cx, tag, style.clone(), true, &mut runs),
                        event => append_event(cx, event, range, &style, true, &mut runs),
                    }
                    settle(cx, &style, &mut runs);
                }
                extend_paragraph(&mut out, runs);
            }
            _ => {}
        }
    }
    out
}
fn extend_paragraph(out: &mut Vec<Block>, runs: Vec<InlineRun>) {
    if let Some(Block::Paragraph { runs: previous }) = out.last_mut() {
        previous.extend(runs);
    } else {
        out.push(Block::Paragraph { runs });
    }
}
/// A paragraph that is only a `visual:` image places that visual. Consumes
/// the paragraph when it matches; leaves the events untouched otherwise.
fn placed_visual(cx: &mut Inline<'_>) -> Option<Block> {
    let id = match cx.peek()? {
        (Event::Start(Tag::Image { dest_url, .. }), _) => dest_url
            .strip_prefix("visual:")
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 96
                    && id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            })?
            .to_owned(),
        _ => return None,
    };
    cx.next();
    let mut caption = String::new();
    while let Some((event, _)) = cx.next() {
        match event {
            Event::End(TagEnd::Image) => break,
            Event::Text(text) | Event::Code(text) => caption.push_str(&text),
            _ => {}
        }
    }
    // Anything after the image on the same line stays prose.
    let rest = inlines(cx, TagEnd::Paragraph);
    let rest: String = rest.iter().map(|r| r.text.as_str()).collect();
    if !rest.trim().is_empty() {
        caption = format!("{caption} {}", rest.trim()).trim().to_owned();
    }
    Some(Block::Visual { id, caption })
}
fn inlines(cx: &mut Inline<'_>, end: TagEnd) -> Vec<InlineRun> {
    let style = InlineStyle::default();
    let mut out = Vec::new();
    while let Some((event, range)) = cx.next() {
        match event {
            Event::End(tag) if tag == end => break,
            Event::Start(tag) => inline_span(cx, tag, style.clone(), true, &mut out),
            event => append_event(cx, event, range, &style, true, &mut out),
        }
        settle(cx, &style, &mut out);
    }
    out
}
fn inline_span(
    cx: &mut Inline<'_>,
    tag: Tag<'_>,
    mut style: InlineStyle,
    autolink: bool,
    out: &mut Vec<InlineRun>,
) {
    let end = tag.to_end();
    // Link text and image alt text never autolink.
    let autolink = autolink && !matches!(tag, Tag::Link { .. } | Tag::Image { .. });
    match tag {
        Tag::Strong => style.bold = true,
        Tag::Emphasis => style.italic = true,
        Tag::Strikethrough => style.strikethrough = true,
        Tag::Link {
            link_type: LinkType::Email,
            dest_url,
            ..
        } => style.link = Some(format!("mailto:{dest_url}")),
        Tag::Link { dest_url, .. } if dest_url.as_ref() != "streamdown:incomplete-link" => {
            style.link = Some(dest_url.into_string());
        }
        _ => {}
    }
    while let Some((event, range)) = cx.next() {
        match event {
            Event::End(tag) if tag == end => break,
            Event::Start(tag) => inline_span(cx, tag, style.clone(), autolink, out),
            event => append_event(cx, event, range, &style, autolink, out),
        }
        if !settle(cx, &style, out) {
            return;
        }
    }
}
/// After an inline event: whether this span goes on. A bare link that ran
/// past its closing delimiter ends it, and the source after such a link
/// belongs to the span that remains open.
fn settle(cx: &mut Inline<'_>, style: &InlineStyle, out: &mut Vec<InlineRun>) -> bool {
    loop {
        if cx.close > 0 {
            cx.close -= 1;
            return false;
        }
        let Some(rest) = cx.spill.take() else {
            return true;
        };
        append_source(cx, rest, style, out);
    }
}
fn append_event(
    cx: &mut Inline<'_>,
    event: Event<'_>,
    range: Range<usize>,
    style: &InlineStyle,
    autolink: bool,
    out: &mut Vec<InlineRun>,
) {
    let mut style = style.clone();
    let text = match event {
        Event::Text(text) => {
            // Entities and escapes are decoded; bare links are found in
            // text that reads as the source does.
            if autolink && style.link.is_none() && text.as_ref() == &cx.source[range.clone()] {
                return append_source(cx, range, &style, out);
            }
            text.into_string()
        }
        Event::Code(text) => {
            style.code = true;
            text.into_string()
        }
        Event::SoftBreak => " ".into(),
        Event::HardBreak => "\n".into(),
        Event::TaskListMarker(checked) => {
            style.task = Some(TaskMarker { checked });
            "\u{200b}".into()
        }
        Event::InlineMath(text) => {
            style.math = true;
            text.into_string()
        }
        // A span whose opening delimiter a bare link swallowed: its closing
        // delimiter stays text, as GFM renders it.
        Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => {
            cx.source[cx.before..range.end].to_owned()
        }
        _ => return,
    };
    out.push(InlineRun { text, style });
}
fn push_text(out: &mut Vec<InlineRun>, text: &str, style: &InlineStyle) {
    if !text.is_empty() {
        out.push(InlineRun {
            text: text.to_owned(),
            style: style.clone(),
        });
    }
}
/// What the next event is to a bare link running past the current text.
enum Covered {
    Opener,
    Closer,
    Text { verbatim: bool },
    SpanEnd,
    Leaf,
}
fn covered(event: &Event<'_>, range: &Range<usize>, source: &str) -> Covered {
    match event {
        Event::Start(Tag::Strong | Tag::Emphasis | Tag::Strikethrough) => Covered::Opener,
        Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => Covered::Closer,
        Event::Text(text) => Covered::Text {
            verbatim: text.as_ref() == &source[range.clone()],
        },
        Event::End(_) => Covered::SpanEnd,
        _ => Covered::Leaf,
    }
}
/// Where a bare link that runs past the current event (ending at `pos_end`)
/// has to stop: before the first event it cannot take, or at `end`.
fn link_end(cx: &Inline<'_>, mut end: usize, mut pos_end: usize) -> usize {
    for slot in &cx.events[cx.at..] {
        let Some((event, r)) = slot else { break };
        match covered(event, r, cx.source) {
            Covered::Opener => {
                if r.start >= end {
                    break;
                }
            }
            Covered::Closer => {
                if pos_end >= end {
                    break;
                }
                if r.end > end {
                    end = pos_end;
                    break;
                }
                pos_end = r.end;
            }
            Covered::Text { verbatim } => {
                if r.start >= end {
                    break;
                }
                // An entity is not cut in half.
                if r.end > end && !verbatim {
                    end = r.start;
                    break;
                }
                pos_end = r.end;
                if r.end >= end {
                    break;
                }
            }
            Covered::SpanEnd => {
                end = end.min(pos_end);
                break;
            }
            Covered::Leaf => {
                end = end.min(r.start);
                break;
            }
        }
    }
    end
}
/// Appends verbatim source, linking bare URLs and emails like GFM's
/// extended autolinks. The parser splits emphasis delimiters, entities and
/// escapes out of the text around them, so a link found here follows the
/// source past this event and takes the events it covers.
fn append_source(
    cx: &mut Inline<'_>,
    mut range: Range<usize>,
    style: &InlineStyle,
    out: &mut Vec<InlineRun>,
) {
    let source = cx.source;
    loop {
        let Some(link) = bare_link(source, range.clone()) else {
            push_text(out, &source[range], style);
            return;
        };
        let start = link.range.start;
        push_text(out, &source[range.start..start], style);
        let mut end = link.range.end;
        if end > range.end {
            end = link_end(cx, end, range.end);
            // Stopping short can leave sentence punctuation at the end.
            if end < link.range.end {
                end = trim_link_end(source, start, end);
            }
        }
        if end <= start + link.prefix() {
            // Nothing valid remains: the first character stays prose.
            push_text(out, &source[start..start + 1], style);
            range = start + 1..range.end;
            continue;
        }
        // Take the events the link covers.
        let mut rest = None;
        let (mut close, mut depth) = (0, 0usize);
        while let Some((event, r)) = cx.peek() {
            let r = r.clone();
            match covered(event, &r, source) {
                Covered::Opener if r.start < end => {
                    depth += 1;
                    cx.next();
                }
                Covered::Closer if cx.leaf_end < end && r.end <= end => {
                    if depth > 0 {
                        depth -= 1;
                    } else {
                        close += 1;
                    }
                    cx.next();
                }
                Covered::Text { .. } if r.start < end => {
                    cx.next();
                    if r.end > end {
                        rest = Some(end..r.end);
                        break;
                    }
                }
                _ => break,
            }
        }
        let mut linked = style.clone();
        linked.link = Some(link.target(&source[start..end]));
        push_text(out, &source[start..end], &linked);
        if rest.is_none() && end < range.end {
            rest = Some(end..range.end);
        }
        if close > 0 {
            cx.close = close;
            cx.spill = rest;
            return;
        }
        match rest {
            Some(rest) => range = rest,
            None => return,
        }
    }
}
#[derive(Clone, Copy)]
enum LinkKind {
    Http { scheme: usize },
    Www,
    Email,
}
struct BareLink {
    range: Range<usize>,
    kind: LinkKind,
}
impl BareLink {
    fn prefix(&self) -> usize {
        match self.kind {
            LinkKind::Http { scheme } => scheme,
            LinkKind::Www => 4,
            LinkKind::Email => 0,
        }
    }
    fn target(&self, text: &str) -> String {
        match self.kind {
            LinkKind::Http { scheme } => {
                format!("{}{}", text[..scheme].to_ascii_lowercase(), &text[scheme..])
            }
            LinkKind::Www => format!("http://{text}"),
            LinkKind::Email => format!("mailto:{text}"),
        }
    }
}
/// The first bare link starting inside `range`. It may end past the range,
/// where the parser moved the rest of it into other events.
fn bare_link(source: &str, range: Range<usize>) -> Option<BareLink> {
    let bytes = source.as_bytes();
    let mut at = range.start;
    while at < range.end {
        at += bytes[at..range.end]
            .iter()
            .position(|b| matches!(b, b'h' | b'H' | b'w' | b'W' | b'@'))?;
        let link = if bytes[at] == b'@' {
            email_at(source, range.start, at)
        } else {
            url_at(source, range.start, at)
        };
        if link.is_some() {
            return link;
        }
        at += 1;
    }
    None
}
fn is_local(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._+".contains(c)
}
fn is_label(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_.".contains(c)
}
/// An email around the `@` at `at`, with its local part no earlier than `from`.
fn email_at(source: &str, from: usize, at: usize) -> Option<BareLink> {
    let start = source[from..at]
        .char_indices()
        .rev()
        .find(|(_, c)| !is_local(*c))
        .map_or(from, |(ix, c)| from + ix + c.len_utf8());
    if start == at || source[..start].ends_with('/') {
        return None;
    }
    // GFM: the domain has a dot, ends in a letter and stops before `..`.
    let rest = &source[at + 1..];
    let mut domain = &rest[..rest.find(|c| !is_label(c)).unwrap_or(rest.len())];
    if let Some(ix) = domain.find("..") {
        domain = &domain[..ix];
    }
    let domain = domain.trim_end_matches('.');
    if !domain.contains('.') || !domain.ends_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    Some(BareLink {
        range: start..at + 1 + domain.len(),
        kind: LinkKind::Email,
    })
}
/// A URL or www address starting at `at`.
fn url_at(source: &str, from: usize, at: usize) -> Option<BareLink> {
    let rest = &source[at..];
    let has = |prefix: &str| {
        rest.get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    };
    let kind = if has("https://") {
        LinkKind::Http { scheme: 8 }
    } else if has("http://") {
        LinkKind::Http { scheme: 7 }
    } else if has("www.") {
        LinkKind::Www
    } else {
        return None;
    };
    let before = source[..at].chars().next_back();
    let bounded = match kind {
        LinkKind::Http { .. } => !before.is_some_and(|c| c.is_ascii_alphabetic()),
        // GFM: a www address follows a line start, whitespace or `*_~([]`,
        // so a path like `src/www.config.js` stays text.
        _ => before.is_none_or(|c| c.is_whitespace() || "*_~([]".contains(c)),
    };
    if !bounded {
        return None;
    }
    // `www.me@a.com` is an email.
    let local_end = at + rest.find(|c| !is_local(c)).unwrap_or(rest.len());
    if source[local_end..].starts_with('@')
        && let Some(email) = email_at(source, from, local_end)
        && email.range.start <= at
    {
        return Some(email);
    }
    let prefix = BareLink {
        range: at..at,
        kind,
    }
    .prefix();
    let after = &source[at + prefix..];
    let domain_len = after.find(|c| !is_label(c)).unwrap_or(after.len());
    if domain_len == 0 {
        return None;
    }
    // The path runs to whitespace or `<`. CJK punctuation ends it too, since
    // prose in those scripts has no space before it.
    let stop =
        |c: char| c.is_whitespace() || c == '<' || "，。！？；：、（）「」《》【】".contains(c);
    let path = &after[domain_len..];
    let mut end = at + prefix + domain_len + path.find(stop).unwrap_or(path.len());
    // Broken link syntax: `[https://a.com](https://b` is two links, not one.
    if let Some(ix) = source[at..end].find("](") {
        end = at + ix;
    }
    end = trim_link_end(source, at, end);
    if end <= at + prefix {
        return None;
    }
    // GFM: no underscore in the last two labels of the domain. A trimmed
    // delimiter, as in `_www.a.com_`, is not part of it.
    let domain = &source[at + prefix..(at + prefix + domain_len).min(end)];
    if domain.rsplit('.').take(2).any(|label| label.contains('_')) {
        return None;
    }
    Some(BareLink {
        range: at..end,
        kind,
    })
}
/// Drops the sentence punctuation a URL runs into, as GFM does: trailing
/// `?!.,:*_~'"]`, unbalanced `)` and a final entity reference.
fn trim_link_end(source: &str, start: usize, mut end: usize) -> usize {
    let (mut opens, mut closes) = (0, 0);
    for b in source[start..end].bytes() {
        match b {
            b'(' => opens += 1,
            b')' => closes += 1,
            _ => {}
        }
    }
    while let Some(last) = source[start..end].bytes().next_back() {
        match last {
            b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~' | b'\'' | b'"' | b']' => end -= 1,
            b')' if closes > opens => {
                end -= 1;
                closes -= 1;
            }
            b';' => {
                let body = &source[start..end - 1];
                end = match body.rfind('&') {
                    Some(amp)
                        if body.len() > amp + 1
                            && body[amp + 1..].bytes().all(|b| b.is_ascii_alphanumeric()) =>
                    {
                        start + amp
                    }
                    _ => end - 1,
                };
            }
            _ => break,
        }
    }
    end
}

#[cfg(test)]
mod bare_link_tests {
    use super::*;

    fn runs(text: &str) -> Vec<(String, Option<String>)> {
        fn collect(block: &Block, out: &mut Vec<(String, Option<String>)>) {
            let runs = |runs: &[InlineRun], out: &mut Vec<_>| {
                out.extend(
                    runs.iter()
                        .map(|run| (run.text.clone(), run.style.link.clone())),
                )
            };
            match block {
                Block::Paragraph { runs: r } | Block::Heading { runs: r, .. } => runs(r, out),
                Block::List { items, .. } => items.iter().flatten().for_each(|b| collect(b, out)),
                Block::BlockQuote { children } => children.iter().for_each(|b| collect(b, out)),
                Block::Table { header, rows, .. } => {
                    header.iter().for_each(|cell| runs(cell, out));
                    rows.iter().flatten().for_each(|cell| runs(cell, out));
                }
                _ => {}
            }
        }
        let tree = parse_full(text);
        let mut out = Vec::new();
        for top in &tree.blocks {
            collect(&top.block, &mut out);
        }
        out
    }
    fn links(text: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (_, link) in runs(text) {
            if let Some(link) = link
                && out.last() != Some(&link)
            {
                out.push(link);
            }
        }
        out
    }
    fn text(text: &str) -> String {
        runs(text).into_iter().map(|(text, _)| text).collect()
    }

    #[test]
    fn bare_urls_become_links_and_keep_the_surrounding_text() {
        let url = "https://github.com/hardbeat920/monocode/issues/1260";
        assert_eq!(links(url), [url]);
        assert_eq!(
            runs(&format!("See {url} now")),
            [
                ("See ".into(), None),
                (url.into(), Some(url.into())),
                (" now".into(), None)
            ]
        );
    }

    #[test]
    fn urls_keep_the_characters_the_parser_splits_them_at() {
        for url in [
            "https://github.com/x/y/blob/main/__init__.py",
            "https://a.com/_x_/y",
            "https://a.com/*x*/y",
            "https://a.com/~me/[1]/x",
            "http://example.com/wiki/Rust_(language)",
            "https://example.com/a_b_c?x=1&y=2#z",
            "https://a.com/x&amp;y=1",
            "https://a.com/x\\_y",
        ] {
            assert_eq!(runs(&format!("go {url} here")).len(), 3, "{url}");
            assert_eq!(links(&format!("go {url} here")), [url], "{url}");
        }
    }

    #[test]
    fn a_link_that_runs_through_a_delimiter_keeps_the_text_as_gfm_renders_it() {
        // The `_` closes nothing once the link has its opener.
        let source = "https://a.com/_x and more_";
        assert_eq!(links(source), ["https://a.com/_x"]);
        assert_eq!(text(source), source);
        // The strong span ends inside the link; the remaining `**` are text.
        let source = "**bold https://a.com/x**y** end";
        assert_eq!(links(source), ["https://a.com/x**y"]);
        assert_eq!(text(source), "bold https://a.com/x**y** end");
        assert!(runs(source)[0].0 == "bold ");
        let source = "- https://a.com/_x and more_";
        assert_eq!(text(source), "https://a.com/_x and more_");
        assert_eq!(links("*a https://x.com/*b* c*"), ["https://x.com/*b"]);
    }

    #[test]
    fn a_link_stops_at_code_other_links_and_the_end_of_a_cell() {
        assert_eq!(links("https://a.com/x`y`"), ["https://a.com/x"]);
        assert_eq!(text("https://a.com/x`y`"), "https://a.com/xy");
        assert_eq!(
            links("[https://a.com/x](https://exa"),
            ["https://a.com/x", "https://exa"]
        );
        assert_eq!(links("|h|\n|-|\n|https://a.com/x|y|"), ["https://a.com/x"]);
        assert_eq!(links("# https://a.com/x #"), ["https://a.com/x"]);
        assert_eq!(links("https://a.com/x\\\nnext"), ["https://a.com/x"]);
        assert_eq!(links("https://a.com/x&amp;"), ["https://a.com/x"]);
        assert_eq!(text("https://a.com/x&amp;"), "https://a.com/x&");
    }

    #[test]
    fn urls_are_found_in_every_inline_context() {
        let url = "https://example.com/x";
        assert_eq!(links(&format!("- item {url}")), [url]);
        assert_eq!(links(&format!("# title {url}")), [url]);
        assert_eq!(links(&format!("**{url}**")), [url]);
        assert_eq!(links(&format!("~~{url}~~")), [url]);
        assert_eq!(links(&format!("> {url}")), [url]);
        assert_eq!(links(&format!("|a|\n|-|\n|{url}|")), [url]);
        assert_eq!(links(&format!("链接{url}，然后")), [url]);
        assert_eq!(links(&format!("1{url}")), [url]);
        assert_eq!(links(&format!("x{url}")), Vec::<String>::new());
    }

    #[test]
    fn sentence_punctuation_stays_outside_the_link() {
        let url = "https://example.com/x";
        for text in [
            format!("{url}."),
            format!("({url})"),
            format!("[{url}]"),
            format!("\"{url}\","),
            format!("{url}?!"),
            format!("{url}_"),
            format!("{url}*"),
            format!("{url};"),
            format!("{url}。"),
        ] {
            assert_eq!(links(&text), [url], "{text}");
        }
        assert_eq!(links("https://a.com/(x))"), ["https://a.com/(x)"]);
        assert_eq!(links("https://a.com/x}"), ["https://a.com/x}"]);
    }

    #[test]
    fn schemes_match_in_any_case_and_open_in_lowercase() {
        assert_eq!(links("HTTPS://A.COM/x"), ["https://A.COM/x"]);
        assert_eq!(text("HTTPS://A.COM/x"), "HTTPS://A.COM/x");
        assert_eq!(links("Www.a.com/x"), ["http://Www.a.com/x"]);
    }

    #[test]
    fn www_addresses_open_over_http_and_need_a_delimiter_before_them() {
        assert_eq!(links("see www.a.com."), ["http://www.a.com"]);
        assert_eq!(links("(www.a.com/x)"), ["http://www.a.com/x"]);
        assert_eq!(links("*www.a.com*"), ["http://www.a.com"]);
        assert_eq!(links("_www.a.com_"), ["http://www.a.com"]);
        assert_eq!(text("www.a.com/x"), "www.a.com/x");
        for text in [
            "awww.a.com",
            "www. alone",
            "edit src/www.config.js",
            "foo.www.bar.com",
        ] {
            assert!(links(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn hostnames_follow_gfm_underscore_rules() {
        assert_eq!(links("http://localhost:1420"), ["http://localhost:1420"]);
        assert_eq!(
            links("https://my_app.example.com/x"),
            ["https://my_app.example.com/x"]
        );
        assert!(links("http://my_host:8080/x").is_empty());
        assert!(links("https://_a.com/x").is_empty());
    }

    #[test]
    fn emails_become_mailto_links() {
        for email in [
            "me@a.com",
            "me.you+x@a.co.uk",
            "me@www.a.com",
            "www.me@a.com",
            "me@mail.www.example.org",
            "pkg@1.0.0-beta",
        ] {
            let target = format!("mailto:{email}");
            assert_eq!(
                links(&format!("write to {email}.")),
                [target.as_str()],
                "{email}"
            );
            assert_eq!(links(&format!("邮箱{email}")), [target.as_str()], "{email}");
        }
        assert_eq!(links("<me@a.com>"), ["mailto:me@a.com"]);
        assert_eq!(
            runs("mailto:me@a.com"),
            [
                ("mailto:".into(), None),
                ("me@a.com".into(), Some("mailto:me@a.com".into()))
            ]
        );
        assert_eq!(links("a&amp;b@c.com"), ["mailto:b@c.com"]);
    }

    #[test]
    fn versions_handles_and_url_credentials_are_not_emails() {
        for text in [
            "react@18.2.0",
            "@types/node",
            "a@b",
            "x/me@a.com",
            "a@b..com",
            "a@b.c-",
        ] {
            assert!(links(text).is_empty(), "{text}");
        }
        assert_eq!(links("https://me:pw@a.com/x"), ["https://me:pw@a.com/x"]);
    }

    #[test]
    fn link_text_alt_text_and_code_never_autolink() {
        assert!(links("![https://a.com/x](b.png)").is_empty());
        assert!(links("- ![https://a.com/x](b.png)").is_empty());
        assert_eq!(
            links("[![https://a.com/x](b.png)](https://target.example)"),
            ["https://target.example"]
        );
        assert_eq!(
            links("[https://shown.example](https://target.example)"),
            ["https://target.example"]
        );
        assert!(links("`https://example.com`").is_empty());
        assert!(links("```\nhttps://example.com\n```").is_empty());
        assert!(links("https:// and http and xhttps://example.com").is_empty());
    }

    #[test]
    fn deep_nesting_treats_alt_text_the_same() {
        let deep = format!("{}![https://a.com/x](b.png)", "> ".repeat(40));
        assert!(links(&deep).is_empty());
    }
}

#[cfg(test)]
mod note_break_tests {
    use super::*;

    #[test]
    fn notes_preserve_prose_breaks_without_changing_chat_or_code() {
        fn prose(tree: &BlockTree) -> String {
            let Block::Paragraph { runs } = &tree.blocks[0].block else {
                panic!("expected paragraph")
            };
            runs.iter().map(|run| run.text.as_str()).collect()
        }
        assert_eq!(prose(&parse_full("one\ntwo")), "one two");
        assert_eq!(prose(&parse_full_hard_breaks("one\ntwo")), "one\ntwo");
        let tree = parse_full_hard_breaks("```text\none\ntwo\n```");
        let Block::CodeBlock { code, .. } = &tree.blocks[0].block else {
            panic!("expected code")
        };
        assert_eq!(code, "one\ntwo\n");
    }
}

#[cfg(test)]
#[path = "markdown_model/streaming_tests.rs"]
mod streaming_tests;
