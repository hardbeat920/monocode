//! Port of src/integrations/harness/core/apply.ts: the pure reducer that
//! applies harness events to a session.
//!
//! The TypeScript built a new session for every change and returned the same
//! object when nothing changed. Here each `_mut` function changes the session
//! in place and returns `false` exactly where the TypeScript returned the
//! session it was given, so callers keep that signal without copying the
//! transcript. The engine applies a batch every frame while text streams, so
//! it should call [`apply_harness_events_mut`]. The functions that take a
//! `&Session` and return a `Session` clone it once and then apply in place.
//!
//! `crypto.randomUUID()` and `Date.now()` come from a [`ReducerEnv`], so tests
//! can pin block ids and time.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::attachment::Attachment;
use crate::block::{
    AgentRunMeta, AgentStep, AgentStepKind, ApprovalDecided, Block, BlockApproval, BlockNotice,
    BlockRole, BlockTool, Extra, GeneratedImageMeta, InterjectionMeta, PlanBlockMeta, PlanStatus,
    SecondOpinionMeta, TaskListItem, TaskListItemStatus, TaskListMeta, ToolPreview,
    ToolPreviewLineKind, TurnIntent, TurnMetrics, TurnModel,
};
use crate::context_usage::{ContextReading, merge_context_usage};
use crate::harness_event::{GeneratedImage, HarnessEvent};
use crate::js;
use crate::models::ModelCatalog;
use crate::notes::NoteCardMeta;
use crate::orchestration::OrchestrationProposalStatus;
use crate::paths::display_path;
use crate::plan::is_reviewable_plan;
use crate::reducer::js_regex::{js_regex, nonempty};
use crate::reducer::preview::{
    ToolTitleInput, compose_tool_title, is_file_tool, is_weak_tool_title, merge_tool_preview,
    stub_file_preview,
};
use crate::reducer::stream_text::{join_stream_text, join_stream_text_into};
use crate::session::{Session, UsageLimit};
use crate::task_list::task_list_text;
use crate::user_question::UserQuestionPrompt;

/// Where the reducer gets new block ids and the current time.
pub trait ReducerEnv {
    /// A fresh block id, `crypto.randomUUID()` in TypeScript.
    fn new_id(&mut self) -> String;
    /// Epoch milliseconds, `Date.now()` in TypeScript.
    fn now_ms(&mut self) -> i64;
}

/// Random v4 UUIDs and the system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

