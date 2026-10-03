//! Port of src/features/sessions/model/transcriptActivity.ts: how a turn's
//! blocks group into prose, activity, and subagent rows, how activity splits
//! into phases, what each phase is called, and which work folds away.
//!
//! Blocks are shared as [`BlockRef`] so groups and phases hold cheap clones
//! of the session's blocks instead of copies.

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use crate::block::{ApprovalDecided, BlockNotice, ToolPreview, ToolPreviewKind};
use crate::js;
use crate::models::ModelCatalog;
use crate::paths::{display_path, path_key};
use crate::reducer::{
    ToolTitleInput, compose_tool_title, is_agent_tool, is_edit_tool, is_execute_tool, is_read_tool,
    is_search_tool, is_weak_tool_title,
};
use crate::{Block, BlockRole};
use regex::Regex;

use super::INTERRUPT_MESSAGE;
use super::monocode_call::monocode_work_summary;
use super::paths::{leaf_name, resolve_workspace_path};

/// A transcript block shared between the session snapshot and the groups
/// built from it.
pub type BlockRef = Arc<Block>;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("transcript activity pattern")
}

/// `ToolCallState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCallState {
    Pending,
    Accepted,
    Rejected,
}

/// `TurnItem`: one row of a turn before folding.
#[derive(Debug, Clone, PartialEq)]
pub enum TurnItem {
    Block(BlockRef),
    Activity(Vec<BlockRef>),
    /// Delegated runs spawned together, kept out of the folding work trail.
    Subagents(Vec<BlockRef>),
}

impl TurnItem {
    /// `turnItemKey`: the item's identity, stable as the group it names grows.
    pub fn key(&self) -> &str {
        match self {
            TurnItem::Block(block) => &block.id,
            TurnItem::Activity(blocks) | TurnItem::Subagents(blocks) => {
                blocks.first().map(|block| block.id.as_str()).unwrap_or("")
            }
        }
    }

    /// Every block in the item.
    pub fn blocks(&self) -> &[BlockRef] {
        match self {
            TurnItem::Block(block) => std::slice::from_ref(block),
            TurnItem::Activity(blocks) | TurnItem::Subagents(blocks) => blocks,
        }
    }

    pub fn is_activity(&self) -> bool {
        matches!(self, TurnItem::Activity(_))
    }

    pub fn is_subagents(&self) -> bool {
        matches!(self, TurnItem::Subagents(_))
    }

    /// The block, when the item is a single block.
    pub fn as_block(&self) -> Option<&BlockRef> {
        match self {
            TurnItem::Block(block) => Some(block),
            _ => None,
        }
    }
}

/// `needsApproval`.
pub fn needs_approval(block: &Block) -> bool {
    block
        .approval
        .as_ref()
        .is_some_and(|approval| approval.decided.is_none())
}

/// `isFailedStatus`: statuses a provider uses for a call that did not work.
pub fn is_failed_status(status: Option<&str>) -> bool {
    let value = status.unwrap_or("").to_lowercase();
    matches!(
        value.as_str(),
        "failed" | "error" | "cancelled" | "canceled"
    )
}

fn tool_status(block: &Block) -> String {
    block
        .tool
        .as_ref()
        .and_then(|tool| tool.status.as_deref())
        .unwrap_or("")
        .to_lowercase()
}

/// `toolCallState`.
pub fn tool_call_state(block: &Block) -> ToolCallState {
    let status = tool_status(block);
    let decided = block
        .approval
        .as_ref()
        .and_then(|approval| approval.decided);

    if decided == Some(ApprovalDecided::Deny) {
        return ToolCallState::Rejected;
    }
    if is_failed_status(Some(&status)) {
        return ToolCallState::Rejected;
    }
    if needs_approval(block) {
        return ToolCallState::Pending;
    }
    if status == "completed" || status == "success" {
        return ToolCallState::Accepted;
    }
    if block.is_streaming() || matches!(status.as_str(), "in_progress" | "pending" | "running") {
        return ToolCallState::Pending;
    }
    if matches!(
        decided,
        Some(ApprovalDecided::Allow) | Some(ApprovalDecided::Cancelled)
    ) || status.is_empty()
    {
        return ToolCallState::Accepted;
    }
    ToolCallState::Pending
}

/// `block.text || block.tool?.title`.
pub fn tool_title(block: &Block) -> Option<&str> {
    if !block.text.is_empty() {
        Some(&block.text)
    } else {
        block.tool.as_ref().and_then(|tool| tool.title.as_deref())
    }
}

fn tool_kind(block: &Block) -> Option<&str> {
    block.tool.as_ref().and_then(|tool| tool.kind.as_deref())
}

