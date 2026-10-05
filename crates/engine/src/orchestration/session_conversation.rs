//! Port of src/features/agent-app/model/sessionConversation.ts: a session's
//! conversation prose for `sessions.read`, with a stable cursor for older
//! exchanges.

use monocode_core::{Block, BlockRole, Session, js};
use serde::Serialize;
use serde_json::Value;

use super::plan::js_integer;
use crate::submit::operator_command::operator_user_prompt;

/// `SessionReadOptions`: the raw JSON fields, checked here.
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionReadOptions<'a> {
    pub before: Option<&'a str>,
    pub limit: Option<&'a Value>,
    pub max_chars: Option<&'a Value>,
}

/// One capped message.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Capped {
    pub text: String,
    pub truncated: bool,
}

/// One user turn and the last assistant reply to it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTurn {
    pub turn_id: String,
    pub user: Capped,
    pub assistant: Option<Capped>,
    pub earlier_assistant_messages: usize,
}

/// What `sessions.read` returns.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPage {
    pub session_id: String,
    pub title: String,
    pub busy: bool,
    pub has_draft: bool,
    pub turns: Vec<ConversationTurn>,
    pub next_before: Option<String>,
}

fn capped(text: &str, max_chars: usize) -> Capped {
    let trimmed = js::trim(text);
    Capped {
        text: js::slice_prefix(trimmed, max_chars).to_string(),
        truncated: js::len(trimmed) > max_chars,
    }
}

/// `options.x ?? fallback`, then `Number.isInteger` and the range.
fn bounded(value: Option<&Value>, fallback: i64, min: i64, max: i64) -> Option<i64> {
    match value {
        None | Some(Value::Null) => Some(fallback),
        Some(value) => js_integer(Some(value)).filter(|n| (min..=max).contains(n)),
    }
}

struct Exchange<'a> {
    user: &'a Block,
    assistants: Vec<&'a Block>,
}

