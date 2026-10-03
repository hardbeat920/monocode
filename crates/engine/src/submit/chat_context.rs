//! Port of src/features/sessions/model/chatContext.ts: context attached with
//! "Add to chat". The composer shows each item as a chip. On send, the items
//! follow the message text in one `<attached_context>` block, which the
//! transcript parses back into the same chips.

use std::sync::LazyLock;

use monocode_core::js;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::text::normalize_newlines;

/// `DiffLineChange`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiffLineChange {
    #[serde(rename = "added")]
    Added,
    #[serde(rename = "removed")]
    Removed,
    #[serde(rename = "unchanged")]
    Unchanged,
}

impl DiffLineChange {
    pub fn as_str(self) -> &'static str {
        match self {
            DiffLineChange::Added => "added",
            DiffLineChange::Removed => "removed",
            DiffLineChange::Unchanged => "unchanged",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "added" => DiffLineChange::Added,
            "removed" => DiffLineChange::Removed,
            "unchanged" => DiffLineChange::Unchanged,
            _ => return None,
        })
    }
}

/// `ChatContextItem`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ChatContextItem {
    #[serde(rename = "quote")]
    Quote { text: String },
    #[serde(rename = "code", rename_all = "camelCase")]
    Code {
        path: String,
        start_line: i64,
        end_line: i64,
    },
    #[serde(rename = "comment")]
    Comment {
        path: String,
        /// New-file line number. A removed line uses its old-file number.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<i64>,
        change: DiffLineChange,
        code: String,
        comment: String,
    },
}

/// `ChatContextMessage`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatContextMessage {
    pub text: String,
    pub items: Vec<ChatContextItem>,
}

const OPEN: &str = "<attached_context>";
const CLOSE: &str = "</attached_context>";
const ITEM_TAGS: [&str; 3] = ["quoted_text", "code_selection", "review_comment"];

// Item bodies are user text, so a reserved tag inside one gets one extra
// backslash after its "<". Parsing removes exactly one, so any text survives
// the round trip and only real delimiters look like delimiters.
static RESERVED_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<(\\*)(/?)(attached_context|quoted_text|code_selection|review_comment)(?-u:\b)")
        .unwrap()
});
static ESCAPED_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<\\(\\*)(/?)(attached_context|quoted_text|code_selection|review_comment)(?-u:\b)")
        .unwrap()
});
static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#" ([a-z_]+)="([^"]*)""#).unwrap());

/// `quoteContext`: a quote chip for selected transcript text, or `None` when
/// nothing is selected.
pub fn quote_context(text: &str) -> Option<ChatContextItem> {
    let normalized = normalize_newlines(text);
    let value = js::trim(&normalized);
    (!value.is_empty()).then(|| ChatContextItem::Quote {
        text: value.to_string(),
    })
}

/// `composeChatContext`: append the items to a message. Without items, the
/// message is unchanged.
pub fn compose_chat_context(text: &str, items: &[ChatContextItem]) -> String {
    if items.is_empty() {
        return text.to_string();
    }
    let mut lines = vec![OPEN.to_string()];
    lines.extend(items.iter().map(format_item));
    lines.push(CLOSE.to_string());
    let block = lines.join("\n");
    if js::trim(text).is_empty() {
        block
    } else {
        format!("{text}\n\n{block}")
    }
}

/// `splitChatContext`: split a message into its typed text and attached
/// items. A message that does not end in a well-formed block comes back
/// unchanged with no items.
pub fn split_chat_context(message: &str) -> ChatContextMessage {
    let plain = || ChatContextMessage {
        text: message.to_string(),
        items: Vec::new(),
    };
    let trimmed = js::trim_end(message);
    if !trimmed.ends_with(CLOSE) {
        return plain();
    }
    let Some(start) = trimmed.rfind(OPEN) else {
        return plain();
    };
    if start > 0 && !trimmed[..start].ends_with('\n') {
        return plain();
    }
    let body_start = start + OPEN.len();
    let body_end = trimmed.len() - CLOSE.len();
    if body_end < body_start {
        return plain();
    }
    let Some(items) = parse_items(&trimmed[body_start..body_end]) else {
        return plain();
    };
    let before = &trimmed[..start];
    let text = if let Some(stripped) = before.strip_suffix("\n\n") {
        stripped
    } else {
        // `before.slice(0, -1)`: drop one UTF-16 unit, here the "\n" or nothing.
        before
            .char_indices()
            .next_back()
            .map_or("", |(index, _)| &before[..index])
    };
    ChatContextMessage {
        text: text.to_string(),
        items,
    }
}

