//! Native Markdown model for AgentMarkdown's Streamdown elements.
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::{iter::Peekable, ops::Range, sync::Arc};

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
type Events<'a> = Peekable<std::vec::IntoIter<Event<'a>>>;
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
        pending.push(if hard_breaks && matches!(event, Event::SoftBreak) {
            Event::HardBreak
        } else {
            event
        });
        if depth == 0 {
            let mut events = std::mem::take(&mut pending).into_iter().peekable();
            tree.blocks
                .extend(blocks(&mut events, None, 0).into_iter().map(|block| {
                    Arc::new(TopBlock {
                        range: base + start..base + range.end,
                        block,
                    })
                }));
        }
    }
    tree
}
fn blocks(events: &mut Events<'_>, until: Option<TagEnd>, depth: usize) -> Vec<Block> {
    let mut out = Vec::new();
    while let Some(event) = events.next() {
        match event {
            Event::End(end) if until == Some(end) => break,
            Event::Start(tag) if depth < 32 => match tag {
                Tag::Paragraph => {
                    if let Some(visual) = placed_visual(events) {
                        out.push(visual);
                        continue;
                    }
                    out.push(Block::Paragraph {
                        runs: inlines(events, TagEnd::Paragraph),
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
                        runs: inlines(events, TagEnd::Heading(level)),
                    });
                }
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(info) if !info.is_empty() => Some(info.into_string()),
                        _ => None,
                    };
                    let mut code = String::new();
                    for event in events.by_ref() {
                        match event {
                            Event::End(TagEnd::CodeBlock) => break,
                            Event::Text(text) => code.push_str(&text),
                            _ => {}
                        }
                    }
                    out.push(Block::CodeBlock { language, code });
                }
                Tag::BlockQuote(kind) => out.push(Block::BlockQuote {
                    children: blocks(events, Some(TagEnd::BlockQuote(kind)), depth + 1),
                }),
                Tag::List(start) => {
                    let mut items = Vec::new();
                    while let Some(event) = events.next() {
                        match event {
                            Event::End(TagEnd::List(_)) => break,
                            Event::Start(Tag::Item) => {
                                items.push(blocks(events, Some(TagEnd::Item), depth + 1))
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
                    while let Some(event) = events.next() {
                        match event {
                            Event::End(TagEnd::Table) => break,
                            Event::Start(Tag::TableHead) => head = true,
                            Event::Start(Tag::TableCell) => {
                                row.push(inlines(events, TagEnd::TableCell))
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
                tag @ (Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. }) => {
                    let mut runs = Vec::new();
                    inline_span(events, tag, InlineStyle::default(), &mut runs);
                    if let Some(Block::Paragraph { runs: previous }) = out.last_mut() {
                        previous.extend(runs);
                    } else {
                        out.push(Block::Paragraph { runs });
                    }
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
                append_run(&mut runs, event, InlineStyle::default());
                while let Some(next) = events.peek() {
                    if matches!(
                        next,
                        Event::Start(
                            Tag::List(_) | Tag::BlockQuote(_) | Tag::CodeBlock(_) | Tag::Paragraph
                        ) | Event::End(TagEnd::Item)
                    ) {
                        break;
                    }
                    if matches!(next, Event::End(_)) {
                        break;
                    }
                    let next = events.next().unwrap();
                    match next {
                        Event::Start(tag) => {
                            inline_span(events, tag, InlineStyle::default(), &mut runs)
                        }
                        event => append_run(&mut runs, event, InlineStyle::default()),
                    }
                }
                if let Some(Block::Paragraph { runs: previous }) = out.last_mut() {
                    previous.extend(runs);
                } else {
                    out.push(Block::Paragraph { runs });
                }
            }
            Event::Start(
                tag @ (Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. }),
            ) => {
                let mut runs = vec![];
                inline_span(events, tag, InlineStyle::default(), &mut runs);
                if let Some(Block::Paragraph { runs: previous }) = out.last_mut() {
                    previous.extend(runs);
                } else {
                    out.push(Block::Paragraph { runs });
                }
            }
            _ => {}
        }
    }
    out
}
/// A paragraph that is only a `visual:` image places that visual. Consumes
/// the paragraph when it matches; leaves the events untouched otherwise.
fn placed_visual(events: &mut Events<'_>) -> Option<Block> {
    let id = match events.peek()? {
        Event::Start(Tag::Image { dest_url, .. }) => dest_url
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
    events.next();
    let mut caption = String::new();
    for event in events.by_ref() {
        match event {
            Event::End(TagEnd::Image) => break,
            Event::Text(text) | Event::Code(text) => caption.push_str(&text),
            _ => {}
        }
    }
    // Anything after the image on the same line stays prose.
    let rest = inlines(events, TagEnd::Paragraph);
    let rest: String = rest.iter().map(|r| r.text.as_str()).collect();
    if !rest.trim().is_empty() {
        caption = format!("{caption} {}", rest.trim()).trim().to_owned();
    }
    Some(Block::Visual { id, caption })
}
fn inlines(events: &mut Events<'_>, end: TagEnd) -> Vec<InlineRun> {
    let mut out = Vec::new();
    while let Some(event) = events.next() {
        match event {
            Event::End(tag) if tag == end => break,
            Event::Start(tag) => inline_span(events, tag, InlineStyle::default(), &mut out),
            event => append_run(&mut out, event, InlineStyle::default()),
        }
    }
    out
}
fn inline_span(
    events: &mut Events<'_>,
    tag: Tag<'_>,
    mut style: InlineStyle,
    out: &mut Vec<InlineRun>,
) {
    let end = tag.to_end();
    match tag {
        Tag::Strong => style.bold = true,
        Tag::Emphasis => style.italic = true,
        Tag::Strikethrough => style.strikethrough = true,
        Tag::Link { dest_url, .. } if dest_url.as_ref() != "streamdown:incomplete-link" => {
            style.link = Some(dest_url.into_string());
        }
        _ => {}
    }
    while let Some(event) = events.next() {
        match event {
            Event::End(tag) if tag == end => break,
            Event::Start(tag) => inline_span(events, tag, style.clone(), out),
            event => append_run(out, event, style.clone()),
        }
    }
}
fn append_run(out: &mut Vec<InlineRun>, event: Event<'_>, mut style: InlineStyle) {
    let text = match event {
        Event::Text(text) => text.into_string(),
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
        _ => return,
    };
    out.push(InlineRun { text, style });
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