impl ReducerEnv for SystemEnv {
    fn new_id(&mut self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn now_ms(&mut self) -> i64 {
        now_ms()
    }
}

/// `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// `applyHarnessEvents`: apply one delivery batch without copying the
/// transcript for every token.
pub fn apply_harness_events(session: &Session, events: &[HarnessEvent]) -> Session {
    let mut next = session.clone();
    apply_harness_events_mut(&mut SystemEnv, &mut next, events);
    next
}

/// [`apply_harness_events`] in place. Runs of `message.delta` or
/// `reasoning.delta` fold into the open block in one step. Returns whether
/// the session changed.
pub fn apply_harness_events_mut(
    env: &mut dyn ReducerEnv,
    session: &mut Session,
    events: &[HarnessEvent],
) -> bool {
    let mut changed = false;
    let mut index = 0;
    while index < events.len() {
        let Some(role) = delta_role(&events[index]) else {
            changed |= apply_harness_event_mut(env, session, &events[index]);
            index += 1;
            continue;
        };
        let start = index;
        while index + 1 < events.len() && delta_role(&events[index + 1]) == Some(role) {
            index += 1;
        }
        let texts: Vec<&str> = events[start..=index]
            .iter()
            .filter_map(delta_text)
            .collect();
        changed |= patch_streaming(env, session, role, &texts, true);
        index += 1;
    }
    changed
}

fn delta_role(event: &HarnessEvent) -> Option<BlockRole> {
    match event {
        HarnessEvent::MessageDelta { .. } => Some(BlockRole::Assistant),
        HarnessEvent::ReasoningDelta { .. } => Some(BlockRole::Reasoning),
        _ => None,
    }
}

fn delta_text(event: &HarnessEvent) -> Option<&str> {
    match event {
        HarnessEvent::MessageDelta { text } | HarnessEvent::ReasoningDelta { text } => Some(text),
        _ => None,
    }
}

/// `applyHarnessEvent`.
pub fn apply_harness_event(session: &Session, event: &HarnessEvent) -> Session {
    let mut next = session.clone();
    apply_harness_event_mut(&mut SystemEnv, &mut next, event);
    next
}

/// [`apply_harness_event`] in place. Returns whether the session changed.
pub fn apply_harness_event_mut(
    env: &mut dyn ReducerEnv,
    session: &mut Session,
    event: &HarnessEvent,
) -> bool {
    match event {
        HarnessEvent::MessageDelta { text } => {
            patch_streaming(env, session, BlockRole::Assistant, &[text.as_str()], true)
        }
        HarnessEvent::MessageCompleted => {
            finish_role(session, BlockRole::Assistant);
            true
        }
        HarnessEvent::ImageGenerated(GeneratedImage::File {
            path,
            name,
            mime_type,
            size,
            alt,
            ..
        }) => {
            append_block(
                session,
                Block {
                    image: Some(GeneratedImageMeta {
                        path: path.clone(),
                        name: name.clone(),
                        mime_type: mime_type.clone(),
                        size: *size,
                        alt: nonempty(alt.as_deref()).map(str::to_string),
                        extra: Extra::new(),
                    }),
                    ..Block::new(env.new_id(), BlockRole::Image, "")
                },
            );
            true
        }
        HarnessEvent::ImageGenerated(GeneratedImage::Inline { .. }) => false,
        HarnessEvent::ReasoningDelta { text } => {
            patch_streaming(env, session, BlockRole::Reasoning, &[text.as_str()], true)
        }
        HarnessEvent::ReasoningCompleted => {
            finish_role(session, BlockRole::Reasoning);
            true
        }
        HarnessEvent::ToolStarted {
            agent_model,
            call_id,
            title,
            kind,
            status,
            background,
            preview,
            ..
        } => upsert_tool(
            env,
            session,
            ToolPatch {
                call_id,
                title: Some(title),
                kind: kind.as_deref(),
                status: status.as_deref(),
                detail: None,
                preview: preview.as_ref(),
                streaming: true,
                agent_model: agent_model.as_deref(),
                background: *background == Some(true),
            },
        ),
        HarnessEvent::ToolUpdated {
            agent_model,
            call_id,
            title,
            kind,
            status,
            detail,
            preview,
            ..
        } => upsert_tool(
            env,
            session,
            ToolPatch {
                call_id,
                title: title.as_deref(),
                kind: kind.as_deref(),
                status: status.as_deref(),
                detail: detail.as_deref(),
                preview: preview.as_ref(),
                streaming: !matches!(status.as_deref(), Some("completed" | "failed")),
                agent_model: agent_model.as_deref(),
                background: false,
            },
        ),
        HarnessEvent::AgentStep { .. } => record_agent_step(session, event),
        HarnessEvent::ApprovalRequested { .. } => attach_approval(env, session, event),
        HarnessEvent::ApprovalResolved {
            request_id,
            decision,
        } => {
            for block in &mut session.blocks {
                if let Some(approval) = &mut block.approval
                    && approval.request_id == *request_id
                {
                    approval.decided = Some(*decision);
                }
            }
            true
        }
        HarnessEvent::QuestionAsked {
            request_id,
            title,
            questions,
            auto_resolve_at,
            ..
        } => {
            session.pending_question = Some(UserQuestionPrompt {
                request_id: *request_id,
                title: nonempty(title.as_deref()).map(str::to_string),
                questions: questions.clone(),
                auto_resolve_at: *auto_resolve_at,
            });
            true
        }
        HarnessEvent::QuestionUpdated {
            request_id,
            auto_resolve_at,
        } => match &mut session.pending_question {
            Some(pending) if pending.request_id == *request_id => {
                pending.auto_resolve_at = *auto_resolve_at;
                true
            }
            _ => false,
        },
        HarnessEvent::QuestionResolved { request_id, .. } => {
            if session
                .pending_question
                .as_ref()
                .is_some_and(|pending| pending.request_id == *request_id)
            {
                session.pending_question = None;
                true
            } else {
                false
            }
        }
        HarnessEvent::Context { used, window } => {
            session.context = Some(merge_context_usage(
                session.context.as_ref(),
                ContextReading {
                    used: *used,
                    window: *window,
                },
            ));
            true
        }
        HarnessEvent::TurnMetrics(metrics) => merge_turn_metrics(session, metrics),
        HarnessEvent::TasksUpdated { .. } => upsert_task_list(env, session, event),
        HarnessEvent::BackgroundUpdated { tasks } => {
            if tasks.is_empty() {
                return session.background_tasks.take().is_some();
            }
            session.background_tasks = Some(tasks.clone());
            true
        }
        HarnessEvent::Plan { .. } => upsert_plan(env, session, event),
        HarnessEvent::SessionError { message } => {
            fail_streaming(env, session);
            append_block(
                session,
                Block {
                    notice: Some(BlockNotice::Error),
                    ..Block::new(env.new_id(), BlockRole::System, message.as_str())
                },
            );
            true
        }
        HarnessEvent::SessionProviderBound {
            provider_session_id,
        } => {
            session.provider_session_id = Some(provider_session_id.clone());
            true
        }
        HarnessEvent::TurnStarted { provider_turn_id } => {
            let Some(index) = last_user_index(&session.blocks) else {
                return false;
            };
            let block = &mut session.blocks[index];
            if block.provider_turn_id.as_deref() == Some(provider_turn_id.as_str()) {
                return false;
            }
            block.provider_turn_id = Some(provider_turn_id.clone());
            true
        }
        HarnessEvent::SessionConfigChanged {
            model,
            model_settings,
        } => {
            if let Some(model) = nonempty(model.as_deref()) {
                session.model = model.to_string();
            }
            if let Some(settings) = model_settings {
                session.model_settings.extend(settings.clone());
            }
            true
        }
        HarnessEvent::Status { text } => append_status(env, session, text),
        HarnessEvent::UsageLimited { resets_at } => {
            session.usage_limit = Some(UsageLimit {
                resets_at: *resets_at,
                resume_at_reset: None,
            });
            true
        }
        HarnessEvent::Interjection {
            text,
            custom_type,
            severity,
        } => {
            // A visible boundary the user must not miss, so unlike status it
            // never deduplicates and never reads as turn lifecycle.
            append_block(
                session,
                Block {
                    interjection: Some(InterjectionMeta {
                        custom_type: custom_type.clone(),
                        severity: *severity,
                        extra: Extra::new(),
                    }),
                    ..Block::new(env.new_id(), BlockRole::System, text.as_str())
                },
            );
            true
        }
        HarnessEvent::SessionStarted | HarnessEvent::SessionEnded { .. } => false,
    }
}

fn merge_turn_metrics(session: &mut Session, event: &TurnMetrics) -> bool {
    let Some(index) = last_user_index(&session.blocks) else {
        return false;
    };
    let metrics = session.blocks[index]
        .turn_metrics
        .get_or_insert_with(TurnMetrics::default);
    if event.input_tokens.is_some() {
        metrics.input_tokens = event.input_tokens;
    }
    if event.output_tokens.is_some() {
        metrics.output_tokens = event.output_tokens;
    }
    if event.cache_read_tokens.is_some() {
        metrics.cache_read_tokens = event.cache_read_tokens;
    }
    if event.cache_write_tokens.is_some() {
        metrics.cache_write_tokens = event.cache_write_tokens;
    }
    if event.cache_hit_percent.is_some() {
        metrics.cache_hit_percent = event.cache_hit_percent;
    }
    true
}

fn upsert_plan(env: &mut dyn ReducerEnv, session: &mut Session, event: &HarnessEvent) -> bool {
    let HarnessEvent::Plan {
        text,
        key,
        append,
        streaming,
    } = event
    else {
        return false;
    };
    let key = nonempty(key.as_deref().map(js::trim)).map(str::to_string);
    let last_user = last_user_index(&session.blocks);
    let existing = last_matching_block(&session.blocks, |block, index| {
        if block.role != BlockRole::Plan {
            return false;
        }
        let block_key = block.plan.as_ref().and_then(|plan| plan.key.as_deref());
        if let Some(key) = key.as_deref() {
            return block_key == Some(key)
                || (nonempty(block_key).is_none() && after(index, last_user));
        }
        after(index, last_user)
    });
    let streaming = streaming.unwrap_or(false);
    let status = if streaming {
        PlanStatus::Streaming
    } else {
        PlanStatus::Ready
    };

    if let Some(index) = existing {
        let current = &mut session.blocks[index];
        if *append == Some(true) {
            join_stream_text_into(&mut current.text, text);
        } else if !text.is_empty() {
            current.text = text.clone();
        }
        current.streaming = Some(streaming);
        let plan = current.plan.get_or_insert_with(|| PlanBlockMeta {
            status,
            ..PlanBlockMeta::default()
        });
        if key.is_some() {
            plan.key = key;
        }
        plan.status = status;
        if !streaming && !current.text.is_empty() {
            plan.original_text = Some(current.text.clone());
            plan.edited = Some(false);
        }
        return true;
    }

    if text.is_empty() {
        return false;
    }
    append_block(
        session,
        Block {
            streaming: Some(streaming),
            plan: Some(PlanBlockMeta {
                key,
                status,
                original_text: (!streaming).then(|| text.clone()),
                ..PlanBlockMeta::default()
            }),
            ..Block::new(env.new_id(), BlockRole::Plan, text.as_str())
        },
    );
    true
}

fn upsert_task_list(env: &mut dyn ReducerEnv, session: &mut Session, event: &HarnessEvent) -> bool {
    let HarnessEvent::TasksUpdated {
        key,
        explanation,
        merge,
        authoritative,
        provider_session_id,
        items: event_items,
    } = event
    else {
        return false;
    };
    let key = nonempty(key.as_deref().map(js::trim)).map(str::to_string);
    let provider_session_id = nonempty(provider_session_id.as_deref());
    let last_user = last_user_index(&session.blocks);
    let existing = last_matching_block(&session.blocks, |block, index| {
        if block.role != BlockRole::Tasks {
            return false;
        }
        if let Some(key) = key.as_deref() {
            let list = block.task_list.as_ref();
            if list.and_then(|list| list.key.as_deref()) != Some(key) {
                return false;
            }
            // A list from another provider conversation stays as history.
            return provider_session_id.is_none()
                || list.and_then(|list| list.provider_session_id.as_deref())
                    == provider_session_id;
        }
        after(index, last_user)
    });
    let previous_items = existing
        .and_then(|index| session.blocks[index].task_list.as_ref())
        .map(|list| &list.items);
    let items = match previous_items {
        Some(previous) if *merge == Some(true) => merge_task_list_items(previous, event_items),
        Some(_) if *authoritative == Some(true) => event_items.clone(),
        Some(previous) => preserve_task_list_labels(previous, event_items),
        None => event_items.clone(),
    };

    if items.is_empty() {
        let Some(index) = existing else {
            return false;
        };
        session.blocks.remove(index);
        return true;
    }

    let text = task_list_text(&items);
    let task_list = TaskListMeta {
        key,
        provider_session_id: provider_session_id.map(str::to_string),
        explanation: nonempty(explanation.as_deref().map(js::trim)).map(str::to_string),
        items,
        extra: Extra::new(),
    };
    if let Some(index) = existing {
        let block = &mut session.blocks[index];
        block.text = text;
        block.task_list = Some(task_list);
        return true;
    }

    append_block(
        session,
        Block {
            task_list: Some(task_list),
            ..Block::new(env.new_id(), BlockRole::Tasks, text)
        },
    );
    true
}

fn merge_task_list_items(existing: &[TaskListItem], updates: &[TaskListItem]) -> Vec<TaskListItem> {
    if updates.is_empty() {
        return existing.to_vec();
    }
    let mut items = existing.to_vec();
    let mut index_by_id: HashMap<String, usize> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(id) = nonempty(item.id.as_deref()) {
            index_by_id.insert(id.to_string(), index);
        }
    }

