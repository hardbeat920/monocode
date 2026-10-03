//! The part of src/features/sessions/model/chatContext.ts and quoteDraft.ts
//! that opening a chat from "Add to chat" needs: the `ChatContextItem` shape,
//! `composeChatContext`, `composerSeedForAddToChat`, and
//! `isMarkdownBlockquotePosition`. The composer package owns the parser and
//! the chip helpers.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

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
    pub const fn as_str(self) -> &'static str {
        match self {
            DiffLineChange::Added => "added",
            DiffLineChange::Removed => "removed",
            DiffLineChange::Unchanged => "unchanged",
        }
    }
}

/// `ChatContextItem`: context attached with "Add to chat".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<i64>,
        change: DiffLineChange,
        code: String,
        comment: String,
    },
}

const OPEN: &str = "<attached_context>";
const CLOSE: &str = "</attached_context>";

static RESERVED_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<(\\*)(/?)(attached_context|quoted_text|code_selection|review_comment)\b")
        .expect("reserved tag pattern")
});

static BLOCKQUOTE_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}>").expect("blockquote pattern"));

/// `lineRange`: "12" for one line, "12-40" for a range.
pub fn line_range(start_line: i64, end_line: i64) -> String {
    if end_line > start_line {
        format!("{start_line}-{end_line}")
    } else {
        start_line.to_string()
    }
}

/// `composeChatContext`: append the items to a message. Without items, the
/// message is unchanged.
pub fn compose_chat_context(text: &str, items: &[ChatContextItem]) -> String {
    if items.is_empty() {
        return text.to_string();
    }
    let mut lines = vec![OPEN.to_string()];
    lines.extend(items.iter().map(format_item));
    lines.push(CLOSE.into());
    let block = lines.join("\n");
    if monocode_core::js::trim(text).is_empty() {
        block
    } else {
        format!("{text}\n\n{block}")
    }
}

/// `composerSeedForAddToChat`: the first composer draft for an add-to-chat
/// request that opens a new session.
pub fn composer_seed_for_add_to_chat(item: &ChatContextItem) -> String {
    compose_chat_context("", std::slice::from_ref(item))
}

/// `isMarkdownBlockquotePosition`: `position` sits on a `>` quote line.
pub fn is_markdown_blockquote_position(text: &str, position: usize) -> bool {
    let index = floor_char_boundary(text, position.min(text.len()));
    let line_start = text[..index].rfind('\n').map_or(0, |at| at + 1);
    BLOCKQUOTE_PREFIX.is_match(&text[line_start..index])
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
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

fn escape_body(text: &str) -> String {
    RESERVED_TAG.replace_all(text, "<\\$1$2$3").into_owned()
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', "&#10;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_a_code_selection_block() {
        let item = ChatContextItem::Code {
            path: "src/value.ts".into(),
            start_line: 3,
            end_line: 5,
        };
        assert_eq!(
            composer_seed_for_add_to_chat(&item),
            "<attached_context>\n<code_selection path=\"src/value.ts\" lines=\"3-5\" />\n</attached_context>"
        );
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            serde_json::json!({ "kind": "code", "path": "src/value.ts", "startLine": 3, "endLine": 5 })
        );
    }

    #[test]
    fn escapes_reserved_tags_in_quotes() {
        let item = ChatContextItem::Quote {
            text: "a <quoted_text> b\n\nc".into(),
        };
        assert_eq!(
            compose_chat_context("hi", &[item]),
            "hi\n\n<attached_context>\n<quoted_text>\n> a <\\quoted_text> b\n>\n> c\n</quoted_text>\n</attached_context>"
        );
    }

    #[test]
    fn finds_blockquote_positions() {
        let text = "@a\n> @b";
        assert!(!is_markdown_blockquote_position(text, 0));
        assert!(is_markdown_blockquote_position(text, 5));
        assert!(!is_markdown_blockquote_position("    > x", 6));
    }
}