/// `sessionConversationPage`: conversation prose only, newest first page.
pub fn session_conversation_page(
    session: &Session,
    options: SessionReadOptions<'_>,
) -> Result<ConversationPage, String> {
    let limit = bounded(options.limit, 3, 1, 3)
        .ok_or_else(|| "limit must be an integer from 1 to 3".to_string())?
        as usize;
    let max_chars = bounded(options.max_chars, 1200, 200, 6000)
        .ok_or_else(|| "maxChars must be an integer from 200 to 6000".to_string())?
        as usize;
    let mut exchanges: Vec<Exchange> = Vec::new();
    for block in &session.blocks {
        if block.role == BlockRole::User && !block.is_internal() && !block.is_draft() {
            exchanges.push(Exchange {
                user: block,
                assistants: Vec::new(),
            });
        } else if block.role == BlockRole::Assistant
            && !block.is_internal()
            && !js::trim(&block.text).is_empty()
            && let Some(last) = exchanges.last_mut()
        {
            last.assistants.push(block);
        }
    }
    let end = match options.before {
        Some(before) => exchanges
            .iter()
            .position(|exchange| exchange.user.id == before)
            .ok_or_else(|| "before is not a turn ID in this session".to_string())?,
        None => exchanges.len(),
    };
    let start = end.saturating_sub(limit);
    let selected = &exchanges[start..end];
    Ok(ConversationPage {
        session_id: session.id.clone(),
        title: session.title.clone(),
        busy: session.is_busy(),
        has_draft: session
            .blocks
            .iter()
            .any(|block| block.role == BlockRole::User && block.is_draft()),
        turns: selected
            .iter()
            .map(|exchange| ConversationTurn {
                turn_id: exchange.user.id.clone(),
                user: capped(&operator_user_prompt(exchange.user), max_chars),
                assistant: exchange
                    .assistants
                    .last()
                    .map(|block| capped(&block.text, max_chars)),
                earlier_assistant_messages: exchange.assistants.len().saturating_sub(1),
            })
            .collect(),
        next_before: if start > 0 {
            selected.first().map(|exchange| exchange.user.id.clone())
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;
    use serde_json::json;

    fn conversation(blocks: Vec<Block>) -> Session {
        let mut session = Session::blank("target", HarnessId::Codex, "codex:test", "/repo");
        session.blocks = blocks;
        session
    }

    fn block(id: &str, role: BlockRole, text: &str) -> Block {
        Block::new(id, role, text)
    }

    #[test]
    fn returns_the_newest_three_exchanges_without_tools_or_reasoning_then_older_pages() {
        let mut blocks = Vec::new();
        for i in 1..=5 {
            blocks.push(block(
                &format!("u{i}"),
                BlockRole::User,
                &format!("Question {i}"),
            ));
            blocks.push(block(
                &format!("t{i}"),
                BlockRole::Tool,
                "secret tool output",
            ));
            blocks.push(block(
                &format!("r{i}"),
                BlockRole::Reasoning,
                "private reasoning",
            ));
            blocks.push(block(
                &format!("a{i}"),
                BlockRole::Assistant,
                &format!("Answer {i}"),
            ));
        }
        let session = conversation(blocks);
        let recent = session_conversation_page(&session, Default::default()).unwrap();
        assert_eq!(
            recent
                .turns
                .iter()
                .map(|t| t.turn_id.as_str())
                .collect::<Vec<_>>(),
            ["u3", "u4", "u5"]
        );
        assert_eq!(recent.next_before.as_deref(), Some("u3"));
        let text = serde_json::to_string(&recent).unwrap();
        assert!(!text.contains("secret tool output") && !text.contains("private reasoning"));
        let limit = json!(2);
        let older = session_conversation_page(
            &session,
            SessionReadOptions {
                before: recent.next_before.as_deref(),
                limit: Some(&limit),
                max_chars: None,
            },
        )
        .unwrap();
        assert_eq!(
            older
                .turns
                .iter()
                .map(|t| t.turn_id.as_str())
                .collect::<Vec<_>>(),
            ["u1", "u2"]
        );
        assert_eq!(older.next_before, None);
    }

    #[test]
    fn caps_individual_messages_and_lets_a_caller_request_a_larger_excerpt() {
        let session = conversation(vec![
            block("u", BlockRole::User, &"U".repeat(7000)),
            block("a", BlockRole::Assistant, &"A".repeat(7000)),
        ]);
        assert_eq!(
            session_conversation_page(&session, Default::default())
                .unwrap()
                .turns[0]
                .assistant,
            Some(Capped {
                text: "A".repeat(1200),
                truncated: true
            })
        );
        let wide = json!(6000);
        let page = session_conversation_page(
            &session,
            SessionReadOptions {
                max_chars: Some(&wide),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.turns[0].user.text.len(), 6000);
        let four = json!(4);
        assert!(
            session_conversation_page(
                &session,
                SessionReadOptions {
                    limit: Some(&four),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .contains("limit")
        );
        assert!(
            session_conversation_page(
                &session,
                SessionReadOptions {
                    before: Some("missing"),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .contains("before")
        );
    }

    #[test]
    fn omits_drafts_and_shows_the_latest_assistant_response_in_each_exchange() {
        let session = conversation(vec![
            block("u", BlockRole::User, "/monocode list notes"),
            block("a1", BlockRole::Assistant, "Working"),
            block("a2", BlockRole::Assistant, "Done"),
            Block {
                draft: Some(true),
                ..block("draft", BlockRole::User, "unsent")
            },
        ]);
        let page = session_conversation_page(&session, Default::default()).unwrap();
        assert!(page.has_draft);
        assert_eq!(
            serde_json::to_value(&page.turns).unwrap(),
            json!([{
                "turnId": "u",
                "user": { "text": "list notes", "truncated": false },
                "assistant": { "text": "Done", "truncated": false },
                "earlierAssistantMessages": 1
            }])
        );
    }
}
