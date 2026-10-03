//! The pure helpers App.tsx defines above `App` (lines 719-825), plus the
//! small functions submit borrows from worktrees.ts (`temporaryWorktreeBranchName`,
//! `namedWorktreeBranch`) and orchestration.ts (`shellPath`).

use monocode_core::block::{Block, BlockRole, PlanBlockMeta, PlanBuildTarget, PlanStatus};
use monocode_core::context_usage::drop_context_window;
use monocode_core::handoff::ComposerSwitchPlan;
use monocode_core::models::ModelCatalog;
use monocode_core::reducer::UserTurnExtra;
use monocode_core::session::{format_session_title, session_display_title};
use monocode_core::{Extra, HarnessId, ModelSettings, Session, js};

use crate::submit::handoff::plan_composer_switch;

/// `withPlanStatus`.
pub fn with_plan_status(session: &mut Session, block_id: &str, status: PlanStatus) {
    for block in &mut session.blocks {
        if block.id == block_id && block.role == BlockRole::Plan {
            let plan = block.plan.get_or_insert_with(|| PlanBlockMeta {
                key: None,
                status: PlanStatus::Ready,
                original_text: None,
                approved_text: None,
                edited: None,
                extra: Extra::new(),
            });
            plan.status = status;
        }
    }
}

/// `lastAssistantTextInTurn`: the last non-blank assistant text after the
/// last user message.
pub fn last_assistant_text_in_turn(session: &Session) -> String {
    for block in session.blocks.iter().rev() {
        if block.role == BlockRole::User {
            return String::new();
        }
        if block.role == BlockRole::Assistant && !js::trim(&block.text).is_empty() {
            return block.text.clone();
        }
    }
    String::new()
}

/// `withHarnessChoice`.
pub fn with_harness_choice(
    session: &mut Session,
    harness: HarnessId,
    model: &str,
    model_settings: ModelSettings,
) {
    session.title = if session.blocks.is_empty() {
        harness.label().to_string()
    } else {
        format_session_title(
            harness,
            &session_display_title(&session.title, session.harness),
        )
    };
    if session.model != model {
        session.context = drop_context_window(session.context.as_ref());
    }
    if session.harness != harness {
        session.provider_session_id = None;
        session.provider_account_id = None;
    }
    session.harness = harness;
    session.model = model.to_string();
    session.model_settings = model_settings;
}

/// Apply a `planComposerSwitch` result after `withHarnessChoice`.
pub(crate) fn apply_switch_plan(session: &mut Session, plan: ComposerSwitchPlan) {
    match plan {
        ComposerSwitchPlan::Arm { pending } => session.pending_switch = Some(pending),
        ComposerSwitchPlan::Revert {
            restore_provider_session_id,
            restore_provider_account_id,
        } => {
            session.pending_switch = None;
            session.provider_session_id = restore_provider_session_id;
            session.provider_account_id = restore_provider_account_id;
        }
        ComposerSwitchPlan::Empty { .. } => session.pending_switch = None,
        ComposerSwitchPlan::Model => {}
    }
}

/// `withPlanBuildTarget`: switch the session to the provider and model a
/// plan is built with.
pub fn with_plan_build_target(
    session: &mut Session,
    target: &PlanBuildTarget,
    catalog: &ModelCatalog,
) {
    let resolved = catalog.resolve_model(target.harness, Some(&target.model));
    let model_settings = catalog.merge_model_settings(&resolved, Some(&target.model_settings));
    let plan = plan_composer_switch(session, target.harness);
    with_harness_choice(session, target.harness, &resolved.id, model_settings);
    apply_switch_plan(session, plan);
}

/// The fields a user turn carries (`userTurnFields` in apply.ts), for the
/// user block written without `appendUser`.
pub(crate) fn apply_user_turn_fields(block: &mut Block, extra: &UserTurnExtra) {
    block.second_opinion = extra.second_opinion.clone();
    block.note_card = extra.note_card.clone();
    block.ci_context = extra
        .ci_context
        .clone()
        .filter(|context| !context.is_empty());
    block.internal = extra.internal.then_some(true);
    block.monocode = extra.monocode.then_some(true);
    block.intent = extra.intent;
    block.app_request_id = extra.app_request_id.clone().filter(|id| !id.is_empty());
}

/// `temporaryWorktreeBranchName`.
pub fn temporary_worktree_branch_name(id: &str) -> String {
    let token: String = id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(8)
        .collect::<String>()
        .to_lowercase();
    if token.is_empty() {
        format!("mc/{}", radix36(monocode_core::reducer::now_ms() as u64))
    } else {
        format!("mc/{token}")
    }
}

fn radix36(mut value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// `namedWorktreeBranch`.
pub fn named_worktree_branch(fragment: &str) -> Option<String> {
    let mut clean = js::trim(fragment);
    for prefix in ["mc", "monocode"] {
        if let Some(rest) = clean.strip_prefix(prefix)
            && rest.starts_with('/')
        {
            clean = rest.trim_start_matches('/');
            break;
        }
    }
    let clean = clean.trim_matches('/');
    (!clean.is_empty()).then(|| format!("mc/{clean}"))
}

/// `shellPath`: quote a path for a shell command line.
pub fn shell_path(path: &str) -> String {
    if path.starts_with('/') {
        let plain = path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | ':' | '-'));
        return if plain {
            path.to_string()
        } else {
            format!("'{}'", path.replace('\'', "'\\''"))
        };
    }
    if path.chars().any(|c| js::is_space(c) || c == '"') {
        format!("\"{}\"", path.replace('"', ""))
    } else {
        path.to_string()
    }
}

