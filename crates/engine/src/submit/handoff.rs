//! Port of src/features/sessions/model/handoff.ts: switching a conversation
//! to another provider. The composer arms the switch, the next send asks the
//! outgoing agent for a recap (or builds one), and the recap rides on the
//! incoming provider's prompts until it accepts a turn.
//!
//! `sessionChildHarnesses` lives in `runtime::reducer`; it is re-exported
//! here.

use monocode_core::block::{HandoffMeta, HandoffStatus, SecondOpinionKind, SecondOpinionMeta};
use monocode_core::handoff::{ComposerSwitchPlan, HandoffComposerCard};
use monocode_core::paths::display_path;
use monocode_core::session::PendingHarnessSwitch;
use monocode_core::{Block, BlockRole, Extra, HarnessId, Session, js};
use monocode_harness::core::json_text::limit_section;

pub use crate::runtime::reducer::session_child_harnesses;

use super::ci_repair::compact_ci_repair_context;
use super::second_opinion::{is_edit_block, set_label, tool_title};
use super::text::collapse_space;

const USER_LINE_LIMIT: usize = 240;
const ASSISTANT_LIMIT: usize = 500;
const PLAN_LIMIT: usize = 400;
const BRIEF_LIMIT: usize = 1_800;
const REQUEST_LIMIT: usize = 1_500;
const MAX_PRIOR_USERS: usize = 2;
const MIN_AGENT_BRIEF: usize = 40;

/// `buildHandoffComposerCard`.
pub fn build_handoff_composer_card(
    from: HarnessId,
    to: HarnessId,
    brief: &str,
    user_request: &str,
    files: &[String],
) -> HandoffComposerCard {
    let request = js::trim(&collapse_space(user_request)).to_string();
    HandoffComposerCard {
        from,
        to,
        brief: brief.to_string(),
        request: (!request.is_empty()).then(|| js::slice_prefix(&request, 240).to_string()),
        files: (!files.is_empty()).then_some(files.len() as i64),
    }
}

/// `handoffTurnCard`: the transcript card for a handoff user turn.
pub fn handoff_turn_card(card: &HandoffComposerCard) -> SecondOpinionMeta {
    SecondOpinionMeta {
        from: card.from,
        to: card.to,
        request: card.request.clone().filter(|request| !request.is_empty()),
        files: card.files.filter(|files| *files > 0),
        kind: Some(SecondOpinionKind::Handoff),
        extra: Extra::new(),
    }
}

/// `planComposerSwitch`: what changing the composer's provider does.
pub fn plan_composer_switch(session: &Session, next: HarnessId) -> ComposerSwitchPlan {
    if session.harness == next {
        return ComposerSwitchPlan::Model;
    }
    if let Some(pending) = &session.pending_switch
        && next == pending.from
    {
        return ComposerSwitchPlan::Revert {
            restore_provider_session_id: pending
                .from_provider_session_id
                .clone()
                .filter(|id| !id.is_empty()),
            restore_provider_account_id: pending
                .from_provider_account_id
                .clone()
                .filter(|id| !id.is_empty()),
        };
    }
    if !session
        .blocks
        .iter()
        .any(|block| block.role == BlockRole::User)
        && session.pending_switch.is_none()
    {
        return ComposerSwitchPlan::Empty {
            forget: session.harness,
        };
    }
    ComposerSwitchPlan::Arm {
        pending: session
            .pending_switch
            .clone()
            .unwrap_or_else(|| PendingHarnessSwitch {
                from: session.harness,
                from_model: session.model.clone(),
                from_settings: session.model_settings.clone(),
                from_provider_session_id: session
                    .provider_session_id
                    .clone()
                    .filter(|id| !id.is_empty()),
                from_provider_account_id: session
                    .provider_account_id
                    .clone()
                    .filter(|id| !id.is_empty()),
            }),
    }
}

/// `sessionThroughTurn`: the session as of the end of this turn, so a later
/// turn is not in the recap.
pub fn session_through_turn(session: &Session, turn: &[Block]) -> Session {
    let Some(last_id) = turn.last().map(|block| block.id.as_str()) else {
        return session.clone();
    };
    let Some(end) = session.blocks.iter().position(|block| block.id == last_id) else {
        return session.clone();
    };
    Session {
        blocks: session.blocks[..=end].to_vec(),
        ..session.clone()
    }
}