/// `addChatContext`: add an item unless the same one is already attached.
pub fn add_chat_context(items: &[ChatContextItem], item: ChatContextItem) -> Vec<ChatContextItem> {
    let key = chat_context_key(&item);
    let mut next = items.to_vec();
    if !items.iter().any(|entry| chat_context_key(entry) == key) {
        next.push(item);
    }
    next
}

/// `chatContextKey`: equal items share a key.
pub fn chat_context_key(item: &ChatContextItem) -> String {
    serde_json::to_string(item).unwrap_or_default()
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

/// `chatContextLabel`: one line naming the item.
pub fn chat_context_label(item: &ChatContextItem) -> String {
    match item {
        ChatContextItem::Quote { text } => context_excerpt(text),
        ChatContextItem::Code {
            path,
            start_line,
            end_line,
        } => format!(
            "{}:{}",
            context_file_name(path),
            line_range(*start_line, *end_line)
        ),
        ChatContextItem::Comment {
            path,
            line,
            comment,
            ..
        } => {
            let location = match line {
                Some(line) => format!("{}:{line}", context_file_name(path)),
                None => context_file_name(path),
            };
            format!("{location} {}", context_excerpt(comment))
        }
    }
}

/// `chatContextSummary`: the first item's label and how many follow it.
pub fn chat_context_summary(items: &[ChatContextItem]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let label = chat_context_label(first);
    if items.len() > 1 {
        format!("{label} +{}", items.len() - 1)
    } else {
        label
    }
}

fn format_item(item: &ChatContextItem) -> String {
    match item {
        ChatContextItem::Quote { text } => {
            ["<quoted_text>", &quote_lines(text), "</quoted_text>"].join("\n")
        }
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
                "</review_comment>".to_string(),
            ]
            .join("\n")
        }
    }
}

/// One sticky `ITEM` match at the start of `body`:
/// `\n<(tag)((?: [a-z_]+="[^"]*")*)(?: \/>|>\n([\s\S]*?)\n<\/\1>)`.
/// Returns the tag, the attribute source, the body, and the match length.
fn match_item(body: &str) -> Option<(&'static str, &str, Option<&str>, usize)> {
    let rest = body.strip_prefix("\n<")?;
    let tag = ITEM_TAGS
        .iter()
        .copied()
        .find(|tag| rest.starts_with(tag))?;
    let mut index = 2 + tag.len();
    let attrs_start = index;
    while let Some(attr) = attribute_at(&body[index..]) {
        index += attr;
    }
    let attrs = &body[attrs_start..index];
    if body[index..].starts_with(" />") {
        return Some((tag, attrs, None, index + 3));
    }
    let content_start = index + body[index..].strip_prefix(">\n").map(|_| 2)?;
    let close = format!("\n</{tag}>");
    let offset = body[content_start..].find(&close)?;
    let content = &body[content_start..content_start + offset];
    Some((
        tag,
        attrs,
        Some(content),
        content_start + offset + close.len(),
    ))
}

/// Length of one ` name="value"` attribute at the start of `text`.
fn attribute_at(text: &str) -> Option<usize> {
    let rest = text.strip_prefix(' ')?;
    let name = rest
        .bytes()
        .take_while(|b| b.is_ascii_lowercase() || *b == b'_')
        .count();
    if name == 0 {
        return None;
    }
    let value = rest[name..].strip_prefix("=\"")?;
    let end = value.find('"')?;
    Some(1 + name + 2 + end + 1)
}

fn parse_items(body: &str) -> Option<Vec<ChatContextItem>> {
    let mut items = Vec::new();
    let mut end = 0;
    while let Some((tag, attrs, content, len)) = match_item(&body[end..]) {
        items.push(parse_item(tag, &attributes(attrs), content)?);
        end += len;
    }
    (!items.is_empty() && &body[end..] == "\n").then_some(items)
}