fn tool_preview(block: &Block) -> Option<&ToolPreview> {
    block.tool.as_ref().and_then(|tool| tool.preview.as_ref())
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

/// `toolCallLabel`: the row's one-line label, such as `Read src/app.ts`.
pub fn tool_call_label(block: &Block, cwd: Option<&str>) -> String {
    let preview = tool_preview(block);
    let path = match preview.and_then(|preview| nonempty(preview.path.as_deref())) {
        Some(path) => Some(display_path(path, cwd)),
        None => preview.and_then(|preview| preview.file_name.clone()),
    };
    let title = compose_tool_title(&ToolTitleInput {
        kind: tool_kind(block),
        title: tool_title(block),
        path: path.as_deref(),
        query: preview.and_then(|preview| preview.query.as_deref()),
        preview_kind: preview.map(|preview| preview.kind),
        cwd,
        ..Default::default()
    });
    if title.is_empty() {
        "Working".into()
    } else {
        title
    }
}

/// `isIncompleteTool`: a pending call with nothing to show yet.
pub fn is_incomplete_tool(block: &Block, label: &str, state: ToolCallState) -> bool {
    if state != ToolCallState::Pending {
        return false;
    }
    if let Some(kind) = tool_kind(block).map(str::to_lowercase)
        && !kind.is_empty()
        && kind != "other"
    {
        return false;
    }
    if let Some(preview) = tool_preview(block)
        && (nonempty(preview.path.as_deref()).is_some()
            || nonempty(preview.query.as_deref()).is_some()
            || preview
                .lines
                .as_ref()
                .is_some_and(|lines| !lines.is_empty()))
    {
        return false;
    }
    label.is_empty() || is_weak_tool_title(label)
}

/// `isHiddenTool`.
pub fn is_hidden_tool(block: &Block) -> bool {
    if block.role != BlockRole::Tool && block.role != BlockRole::Approval {
        return false;
    }
    // Harnesses also publish todo mutations as ordinary tool calls. The
    // canonical tasks block is the user-facing representation, so keep the
    // provider-internal call out of the activity stack.
    if tool_kind(block).is_some_and(|kind| kind.to_lowercase() == "tasks") {
        return true;
    }
    if is_edit_tool(tool_kind(block), tool_title(block), tool_preview(block)) {
        return false;
    }
    let state = tool_call_state(block);
    is_incomplete_tool(block, &tool_call_label(block, None), state)
}

/// `isNoticeBlock`: a system row the reader must not miss, an error or the
/// note that a quit cut the turn short. Sessions persisted before the
/// `notice` tag still carry the interrupt's literal text.
pub fn is_notice_block(block: &Block) -> bool {
    block.role == BlockRole::System && (block.notice.is_some() || block.text == INTERRUPT_MESSAGE)
}

/// `isStatusStep`: turn chrome the trail absorbed, a status ping.
fn is_status_step(block: &Block) -> bool {
    block.role == BlockRole::System && block.interjection.is_none()
}

/// `isActivityBlock`: foldable work, which is tool calls, thinking, edits,
/// and status rows. An edit awaiting approval stays out, as do notices and
/// interjections (the caller folds interjections in once a turn settles).
pub fn is_activity_block(block: &Block) -> bool {
    if is_thinking_block(block) {
        return true;
    }
    if block.role == BlockRole::System {
        return block.interjection.is_none() && !is_notice_block(block);
    }
    if block.role != BlockRole::Tool && block.role != BlockRole::Approval {
        return false;
    }
    if is_edit_tool(tool_kind(block), tool_title(block), tool_preview(block))
        && needs_approval(block)
    {
        return false;
    }
    !is_hidden_tool(block)
}

/// `isThinkingBlock`: reasoning the agent streams while it works.
pub fn is_thinking_block(block: &Block) -> bool {
    block.role == BlockRole::Reasoning && !js::trim(&block.text).is_empty()
}

/// `isToolBlock`.
pub fn is_tool_block(block: &Block) -> bool {
    block.role == BlockRole::Tool || block.role == BlockRole::Approval
}

/// `isProseBlock`: assistant prose with something in it.
pub fn is_prose_block(block: &Block) -> bool {
    block.role == BlockRole::Assistant && !js::trim(&block.text).is_empty()
}

static FENCE: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)```.*?(?:```|\z)"));
static PARAGRAPH_BREAK: LazyLock<Regex> = LazyLock::new(|| re(r"\n\s*\n"));
static LINE_MARKER: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?m)^\s{0,3}(?:#{1,6}|>|[-*+]|\d+\.)\s+"));
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| re(r"`([^`]*)`"));
static LINK: LazyLock<Regex> = LazyLock::new(|| re(r"!?\[([^\]]*)\]\([^)]*\)"));
static STRONG: LazyLock<Regex> = LazyLock::new(|| re(r"\*\*(.+?)\*\*|__(.+?)__"));
static EMPHASIS: LazyLock<Regex> = LazyLock::new(|| re(r"\*(.+?)\*|_(.+?)_"));
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));

/// Either alternative's capture, the `$2` of `(\*\*|__)(.+?)\1`.
fn either_capture(caps: &regex::Captures<'_>) -> String {
    caps.get(1)
        .or_else(|| caps.get(2))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// `proseSummary`: the first paragraph of a prose block as one plain line.
pub fn prose_summary(text: &str) -> String {
    let body = FENCE.replace_all(text, " ");
    let paragraph = PARAGRAPH_BREAK
        .split(&body)
        .map(js::trim)
        .find(|part| !part.is_empty())
        .unwrap_or("");
    let value = LINE_MARKER.replace_all(paragraph, "");
    let value = INLINE_CODE.replace_all(&value, "$1");
    let value = LINK.replace_all(&value, "$1");
    let value = STRONG.replace_all(&value, either_capture);
    let value = EMPHASIS.replace_all(&value, either_capture);
    let value = WHITESPACE.replace_all(&value, " ");
    js::trim(&value).to_string()
}

/// `editVerb`: the canonical verb for a write-preview row.
pub fn edit_verb(label: &str) -> &'static str {
    let word = js::trim(label)
        .split(|c: char| js::is_space(c) || js::is_line_terminator(c))
        .next()
        .unwrap_or("")
        .to_lowercase();
    match word.as_str() {
        "delete" | "deleted" | "remove" | "removed" => "Delete",
        "move" | "moved" | "rename" | "renamed" => "Move",
        "create" | "created" | "add" | "added" | "new" => "Create",
        "write" | "wrote" | "writing" => "Write",
        _ => "Edit",
    }
}