/// `lastHandoffBlock`.
pub fn last_handoff_block(blocks: &[Block]) -> Option<&Block> {
    blocks
        .iter()
        .rev()
        .find(|block| block.role == BlockRole::Handoff)
}

/// `isPreparingHandoff`.
pub fn is_preparing_handoff(session: &Session) -> bool {
    session.blocks.iter().any(|block| {
        block.role == BlockRole::Handoff
            && block
                .handoff
                .as_ref()
                .is_some_and(|handoff| handoff.status == HandoffStatus::Preparing)
    })
}

/// `pendingHandoff`'s result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingHandoff {
    pub from: HarnessId,
    pub to: HarnessId,
    pub text: String,
}

/// `pendingHandoff`: a ready recap that has not reached the incoming
/// provider yet.
pub fn pending_handoff(session: &Session) -> Option<PendingHandoff> {
    let last = last_handoff_block(&session.blocks)?;
    let handoff = last.handoff.as_ref()?;
    if handoff.pending != Some(true) || handoff.status != HandoffStatus::Ready {
        return None;
    }
    let trimmed = js::trim(&last.text);
    let text = if trimmed.is_empty() {
        build_deterministic_handoff(session, None, None)
    } else {
        trimmed.to_string()
    };
    if js::trim(&text).is_empty() {
        return None;
    }
    Some(PendingHandoff {
        from: handoff.from,
        to: handoff.to,
        text,
    })
}

/// `appendPreparingHandoff`.
pub fn append_preparing_handoff(session: &Session, from: HarnessId, to: HarnessId) -> Session {
    append_handoff_block(session, from, to, HandoffStatus::Preparing, "", false)
}

/// `appendReadyHandoff`.
pub fn append_ready_handoff(
    session: &Session,
    from: HarnessId,
    to: HarnessId,
    text: &str,
) -> Session {
    append_handoff_block(session, from, to, HandoffStatus::Ready, text, true)
}

/// `completeHandoff`: the recap is ready and pending for the next prompt.
pub fn complete_handoff(session: &Session, text: &str) -> Session {
    let Some(last) = last_handoff_block(&session.blocks) else {
        return session.clone();
    };
    let Some(handoff) = last.handoff.clone() else {
        return session.clone();
    };
    let stripped = strip_goal_sections(text);
    let brief = if stripped.is_empty() {
        build_deterministic_handoff(session, None, None)
    } else {
        stripped
    };
    let id = last.id.clone();
    patch_handoff(
        session,
        &id,
        HandoffMeta {
            status: HandoffStatus::Ready,
            pending: Some(true),
            ..handoff
        },
        Some(brief),
    )
}

/// `userMessagesAfterHandoff`: user messages already on the transcript after
/// the last handoff divider.
pub fn user_messages_after_handoff(session: &Session) -> Vec<String> {
    let Some(last) = last_handoff_block(&session.blocks) else {
        return Vec::new();
    };
    let Some(start) = session.blocks.iter().position(|block| block.id == last.id) else {
        return Vec::new();
    };
    session.blocks[start + 1..]
        .iter()
        .filter(|block| block.role == BlockRole::User)
        .map(|block| {
            let text = block
                .ci_context
                .as_deref()
                .filter(|context| !context.is_empty())
                .unwrap_or(&block.text);
            js::trim(text).to_string()
        })
        .filter(|text| !text.is_empty())
        .collect()
}

/// `consumeHandoff`: the incoming provider took the recap.
pub fn consume_handoff(session: &Session) -> Session {
    let Some(last) = last_handoff_block(&session.blocks) else {
        return session.clone();
    };
    let Some(handoff) = last
        .handoff
        .clone()
        .filter(|handoff| handoff.pending == Some(true))
    else {
        return session.clone();
    };
    let id = last.id.clone();
    patch_handoff(
        session,
        &id,
        HandoffMeta {
            pending: Some(false),
            ..handoff
        },
        None,
    )
}

