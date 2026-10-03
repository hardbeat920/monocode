//! Small helpers the transcript reads from other TypeScript modules:
//!
//! - `supportsHarnessLogin` and `isHarnessAuthError` from
//!   src/integrations/harness/core/authSupport.ts.
//! - `visibleUserPrompt` from src/features/orchestration/model/orchestration.ts.
//! - `harnessForTurn` from src/features/sessions/model/secondOpinion.ts.
//! - `lastUserTurnBlock` from src/features/sessions/model/editLastTurn.ts.
//! - `isOperatorUserTurn` and `operatorUserPrompt` from
//!   src/features/sessions/model/operatorCommand.ts.
//!
//! The engine owns the full versions of these modules. The transcript only
//! needs these read-only pieces, so they are ported here to keep the view free
//! of the engine.

use std::sync::{Arc, LazyLock};

use monocode_core::js;
use monocode_core::{Block, BlockRole, HarnessId};
use regex::Regex;

/// `supportsHarnessLogin`: providers with an account-level login command.
pub fn supports_harness_login(harness: HarnessId) -> bool {
    matches!(
        harness,
        HarnessId::Claude | HarnessId::Codex | HarnessId::Cursor | HarnessId::Grok | HarnessId::Fx
    )
}

static AUTH_ERRORS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\bauthentication required\b",
        r"(?i)\bnot (?:authenticated|signed in|logged in)\b",
        r"(?i)\b(?:sign-in|login|session) (?:has )?expired\b",
        r"(?i)\bplease (?:sign|log) in\b",
        r#"(?i)\brun [`'"]?\S+ (?:auth )?login\b"#,
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("auth error pattern"))
    .collect()
});

/// `isHarnessAuthError`: a provider error asking the user to sign in again.
pub fn is_harness_auth_error(message: &str) -> bool {
    AUTH_ERRORS.iter().any(|pattern| pattern.is_match(message))
}

static ASSIGNMENT_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)(?:\r?\n[ \t]*)*<monocode_assignment\b[^>]*>.*?</monocode_assignment>")
        .expect("assignment pattern")
});

/// `visibleUserPrompt`: the prompt without the worker assignment envelope.
pub fn visible_user_prompt(text: &str) -> String {
    let stripped = ASSIGNMENT_BLOCK.replace_all(text, "");
    js::trim_end(&stripped).to_string()
}

/// `harnessForTurn`: which provider answered a turn. The recorded model wins,
/// then the handoff before the turn, then the first handoff's source.
pub fn harness_for_turn(
    blocks: &[Arc<Block>],
    turn: &[Arc<Block>],
    session_harness: HarnessId,
) -> HarnessId {
    if let Some(recorded) = turn
        .iter()
        .find(|block| block.role == BlockRole::User)
        .and_then(|block| block.turn_model.as_ref())
    {
        return recorded.harness;
    }
    let start = turn
        .first()
        .and_then(|first| blocks.iter().position(|block| block.id == first.id));
    if let Some(start) = start
        && start > 0
    {
        for block in blocks[..start].iter().rev() {
            if let Some(handoff) = &block.handoff {
                return handoff.to;
            }
        }
    }
    blocks
        .iter()
        .find_map(|block| block.handoff.as_ref())
        .map(|handoff| handoff.from)
        .unwrap_or(session_harness)
}

/// `lastUserTurnStartIndex`.
pub fn last_user_turn_start_index(blocks: &[Arc<Block>]) -> Option<usize> {
    blocks.iter().rposition(|block| {
        block.role == BlockRole::User && !block.is_internal() && !block.is_draft()
    })
}

/// `lastUserTurnBlock`: the user block that starts the latest turn.
pub fn last_user_turn_block(blocks: &[Arc<Block>]) -> Option<&Arc<Block>> {
    last_user_turn_start_index(blocks).map(|index| &blocks[index])
}

/// `LEGACY_COMMAND`: `/^\s*\/(?:mono|monocode)(?=\s|$)\s*/i`.
static LEGACY_COMMAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*/(?:mono|monocode)(?:\s+|$)").expect("legacy command"));