/// `ToolCallDisplay`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallDisplay {
    pub action: Option<String>,
    pub target: Option<String>,
    pub file_name: String,
    pub file_path: Option<String>,
    pub is_file: bool,
    /// False when a write preview's own path resolves to a different file
    /// than `file_path`. Such a row falls back to the plain file control.
    pub preview_matches_file: bool,
}

static LABEL_PARTS: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(Read|Find|Skill|List|Edit|Write)\s+(.+)$"));

/// `resolveToolCallDisplay`: split a row's label into the action and target
/// shown on screen, and resolve the file a click opens. The opened path comes
/// from `target`, what the user reads, never from `preview.path` alone.
pub fn resolve_tool_call_display(
    label: &str,
    preview: Option<&ToolPreview>,
    cwd: Option<&str>,
) -> ToolCallDisplay {
    let parts = LABEL_PARTS.captures(label);
    let label_verb = parts
        .as_ref()
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str());
    let label_target = parts
        .as_ref()
        .and_then(|caps| caps.get(2))
        .map(|m| m.as_str());
    let preview_path = preview.and_then(|preview| nonempty(preview.path.as_deref()));
    let preview_file_name = preview.and_then(|preview| preview.file_name.as_deref());
    let preview_path_or_name = || -> Option<String> {
        match preview_path {
            Some(path) => Some(display_path(path, cwd)),
            None => preview_file_name.map(str::to_string),
        }
    };
    // A write preview carries the path itself, so edits get the same verb
    // and file chip as reads.
    let write_target = if preview.is_some_and(|preview| preview.kind == ToolPreviewKind::Write) {
        preview_path_or_name()
    } else {
        None
    };
    let is_file_verb =
        |verb: Option<&str>| matches!(verb, Some("Read" | "List" | "Edit" | "Write"));
    // A file verb's target is only trustworthy as a path when it looks like
    // one. Harnesses sometimes phrase these in plain English.
    let trusted_label_target = label_target
        .filter(|target| !is_file_verb(label_verb) || resolve_workspace_path(target, cwd).is_some())
        .map(str::to_string);
    let trimmed = js::trim(label);
    let has_path_or_name = preview_path.is_some() || nonempty(preview_file_name).is_some();
    let action: Option<String> = label_verb
        .map(str::to_string)
        .or_else(|| write_target.as_ref().map(|_| edit_verb(label).to_string()))
        .or_else(|| {
            if trimmed.eq_ignore_ascii_case("read") && has_path_or_name {
                Some("Read".into())
            } else if trimmed.eq_ignore_ascii_case("find")
                && preview.is_some_and(|preview| nonempty(preview.query.as_deref()).is_some())
            {
                Some("Find".into())
            } else if trimmed.eq_ignore_ascii_case("list") && has_path_or_name {
                Some("List".into())
            } else if trimmed.eq_ignore_ascii_case("skill") {
                Some("Skill".into())
            } else {
                None
            }
        });
    let target = trusted_label_target
        .or_else(|| write_target.clone())
        .or_else(|| match action.as_deref() {
            Some("Read" | "List" | "Edit" | "Write") => preview_path_or_name(),
            Some("Find") => preview.and_then(|preview| preview.query.clone()),
            _ => None,
        });
    let (Some(action), Some(target)) = (action, target.filter(|target| !target.is_empty())) else {
        return ToolCallDisplay {
            action: None,
            target: None,
            file_name: "file".into(),
            file_path: None,
            is_file: false,
            preview_matches_file: true,
        };
    };
    let is_file = action != "Find" && action != "Skill";
    let file_name = nonempty(preview_file_name)
        .map(str::to_string)
        .or_else(|| {
            target
                .trim_end_matches(['/', '\\'])
                .split(['/', '\\'])
                .rfind(|part| !part.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "file".into());
    // Resolve from `target`, not `preview.path`, so the file that opens always
    // matches the path the row displays.
    let file_path = resolve_workspace_path(&target, cwd);
    // A write preview's own path can still disagree with `target`. An
    // unresolved one is unknown, not a match.
    let has_write_preview_path = preview
        .is_some_and(|preview| preview.kind == ToolPreviewKind::Write)
        && preview_path.is_some();
    let preview_matches_file = if !has_write_preview_path {
        true
    } else {
        let resolved =
            preview_path.and_then(|path| resolve_workspace_path(&display_path(path, cwd), cwd));
        match (resolved, &file_path) {
            (Some(preview), Some(file)) => path_key(&preview) == path_key(file),
            _ => false,
        }
    };
    ToolCallDisplay {
        action: Some(action),
        target: Some(target),
        file_name,
        file_path,
        is_file,
        preview_matches_file,
    }
}

/// `groupTurns`: user turns, with handoff dividers on their own row.
/// `managed` is a worker's own transcript, where the app-written turns are
/// the orchestrator talking to it.
pub fn group_turns(blocks: &[BlockRef], managed: bool) -> Vec<Vec<BlockRef>> {
    let mut turns = Vec::new();
    let mut current: Vec<BlockRef> = Vec::new();
    for block in blocks {
        // A turn the app wrote to keep an orchestration moving is not a user
        // message. Dropping it folds the reply into the turn above.
        if block.is_internal() && !managed {
            continue;
        }
        if block.role == BlockRole::Handoff {
            if !current.is_empty() {
                turns.push(std::mem::take(&mut current));
            }
            turns.push(vec![block.clone()]);
            continue;
        }
        if block.role == BlockRole::User && !current.is_empty() {
            turns.push(std::mem::take(&mut current));
        }
        current.push(block.clone());
    }
    if !current.is_empty() {
        turns.push(current);
    }
    turns
}

/// `groupTurnItems`: fold contiguous runs of tool calls and reasoning into
/// activity groups. Assistant prose always stands on its own. A settled turn
/// also folds interjections and delegated runs into the trail, except a run
/// that failed, which keeps its own row.
pub fn group_turn_items(blocks: &[BlockRef], settled: bool) -> Vec<TurnItem> {
    let visible = without_superseded_initial_thinking(
        blocks
            .iter()
            .filter(|block| !is_ignored_turn_block(block) && !is_hidden_tool(block))
            .cloned()
            .collect(),
    );
    let mut items: Vec<TurnItem> = Vec::new();
    let mut activity: Vec<BlockRef> = Vec::new();
    let flush = |items: &mut Vec<TurnItem>, activity: &mut Vec<BlockRef>| {
        if !activity.is_empty() {
            items.push(TurnItem::Activity(std::mem::take(activity)));
        }
    };
    for block in visible {
        // Delegated runs keep their own rows while the turn is live. Once it
        // settles they are work like any other call, except one that died.
        if is_subagent_block(&block)
            && (!settled || tool_call_state(&block) == ToolCallState::Rejected)
        {
            flush(&mut items, &mut activity);
            if let Some(TurnItem::Subagents(stack)) = items.last_mut() {
                stack.push(block);
            } else {
                items.push(TurnItem::Subagents(vec![block]));
            }
            continue;
        }
        if is_activity_block(&block) || (settled && block.interjection.is_some()) {
            activity.push(block);
            continue;
        }
        flush(&mut items, &mut activity);
        items.push(TurnItem::Block(block));
    }
    flush(&mut items, &mut activity);
    items
}

/// `withoutSupersededInitialThinking`: reasoning published before the first
/// assistant text is dropped once that text arrives, unless a tool came first.
fn without_superseded_initial_thinking(blocks: Vec<BlockRef>) -> Vec<BlockRef> {
    let mut start = 0;
    while start < blocks.len() && matches!(blocks[start].role, BlockRole::User | BlockRole::System)
    {
        start += 1;
    }
    let mut end = start;
    while end < blocks.len() && is_thinking_block(&blocks[end]) {
        end += 1;
    }
    if end == start {
        return blocks;
    }
    let following = &blocks[end..];
    let Some(prose_index) = following.iter().position(|block| is_prose_block(block)) else {
        return blocks;
    };
    if let Some(tool_index) = following.iter().position(|block| is_tool_block(block))
        && tool_index < prose_index
    {
        return blocks;
    }
    let mut out = blocks[..start].to_vec();
    out.extend_from_slice(&blocks[end..]);
    out
}

/// `initialThinkingIndex`: the leading reasoning-only activity shown before
/// the first response arrives.
pub fn initial_thinking_index(items: &[TurnItem]) -> Option<usize> {
    for (index, item) in items.iter().enumerate() {
        if let TurnItem::Block(block) = item
            && matches!(block.role, BlockRole::User | BlockRole::System)
        {
            continue;
        }
        if let TurnItem::Activity(blocks) = item
            && !blocks.is_empty()
            && blocks.iter().all(|block| is_thinking_block(block))
        {
            return Some(index);
        }
        return None;
    }
    None
}

/// `isIgnoredTurnBlock`: empty reasoning and empty assistant placeholders.
fn is_ignored_turn_block(block: &Block) -> bool {
    match block.role {
        BlockRole::Reasoning | BlockRole::Assistant => js::trim(&block.text).is_empty(),
        _ => false,
    }
}

/// `turnCopyText`: what the user reads, which is assistant prose, tasks, and
/// plans.
pub fn turn_copy_text(blocks: &[BlockRef]) -> String {
    blocks
        .iter()
        .filter(|block| {
            matches!(
                block.role,
                BlockRole::Assistant | BlockRole::Tasks | BlockRole::Plan
            )
        })
        .map(|block| {
            let normalized = block.text.replace("\r\n", "\n").replace('\r', "\n");
            js::trim(&normalized).to_string()
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `lastActivityIndex`: the activity group a settled turn hangs its "Worked
/// for" line on.
pub fn last_activity_index(items: &[TurnItem]) -> Option<usize> {
    items.iter().rposition(TurnItem::is_activity)
}

/// `activityStillRunning`: a tool is still running or waiting on the user.
pub fn activity_still_running(blocks: &[BlockRef]) -> bool {
    blocks.iter().any(|block| {
        (is_tool_block(block)
            && !is_hidden_tool(block)
            && tool_call_state(block) == ToolCallState::Pending)
            || needs_approval(block)
    })
}

/// `hasRunningSubagent`.
pub fn has_running_subagent(blocks: &[BlockRef]) -> bool {
    blocks
        .iter()
        .any(|block| is_subagent_block(block) && tool_call_state(block) == ToolCallState::Pending)
}

/// `isSubagentBlock`: the call that spawned a subagent. One still waiting on
/// approval stays in the work trail, where its controls are.
pub fn is_subagent_block(block: &Block) -> bool {
    is_tool_block(block)
        && !needs_approval(block)
        && is_agent_tool(tool_kind(block), tool_title(block))
}

static AGENT_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^(?:agent|task|subagent)\b[\s:·-]*"));

/// `subagentBrief`: the whole brief a run was spawned with.
pub fn subagent_brief(block: &Block) -> String {
    let named = block
        .agent_run
        .as_ref()
        .map(|run| js::trim(&run.name))
        .filter(|name| !name.is_empty());
    let name = match named {
        Some(name) => name.to_string(),
        None => js::trim(tool_title(block).unwrap_or("")).to_string(),
    };
    let stripped = AGENT_PREFIX.replace(&name, "");
    let stripped = js::trim(&stripped);
    if stripped.is_empty() {
        "Subagent".into()
    } else {
        stripped.to_string()
    }
}

/// `MAX_SUBAGENT_NAME`: past this a name starts being the brief again.
const MAX_SUBAGENT_NAME: usize = 56;

static SENTENCE: LazyLock<Regex> = LazyLock::new(|| re(r"^[^.!?]*[.!?]?"));
static TRAILING_PUNCTUATION: LazyLock<Regex> = LazyLock::new(|| re(r"[\s,;:]+$"));

/// `subagentName`: the brief's first sentence, capped on a word.
pub fn subagent_name(block: &Block) -> String {
    let brief = subagent_brief(block);
    let sentence = SENTENCE
        .find(&brief)
        .map(|m| js::trim(m.as_str()).to_string())
        .unwrap_or_else(|| brief.clone());
    let name = if sentence.is_empty() { brief } else { sentence };
    if js::len(&name) <= MAX_SUBAGENT_NAME {
        return name;
    }
    let cut = js::slice_prefix(&name, MAX_SUBAGENT_NAME);
    let trimmed = match cut.rfind(' ') {
        Some(space) if js::len(&cut[..space]) > MAX_SUBAGENT_NAME / 2 => &cut[..space],
        _ => cut,
    };
    format!("{}\u{2026}", TRAILING_PUNCTUATION.replace(trimmed, ""))
}

static UNSPECIFIED_MODEL: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^(?:auto|default|inherit|unspecified)$"));

/// `subagentModelName`: the child's model as a display name, or `None` when
/// the provider did not say.
pub fn subagent_model_name(block: &Block, catalog: &ModelCatalog) -> Option<String> {
    let id = block
        .agent_run
        .as_ref()
        .and_then(|run| run.model.as_deref())
        .map(js::trim)
        .filter(|id| !id.is_empty())?;
    if UNSPECIFIED_MODEL.is_match(id) {
        return None;
    }
    Some(
        catalog
            .all_models()
            .find(|model| model.id == id || model.native_id.as_deref() == Some(id))
            .map(|model| model.name.clone())
            .unwrap_or_else(|| id.to_string()),
    )
}

/// `subagentReport`: what a finished run handed back, its report or the
/// reason it died.
pub fn subagent_report(block: &Block) -> Option<String> {
    if tool_call_state(block) == ToolCallState::Pending {
        return None;
    }
    block
        .tool
        .as_ref()
        .and_then(|tool| tool.detail.as_deref())
        .map(js::trim)
        .filter(|detail| !detail.is_empty())
        .map(str::to_string)
}

/// `subagentFailureSummary`: a failed delegated call stays visible even when
/// the work trail folds.
pub fn subagent_failure_summary(blocks: &[BlockRef]) -> Option<String> {
    let failed = blocks
        .iter()
        .filter(|block| {
            is_tool_block(block)
                && is_agent_tool(tool_kind(block), tool_title(block))
                && tool_call_state(block) == ToolCallState::Rejected
        })
        .count();
    match failed {
        0 => None,
        1 => Some("Subagent failed".into()),
        n => Some(format!("{n} subagents failed")),
    }
}

/// `ActivityWorkKind`: what a run of tool calls was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityWorkKind {
    Research,
    Edit,
    Run,
    Agent,
    Other,
}

/// `ActivityPhaseKind`: a work kind, or a group the agent only narrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityPhaseKind {
    Research,
    Edit,
    Run,
    Agent,
    Other,
    Think,
    Note,
}

impl From<ActivityWorkKind> for ActivityPhaseKind {
    fn from(kind: ActivityWorkKind) -> Self {
        match kind {
            ActivityWorkKind::Research => Self::Research,
            ActivityWorkKind::Edit => Self::Edit,
            ActivityWorkKind::Run => Self::Run,
            ActivityWorkKind::Agent => Self::Agent,
            ActivityWorkKind::Other => Self::Other,
        }
    }
}

/// `ActivityPhase`: the line the agent wrote before it started, and the
/// calls that line introduced.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityPhase {
    pub id: String,
    pub kind: ActivityPhaseKind,
    /// The agent's own words for this run, when it wrote some.
    pub headline: Option<BlockRef>,
    pub steps: Vec<BlockRef>,
}

/// `WORK_KIND_ORDER`: ties break towards the kind that changed the most.
const WORK_KIND_ORDER: [ActivityWorkKind; 5] = [
    ActivityWorkKind::Edit,
    ActivityWorkKind::Run,
    ActivityWorkKind::Agent,
    ActivityWorkKind::Research,
    ActivityWorkKind::Other,
];

fn kind_slot(kind: ActivityWorkKind) -> usize {
    match kind {
        ActivityWorkKind::Edit => 0,
        ActivityWorkKind::Run => 1,
        ActivityWorkKind::Agent => 2,
        ActivityWorkKind::Research => 3,
        ActivityWorkKind::Other => 4,
    }
}

static EDIT_LABEL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:Edit|Write)\s+\S"));
static RESEARCH_LABEL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:Read|List|Find)\b"));