    for update in updates {
        let by_text =
            |items: &[TaskListItem]| items.iter().position(|item| item.text == update.text);
        let index = match nonempty(update.id.as_deref()) {
            Some(id) => index_by_id.get(id).copied().or_else(|| by_text(&items)),
            None => by_text(&items),
        };
        let Some(index) = index else {
            items.push(update.clone());
            if let Some(id) = nonempty(update.id.as_deref()) {
                index_by_id.insert(id.to_string(), items.len() - 1);
            }
            continue;
        };
        let current = &items[index];
        let has_id =
            nonempty(current.id.as_deref()).is_some() || nonempty(update.id.as_deref()).is_some();
        items[index] = TaskListItem {
            id: if has_id {
                current.id.clone().or_else(|| update.id.clone())
            } else {
                None
            },
            // A merge update changes state. Full snapshots remain responsible
            // for intentional task renames or reordered lists.
            text: current.text.clone(),
            status: update.status,
            extra: Extra::new(),
        };
    }
    items
}

fn preserve_task_list_labels(
    existing: &[TaskListItem],
    snapshot: &[TaskListItem],
) -> Vec<TaskListItem> {
    let mut existing_by_id: HashMap<&str, &TaskListItem> = HashMap::new();
    for item in existing {
        if let Some(id) = nonempty(item.id.as_deref()) {
            existing_by_id.insert(id, item);
        }
    }
    snapshot
        .iter()
        .map(|item| {
            let previous = nonempty(item.id.as_deref()).and_then(|id| existing_by_id.get(id));
            match previous {
                Some(previous) if previous.text != item.text => TaskListItem {
                    text: previous.text.clone(),
                    ..item.clone()
                },
                _ => item.clone(),
            }
        })
        .collect()
}

/// `lastMatchingBlock`, with `None` for the TypeScript's `-1`.
fn last_matching_block(
    blocks: &[Block],
    predicate: impl Fn(&Block, usize) -> bool,
) -> Option<usize> {
    (0..blocks.len())
        .rev()
        .find(|index| predicate(&blocks[*index], *index))
}

fn last_user_index(blocks: &[Block]) -> Option<usize> {
    blocks
        .iter()
        .rposition(|block| block.role == BlockRole::User)
}

/// `index > lastUser`, where no user block reads as `-1`.
fn after(index: usize, last_user: Option<usize>) -> bool {
    last_user.is_none_or(|last_user| index > last_user)
}

/// `UserTurnExtra`: fields a submitted user turn can carry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserTurnExtra {
    pub second_opinion: Option<SecondOpinionMeta>,
    pub note_card: Option<NoteCardMeta>,
    pub ci_context: Option<String>,
    pub internal: bool,
    pub monocode: bool,
    pub intent: Option<TurnIntent>,
    pub app_request_id: Option<String>,
}