/// `chooseHandoffBrief`: the outgoing agent's recap when it is long enough,
/// otherwise the deterministic one.
pub fn choose_handoff_brief(agent_text: &str, session: &Session, request: Option<&str>) -> String {
    let agent = strip_goal_sections(agent_text);
    let fallback = handoff_parts(session, request, Some(&session.cwd));
    let brief = if js::len(&agent) >= MIN_AGENT_BRIEF {
        agent
    } else {
        strip_goal_sections(&fallback.brief)
    };
    format_handoff_brief(&brief, &fallback.ci_context)
}

/// `hasSessionEdits`.
pub fn has_session_edits(session: &Session) -> bool {
    session.blocks.iter().any(is_edit_block)
}

/// `shouldAskOutgoingAgent`: only a live provider thread that edited files
/// is worth a recap turn.
pub fn should_ask_outgoing_agent(session: &Session) -> bool {
    let live_id = session
        .pending_switch
        .as_ref()
        .and_then(|pending| pending.from_provider_session_id.clone())
        .or_else(|| session.provider_session_id.clone());
    live_id.is_some_and(|id| !id.is_empty()) && has_session_edits(session)
}

/// `buildOutgoingHandoffPrompt`.
pub fn build_outgoing_handoff_prompt(user_request: &str) -> String {
    let trimmed = js::trim(user_request);
    let request = if trimmed.is_empty() {
        "(no text)"
    } else {
        trimmed
    };
    format!(
        "The user is switching to another coding agent. Their new message will be sent separately — do not repeat it, and do not add a Goal heading.

<user_request>
{}
</user_request>

Write a short recap of this conversation so the next agent can continue. Under 120 words. Plain markdown. No title card. No greeting. Do not paste the whole transcript.

Rules:
- Use only this conversation.
- Do not run git, do not inspect the working tree, do not read files, do not call tools.
- Mention files only if this chat edited them.
- If the chat was a greeting or has no task yet, say that in one sentence. Do not invent work from uncommitted repo files.

Include only sections that have session-specific content:
- Session so far (a few bullets, not a transcript)
- Files edited in this session
- Suggested next step",
        limit_section(request, REQUEST_LIMIT)
    )
}

/// `buildDeterministicHandoff`. `cwd` defaults to the session's.
pub fn build_deterministic_handoff(
    session: &Session,
    request: Option<&str>,
    cwd: Option<&str>,
) -> String {
    let parts = handoff_parts(session, request, Some(cwd.unwrap_or(&session.cwd)));
    format_handoff_brief(&parts.brief, &parts.ci_context)
}

struct HandoffParts {
    brief: String,
    ci_context: String,
}

