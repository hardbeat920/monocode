//! Port of src/features/sessions/model/operatorCommand.ts: the `/operator`
//! composer command that gives a thread access to MonoCode's app CLI.

use monocode_core::{Block, BlockRole, js};

use super::commands::ConsumedCommand;
use super::skills::BuiltinSkill;
use super::text::leading_command;

/// `OPERATOR_COMMAND`.
pub const OPERATOR_COMMAND: BuiltinSkill = BuiltinSkill::new(
    "operator",
    "operator",
    "Give this thread access to MonoCode sessions, folders, and notes.",
);

/// The answer when `/operator` arrives with no request.
pub const OPERATOR_DEFAULT_PROMPT: &str = "Explain what you can do in MonoCode with the app CLI.";

/// `consumeOperatorCommand`: activate app access with a leading composer
/// command. `/mono` and `/monocode` stay as unlisted aliases for existing
/// drafts and threads.
pub fn consume_operator_command(text: &str) -> ConsumedCommand {
    match leading_command(text, &["operator", "mono", "monocode"]) {
        Some(end) => ConsumedCommand {
            text: text[end..].to_string(),
            matched: true,
        },
        None => ConsumedCommand {
            text: text.to_string(),
            matched: false,
        },
    }
}

fn legacy_command(text: &str) -> Option<usize> {
    leading_command(text, &["mono", "monocode"])
}

/// `isOperatorUserTurn`: a submitted `/operator` turn keeps CLI access
/// available in later turns.
pub fn is_operator_user_turn(block: &Block) -> bool {
    block.role == BlockRole::User
        && !block.is_draft()
        && !block.is_internal()
        // This persisted field keeps its original name for saved session compatibility.
        && (block.monocode == Some(true) || legacy_command(&block.text).is_some())
}

/// `operatorUserPrompt`: old command messages render without their prefix.
pub fn operator_user_prompt(block: &Block) -> String {
    let Some(end) = legacy_command(&block.text) else {
        return block.text.clone();
    };
    let rest = js::trim(&block.text[end..]);
    if rest.is_empty() {
        OPERATOR_DEFAULT_PROMPT.to_string()
    } else {
        rest.to_string()
    }
}

/// `operatorEnabledInThread`.
pub fn operator_enabled_in_thread(blocks: &[Block]) -> bool {
    blocks.iter().any(is_operator_user_turn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consumed(text: &str, matched: bool) -> ConsumedCommand {
        ConsumedCommand {
            text: text.into(),
            matched,
        }
    }

    #[test]
    fn exposes_a_local_slash_command() {
        assert_eq!(OPERATOR_COMMAND.invocation, "operator");
        assert_eq!(OPERATOR_COMMAND.scope, "builtin");
    }

    #[test]
    fn consumes_only_a_leading_standalone_command() {
        assert_eq!(
            consume_operator_command("/operator list my notes"),
            consumed("list my notes", true)
        );
        assert_eq!(
            consume_operator_command("  /OPERATOR\nstart a session"),
            consumed("start a session", true)
        );
        assert_eq!(
            consume_operator_command("/operator-extra list notes"),
            consumed("/operator-extra list notes", false)
        );
        assert_eq!(
            consume_operator_command("Explain /operator"),
            consumed("Explain /operator", false)
        );
        for alias in ["/mono", "/monocode"] {
            assert_eq!(
                consume_operator_command(&format!("{alias} list notes")),
                consumed("list notes", true)
            );
        }
    }

    #[test]
    fn keeps_access_for_later_turns_when_a_submitted_user_turn_enabled_it() {
        let first = Block {
            monocode: Some(true),
            ..Block::new("first", BlockRole::User, "list notes")
        };
        assert!(operator_enabled_in_thread(&[
            first,
            Block::new("reply", BlockRole::Assistant, "Here are your notes."),
            Block::new("followup", BlockRole::User, "Start two sessions"),
        ]));
        let draft = Block {
            draft: Some(true),
            monocode: Some(true),
            ..Block::new("draft", BlockRole::User, "list notes")
        };
        assert!(!operator_enabled_in_thread(&[
            draft,
            Block::new("other", BlockRole::User, "Explain /operator"),
        ]));
        for alias in ["/mono", "/monocode"] {
            let legacy = Block::new("legacy", BlockRole::User, format!("{alias} list notes"));
            assert!(is_operator_user_turn(&legacy));
            assert_eq!(operator_user_prompt(&legacy), "list notes");
            assert!(operator_enabled_in_thread(&[legacy]));
        }
    }
}