/// `userTurnFields`.
fn apply_user_turn_fields(block: &mut Block, extra: Option<&UserTurnExtra>) {
    let Some(extra) = extra else {
        return;
    };
    block.second_opinion = extra.second_opinion.clone();
    block.note_card = extra.note_card.clone();
    block.ci_context = nonempty(extra.ci_context.as_deref()).map(str::to_string);
    block.internal = extra.internal.then_some(true);
    block.monocode = extra.monocode.then_some(true);
    block.intent = extra.intent;
    block.app_request_id = nonempty(extra.app_request_id.as_deref()).map(str::to_string);
}

/// `turnModelFields`.
fn turn_model(catalog: &ModelCatalog, session: &Session) -> TurnModel {
    let model = catalog.resolve_model(session.harness, Some(&session.model));
    TurnModel {
        harness: session.harness,
        id: session.model.clone(),
        name: model.name,
        extra: Extra::new(),
    }
}

/// `appendUser`: settle stale approvals, mark the session busy, and append a
/// user turn stamped with the current time and model.
pub fn append_user(
    catalog: &ModelCatalog,
    session: &Session,
    text: &str,
    attachments: &[Attachment],
    extra: Option<&UserTurnExtra>,
) -> Session {
    let mut next = session.clone();
    append_user_mut(&mut SystemEnv, catalog, &mut next, text, attachments, extra);
    next
}

/// [`append_user`] in place.
pub fn append_user_mut(
    env: &mut dyn ReducerEnv,
    catalog: &ModelCatalog,
    session: &mut Session,
    text: &str,
    attachments: &[Attachment],
    extra: Option<&UserTurnExtra>,
) {
    settle_pending_approvals(session);
    session.busy = Some(true);
    let mut block = Block {
        started_at: Some(env.now_ms()),
        turn_model: Some(turn_model(catalog, session)),
        attachments: (!attachments.is_empty()).then(|| attachments.to_vec()),
        ..Block::new(env.new_id(), BlockRole::User, text)
    };
    apply_user_turn_fields(&mut block, extra);
    append_block(session, block);
}

/// `appendSteerUser`: append a follow-up user message during an active turn
/// without sealing streams.
pub fn append_steer_user(
    catalog: &ModelCatalog,
    session: &Session,
    text: &str,
    attachments: &[Attachment],
    extra: Option<&UserTurnExtra>,
) -> Session {
    let mut next = session.clone();
    append_steer_user_mut(&mut SystemEnv, catalog, &mut next, text, attachments, extra);
    next
}

/// [`append_steer_user`] in place.
pub fn append_steer_user_mut(
    env: &mut dyn ReducerEnv,
    catalog: &ModelCatalog,
    session: &mut Session,
    text: &str,
    attachments: &[Attachment],
    extra: Option<&UserTurnExtra>,
) {
    session.busy = Some(true);
    let mut block = Block {
        turn_model: Some(turn_model(catalog, session)),
        attachments: (!attachments.is_empty()).then(|| attachments.to_vec()),
        ..Block::new(env.new_id(), BlockRole::User, text)
    };
    apply_user_turn_fields(&mut block, extra);
    session.blocks.push(block);
}

/// `stopStreaming`: end the turn. TypeScript defaulted `ended_at` to
/// `Date.now()`; pass [`now_ms`] for that.
pub fn stop_streaming(session: &Session, ended_at: i64) -> Session {
    let mut next = session.clone();
    stop_streaming_mut(&mut next, ended_at);
    next
}

/// [`stop_streaming`] in place. It always changes the session.
pub fn stop_streaming_mut(session: &mut Session, ended_at: i64) {
    settle_pending_approvals(session);
    session.background_tasks = None;
    session.busy = Some(false);
    session.pending_question = None;
    for block in &mut session.blocks {
        stop_block_progress(block);
    }
    stamp_turn_duration(&mut session.blocks, ended_at);
}

fn tool_status_lower(block: &Block) -> String {
    block
        .tool
        .as_ref()
        .and_then(|tool| tool.status.as_deref())
        .map(str::to_lowercase)
        .unwrap_or_default()
}

/// `settlePendingApprovals`.
///
/// Approval request ids are live only for the turn that produced them. Once
/// that turn has stopped (or a later turn is about to start), leaving one
/// undecided makes its old Allow and Deny controls and notification
/// actionable even though the harness can no longer receive the response.
fn settle_pending_approvals(session: &mut Session) -> bool {
    let pending = |block: &Block| {
        block
            .approval
            .as_ref()
            .is_some_and(|approval| approval.decided.is_none())
    };
    if !session.blocks.iter().any(pending) {
        return false;
    }
    session.blocks.retain_mut(|block| {
        if !pending(block) {
            return true;
        }
        if block.role == BlockRole::Approval {
            return false;
        }
        let tool_finished = matches!(
            tool_status_lower(block).as_str(),
            "completed" | "success" | "failed" | "error" | "cancelled" | "canceled"
        );
        block.streaming = Some(false);
        if let Some(tool) = &mut block.tool
            && !tool_finished
        {
            tool.status = Some("cancelled".into());
        }
        if let Some(approval) = &mut block.approval {
            approval.decided = Some(ApprovalDecided::Cancelled);
        }
        true
    });
    true
}

/// `failStreaming`.
///
/// A terminal provider failure also settles work whose final tool event was
/// lost with the transport. Leaving those calls `in_progress` hides the real
/// failure behind a neutral completed-turn summary.
fn fail_streaming(env: &mut dyn ReducerEnv, session: &mut Session) {
    let open_tools: HashSet<String> = session
        .blocks
        .iter()
        .filter(|block| {
            matches!(block.role, BlockRole::Tool | BlockRole::Approval)
                && (block.is_streaming()
                    || matches!(
                        tool_status_lower(block).as_str(),
                        "in_progress" | "pending" | "running"
                    ))
        })
        .map(|block| block.id.clone())
        .collect();
    stop_streaming_mut(session, env.now_ms());
    for block in &mut session.blocks {
        let open = open_tools.contains(&block.id);
        let pending_approval = block
            .approval
            .as_ref()
            .is_some_and(|approval| approval.decided.is_none());
        if !open && !pending_approval {
            continue;
        }
        block.streaming = Some(false);
        if open && let Some(tool) = &mut block.tool {
            tool.status = Some("failed".into());
        }
        if let Some(approval) = &mut block.approval
            && approval.decided.is_none()
        {
            approval.decided = Some(ApprovalDecided::Cancelled);
        }
    }
}

