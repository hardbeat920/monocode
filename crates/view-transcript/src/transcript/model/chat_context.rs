//! Port of the parsing half of src/features/sessions/model/chatContext.ts and
//! the chip labels in src/features/sessions/ui/ChatContextChip.tsx.
//!
//! Context attached with "Add to chat" travels as one `<attached_context>`
//! block after the message text. The transcript parses it back into chips.

use std::sync::LazyLock;

use regex::Regex;

/// `DiffLineChange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineChange {
    Added,
    Removed,
    Unchanged,
}

impl DiffLineChange {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "added" => Some(Self::Added),
            "removed" => Some(Self::Removed),
            "unchanged" => Some(Self::Unchanged),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Unchanged => "unchanged",
        }
    }
}

/// `ChatContextItem`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatContextItem {
    Quote {
        text: String,
    },
    Code {
        path: String,
        start_line: i64,
        end_line: i64,
    },
    Comment {
        path: String,
        /// New-file line number. A removed line uses its old-file number.
        line: Option<i64>,
        change: DiffLineChange,
        code: String,
        comment: String,
    },
}

impl ChatContextItem {
    /// The `data-chat-context-chip` kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Quote { .. } => "quote",
            Self::Code { .. } => "code",
            Self::Comment { .. } => "comment",
        }
    }
}

/// `ChatContextMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatContextMessage {
    pub text: String,
    pub items: Vec<ChatContextItem>,
}

const OPEN: &str = "<attached_context>";
const CLOSE: &str = "</attached_context>";
const TAGS: [&str; 3] = ["quoted_text", "code_selection", "review_comment"];

static RESERVED_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<(\\*)(/?)(attached_context|quoted_text|code_selection|review_comment)\b")
        .expect("reserved tag")
});
static ESCAPED_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<\\(\\*)(/?)(attached_context|quoted_text|code_selection|review_comment)\b")
        .expect("escaped tag")
});

/// `composeChatContext`: append the items to a message.
pub fn compose_chat_context(text: &str, items: &[ChatContextItem]) -> String {
    if items.is_empty() {
        return text.to_string();
    }
    let mut parts = vec![OPEN.to_string()];
    parts.extend(items.iter().map(format_item));
    parts.push(CLOSE.into());
    let block = parts.join("\n");
    if monocode_core::js::trim(text).is_empty() {
        block
    } else {
        format!("{text}\n\n{block}")
    }
}

/// `splitChatContext`: the typed text and attached items. A message that does
/// not end in a well-formed block comes back unchanged with no items.
pub fn split_chat_context(message: &str) -> ChatContextMessage {
    let plain = || ChatContextMessage {
        text: message.to_string(),
        items: Vec::new(),
    };
    let trimmed = message.trim_end_matches(|c: char| {
        monocode_core::js::is_space(c) || monocode_core::js::is_line_terminator(c)
    });
    if !trimmed.ends_with(CLOSE) {
        return plain();
    }
    let Some(start) = trimmed.rfind(OPEN) else {
        return plain();
    };
    if start > 0 && !trimmed[..start].ends_with('\n') {
        return plain();
    }
    let body = &trimmed[start + OPEN.len()..trimmed.len() - CLOSE.len()];
    let Some(items) = parse_items(body) else {
        return plain();
    };
    let before = &trimmed[..start];
    let text = if let Some(text) = before.strip_suffix("\n\n") {
        text
    } else {
        before.strip_suffix('\n').unwrap_or(before)
    };
    ChatContextMessage {
        text: text.to_string(),
        items,
    }
}

fn format_item(item: &ChatContextItem) -> String {
    match item {
        ChatContextItem::Quote { text } => [
            "<quoted_text>".to_string(),
            quote_lines(text),
            "</quoted_text>".into(),
        ]
        .join("\n"),
        ChatContextItem::Code {
            path,
            start_line,
            end_line,
        } => format!(
            "<code_selection path=\"{}\" lines=\"{}\" />",
            escape_attribute(path),
            line_range(*start_line, *end_line)
        ),
        ChatContextItem::Comment {
            path,
            line,
            change,
            code,
            comment,
        } => {
            let line = line
                .map(|line| format!(" line=\"{line}\""))
                .unwrap_or_default();
            [
                format!(
                    "<review_comment path=\"{}\"{line} change=\"{}\">",
                    escape_attribute(path),
                    change.as_str()
                ),
                quote_lines(code),
                String::new(),
                escape_body(comment),
                "</review_comment>".into(),
            ]
            .join("\n")
        }
    }
}

/// One parsed `ITEM` match: the tag, its attributes, and its body.
struct RawItem<'a> {
    tag: &'a str,
    attrs: Vec<(String, String)>,
    body: Option<&'a str>,
    end: usize,
}