#[cfg(test)]
thread_local! {
    /// How many times `tool_category` ran, so a test can check that phase
    /// building stays linear.
    static CATEGORY_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `toolCategory`.
pub fn tool_category(block: &Block) -> ActivityWorkKind {
    #[cfg(test)]
    CATEGORY_CALLS.with(|calls| calls.set(calls.get() + 1));
    let kind = tool_kind(block);
    let title = tool_title(block);
    let preview = tool_preview(block);
    if is_agent_tool(kind, title) {
        return ActivityWorkKind::Agent;
    }
    if is_edit_tool(kind, title, preview) {
        return ActivityWorkKind::Edit;
    }
    if is_search_tool(kind, title, preview) || is_read_tool(kind, title, preview) {
        return ActivityWorkKind::Research;
    }
    let label = tool_call_label(block, None);
    if EDIT_LABEL.is_match(&label) {
        return ActivityWorkKind::Edit;
    }
    if RESEARCH_LABEL.is_match(&label) {
        return ActivityWorkKind::Research;
    }
    if is_execute_tool(kind, title) {
        return ActivityWorkKind::Run;
    }
    ActivityWorkKind::Other
}

/// `buildActivityPhases`: only the agent saying what it is about to do starts
/// a new group, so a run of mixed work reads as one thing done.
pub fn build_activity_phases(blocks: &[BlockRef]) -> Vec<ActivityPhase> {
    let mut phases: Vec<ActivityPhase> = Vec::new();
    let mut counts = [0usize; 5];

    fn open(
        phases: &mut Vec<ActivityPhase>,
        counts: &mut [usize; 5],
        kind: ActivityPhaseKind,
        headline: Option<BlockRef>,
    ) {
        *counts = [0; 5];
        phases.push(ActivityPhase {
            id: headline
                .as_ref()
                .map(|block| block.id.clone())
                .unwrap_or_default(),
            kind,
            headline,
            steps: Vec::new(),
        });
    }

    for block in blocks {
        // Reasoning is a step, never a header.
        if is_thinking_block(block) {
            if phases.is_empty() {
                open(&mut phases, &mut counts, ActivityPhaseKind::Think, None);
            }
            let current = phases.last_mut().expect("open phase");
            current.steps.push(block.clone());
            if current.id.is_empty() {
                current.id = block.id.clone();
            }
            continue;
        }
        if is_prose_block(block) {
            let narrating = phases.last().is_some_and(|phase| {
                matches!(
                    phase.kind,
                    ActivityPhaseKind::Think | ActivityPhaseKind::Note
                )
            });
            // A line after work has started is the title of what comes next.
            if phases.is_empty() || !narrating {
                open(
                    &mut phases,
                    &mut counts,
                    ActivityPhaseKind::Note,
                    Some(block.clone()),
                );
            } else {
                let current = phases.last_mut().expect("open phase");
                if current.headline.is_none() {
                    // A group that opened on a thought takes the agent's words
                    // as its title, keeping the id it already has.
                    current.headline = Some(block.clone());
                    current.kind = ActivityPhaseKind::Note;
                } else {
                    current.steps.push(block.clone());
                }
            }
            continue;
        }
        // Status rows and interjections are steps, never headlines.
        if block.role == BlockRole::System {
            if phases.is_empty() {
                open(&mut phases, &mut counts, ActivityPhaseKind::Note, None);
            }
            let current = phases.last_mut().expect("open phase");
            current.steps.push(block.clone());
            if current.id.is_empty() {
                current.id = block.id.clone();
            }
            continue;
        }
        let category = is_tool_block(block).then(|| tool_category(block));
        if phases.is_empty() {
            let kind = category.unwrap_or_else(|| tool_category(block));
            open(&mut phases, &mut counts, kind.into(), None);
        }
        let current = phases.last_mut().expect("open phase");
        current.steps.push(block.clone());
        // Count each call once, so long runs stay linear.
        if let Some(kind) = category {
            counts[kind_slot(kind)] += 1;
            if let Some(dominant) = dominant_counted_work_kind(&counts) {
                current.kind = dominant.into();
            }
        }
        if current.id.is_empty() {
            current.id = block.id.clone();
        }
    }
    phases
}

fn dominant_work_kind(steps: &[BlockRef]) -> Option<ActivityWorkKind> {
    let mut counts = [0usize; 5];
    for block in steps {
        if is_tool_block(block) {
            counts[kind_slot(tool_category(block))] += 1;
        }
    }
    dominant_counted_work_kind(&counts)
}

fn dominant_counted_work_kind(counts: &[usize; 5]) -> Option<ActivityWorkKind> {
    let mut best: Option<ActivityWorkKind> = None;
    for kind in WORK_KIND_ORDER {
        let count = counts[kind_slot(kind)];
        if count > 0 && best.is_none_or(|best| count > counts[kind_slot(best)]) {
            best = Some(kind);
        }
    }
    best
}

/// An insertion-ordered set of targets, `Set<string>` in TypeScript.
#[derive(Default)]
struct Targets {
    first: Option<String>,
    seen: HashSet<String>,
}

impl Targets {
    fn add(&mut self, target: &str) {
        if self.seen.insert(target.to_string()) && self.first.is_none() {
            self.first = Some(target.to_string());
        }
    }

    fn len(&self) -> usize {
        self.seen.len()
    }
}

#[derive(Default)]
struct PhaseTally {
    /// The kinds of work in the group, in the order the agent first did them.
    order: Vec<ActivityWorkKind>,
    reads: Targets,
    edits: Targets,
    searches: usize,
    runs: usize,
    /// Commands the agent left running when it yielded, and how many still are.
    background: usize,
    background_live: usize,
    agents: usize,
    others: usize,
    /// Interjections the turn absorbed. Status rows count nowhere.
    notes: usize,
}

static LABELLED_TARGET: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)^(?:Read|List|Edit|Write)\s+(.+)$"));
static FIND_LABEL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^Find\b"));