/// `promoteLastAssistantToPlan`.
///
/// Harnesses without a structured plan event return their plan as the final
/// assistant message. Convert only that final message after the turn has
/// actually ended; progress commentary earlier in the turn must stay normal
/// assistant text.
pub fn promote_last_assistant_to_plan(session: &Session, key: Option<&str>) -> Session {
    let mut next = session.clone();
    promote_last_assistant_to_plan_mut(&mut next, key);
    next
}

/// [`promote_last_assistant_to_plan`] in place. Returns whether the session
/// changed.
pub fn promote_last_assistant_to_plan_mut(session: &mut Session, key: Option<&str>) -> bool {
    let last_user = last_user_index(&session.blocks);
    let turn_start = last_user.map_or(0, |index| index + 1);
    let turn = &session.blocks[turn_start..];
    if turn.iter().any(|block| block.role == BlockRole::Plan) {
        return false;
    }
    let Some(offset) = turn
        .iter()
        .rposition(|block| block.role == BlockRole::Assistant && !js::trim(&block.text).is_empty())
    else {
        return false;
    };
    let block = &mut session.blocks[turn_start + offset];
    if !is_reviewable_plan(&block.text) {
        return false;
    }
    block.role = BlockRole::Plan;
    block.streaming = Some(false);
    block.plan = Some(PlanBlockMeta {
        key: nonempty(key).map(str::to_string),
        status: PlanStatus::Ready,
        original_text: Some(block.text.clone()),
        approved_text: None,
        edited: Some(false),
        extra: Extra::new(),
    });
    true
}

fn stop_block_progress(block: &mut Block) {
    if block.is_streaming() {
        block.streaming = Some(false);
    }
    if let Some(orchestration) = &mut block.orchestration
        && orchestration.status == OrchestrationProposalStatus::Planning
    {
        orchestration.status = OrchestrationProposalStatus::Invalid;
        orchestration.error =
            Some("Planning was interrupted. Generate the assignments again.".into());
    }
    if block.role == BlockRole::Plan
        && let Some(plan) = &mut block.plan
        && plan.status == PlanStatus::Streaming
    {
        plan.status = PlanStatus::Ready;
        plan.original_text = Some(block.text.clone());
        plan.edited = Some(false);
    }
    if let Some(list) = &mut block.task_list
        && list
            .items
            .iter()
            .any(|item| item.status == TaskListItemStatus::InProgress)
    {
        for item in &mut list.items {
            if item.status == TaskListItemStatus::InProgress {
                item.status = TaskListItemStatus::Pending;
            }
        }
        block.text = task_list_text(&list.items);
    }
}

fn stamp_turn_duration(blocks: &mut [Block], ended_at: i64) {
    let Some(index) = last_user_index(blocks) else {
        return;
    };
    let user = &mut blocks[index];
    if user.duration_ms.is_some() {
        return;
    }
    let Some(started_at) = user.started_at else {
        return;
    };
    user.duration_ms = Some((ended_at - started_at).max(0));
}

/// `appendStatus`: status pings repeat, so keep one row per run instead of
/// stacking identical lines.
fn append_status(env: &mut dyn ReducerEnv, session: &mut Session, text: &str) -> bool {
    let trimmed = js::trim(text);
    if trimmed.is_empty() {
        return false;
    }
    let last = session
        .blocks
        .iter()
        .rev()
        .find(|block| block.role != BlockRole::Reasoning);
    if last.is_some_and(|last| last.role == BlockRole::System && last.text == trimmed) {
        return false;
    }
    append_block(
        session,
        Block::new(env.new_id(), BlockRole::System, trimmed),
    );
    true
}

fn append_block(session: &mut Session, block: Block) {
    if !(block.role == BlockRole::System && block.interjection.is_none()) {
        seal_last_stream(&mut session.blocks);
    }
    session.blocks.push(block);
}

/// The last block that is not an ordinary status row. Only ordinary status
/// rows leave an open prose stream intact.
fn last_open_index(blocks: &[Block]) -> Option<usize> {
    blocks
        .iter()
        .rposition(|block| !(block.role == BlockRole::System && block.interjection.is_none()))
}

/// `patchStreaming`.
fn patch_streaming(
    env: &mut dyn ReducerEnv,
    session: &mut Session,
    role: BlockRole,
    texts: &[&str],
    streaming: bool,
) -> bool {
    if role == BlockRole::Reasoning && texts.iter().all(|text| text.is_empty()) {
        return false;
    }
    // A completion closes one provider message. The next delta is a new
    // message even when no tool or status row landed between them; joining
    // the two can turn separate Markdown blocks into text such as
    // `commitConnect`.
    if let Some(index) = last_open_index(&session.blocks) {
        let last = &mut session.blocks[index];
        if last.role == role && last.is_streaming() {
            // Fold against the existing text in order: providers can mix
            // tokens and full snapshots, so concatenating the incoming chunks
            // would duplicate text. The text only ever grows, so it changed
            // exactly when a join did something.
            let mut changed = false;
            for text in texts {
                changed |= join_stream_text_into(&mut last.text, text);
            }
            if !changed && last.streaming == Some(streaming) {
                return false;
            }
            last.streaming = Some(streaming);
            return true;
        }
    }
    seal_last_stream(&mut session.blocks);
    let text = texts
        .iter()
        .fold(String::new(), |acc, text| join_stream_text(&acc, text));
    session.blocks.push(Block {
        streaming: Some(streaming),
        ..Block::new(env.new_id(), role, text)
    });
    true
}