/// `text.slice(-limit)` in UTF-16 units.
pub(crate) fn keep_tail(text: &mut String, limit: usize) {
    let len = js::len(text);
    if len <= limit {
        return;
    }
    let start = crate::submit::text::byte_at_utf16(text, len - limit);
    text.drain(..start);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_worktree_branches() {
        assert_eq!(
            temporary_worktree_branch_name("AB-12cd-34ef-56"),
            "mc/ab12cd34"
        );
        assert!(temporary_worktree_branch_name("--").starts_with("mc/"));
        assert_eq!(
            named_worktree_branch(" mc//fix-footer/ ").as_deref(),
            Some("mc/fix-footer")
        );
        assert_eq!(named_worktree_branch("monocode/x").as_deref(), Some("mc/x"));
        assert_eq!(named_worktree_branch("mcx").as_deref(), Some("mc/mcx"));
        assert_eq!(named_worktree_branch(" / "), None);
    }

    #[test]
    fn quotes_shell_paths() {
        assert_eq!(
            shell_path("/Applications/MonoCode.app/Contents/MacOS/monocode"),
            "/Applications/MonoCode.app/Contents/MacOS/monocode"
        );
        assert_eq!(
            shell_path("/Users/me/My App/mono'code"),
            "'/Users/me/My App/mono'\\''code'"
        );
        assert_eq!(
            shell_path("C:\\Program Files\\monocode.exe"),
            "\"C:\\Program Files\\monocode.exe\""
        );
        assert_eq!(shell_path("C:\\bin\\monocode.exe"), "C:\\bin\\monocode.exe");
    }

    #[test]
    fn keeps_the_tail_in_utf16_units() {
        let mut text = "abcdef".to_string();
        keep_tail(&mut text, 4);
        assert_eq!(text, "cdef");
        let mut short = "ab".to_string();
        keep_tail(&mut short, 4);
        assert_eq!(short, "ab");
    }

    #[test]
    fn switches_harness_and_drops_provider_state() {
        let mut session = Session {
            provider_session_id: Some("p".into()),
            title: "Claude Code · Fix it".into(),
            blocks: vec![Block::new("u", BlockRole::User, "x")],
            ..Session::blank("s", HarnessId::Claude, "claude:a", "/r")
        };
        with_harness_choice(
            &mut session,
            HarnessId::Codex,
            "codex:b",
            ModelSettings::new(),
        );
        assert_eq!(session.harness, HarnessId::Codex);
        assert_eq!(session.provider_session_id, None);
        assert_eq!(session.model, "codex:b");
    }

    #[test]
    fn finds_the_last_assistant_text_in_the_turn() {
        let session = Session {
            blocks: vec![
                Block::new("a0", BlockRole::Assistant, "old"),
                Block::new("u", BlockRole::User, "go"),
                Block::new("a1", BlockRole::Assistant, "first"),
                Block::new("a2", BlockRole::Assistant, "  "),
            ],
            ..Session::blank("s", HarnessId::Claude, "m", "/r")
        };
        assert_eq!(last_assistant_text_in_turn(&session), "first");
    }

    // sessionDraft.test.ts: removeSessionDraft, which `Submit::remove_draft` uses.
    fn draft_session(title: &str, blocks: Vec<Block>) -> Session {
        Session {
            title: title.into(),
            blocks,
            ..Session::blank("s", HarnessId::Codex, "codex:default", "/repo")
        }
    }

    fn draft(id: &str, text: &str) -> Block {
        Block {
            draft: Some(true),
            ..Block::new(id, BlockRole::User, text)
        }
    }

    #[test]
    fn removes_a_follow_up_draft_without_changing_earlier_conversation_history() {
        let session = draft_session(
            "codex · Existing thread",
            vec![
                Block::new("sent", BlockRole::User, "Start here"),
                Block::new("reply", BlockRole::Assistant, "Done"),
                draft("draft", "Maybe later"),
            ],
        );
        let updated = monocode_core::session::remove_session_draft(&session, "draft").unwrap();
        assert_eq!(updated.blocks, session.blocks[..2]);
        assert_eq!(updated.title, "codex · Existing thread");
    }

    #[test]
    fn restores_a_draft_only_session_to_a_blank_untitled_state() {
        let session = draft_session("codex · Maybe later", vec![draft("draft", "Maybe later")]);
        let updated = monocode_core::session::remove_session_draft(&session, "draft").unwrap();
        assert_eq!(updated.title, "codex");
        assert!(updated.blocks.is_empty());
    }

    #[test]
    fn keeps_a_custom_title_when_removing_the_only_draft() {
        let session = draft_session(
            "codex · Keep this name",
            vec![draft("draft", "Maybe later")],
        );
        let updated = monocode_core::session::remove_session_draft(&session, "draft").unwrap();
        assert_eq!(updated.title, "codex · Keep this name");
    }

    #[test]
    fn ignores_sent_messages_and_unknown_blocks() {
        let session = draft_session(
            "codex",
            vec![Block::new("sent", BlockRole::User, "Keep this")],
        );
        assert!(monocode_core::session::remove_session_draft(&session, "sent").is_none());
        assert!(monocode_core::session::remove_session_draft(&session, "missing").is_none());
    }
}