/// `isOperatorUserTurn`: a submitted /operator turn.
pub fn is_operator_user_turn(block: &Block) -> bool {
    block.role == BlockRole::User
        && !block.is_draft()
        && !block.is_internal()
        // This persisted field keeps its original name for saved session compatibility.
        && (block.monocode == Some(true) || LEGACY_COMMAND.is_match(&block.text))
}

/// `operatorUserPrompt`: old command messages render without their prefix.
pub fn operator_user_prompt(block: &Block) -> String {
    match LEGACY_COMMAND.find(&block.text) {
        None => block.text.clone(),
        Some(found) => {
            let rest = js::trim(&block.text[found.end()..]);
            if rest.is_empty() {
                "Explain what you can do in MonoCode with the app CLI.".into()
            } else {
                rest.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::block::{HandoffMeta, HandoffStatus, TurnModel};

    fn block(id: &str, role: BlockRole, text: &str) -> Arc<Block> {
        Arc::new(Block::new(id, role, text))
    }

    #[test]
    fn recognizes_provider_auth_errors() {
        assert!(is_harness_auth_error(
            "Authentication required\n\nGrok Build is not signed in."
        ));
        assert!(is_harness_auth_error("Please log in to continue"));
        assert!(is_harness_auth_error("Run `claude auth login` first"));
        assert!(!is_harness_auth_error("Provider connection lost"));
        assert!(supports_harness_login(HarnessId::Grok));
        assert!(!supports_harness_login(HarnessId::Pi));
    }

    #[test]
    fn hides_the_assignment_envelope() {
        let text = "Review the current branch against main.\n\n<monocode_assignment>\nYou are a worker managed by a MonoCode lead.\n</monocode_assignment>";
        assert_eq!(
            visible_user_prompt(text),
            "Review the current branch against main."
        );
        assert_eq!(visible_user_prompt("plain  "), "plain");
    }

    #[test]
    fn strips_the_legacy_operator_command() {
        let mut new = Block::new("u", BlockRole::User, "list my notes");
        new.monocode = Some(true);
        assert!(is_operator_user_turn(&new));
        assert_eq!(operator_user_prompt(&new), "list my notes");
        let legacy = Block::new("old", BlockRole::User, "/monocode list my notes");
        assert!(is_operator_user_turn(&legacy));
        assert_eq!(operator_user_prompt(&legacy), "list my notes");
        let bare = Block::new("bare", BlockRole::User, "/mono");
        assert_eq!(
            operator_user_prompt(&bare),
            "Explain what you can do in MonoCode with the app CLI."
        );
        assert!(!is_operator_user_turn(&Block::new(
            "x",
            BlockRole::User,
            "/monorepo"
        )));
    }

    #[test]
    fn finds_the_harness_that_answered_a_turn() {
        let mut handoff = Block::new("h", BlockRole::Handoff, "brief");
        handoff.handoff = Some(HandoffMeta {
            from: HarnessId::Cursor,
            to: HarnessId::Claude,
            status: HandoffStatus::Ready,
            pending: None,
            extra: Default::default(),
        });
        let blocks = vec![
            block("u1", BlockRole::User, "go"),
            Arc::new(handoff),
            block("u2", BlockRole::User, "more"),
        ];
        assert_eq!(
            harness_for_turn(&blocks, &blocks[0..1], HarnessId::Codex),
            HarnessId::Cursor
        );
        assert_eq!(
            harness_for_turn(&blocks, &blocks[2..3], HarnessId::Codex),
            HarnessId::Claude
        );
        let mut recorded = Block::new("u3", BlockRole::User, "x");
        recorded.turn_model = Some(TurnModel {
            harness: HarnessId::Pi,
            id: "pi:x".into(),
            name: "X".into(),
            extra: Default::default(),
        });
        let turn = vec![Arc::new(recorded)];
        assert_eq!(
            harness_for_turn(&blocks, &turn, HarnessId::Codex),
            HarnessId::Pi
        );
    }

    #[test]
    fn last_user_turn_skips_drafts_and_internal_turns() {
        let mut draft = Block::new("d", BlockRole::User, "draft");
        draft.draft = Some(true);
        let blocks = vec![
            block("u1", BlockRole::User, "go"),
            block("a", BlockRole::Assistant, "ok"),
            Arc::new(draft),
        ];
        assert_eq!(
            last_user_turn_block(&blocks).map(|b| b.id.as_str()),
            Some("u1")
        );
    }
}