fn attach_approval(env: &mut dyn ReducerEnv, session: &mut Session, event: &HarnessEvent) -> bool {
    let HarnessEvent::ApprovalRequested {
        request_id,
        title,
        kind,
        call_id,
        preview,
    } = event
    else {
        return false;
    };
    let approval = BlockApproval {
        request_id: *request_id,
        decided: None,
        extra: Extra::new(),
    };
    if let Some(index) = find_tool_for_approval(&session.blocks, call_id.as_deref(), title) {
        let prev = &session.blocks[index];
        let prev_tool = prev.tool.as_ref();
        let merged = merge_tool_preview(
            preview.as_ref(),
            prev_tool.and_then(|tool| tool.preview.as_ref()),
        );
        let preferred = prefer_label(&[
            Some(title),
            prev_tool.and_then(|tool| tool.title.as_deref()),
            Some(&prev.text),
        ]);
        let mut label = final_tool_label(
            &session.cwd,
            kind.as_deref()
                .or(prev_tool.and_then(|tool| tool.kind.as_deref())),
            Some(&preferred),
            merged.as_ref(),
        );
        if label.is_empty() {
            label = prev.text.clone();
        }
        let tool = match prev_tool {
            Some(tool) => {
                let mut tool = tool.clone();
                tool.kind = kind.clone().or(tool.kind);
                if !label.is_empty() {
                    tool.title = Some(label.clone());
                }
                if merged.is_some() {
                    tool.preview = merged;
                }
                Some(tool)
            }
            None => nonempty(call_id.as_deref()).map(|call_id| BlockTool {
                call_id: Some(call_id.to_string()),
                title: Some(label.clone()),
                kind: kind.clone(),
                preview: merged,
                ..BlockTool::default()
            }),
        };
        let block = &mut session.blocks[index];
        if !label.is_empty() {
            block.text = label;
        }
        block.tool = tool;
        block.approval = Some(approval);
        return true;
    }
    let label = final_tool_label(
        &session.cwd,
        kind.as_deref(),
        Some(&prefer_label(&[Some(title)])),
        preview.as_ref(),
    );
    let label = if label.is_empty() {
        kind_title(kind.as_deref())
    } else {
        label
    };
    append_block(
        session,
        Block {
            tool: Some(BlockTool {
                call_id: nonempty(call_id.as_deref()).map(str::to_string),
                title: Some(label.clone()),
                kind: kind.clone(),
                preview: preview.clone(),
                ..BlockTool::default()
            }),
            approval: Some(approval),
            ..Block::new(env.new_id(), BlockRole::Tool, label)
        },
    );
    true
}

fn tool_call_id(block: &Block) -> Option<&str> {
    block.tool.as_ref().and_then(|tool| tool.call_id.as_deref())
}

/// `block.text || block.tool?.title || ""`.
fn block_label(block: &Block) -> &str {
    nonempty(Some(&block.text))
        .or_else(|| nonempty(block.tool.as_ref().and_then(|tool| tool.title.as_deref())))
        .unwrap_or("")
}

fn find_tool_for_approval(blocks: &[Block], call_id: Option<&str>, title: &str) -> Option<usize> {
    if let Some(call_id) = nonempty(call_id)
        && let Some(index) = blocks
            .iter()
            .position(|block| tool_call_id(block) == Some(call_id))
    {
        return Some(index);
    }
    let needle = normalize_label(title);
    let mut unmatched = Vec::new();
    for index in (0..blocks.len()).rev() {
        let block = &blocks[index];
        if block.role != BlockRole::Tool || block.approval.is_some() {
            continue;
        }
        unmatched.push(index);
        if !needle.is_empty() && normalize_label(block_label(block)) == needle {
            return Some(index);
        }
    }
    if unmatched.len() == 1 {
        Some(unmatched[0])
    } else {
        None
    }
}

fn normalize_label(value: &str) -> String {
    let value = value.replace(['→', '`'], "");
    let value = js_regex!(r"{S}*\([^)]*\){S}*$").replace(&value, "");
    let value = js_regex!(r"{S}*·{DOT}*$").replace(&value, "");
    js::trim(&value).to_lowercase()
}

/// The fields of `tool.started` and `tool.updated` that `upsertTool` reads.
struct ToolPatch<'a> {
    call_id: &'a str,
    title: Option<&'a str>,
    kind: Option<&'a str>,
    status: Option<&'a str>,
    detail: Option<&'a str>,
    preview: Option<&'a ToolPreview>,
    streaming: bool,
    agent_model: Option<&'a str>,
    background: bool,
}

fn upsert_tool(env: &mut dyn ReducerEnv, session: &mut Session, patch: ToolPatch<'_>) -> bool {
    let Some(index) = find_tool_index(&session.blocks, patch.call_id, patch.title) else {
        let detail = cap_tool_detail(patch.detail);
        let preview = fill_preview(patch.preview.cloned(), patch.kind, patch.title);
        let label = final_tool_label(
            &session.cwd,
            patch.kind,
            Some(&display_label(patch.title, patch.kind, None)),
            preview.as_ref(),
        );
        let agent_run = nonempty(patch.agent_model).map(|model| AgentRunMeta {
            name: label.clone(),
            model: Some(model.to_string()),
            ..AgentRunMeta::default()
        });
        let tool = BlockTool {
            call_id: Some(patch.call_id.to_string()),
            title: Some(label.clone()),
            kind: patch.kind.map(str::to_string),
            status: patch.status.map(str::to_string),
            detail,
            preview,
            background: patch.background.then_some(true),
            extra: Extra::new(),
        };
        append_block(
            session,
            Block {
                streaming: Some(patch.streaming),
                agent_run,
                tool: Some(tool),
                ..Block::new(env.new_id(), BlockRole::Tool, label)
            },
        );
        return true;
    };

    let prev = &session.blocks[index];
    let prev_tool = prev.tool.as_ref();
    let detail =
        cap_tool_detail(patch.detail).or_else(|| prev_tool.and_then(|tool| tool.detail.clone()));
    let kind = patch
        .kind
        .or(prev_tool.and_then(|tool| tool.kind.as_deref()));
    let preview = fill_preview(
        merge_tool_preview(
            patch.preview,
            prev_tool.and_then(|tool| tool.preview.as_ref()),
        ),
        kind,
        patch.title,
    );
    let label = final_tool_label(
        &session.cwd,
        kind,
        Some(&display_label(patch.title, patch.kind, Some(prev))),
        preview.as_ref(),
    );
    let status = patch
        .status
        .or(prev_tool.and_then(|tool| tool.status.as_deref()));
    let agent_name = match &prev.agent_run {
        Some(run) if !run.steps.is_empty() => run.name.clone(),
        _ => label.clone(),
    };
    let agent_model = nonempty(patch.agent_model);
    let unchanged = prev.text == label
        && prev.streaming == Some(patch.streaming)
        && prev_tool.and_then(|tool| tool.title.as_deref()) == Some(label.as_str())
        && prev_tool.and_then(|tool| tool.kind.as_deref()) == kind
        && prev_tool.and_then(|tool| tool.status.as_deref()) == status
        && prev_tool.and_then(|tool| tool.detail.as_deref()) == detail.as_deref()
        && (agent_model.is_none()
            || prev.agent_run.as_ref().and_then(|run| run.model.as_deref()) == agent_model)
        && prev
            .agent_run
            .as_ref()
            .is_none_or(|run| run.name == agent_name)
        && same_preview(
            prev_tool.and_then(|tool| tool.preview.as_ref()),
            preview.as_ref(),
        );
    if unchanged {
        return false;
    }

    let kind = kind.map(str::to_string);
    let status = status.map(str::to_string);
    let background = prev_tool.and_then(|tool| tool.background) == Some(true);
    let block = &mut session.blocks[index];
    block.text = label.clone();
    block.streaming = Some(patch.streaming);
    if agent_model.is_some() || block.agent_run.is_some() {
        let run = block.agent_run.get_or_insert_with(AgentRunMeta::default);
        run.name = agent_name;
        if let Some(model) = agent_model {
            run.model = Some(model.to_string());
        }
    }
    block.tool = Some(BlockTool {
        call_id: Some(patch.call_id.to_string()),
        title: Some(label),
        kind,
        status,
        detail,
        preview,
        background: background.then_some(true),
        extra: Extra::new(),
    });
    true
}