fn tally_steps(steps: &[BlockRef]) -> PhaseTally {
    let mut tally = PhaseTally::default();
    for block in steps {
        if block.interjection.is_some() {
            tally.notes += 1;
            continue;
        }
        if !is_tool_block(block) {
            continue;
        }
        let kind = tool_kind(block);
        let title = tool_title(block);
        let preview = tool_preview(block);
        let label = tool_call_label(block, None);
        let labelled_target = LABELLED_TARGET
            .captures(&label)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string());
        let target = preview
            .and_then(|preview| preview.path.clone())
            .or_else(|| preview.and_then(|preview| preview.file_name.clone()))
            .or(labelled_target)
            .unwrap_or_else(|| block.id.clone());
        let category = tool_category(block);
        if !tally.order.contains(&category) {
            tally.order.push(category);
        }
        match category {
            ActivityWorkKind::Edit => tally.edits.add(&target),
            ActivityWorkKind::Agent => tally.agents += 1,
            ActivityWorkKind::Run => {
                // A background row is the same command again, waited on.
                if block.tool.as_ref().and_then(|tool| tool.background) == Some(true) {
                    tally.background += 1;
                    if tool_call_state(block) == ToolCallState::Pending {
                        tally.background_live += 1;
                    }
                } else {
                    tally.runs += 1;
                }
            }
            ActivityWorkKind::Research => {
                if FIND_LABEL.is_match(&label) || is_search_tool(kind, title, preview) {
                    tally.searches += 1;
                } else {
                    tally.reads.add(&target);
                }
            }
            ActivityWorkKind::Other => tally.others += 1,
        }
    }
    tally
}