/// The sticky `ITEM` regex at `pos`:
/// `\n<(tag)((?: [a-z_]+="[^"]*")*)(?: \/>|>\n([\s\S]*?)\n<\/\1>)`.
fn match_item(body: &str, pos: usize) -> Option<RawItem<'_>> {
    let rest = body.get(pos..)?.strip_prefix("\n<")?;
    let tag = TAGS.into_iter().find(|tag| rest.starts_with(tag))?;
    let mut at = pos + 2 + tag.len();
    let mut attrs = Vec::new();
    loop {
        let tail = &body[at..];
        let Some(after_space) = tail.strip_prefix(' ') else {
            break;
        };
        let name_len = after_space
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || *b == b'_')
            .count();
        if name_len == 0 {
            break;
        }
        let Some(value_start) = after_space[name_len..].strip_prefix("=\"") else {
            break;
        };
        let Some(value_len) = value_start.find('"') else {
            break;
        };
        attrs.push((
            after_space[..name_len].to_string(),
            value_start[..value_len].to_string(),
        ));
        at += 1 + name_len + 2 + value_len + 1;
    }
    let tail = &body[at..];
    if tail.starts_with(" />") {
        return Some(RawItem {
            tag,
            attrs,
            body: None,
            end: at + 3,
        });
    }
    let inner_start = at + tail.strip_prefix(">\n").map(|_| 2)?;
    let close = format!("\n</{tag}>");
    let inner_len = body[inner_start..].find(&close)?;
    Some(RawItem {
        tag,
        attrs,
        body: Some(&body[inner_start..inner_start + inner_len]),
        end: inner_start + inner_len + close.len(),
    })
}

fn parse_items(body: &str) -> Option<Vec<ChatContextItem>> {
    let mut items = Vec::new();
    let mut end = 0;
    while let Some(raw) = match_item(body, end) {
        items.push(parse_item(&raw)?);
        end = raw.end;
    }
    (!items.is_empty() && &body[end..] == "\n").then_some(items)
}

fn attr(attrs: &[(String, String)], name: &str) -> Option<String> {
    attrs
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| unescape_attribute(value))
}

fn parse_item(raw: &RawItem<'_>) -> Option<ChatContextItem> {
    if raw.tag == "quoted_text" {
        let text = unquote_lines(&raw.body?.split('\n').collect::<Vec<_>>())?;
        return (!text.is_empty()).then_some(ChatContextItem::Quote { text });
    }
    let path = attr(&raw.attrs, "path").filter(|path| !path.is_empty())?;
    if raw.tag == "code_selection" {
        if raw.body.is_some() {
            return None;
        }
        let lines = attr(&raw.attrs, "lines").unwrap_or_default();
        let (start, end) = match lines.split_once('-') {
            Some((start, end)) => (start.to_string(), end.to_string()),
            None => (lines.clone(), lines.clone()),
        };
        let digits = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
        if !digits(&start) || !digits(&end) {
            return None;
        }
        let start_line: i64 = start.parse().ok()?;
        let end_line: i64 = end.parse().ok()?;
        if start_line < 1 || end_line < start_line {
            return None;
        }
        return Some(ChatContextItem::Code {
            path,
            start_line,
            end_line,
        });
    }
    let change = attr(&raw.attrs, "change").and_then(|change| DiffLineChange::parse(&change))?;
    let body = raw.body?;
    let line = match attr(&raw.attrs, "line") {
        None => None,
        Some(value) => {
            let line: i64 = value.parse().ok()?;
            if line <= 0 {
                return None;
            }
            Some(line)
        }
    };
    let lines: Vec<&str> = body.split('\n').collect();
    let gap = lines.iter().position(|line| line.is_empty())?;
    if gap < 1 {
        return None;
    }
    let code = unquote_lines(&lines[..gap])?;
    let comment = unescape_body(&lines[gap + 1..].join("\n"));
    if comment.is_empty() {
        return None;
    }
    Some(ChatContextItem::Comment {
        path,
        line,
        change,
        code,
        comment,
    })
}

fn quote_lines(text: &str) -> String {
    escape_body(text)
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unquote_lines(lines: &[&str]) -> Option<String> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if *line == ">" {
            out.push("");
        } else {
            out.push(line.strip_prefix("> ")?);
        }
    }
    Some(unescape_body(&out.join("\n")))
}

fn escape_body(text: &str) -> String {
    RESERVED_TAG.replace_all(text, "<\\$1$2$3").into_owned()
}

fn unescape_body(text: &str) -> String {
    ESCAPED_TAG.replace_all(text, "<$1$2$3").into_owned()
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', "&#10;")
}