const MAX_TOOL_DETAIL_CHARS: usize = 8_000;

fn cap_tool_detail(value: Option<&str>) -> Option<String> {
    let text = js::trim(value?);
    if text.is_empty() {
        return None;
    }
    if js::len(text) <= MAX_TOOL_DETAIL_CHARS {
        return Some(text.to_string());
    }
    Some(format!(
        "{}\n…",
        js::slice_prefix(text, MAX_TOOL_DETAIL_CHARS)
    ))
}

/// `samePreview`. The TypeScript compared `lines` by reference; this compares
/// them by value, so a resent but identical diff counts as no change. Like
/// the TypeScript, it ignores `title`.
fn same_preview(a: Option<&ToolPreview>, b: Option<&ToolPreview>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.kind == b.kind
                && a.path == b.path
                && a.query == b.query
                && a.file_name == b.file_name
                && a.additions == b.additions
                && a.deletions == b.deletions
                && a.content_only == b.content_only
                && a.start_line == b.start_line
                && a.output == b.output
                && a.lines == b.lines
        }
        _ => false,
    }
}

fn fill_preview(
    preview: Option<ToolPreview>,
    kind: Option<&str>,
    title: Option<&str>,
) -> Option<ToolPreview> {
    match preview {
        Some(preview)
            if preview.content_only == Some(true)
                || preview.lines.as_ref().is_some_and(|lines| {
                    lines.iter().any(|line| {
                        matches!(
                            line.kind,
                            ToolPreviewLineKind::Add | ToolPreviewLineKind::Del
                        )
                    })
                }) =>
        {
            Some(preview)
        }
        Some(mut preview) => {
            preview.lines = None;
            Some(preview)
        }
        None if is_file_tool(kind, title, None) => Some(stub_file_preview(kind, title)),
        None => None,
    }
}

/// How much of a subagent's trail the parent keeps. A delegated run can be
/// thousands of calls long; the transcript only ever shows a window of it,
/// and an unbounded array would grow the saved session without bound.
const MAX_AGENT_STEPS: usize = 300;

const MAX_AGENT_STEP_CHARS: usize = 2_000;

/// `recordAgentStep`: mirror one subagent action onto its parent Agent tool
/// block. Steps merge by provider id, so a call that starts pending and later
/// completes stays one row instead of appearing twice.
fn record_agent_step(session: &mut Session, event: &HarnessEvent) -> bool {
    let HarnessEvent::AgentStep {
        call_id,
        step_id,
        kind,
        text,
        tool_kind,
        status,
        detail,
        preview,
        agent_name,
        agent_type,
    } = event
    else {
        return false;
    };
    let Some(index) = session
        .blocks
        .iter()
        .position(|block| tool_call_id(block) == Some(call_id.as_str()))
    else {
        return false;
    };
    let text = cap_agent_step_text(text);
    // A tool step earns a row on its label alone; prose with nothing in it
    // does not.
    if text.is_empty() && *kind != AgentStepKind::Tool {
        return false;
    }

    let prev = &session.blocks[index];
    let run = prev.agent_run.as_ref();
    let step = AgentStep {
        id: step_id.clone(),
        kind: *kind,
        text: text.clone(),
        tool_kind: nonempty(tool_kind.as_deref()).map(str::to_string),
        status: nonempty(status.as_deref()).map(str::to_string),
        detail: cap_tool_detail(detail.as_deref()),
        preview: preview.clone(),
        extra: Extra::new(),
    };

    let at = run.and_then(|run| run.steps.iter().position(|entry| entry.id == *step_id));
    let steps = match (run, at) {
        (Some(run), Some(at)) => {
            let existing = &run.steps[at];
            let mut merged = existing.clone();
            merged.id = step.id;
            merged.kind = step.kind;
            // A completion carries the result, not the request: keep the label
            // the call announced itself with rather than letting the result
            // rename it.
            if !text.is_empty() {
                merged.text = text;
            }
            if step.tool_kind.is_some() {
                merged.tool_kind = step.tool_kind;
            }
            if step.status.is_some() {
                merged.status = step.status;
            }
            if step.detail.is_some() {
                merged.detail = step.detail;
            }
            merged.preview = merge_tool_preview(preview.as_ref(), existing.preview.as_ref());
            let mut steps = run.steps.clone();
            steps[at] = merged;
            steps
        }
        _ => {
            let mut steps = run.map(|run| run.steps.clone()).unwrap_or_default();
            steps.push(step);
            if steps.len() > MAX_AGENT_STEPS {
                steps.drain(..steps.len() - MAX_AGENT_STEPS);
            }
            steps
        }
    };

    let name = nonempty(agent_name.as_deref())
        .or_else(|| run.and_then(|run| nonempty(Some(&run.name))))
        .or_else(|| nonempty(prev.tool.as_ref().and_then(|tool| tool.title.as_deref())))
        .or_else(|| nonempty(Some(&prev.text)))
        .unwrap_or("Subagent")
        .to_string();
    let next = AgentRunMeta {
        name,
        agent_type: agent_type
            .clone()
            .or_else(|| run.and_then(|run| run.agent_type.clone()))
            .filter(|agent_type| !agent_type.is_empty()),
        model: run
            .and_then(|run| run.model.clone())
            .filter(|model| !model.is_empty()),
        steps,
        extra: Extra::new(),
    };
    if run.is_some_and(|run| same_agent_run(run, &next)) {
        return false;
    }
    session.blocks[index].agent_run = Some(next);
    true
}

