//! Port of src/features/sessions/model/quoteDraft.ts: what "Add to chat"
//! puts into a composer, and the Markdown blockquote check the skill and
//! mention parsers share.
//!
//! `requestAddToChat` dispatched a window event. Here
//! `Submit::request_add_to_chat` emits `SubmitEvent::AddToChat` instead.
//! Positions are byte offsets.

use monocode_core::js;

use super::chat_context::{ChatContextItem, add_chat_context, compose_chat_context};
use super::text::normalize_newlines;

/// `ComposerInsert`: text for the draft, or a context chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerInsert {
    Text { text: String },
    Context { item: ChatContextItem },
}

/// `ComposerInsertRequest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerInsertRequest {
    pub id: i64,
    pub insert: ComposerInsert,
}

/// `composerSeedForAddToChat`: the initial draft for an add-to-chat request
/// that opens a new session.
pub fn composer_seed_for_add_to_chat(item: &ChatContextItem) -> String {
    compose_chat_context("", std::slice::from_ref(item))
}

/// `ComposerInsertConsumption`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerInsertConsumption {
    pub draft: String,
    pub context: Vec<ChatContextItem>,
    pub consumed_id: Option<i64>,
    pub changed: bool,
}

/// `isMarkdownBlockquotePosition`: the line up to `position` starts with up
/// to three spaces and a `>`.
pub fn is_markdown_blockquote_position(text: &str, position: usize) -> bool {
    let mut index = position.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    // At 0 the TypeScript sliced an empty line either way.
    if index == 0 {
        return false;
    }
    // `lastIndexOf("\n", index - 1) + 1`.
    let line_start = text[..index].rfind('\n').map_or(0, |newline| newline + 1);
    let line = &text[line_start..index];
    let spaces = line.bytes().take_while(|b| *b == b' ').count().min(3);
    line[spaces..].starts_with('>')
}

/// `appendComposerInsert`.
pub fn append_composer_insert(draft: &str, text: &str) -> String {
    let normalized = normalize_newlines(text);
    let selected = js::trim(&normalized);
    if selected.is_empty() {
        return draft.to_string();
    }
    join_composer_insert(draft, selected)
}

/// `consumeComposerInsert`.
pub fn consume_composer_insert(
    draft: &str,
    context: &[ChatContextItem],
    consumed_id: Option<i64>,
    request: Option<&ComposerInsertRequest>,
) -> ComposerInsertConsumption {
    let unchanged = || ComposerInsertConsumption {
        draft: draft.to_string(),
        context: context.to_vec(),
        consumed_id,
        changed: false,
    };
    let Some(request) = request else {
        return unchanged();
    };
    if Some(request.id) == consumed_id {
        return unchanged();
    }
    match &request.insert {
        ComposerInsert::Context { item } => {
            let next = add_chat_context(context, item.clone());
            let changed = next.len() != context.len();
            ComposerInsertConsumption {
                draft: draft.to_string(),
                context: next,
                consumed_id: Some(request.id),
                changed,
            }
        }
        ComposerInsert::Text { text } => {
            let next = append_composer_insert(draft, text);
            let changed = next != draft;
            ComposerInsertConsumption {
                draft: next,
                context: context.to_vec(),
                consumed_id: Some(request.id),
                changed,
            }
        }
    }
}

/// `acknowledgeComposerInsert`.
pub fn acknowledge_composer_insert(
    current: Option<ComposerInsertRequest>,
    handled_id: i64,
) -> Option<ComposerInsertRequest> {
    current.filter(|request| request.id != handled_id)
}

fn join_composer_insert(draft: &str, block: &str) -> String {
    let separator = if draft.is_empty() || draft.ends_with("\n\n") {
        ""
    } else if draft.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{draft}{separator}{block}\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::submit::chat_context::split_chat_context;

    fn code() -> ChatContextItem {
        ChatContextItem::Code {
            path: "src/value.ts".into(),
            start_line: 3,
            end_line: 5,
        }
    }

    // appendComposerInsert
    #[test]
    fn separates_an_existing_draft() {
        for (draft, expected) in [
            ("", "Comment\n\n"),
            ("draft", "draft\n\nComment\n\n"),
            ("draft\n", "draft\n\nComment\n\n"),
            ("draft\n\n", "draft\n\nComment\n\n"),
        ] {
            assert_eq!(append_composer_insert(draft, " Comment\r\n"), expected);
        }
    }

    #[test]
    fn ignores_whitespace_only_text() {
        assert_eq!(append_composer_insert("draft", "  \n "), "draft");
    }

    // composerSeedForAddToChat
    #[test]
    fn seeds_a_new_composer_with_the_chip_and_no_typed_text() {
        let split = split_chat_context(&composer_seed_for_add_to_chat(&code()));
        assert_eq!(split.text, "");
        assert_eq!(split.items, vec![code()]);
    }

    // isMarkdownBlockquotePosition
    #[test]
    fn recognizes_tokens_after_nested_quote_markers() {
        let text = "plain /one\n> /two\n  >> @file";
        assert!(!is_markdown_blockquote_position(
            text,
            text.find("/one").unwrap()
        ));
        assert!(is_markdown_blockquote_position(
            text,
            text.find("/two").unwrap()
        ));
        assert!(is_markdown_blockquote_position(
            text,
            text.find("@file").unwrap()
        ));
        assert!(!is_markdown_blockquote_position("", 0));
        assert!(!is_markdown_blockquote_position("\n/x", 1));
    }

    // consumeComposerInsert
    fn request() -> ComposerInsertRequest {
        ComposerInsertRequest {
            id: 1,
            insert: ComposerInsert::Context { item: code() },
        }
    }

    #[test]
    fn adds_a_context_chip_once_and_leaves_the_draft_alone() {
        assert_eq!(
            consume_composer_insert("draft", &[], None, Some(&request())),
            ComposerInsertConsumption {
                draft: "draft".into(),
                context: vec![code()],
                consumed_id: Some(1),
                changed: true,
            }
        );
        assert_eq!(
            consume_composer_insert("draft", &[], Some(1), Some(&request())),
            ComposerInsertConsumption {
                draft: "draft".into(),
                context: vec![],
                consumed_id: Some(1),
                changed: false,
            }
        );
    }

    #[test]
    fn does_not_attach_the_same_chip_twice() {
        let second = ComposerInsertRequest { id: 2, ..request() };
        let result = consume_composer_insert("", &[code()], Some(1), Some(&second));
        assert_eq!(result.context, vec![code()]);
        assert_eq!(result.consumed_id, Some(2));
        assert!(!result.changed);
    }

    #[test]
    fn inserts_text_into_the_draft() {
        let text = ComposerInsertRequest {
            id: 3,
            insert: ComposerInsert::Text {
                text: "Note: Auth\n\nUse a cookie.".into(),
            },
        };
        assert_eq!(
            consume_composer_insert("draft", &[code()], None, Some(&text)),
            ComposerInsertConsumption {
                draft: "draft\n\nNote: Auth\n\nUse a cookie.\n\n".into(),
                context: vec![code()],
                consumed_id: Some(3),
                changed: true,
            }
        );
    }

    // acknowledgeComposerInsert
    #[test]
    fn clears_only_the_request_that_was_acknowledged() {
        let current = ComposerInsertRequest {
            id: 2,
            insert: ComposerInsert::Text { text: "x".into() },
        };
        assert_eq!(acknowledge_composer_insert(Some(current.clone()), 2), None);
        assert_eq!(
            acknowledge_composer_insert(Some(current.clone()), 1),
            Some(current)
        );
    }
}