fn file_label(paths: &Targets) -> String {
    if paths.len() == 1
        && let Some(first) = &paths.first
    {
        let leaf = leaf_name(first);
        return if leaf.is_empty() { first.clone() } else { leaf };
    }
    format!("{} files", paths.len())
}

/// `workSummary`: what the calls of one kind add up to.
fn work_summary(kind: ActivityWorkKind, tally: &PhaseTally, live: bool) -> String {
    let tense = |present: &str, past: &str| {
        if live {
            present.to_string()
        } else {
            past.to_string()
        }
    };
    match kind {
        ActivityWorkKind::Edit => format!(
            "{} {}",
            tense("Editing", "Edited"),
            file_label(&tally.edits)
        ),
        ActivityWorkKind::Research => {
            if tally.reads.len() > 0 && tally.searches == 0 {
                return format!("{} {}", tense("Reading", "Read"), file_label(&tally.reads));
            }
            if tally.reads.len() == 0 {
                return tense("Searching the project", "Searched the project");
            }
            tense("Exploring the project", "Explored the project")
        }
        ActivityWorkKind::Run => {
            if tally.background_live > 0 {
                return "Running in background".into();
            }
            if tally.runs == 0 && tally.background > 0 {
                return "Finished in background".into();
            }
            if tally.runs == 1 {
                tense("Running a command", "Ran a command")
            } else {
                format!("{} {} commands", tense("Running", "Ran"), tally.runs)
            }
        }
        ActivityWorkKind::Agent => {
            if tally.agents == 1 {
                tense("Running a subagent", "Ran a subagent")
            } else {
                format!("{} {} subagents", tense("Running", "Ran"), tally.agents)
            }
        }
        ActivityWorkKind::Other => {
            if tally.others == 1 {
                tense("Running a tool", "Ran a tool")
            } else {
                format!("{} {} tools", tense("Running", "Ran"), tally.others)
            }
        }
    }
}