fn same_agent_run(a: &AgentRunMeta, b: &AgentRunMeta) -> bool {
    a.name == b.name
        && a.agent_type == b.agent_type
        && a.model == b.model
        && a.steps.len() == b.steps.len()
        && a.steps
            .iter()
            .zip(&b.steps)
            .all(|(a, b)| same_agent_step(a, b))
}

fn same_agent_step(a: &AgentStep, b: &AgentStep) -> bool {
    a.id == b.id
        && a.kind == b.kind
        && a.text == b.text
        && a.tool_kind == b.tool_kind
        && a.status == b.status
        && a.detail == b.detail
        && same_preview(a.preview.as_ref(), b.preview.as_ref())
}

fn cap_agent_step_text(value: &str) -> String {
    let text = js::trim(value);
    if js::len(text) <= MAX_AGENT_STEP_CHARS {
        return text.to_string();
    }
    format!("{}\u{2026}", js::slice_prefix(text, MAX_AGENT_STEP_CHARS))
}

fn find_tool_index(blocks: &[Block], call_id: &str, title: Option<&str>) -> Option<usize> {
    if !call_id.is_empty()
        && let Some(index) = blocks
            .iter()
            .position(|block| tool_call_id(block) == Some(call_id))
    {
        return Some(index);
    }
    let needle = normalize_label(title.unwrap_or(""));
    if needle.is_empty() {
        return None;
    }
    blocks.iter().position(|block| {
        block.role == BlockRole::Tool
            && block.approval.is_some()
            && nonempty(tool_call_id(block)).is_none()
            && normalize_label(block_label(block)) == needle
    })
}

/// `sealLastStream`: close the open assistant or reasoning block.
fn seal_last_stream(blocks: &mut [Block]) {
    if let Some(index) = last_open_index(blocks) {
        let last = &mut blocks[index];
        if last.is_streaming() && matches!(last.role, BlockRole::Assistant | BlockRole::Reasoning) {
            last.streaming = Some(false);
        }
    }
}

fn display_label(title: Option<&str>, kind: Option<&str>, prev: Option<&Block>) -> String {
    let prev_tool = prev.and_then(|prev| prev.tool.as_ref());
    let label = prefer_label(&[
        title,
        prev_tool.and_then(|tool| tool.title.as_deref()),
        prev.map(|prev| prev.text.as_str()),
    ]);
    if !label.is_empty() {
        return label;
    }
    kind_title(kind.or(prev_tool.and_then(|tool| tool.kind.as_deref())))
}

fn final_tool_label(
    cwd: &str,
    kind: Option<&str>,
    title: Option<&str>,
    preview: Option<&ToolPreview>,
) -> String {
    let path = match preview {
        Some(preview) => match nonempty(preview.path.as_deref()) {
            Some(path) => Some(display_path(path, Some(cwd))),
            None => preview.file_name.clone(),
        },
        None => None,
    };
    let composed = compose_tool_title(&ToolTitleInput {
        kind,
        title,
        path: path.as_deref(),
        query: preview.and_then(|preview| preview.query.as_deref()),
        preview_kind: preview.map(|preview| preview.kind),
        cwd: Some(cwd),
        ..ToolTitleInput::default()
    });
    if !composed.is_empty() {
        return composed;
    }
    let trimmed = title.map(js::trim).unwrap_or("");
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    kind_title(kind)
}

fn prefer_label(parts: &[Option<&str>]) -> String {
    let filled: Vec<&str> = parts
        .iter()
        .filter_map(|part| nonempty(part.map(js::trim)))
        .filter(|part| !is_call_id(part))
        .collect();
    // `compactLabel(part) === part`: one line of at most 240 characters.
    let compact = |part: &&str| !part.contains('\n') && js::len(part) <= 240;
    let strong: Vec<&str> = filled
        .iter()
        .copied()
        .filter(|part| !is_weak_tool_title(part))
        .collect();
    let compact_strong = longest_first(strong.iter().copied().filter(compact).collect());
    if let Some(first) = compact_strong.first() {
        return first.to_string();
    }
    // A long command is still more useful than an earlier "Shell" placeholder.
    if let Some(first) = strong.first() {
        return first.to_string();
    }
    let compact_any = longest_first(filled.iter().copied().filter(compact).collect());
    compact_any
        .first()
        .or(filled.first())
        .map(|part| part.to_string())
        .unwrap_or_default()
}

/// Longest first, with a stable sort as `Array.prototype.sort` is.
fn longest_first(mut parts: Vec<&str>) -> Vec<&str> {
    parts.sort_by_key(|part| std::cmp::Reverse(js::len(part)));
    parts
}

fn kind_title(kind: Option<&str>) -> String {
    let key = kind
        .map(|kind| js::trim(kind).to_lowercase())
        .unwrap_or_default();
    match key.as_str() {
        "read" => "Read".into(),
        "edit" => "Edit".into(),
        "delete" => "Delete".into(),
        "move" => "Move".into(),
        "search" => "Find".into(),
        "execute" | "shell" | "bash" => "Shell".into(),
        "skill" => "Skill".into(),
        "agent" | "task" | "subagent" => "Subagent".into(),
        "think" => "Think".into(),
        "fetch" => "Fetch".into(),
        "other" | "" => "Working".into(),
        _ => {
            let key = key.strip_prefix('_').unwrap_or(&key);
            js_regex!(r"[_-]+").replace_all(key, " ").into_owned()
        }
    }
}

/// `/^(call[-_]?|tool[-_])[a-z0-9_-]+$/i` or a UUID.
fn is_call_id(value: &str) -> bool {
    let text = js::trim(value).to_ascii_lowercase();
    let id_chars = |rest: &str| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    };
    // The optional `[-_]` after `call` is also in the id class, so it never
    // changes the answer.
    if let Some(rest) = text.strip_prefix("call")
        && id_chars(rest)
    {
        return true;
    }
    if let Some(rest) = text
        .strip_prefix("tool-")
        .or_else(|| text.strip_prefix("tool_"))
        && id_chars(rest)
    {
        return true;
    }
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn finish_role(session: &mut Session, role: BlockRole) {
    for block in &mut session.blocks {
        if block.role == role && block.is_streaming() {
            block.streaming = Some(false);
        }
    }
}

#[cfg(test)]
mod tests;