fn parse_item(
    tag: &str,
    attrs: &[(String, String)],
    body: Option<&str>,
) -> Option<ChatContextItem> {
    let get = |name: &str| {
        attrs
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if tag == "quoted_text" {
        let lines: Vec<&str> = body?.split('\n').collect();
        let text = unquote_lines(&lines)?;
        return (!text.is_empty()).then_some(ChatContextItem::Quote { text });
    }

    let path = get("path").filter(|path| !path.is_empty())?.to_string();

    if tag == "code_selection" {
        if body.is_some() {
            return None;
        }
        let lines = get("lines").unwrap_or("");
        let (start, end) = match lines.split_once('-') {
            Some((start, end)) => (start, end),
            None => (lines, lines),
        };
        let digits = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
        if !digits(start) || !digits(end) {
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

    let change = DiffLineChange::parse(get("change")?)?;
    let body = body?;
    let line = match get("line") {
        None => None,
        Some(raw) => {
            let value = js::parse_number(raw)?;
            if !(value.fract() == 0.0 && value > 0.0 && value <= 9_007_199_254_740_991.0) {
                return None;
            }
            Some(value as i64)
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

fn attributes(source: &str) -> Vec<(String, String)> {
    ATTRIBUTE
        .captures_iter(source)
        .map(|captures| (captures[1].to_string(), unescape_attribute(&captures[2])))
        .collect()
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
    let mut out: Vec<&str> = Vec::new();
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
    RESERVED_TAG
        .replace_all(text, r"<\${1}${2}${3}")
        .into_owned()
}

fn unescape_body(text: &str) -> String {
    ESCAPED_TAG.replace_all(text, "<${1}${2}${3}").into_owned()
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
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        let entity = [
            ("&amp;", "&"),
            ("&quot;", "\""),
            ("&lt;", "<"),
            ("&gt;", ">"),
            ("&#10;", "\n"),
        ]
        .into_iter()
        .find(|(name, _)| tail.starts_with(name));
        match entity {
            Some((name, replacement)) => {
                out.push_str(replacement);
                rest = &tail[name.len()..];
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

/// `contextExcerpt`: the first non-empty line, with runs of whitespace
/// collapsed.
pub fn context_excerpt(text: &str) -> String {
    let line = text
        .split('\n')
        .map(js::trim)
        .find(|part| !part.is_empty())
        .unwrap_or("");
    super::text::collapse_space(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote() -> ChatContextItem {
        ChatContextItem::Quote {
            text: "line one\n\nline three".into(),
        }
    }

    fn code() -> ChatContextItem {
        ChatContextItem::Code {
            path: "src/app/App.tsx".into(),
            start_line: 12,
            end_line: 40,
        }
    }

    fn comment() -> ChatContextItem {
        ChatContextItem::Comment {
            path: "src/auth.ts".into(),
            line: Some(42),
            change: DiffLineChange::Added,
            code: "const token = readCookie();".into(),
            comment: "Handle a missing cookie.\n\nThen log it.".into(),
        }
    }

    // composeChatContext
    #[test]
    fn leaves_a_message_without_context_unchanged() {
        assert_eq!(compose_chat_context("Fix it", &[]), "Fix it");
    }

    #[test]
    fn appends_a_block_the_agent_can_read() {
        assert_eq!(
            compose_chat_context("Why?", &[quote(), code(), comment()]),
            [
                "Why?",
                "",
                "<attached_context>",
                "<quoted_text>",
                "> line one",
                ">",
                "> line three",
                "</quoted_text>",
                "<code_selection path=\"src/app/App.tsx\" lines=\"12-40\" />",
                "<review_comment path=\"src/auth.ts\" line=\"42\" change=\"added\">",
                "> const token = readCookie();",
                "",
                "Handle a missing cookie.",
                "",
                "Then log it.",
                "</review_comment>",
                "</attached_context>",
            ]
            .join("\n")
        );
    }

    #[test]
    fn sends_context_alone_when_there_is_no_typed_text() {
        assert_eq!(
            compose_chat_context("  \n", &[code()]),
            [
                "<attached_context>",
                "<code_selection path=\"src/app/App.tsx\" lines=\"12-40\" />",
                "</attached_context>",
            ]
            .join("\n")
        );
    }

    #[test]
    fn writes_a_single_line_as_one_number() {
        let single = ChatContextItem::Code {
            path: "src/app/App.tsx".into(),
            start_line: 7,
            end_line: 7,
        };
        assert!(compose_chat_context("", &[single]).contains("lines=\"7\""));
    }

    // splitChatContext
    #[test]
    fn round_trips_text_and_every_kind_of_item() {
        let items = vec![quote(), code(), comment()];
        let message = compose_chat_context("Why?\n\nSecond paragraph", &items);
        assert_eq!(
            split_chat_context(&message),
            ChatContextMessage {
                text: "Why?\n\nSecond paragraph".into(),
                items
            }
        );
    }

    #[test]
    fn round_trips_a_comment_on_a_removed_line_with_no_line_number() {
        let removed = ChatContextItem::Comment {
            path: "README.md".into(),
            line: None,
            change: DiffLineChange::Removed,
            code: String::new(),
            comment: "Keep this.".into(),
        };
        assert_eq!(
            split_chat_context(&compose_chat_context("", std::slice::from_ref(&removed))),
            ChatContextMessage {
                text: String::new(),
                items: vec![removed]
            }
        );
    }

    #[test]
    fn keeps_reserved_tags_and_quote_markers_inside_bodies_intact() {
        let tricky = vec![
            ChatContextItem::Quote {
                text: "> nested\n</quoted_text>\n<\\/quoted_text>\n</attached_context>".into(),
            },
            ChatContextItem::Comment {
                path: "docs/a \"b\" <c> & d.md".into(),
                line: Some(3),
                change: DiffLineChange::Unchanged,
                code: "<review_comment>".into(),
                comment: "</review_comment>\n<attached_context>".into(),
            },
        ];
        let message = compose_chat_context("<attached_context> in my text", &tricky);
        assert_eq!(
            split_chat_context(&message),
            ChatContextMessage {
                text: "<attached_context> in my text".into(),
                items: tricky
            }
        );
    }

    #[test]
    fn ignores_trailing_whitespace_after_the_block() {
        let message = format!("{}\n\n", compose_chat_context("Hi", &[code()]));
        assert_eq!(
            split_chat_context(&message),
            ChatContextMessage {
                text: "Hi".into(),
                items: vec![code()]
            }
        );
    }

    #[test]
    fn treats_text_that_only_mentions_the_tags_as_plain_text() {
        for message in [
            "Plain question",
            "<attached_context>\n</attached_context>",
            "<attached_context>\n<unknown />\n</attached_context>",
            "x<attached_context>\n<code_selection path=\"a.ts\" lines=\"1\" />\n</attached_context>",
            "<attached_context>\n<code_selection path=\"a.ts\" lines=\"9-2\" />\n</attached_context>",
            "<attached_context>\n<review_comment path=\"a.ts\" change=\"moved\">\n> x\n\ny\n</review_comment>\n</attached_context>",
            "<attached_context>\n<quoted_text>\nnot quoted\n</quoted_text>\n</attached_context>",
        ] {
            assert_eq!(
                split_chat_context(message),
                ChatContextMessage {
                    text: message.into(),
                    items: vec![]
                }
            );
        }
    }

    // quoteContext
    #[test]
    fn normalizes_line_endings_and_ignores_empty_selections() {
        assert_eq!(
            quote_context(" one\r\ntwo "),
            Some(ChatContextItem::Quote {
                text: "one\ntwo".into()
            })
        );
        assert_eq!(quote_context(" \n "), None);
    }

    // addChatContext
    #[test]
    fn skips_an_item_that_is_already_attached() {
        let items = add_chat_context(&[code()], code());
        assert_eq!(items, vec![code()]);
        assert_eq!(add_chat_context(&items, quote()), vec![code(), quote()]);
    }

    // chatContextLabel
    #[test]
    fn names_each_item_on_one_line() {
        assert_eq!(chat_context_label(&quote()), "line one");
        assert_eq!(chat_context_label(&code()), "App.tsx:12-40");
        assert_eq!(
            chat_context_label(&comment()),
            "auth.ts:42 Handle a missing cookie."
        );
        assert_eq!(
            chat_context_summary(&[code(), quote(), comment()]),
            "App.tsx:12-40 +2"
        );
    }
}