fn handoff_parts(session: &Session, request: Option<&str>, cwd: Option<&str>) -> HandoffParts {
    let current = request.map(js::trim).unwrap_or("");
    let mut users: Vec<&Block> = Vec::new();
    let mut last_assistant = String::new();
    let mut last_tasks = String::new();
    let mut last_plan = String::new();
    let mut files: Vec<(String, String)> = Vec::new();

    for block in &session.blocks {
        match block.role {
            BlockRole::Handoff | BlockRole::Reasoning => continue,
            BlockRole::User => users.push(block),
            BlockRole::Assistant => {
                let text = js::trim(&block.text);
                if !text.is_empty() {
                    last_assistant = text.to_string();
                }
            }
            BlockRole::Plan => {
                let text = js::trim(&block.text);
                if !text.is_empty() {
                    last_plan = text.to_string();
                }
            }
            BlockRole::Tasks => {
                let text = js::trim(&block.text);
                if !text.is_empty() {
                    last_tasks = text.to_string();
                }
            }
            BlockRole::Tool | BlockRole::Approval => {
                if !is_edit_block(block) {
                    continue;
                }
                if let Some(label) = tool_handoff_line(block, cwd) {
                    set_label(&mut files, label);
                }
            }
            _ => {}
        }
    }

    let prior_all: &[&Block] = if !current.is_empty()
        && users
            .last()
            .is_some_and(|last| js::trim(&last.text) == current)
    {
        &users[..users.len() - 1]
    } else {
        &users
    };
    // The new request is sent separately and must not erase the prior turn's CI data.
    let ci_context = prior_all
        .last()
        .and_then(|block| block.ci_context.clone())
        .unwrap_or_default();
    let prior_texts: Vec<&str> = prior_all
        .iter()
        .map(|block| js::trim(&block.text))
        .filter(|text| !text.is_empty())
        .collect();
    let omitted = prior_texts.len().saturating_sub(MAX_PRIOR_USERS);
    let prior = &prior_texts[omitted..];

    let mut sections: Vec<String> = Vec::new();
    if omitted > 0 || !prior.is_empty() || !last_assistant.is_empty() {
        let mut lines: Vec<String> = Vec::new();
        if omitted > 0 {
            lines.push(format!("({omitted} earlier messages omitted)"));
        }
        for text in prior {
            lines.push(format!(
                "User: {}",
                one_line(&limit_section(text, USER_LINE_LIMIT))
            ));
        }
        if !last_assistant.is_empty() {
            lines.push(format!(
                "Assistant: {}",
                one_line(&limit_section(&last_assistant, ASSISTANT_LIMIT))
            ));
        }
        lines.retain(|line| !line.is_empty());
        if !lines.is_empty() {
            sections.push(format!("## Session so far\n{}", lines.join("\n")));
        }
    }
    if !files.is_empty() {
        let list: Vec<String> = files
            .iter()
            .take(40)
            .map(|(_, line)| format!("- {line}"))
            .collect();
        sections.push(format!(
            "## Files edited in this session\n{}",
            list.join("\n")
        ));
    }
    if !last_plan.is_empty() {
        sections.push(format!(
            "## Plan\n{}",
            limit_section(&last_plan, PLAN_LIMIT)
        ));
    }
    if !last_tasks.is_empty() {
        sections.push(format!(
            "## Current tasks\n{}",
            limit_section(&last_tasks, PLAN_LIMIT)
        ));
    }

    HandoffParts {
        brief: js::trim(&sections.join("\n\n")).to_string(),
        ci_context,
    }
}

fn format_handoff_brief(brief: &str, ci_context: &str) -> String {
    let recap = limit_section(
        brief,
        if ci_context.is_empty() {
            BRIEF_LIMIT
        } else {
            BRIEF_LIMIT - 400
        },
    );
    if ci_context.is_empty() {
        return recap;
    }
    let header = format!(
        "{}## CI context\n",
        if recap.is_empty() { "" } else { "\n\n" }
    );
    // The budget is soft: only evidence may be cut, never CI instructions or labels.
    let budget = (BRIEF_LIMIT as i64 - js::len(&recap) as i64 - js::len(&header) as i64).max(0);
    format!(
        "{recap}{header}{}",
        compact_ci_repair_context(ci_context, budget)
    )
}

/// `wrapHandoffPrompt`: the incoming provider's first prompt, with the user's
/// request first and the recap after it.
pub fn wrap_handoff_prompt(
    brief: &str,
    from: HarnessId,
    user_text: &str,
    earlier_requests: &[String],
) -> String {
    let from_title = from.title();
    let request = js::trim(user_text);
    let body = strip_goal_sections(brief);
    let earlier: Vec<&str> = earlier_requests
        .iter()
        .map(|text| js::trim(text))
        .filter(|text| !text.is_empty())
        .collect();
    let earlier_block = if earlier.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nAfter the switch, before this message, the user also sent:\n\n{}",
            earlier.join("\n\n")
        )
    };
    let lead = format!(
        "You are continuing an existing conversation handed off from {from_title}. This is not a new session. Do not say you have no prior context.\n\n{request}{earlier_block}"
    );
    if body.is_empty() {
        return format!(
            "{lead}\n\nContinue from a {from_title} session. Do not invent prior work."
        );
    }
    format!(
        "{lead}\n\nPrior conversation from {from_title} — this is the thread you are joining, not optional background:\n\n<handoff>\n{body}\n</handoff>"
    )
}