/// `currentWorkKind`: the kind of the group's most recent call.
fn current_work_kind(steps: &[BlockRef]) -> Option<ActivityWorkKind> {
    steps
        .iter()
        .rev()
        .find(|block| is_tool_block(block))
        .map(|block| tool_category(block))
}

/// `workSummaryLine`: one clause per kind of work, such as "Read 3 files ·
/// Edited 2 files · Ran a command". While live, the clause for the call in
/// flight is present tense. Absorbed interjections add an "N notes" clause.
pub fn work_summary_line(steps: &[BlockRef], live: bool) -> String {
    if let Some(summary) = monocode_work_summary(steps, live) {
        return summary.into();
    }
    let tally = tally_steps(steps);
    let notes = match tally.notes {
        0 => String::new(),
        1 => "1 note".into(),
        n => format!("{n} notes"),
    };
    if tally.order.is_empty() {
        if !notes.is_empty() {
            return notes;
        }
        if !steps.is_empty() && steps.iter().all(|block| is_status_step(block)) {
            return "Status update".into();
        }
        return if live { "Thinking" } else { "Thought" }.into();
    }
    let running = if live { current_work_kind(steps) } else { None };
    let mut clauses: Vec<String> = tally
        .order
        .iter()
        .map(|kind| work_summary(*kind, &tally, Some(*kind) == running))
        .collect();
    if !notes.is_empty() {
        clauses.push(notes);
    }
    clauses.join(" · ")
}