fn unescape_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let decoded = [
            ("&amp;", "&"),
            ("&quot;", "\""),
            ("&lt;", "<"),
            ("&gt;", ">"),
            ("&#10;", "\n"),
        ]
        .into_iter()
        .find(|(entity, _)| tail.starts_with(entity));
        match decoded {
            Some((entity, text)) => {
                out.push_str(text);
                rest = &tail[entity.len()..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `lineRange`: "12" for one line, "12-40" for a range.
pub fn line_range(start_line: i64, end_line: i64) -> String {
    if end_line > start_line {
        format!("{start_line}-{end_line}")
    } else {
        start_line.to_string()
    }
}

/// `contextFileName`.
pub fn context_file_name(path: &str) -> String {
    path.split('/')
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// `contextExcerpt`: the first non-empty line, whitespace collapsed.
pub fn context_excerpt(text: &str) -> String {
    let line = text
        .split('\n')
        .map(monocode_core::js::trim)
        .find(|part| !part.is_empty())
        .unwrap_or("");
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The pieces a chip shows (`chipLabel` in ChatContextChip.tsx).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipLabel {
    pub action: &'static str,
    pub full: String,
    /// The file or quote text.
    pub name: String,
    /// `L3-9`, for code and comment chips.
    pub line_tag: Option<String>,
    /// The comment excerpt, for comment chips.
    pub comment: Option<String>,
}

/// `chipLabel`.
pub fn chip_label(item: &ChatContextItem) -> ChipLabel {
    match item {
        ChatContextItem::Quote { text } => {
            let excerpt = context_excerpt(text);
            ChipLabel {
                action: "Quoted text",
                full: excerpt.clone(),
                name: excerpt,
                line_tag: None,
                comment: None,
            }
        }
        ChatContextItem::Code {
            path,
            start_line,
            end_line,
        } => {
            let lines = line_range(*start_line, *end_line);
            ChipLabel {
                action: "Open selected lines",
                full: format!("{path}, lines {lines}"),
                name: context_file_name(path),
                line_tag: Some(format!("L{lines}")),
                comment: None,
            }
        }
        ChatContextItem::Comment {
            path,
            line,
            comment,
            ..
        } => {
            let excerpt = context_excerpt(comment);
            let location = match line {
                Some(line) => format!("{path}:{line}"),
                None => path.clone(),
            };
            ChipLabel {
                action: "Comment",
                full: format!("{location}, {excerpt}"),
                name: context_file_name(path),
                line_tag: line.map(|line| format!("L{line}")),
                comment: Some(excerpt),
            }
        }
    }
}

/// `fileTarget`: the file and line a chip opens, if any.
pub fn file_target(item: &ChatContextItem) -> Option<(String, i64)> {
    match item {
        ChatContextItem::Code {
            path, start_line, ..
        } => Some((path.clone(), *start_line)),
        ChatContextItem::Comment {
            path,
            line: Some(line),
            change,
            ..
        } if *change != DiffLineChange::Removed => Some((path.clone(), *line)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<ChatContextItem> {
        vec![
            ChatContextItem::Quote {
                text: "The cookie is missing.".into(),
            },
            ChatContextItem::Code {
                path: "src/auth.ts".into(),
                start_line: 3,
                end_line: 9,
            },
            ChatContextItem::Comment {
                path: "src/auth.ts".into(),
                line: Some(42),
                change: DiffLineChange::Added,
                code: "readCookie();".into(),
                comment: "Handle null here.".into(),
            },
        ]
    }

    #[test]
    fn round_trips_attached_context() {
        let message = compose_chat_context("Why does this fail?", &sample());
        let split = split_chat_context(&message);
        assert_eq!(split.text, "Why does this fail?");
        assert_eq!(split.items, sample());
        assert_eq!(
            chip_label(&split.items[1]).line_tag.as_deref(),
            Some("L3-9")
        );
        assert_eq!(
            chip_label(&split.items[2]).comment.as_deref(),
            Some("Handle null here.")
        );
    }

    #[test]
    fn leaves_malformed_blocks_alone() {
        let message = "text\n<attached_context>\n<quoted_text>\nnot quoted\n</quoted_text>\n</attached_context>";
        let split = split_chat_context(message);
        assert_eq!(split.text, message);
        assert!(split.items.is_empty());
        assert!(split_chat_context("plain").items.is_empty());
    }

    #[test]
    fn keeps_reserved_tags_inside_item_bodies() {
        let items = vec![ChatContextItem::Quote {
            text: "a </quoted_text> b".into(),
        }];
        let split = split_chat_context(&compose_chat_context("", &items));
        assert_eq!(split.items, items);
        assert_eq!(split.text, "");
    }

    #[test]
    fn opens_code_and_comment_chips_on_their_line() {
        let items = sample();
        assert_eq!(file_target(&items[0]), None);
        assert_eq!(file_target(&items[1]), Some(("src/auth.ts".into(), 3)));
        assert_eq!(file_target(&items[2]), Some(("src/auth.ts".into(), 42)));
    }
}