fn append_handoff_block(
    session: &Session,
    from: HarnessId,
    to: HarnessId,
    status: HandoffStatus,
    text: &str,
    pending: bool,
) -> Session {
    let mut next = session.clone();
    next.blocks.push(Block {
        handoff: Some(HandoffMeta {
            from,
            to,
            status,
            pending: pending.then_some(true),
            extra: Extra::new(),
        }),
        ..Block::new(uuid::Uuid::new_v4().to_string(), BlockRole::Handoff, text)
    });
    next
}

fn patch_handoff(
    session: &Session,
    id: &str,
    handoff: HandoffMeta,
    text: Option<String>,
) -> Session {
    let mut next = session.clone();
    for block in &mut next.blocks {
        if block.id != id {
            continue;
        }
        if let Some(text) = &text {
            block.text = text.clone();
        }
        block.handoff = Some(handoff.clone());
    }
    next
}

fn tool_handoff_line(block: &Block, cwd: Option<&str>) -> Option<String> {
    let preview = block.tool.as_ref().and_then(|tool| tool.preview.as_ref());
    let path = match preview {
        Some(preview) if preview.path.as_deref().is_some_and(|path| !path.is_empty()) => Some(
            display_path(preview.path.as_deref().unwrap_or_default(), cwd),
        ),
        Some(preview) => preview.file_name.clone(),
        None => None,
    }
    .filter(|path| !path.is_empty());
    let title = js::trim(tool_title(block).unwrap_or("")).to_string();
    match path {
        Some(path) if !title.is_empty() => {
            if title.to_lowercase().contains(&path.to_lowercase()) {
                Some(title)
            } else {
                Some(format!("{title} ({path})"))
            }
        }
        Some(path) => Some(path),
        None => (!title.is_empty()).then_some(title),
    }
}

fn one_line(text: &str) -> String {
    js::trim(&collapse_space(text)).to_string()
}

fn is_heading_start(text: &str) -> bool {
    let hashes = text.bytes().take_while(|b| *b == b'#').count();
    (1..=6).contains(&hashes) && text[hashes..].chars().next().is_some_and(js::is_space)
}

/// `/^#{1,6}\s*Goal\b/i`.
fn is_goal_heading(text: &str) -> bool {
    let hashes = text.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return false;
    }
    let rest = text[hashes..].trim_start_matches(js::is_space);
    let Some(word) = rest.get(..4) else {
        return false;
    };
    word.eq_ignore_ascii_case("goal")
        && rest[4..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

/// `stripGoalSections`: drop a Goal heading so the new message is not
/// duplicated in the recap.
pub fn strip_goal_sections(markdown: &str) -> String {
    let trimmed = js::trim(markdown);
    if trimmed.is_empty() {
        return String::new();
    }
    // `split(/(?=^#{1,6}\s)/m)`: cut before every heading line.
    let mut chunks: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut line_start = true;
    for (index, c) in trimmed.char_indices() {
        if line_start && index > 0 && is_heading_start(&trimmed[index..]) {
            chunks.push(&trimmed[start..index]);
            start = index;
        }
        line_start = js::is_line_terminator(c);
    }
    chunks.push(&trimmed[start..]);
    let kept: String = chunks
        .into_iter()
        .filter(|chunk| !is_goal_heading(chunk.trim_start_matches(js::is_space)))
        .collect();
    let without_heading = js::trim(&kept).to_string();
    js::trim(&remove_goal_line(&without_heading)).to_string()
}

/// `.replace(/^Goal:\s.*(?:\n|$)/im, "")`: the first `Goal:` line.
fn remove_goal_line(text: &str) -> String {
    let mut line_start = true;
    for (index, c) in text.char_indices() {
        if line_start
            && text[index..]
                .get(..5)
                .is_some_and(|head| head.eq_ignore_ascii_case("goal:"))
            && let Some(space) = text[index + 5..]
                .chars()
                .next()
                .filter(|c| js::is_space(*c))
        {
            let mut end = index + 5 + space.len_utf8();
            while let Some(next) = text[end..].chars().next() {
                if js::is_line_terminator(next) {
                    break;
                }
                end += next.len_utf8();
            }
            if text[end..].starts_with('\n') {
                end += 1;
            }
            return format!("{}{}", &text[..index], &text[end..]);
        }
        line_start = js::is_line_terminator(c);
    }
    text.to_string()
}

#[cfg(test)]
mod tests;
