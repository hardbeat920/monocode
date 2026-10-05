//! Port of the small composer command files in src/features/sessions/model:
//! draftCommand.ts, mcpCommand.ts, orchestratorCommand.ts, and compact.ts.
//! `/plan` lives in `monocode_core::plan`.

use super::skills::BuiltinSkill;
use super::text::{leading_command, standalone_command};

/// `{ text, matched }` from the `consume*Command` functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedCommand {
    pub text: String,
    pub matched: bool,
}

fn consume(text: &str, name: &str) -> ConsumedCommand {
    match leading_command(text, &[name]) {
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

/// `DRAFT_COMMAND`.
pub const DRAFT_COMMAND: BuiltinSkill = BuiltinSkill::new(
    "draft",
    "draft",
    "Save this message without starting the agent.",
);

/// `consumeDraftCommand`: consume `/draft` when it leads the composer text.
pub fn consume_draft_command(text: &str) -> ConsumedCommand {
    consume(text, "draft")
}

/// `MCP_COMMAND`.
pub const MCP_COMMAND: BuiltinSkill =
    BuiltinSkill::new("mcp", "mcp", "Find an MCP server for this message.");

/// `isMcpCommand`.
pub fn is_mcp_command(text: &str) -> bool {
    standalone_command(text, "mcp")
}

/// `ORCHESTRATOR_COMMAND`.
pub const ORCHESTRATOR_COMMAND: BuiltinSkill = BuiltinSkill::new(
    "orchestrator",
    "orchestrator",
    "Plan and coordinate agent work.",
);

/// `consumeOrchestratorCommand`.
pub fn consume_orchestrator_command(text: &str) -> ConsumedCommand {
    consume(text, "orchestrator")
}

/// `COMPACT_COMMAND`.
pub const COMPACT_COMMAND: BuiltinSkill = BuiltinSkill::new(
    "compact",
    "compact",
    "Summarize older conversation context to free space.",
);

/// `isCompactCommand`: the standalone command, without prompt text.
pub fn is_compact_command(text: &str) -> bool {
    standalone_command(text, "compact")
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
}
