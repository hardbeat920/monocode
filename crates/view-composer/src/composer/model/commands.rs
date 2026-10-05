//! The composer's slash commands. Ports of the small command files in
//! src/features/sessions/model: plan.ts (`PLAN_COMMAND`; the parser lives in
//! `monocode_core::plan`), operatorCommand.ts, orchestratorCommand.ts,
//! draftCommand.ts, mcpCommand.ts, compact.ts, sessionFolderCommand.ts, and
//! the `/btw` helpers in btw.ts.
//!
//! The parsers are copied from monocode-engine's `submit` package. Delete
//! the copy once the view crates depend on the engine.

use monocode_core::js;

use super::skills::Skill;
use super::text::{leading_command, standalone_command};

pub use monocode_core::plan::{PlanCommand, consume_plan_command};

/// `{ text, matched }` from the `consume*Command` functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedCommand {
    pub text: String,
    pub matched: bool,
}

fn consume(text: &str, names: &[&str]) -> ConsumedCommand {
    match leading_command(text, names) {
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

pub const PLAN: &str = "plan";
pub const OPERATOR: &str = "operator";
pub const ORCHESTRATOR: &str = "orchestrator";
pub const DRAFT: &str = "draft";
pub const MCP: &str = "mcp";
pub const COMPACT: &str = "compact";
pub const SESSION_FOLDER: &str = "add-to-folder";
pub const BTW: &str = "btw";

/// `PLAN_COMMAND`.
pub fn plan_command() -> Skill {
    Skill::builtin(PLAN, PLAN, monocode_core::plan::PLAN_COMMAND_DESCRIPTION)
}

/// `OPERATOR_COMMAND`.
pub fn operator_command() -> Skill {
    Skill::builtin(
        OPERATOR,
        OPERATOR,
        "Give this thread access to MonoCode sessions, folders, and notes.",
    )
}

/// `ORCHESTRATOR_COMMAND`.
pub fn orchestrator_command() -> Skill {
    Skill::builtin(
        ORCHESTRATOR,
        ORCHESTRATOR,
        "Plan and coordinate agent work.",
    )
}

/// `DRAFT_COMMAND`.
pub fn draft_command() -> Skill {
    Skill::builtin(
        DRAFT,
        DRAFT,
        "Save this message without starting the agent.",
    )
}

/// `MCP_COMMAND`.
pub fn mcp_command() -> Skill {
    Skill::builtin(MCP, MCP, "Find an MCP server for this message.")
}

/// `COMPACT_COMMAND`.
pub fn compact_command() -> Skill {
    Skill::builtin(
        COMPACT,
        COMPACT,
        "Summarize older conversation context to free space.",
    )
}

/// `SESSION_FOLDER_COMMAND`.
pub fn session_folder_command() -> Skill {
    Skill::builtin(
        SESSION_FOLDER,
        SESSION_FOLDER,
        "Place this session in an existing or new sidebar folder.",
    )
}

/// `BTW_COMMAND`.
pub fn btw_command() -> Skill {
    Skill::builtin(
        BTW,
        BTW,
        "Ask a read-only side question about the current turn.",
    )
}

/// `consumeOperatorCommand`. `/mono` and `/monocode` stay as unlisted
/// aliases for existing drafts and threads.
pub fn consume_operator_command(text: &str) -> ConsumedCommand {
    consume(text, &[OPERATOR, "mono", "monocode"])
}

/// `consumeOrchestratorCommand`.
pub fn consume_orchestrator_command(text: &str) -> ConsumedCommand {
    consume(text, &[ORCHESTRATOR])
}

/// `consumeDraftCommand`.
pub fn consume_draft_command(text: &str) -> ConsumedCommand {
    consume(text, &[DRAFT])
}

/// `isMcpCommand`.
pub fn is_mcp_command(text: &str) -> bool {
    standalone_command(text, MCP)
}

/// `isCompactCommand`.
pub fn is_compact_command(text: &str) -> bool {
    standalone_command(text, COMPACT)
}

/// `isSessionFolderCommand`.
pub fn is_session_folder_command(text: &str) -> bool {
    standalone_command(text, SESSION_FOLDER)
}

/// `consumeSessionFolderCommand`.
pub fn consume_session_folder_command(text: &str) -> ConsumedCommand {
    consume(text, &[SESSION_FOLDER])
}

/// The input to `runsSessionFolderCommandOnSpace`. Offsets are bytes.
#[derive(Debug, Clone, Default)]
pub struct SpaceKeyInput<'a> {
    pub text: &'a str,
    pub selection_start: usize,
    pub selection_end: usize,
    pub alt_key: bool,
    pub ctrl_key: bool,
    pub meta_key: bool,
}

/// `runsSessionFolderCommandOnSpace`: an unmodified space at the end of
/// `/add-to-folder` runs the command.
pub fn runs_session_folder_command_on_space(input: &SpaceKeyInput<'_>) -> bool {
    !input.alt_key
        && !input.ctrl_key
        && !input.meta_key
        && input.selection_start == input.text.len()
        && input.selection_end == input.text.len()
        && leading_command(input.text, &[SESSION_FOLDER]) == Some(input.text.len())
        && !input.text.ends_with(js::is_space)
}

/// `consumeBtwCommand`: `/btw` as the leading command, with the rest as the
/// side question. `/^\s*\/btw(?:\s+([\s\S]*))?\s*$/i`.
pub fn consume_btw_command(text: &str) -> ConsumedCommand {
    match leading_command(text, &[BTW]) {
        Some(end) => ConsumedCommand {
            text: js::trim(&text[end..]).to_string(),
            matched: true,
        },
        None => ConsumedCommand {
            text: text.to_string(),
            matched: false,
        },
    }
}

/// `consumeBtwPrefix`: the text after a `/btw ` typed at the start, once
/// whitespace commits the command. `/^\s*\/btw\s+/i`.
pub fn consume_btw_prefix(text: &str) -> Option<String> {
    let end = leading_command(text, &[BTW])?;
    // `\s+` needs at least one whitespace character after the name.
    let name_end = text[..end].trim_end_matches(js::is_space).len();
    if name_end == end {
        return None;
    }
    Some(text[end..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // mcpCommand.test.ts
    #[test]
    fn mcp_matches_only_a_standalone_command() {
        assert!(is_mcp_command(" /MCP "));
        assert!(!is_mcp_command("/mcp explain this server"));
        assert!(!is_mcp_command("please check /mcp"));
    }

    // compact.test.ts
    #[test]
    fn matches_a_standalone_compact_command() {
        assert!(is_compact_command("/compact"));
        assert!(is_compact_command("  /COMPACT\n"));
    }

    #[test]
    fn compact_does_not_consume_ordinary_prompt_text() {
        assert!(!is_compact_command("/compact now"));
        assert!(!is_compact_command("mention /compact in docs"));
        assert!(!is_compact_command("/compacted"));
    }

    // operatorCommand.test.ts
    #[test]
    fn consumes_operator_and_its_legacy_aliases() {
        assert_eq!(
            consume_operator_command("/operator list my sessions"),
            ConsumedCommand {
                text: "list my sessions".into(),
                matched: true
            }
        );
        assert!(consume_operator_command("/MonoCode hi").matched);
        assert!(!consume_operator_command("/operators").matched);
    }

    #[test]
    fn consumes_draft_and_orchestrator_prefixes() {
        assert_eq!(
            consume_draft_command("/draft  later"),
            ConsumedCommand {
                text: "later".into(),
                matched: true
            }
        );
        assert!(!consume_draft_command("/drafts").matched);
        assert_eq!(
            consume_orchestrator_command(" /Orchestrator split it").text,
            "split it"
        );
    }

    // sessionFolderCommand.test.ts
    #[test]
    fn matches_a_standalone_add_to_folder_command() {
        assert!(is_session_folder_command("/add-to-folder"));
        assert!(is_session_folder_command("  /ADD-TO-FOLDER\n"));
        assert!(!is_session_folder_command("/add-to-folder project"));
        assert!(!is_session_folder_command("/add-to-folder-later"));
    }

    #[test]
    fn removes_the_leading_folder_command_and_keeps_the_agent_prompt() {
        assert_eq!(
            consume_session_folder_command("/add-to-folder   Build the settings screen"),
            ConsumedCommand {
                text: "Build the settings screen".into(),
                matched: true
            }
        );
    }

    #[test]
    fn runs_the_folder_command_on_an_unmodified_space_at_its_end() {
        let input = |text: &'static str, meta: bool| SpaceKeyInput {
            text,
            selection_start: text.len(),
            selection_end: text.len(),
            meta_key: meta,
            ..SpaceKeyInput::default()
        };
        assert!(runs_session_folder_command_on_space(&input(
            "/add-to-folder",
            false
        )));
        assert!(!runs_session_folder_command_on_space(&input(
            "/add-to-folder",
            true
        )));
        assert!(!runs_session_folder_command_on_space(&input(
            "/add-to-folder ",
            false
        )));
        assert!(!runs_session_folder_command_on_space(&input(
            "/add-to-folder x",
            false
        )));
        assert!(!runs_session_folder_command_on_space(&SpaceKeyInput {
            text: "/add-to-folder",
            selection_start: 0,
            selection_end: 0,
            ..SpaceKeyInput::default()
        }));
    }

    // btw.test.ts
    #[test]
    fn opens_an_empty_btw_thread_for_the_bare_command() {
        assert_eq!(btw_command().invocation, "btw");
        assert_eq!(
            consume_btw_command("  /BTW  "),
            ConsumedCommand {
                text: String::new(),
                matched: true
            }
        );
    }

    #[test]
    fn keeps_the_text_after_the_command_as_the_side_question() {
        assert_eq!(
            consume_btw_command("/btw  what does this do?  "),
            ConsumedCommand {
                text: "what does this do?".into(),
                matched: true
            }
        );
    }

    #[test]
    fn only_consumes_a_leading_btw_command() {
        assert_eq!(
            consume_btw_command("ask /btw about this"),
            ConsumedCommand {
                text: "ask /btw about this".into(),
                matched: false
            }
        );
    }

    #[test]
    fn btw_prefix_commits_once_whitespace_follows_the_command() {
        assert_eq!(consume_btw_prefix("/btw"), None);
        assert_eq!(consume_btw_prefix("/btw "), Some(String::new()));
        assert_eq!(
            consume_btw_prefix(" /BTW  why is this slow?"),
            Some("why is this slow?".into())
        );
    }
}