/// `workKind`: the icon a run of work answers to, whatever it did most of.
pub fn work_kind(steps: &[BlockRef]) -> ActivityPhaseKind {
    if let Some(kind) = dominant_work_kind(steps) {
        return kind.into();
    }
    if steps.iter().any(|block| block.interjection.is_some())
        || (!steps.is_empty() && steps.iter().all(|block| is_status_step(block)))
    {
        ActivityPhaseKind::Note
    } else {
        ActivityPhaseKind::Think
    }
}

/// `activityPhaseTitle`: the agent's own line if it wrote one, otherwise
/// what the calls add up to.
pub fn activity_phase_title(phase: &ActivityPhase, live: bool) -> String {
    if let Some(failure) = subagent_failure_summary(&phase.steps) {
        return failure;
    }
    if let Some(headline) = &phase.headline {
        let summary = prose_summary(&headline.text);
        if !summary.is_empty() {
            return summary;
        }
        return if headline.role == BlockRole::Reasoning {
            "Thinking"
        } else {
            "Working"
        }
        .into();
    }
    work_summary_line(&phase.steps, live)
}

/// `WorkFold`: the span of a turn that folds away once the agent has
/// answered for it. Both ends are item indices, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkFold {
    pub start: usize,
    pub end: usize,
}

/// `foldableWork`: everything from the first thing the agent did up to the
/// last group it already narrated past. Calls awaiting approval never fold,
/// a live interjection stops the fold, and the fold stops above the message
/// the agent yielded with while background work was still running.
pub fn foldable_work(items: &[TurnItem]) -> Option<WorkFold> {
    let mut end = None;
    let mut answered = false;
    for index in (0..yielded_at(items)).rev() {
        let item = &items[index];
        if item.is_activity() {
            if answered && is_foldable_item(item) {
                end = Some(index);
                break;
            }
            continue;
        }
        if let TurnItem::Block(block) = item
            && is_prose_block(block)
        {
            answered = true;
        }
    }
    let end = end?;
    // Only work and commentary fold. A plan, a task list, or a call waiting
    // on approval stays where the agent put it.
    let mut start = end;
    while start > 0 && is_foldable_item(&items[start - 1]) {
        start -= 1;
    }
    Some(WorkFold { start, end })
}

/// `yieldedAt`: the first group of background rows right under the message
/// the agent yielded with, or the whole turn.
fn yielded_at(items: &[TurnItem]) -> usize {
    items
        .iter()
        .enumerate()
        .position(|(at, item)| {
            let TurnItem::Activity(blocks) = item else {
                return false;
            };
            let before = at.checked_sub(1).and_then(|before| items.get(before));
            blocks
                .iter()
                .any(|block| block.tool.as_ref().and_then(|tool| tool.background) == Some(true))
                && matches!(before, Some(TurnItem::Block(block)) if is_prose_block(block))
        })
        .unwrap_or(items.len())
}

/// `isFoldableItem`: work, commentary, and subagent stacks (whose rows the
/// transcript pins outside the fold's body).
fn is_foldable_item(item: &TurnItem) -> bool {
    match item {
        TurnItem::Subagents(_) => true,
        TurnItem::Activity(blocks) => !blocks.iter().any(|block| needs_approval(block)),
        TurnItem::Block(block) => is_prose_block(block),
    }
}

/// `firstFoldableIndex`: where a turn's work begins, fold or no fold, so the
/// fold line has a place to sit from the start.
pub fn first_foldable_index(items: &[TurnItem]) -> Option<usize> {
    items.iter().position(is_foldable_item)
}

/// `foldedBlocks`: every block inside a fold. Subagent stacks keep their own
/// rows, so they are not part of what the fold summarises.
pub fn folded_blocks(items: &[TurnItem], fold: WorkFold) -> Vec<BlockRef> {
    items[fold.start..=fold.end]
        .iter()
        .flat_map(|item| match item {
            TurnItem::Block(block) => vec![block.clone()],
            TurnItem::Subagents(_) => Vec::new(),
            TurnItem::Activity(blocks) => blocks.clone(),
        })
        .collect()
}

/// The scroll metrics `nestedScrollAbsorbsWheel` reads from an element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMetrics {
    pub scroll_top: f32,
    pub scroll_height: f32,
    pub client_height: f32,
}

/// `nestedScrollAbsorbsWheel`: a nested scroller consumes this wheel rather
/// than the transcript.
pub fn nested_scroll_absorbs_wheel(el: ScrollMetrics, delta_y: f32) -> bool {
    if el.scroll_height <= el.client_height + 1. {
        return false;
    }
    let at_top = el.scroll_top <= 0.;
    let at_bottom = el.scroll_top + el.client_height >= el.scroll_height - 1.;
    (delta_y < 0. && !at_top) || (delta_y > 0. && !at_bottom)
}

/// `BlockNotice` re-exported for callers that match on notices.
pub type Notice = BlockNotice;

#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;
