//! Port of src/features/sessions/data/sessionStore.ts: what a session saves,
//! the block sanitizers, the per-session write queues, the fingerprint that
//! skips unchanged saves, and `recordToSession`.
//!
//! The sanitizers work on JSON values, as the TypeScript did on untyped
//! objects, so a malformed nested field drops that field and keeps the block.
//! Saving serializes each block and sanitizes the value; loading sanitizes
//! the stored value and then deserializes it into a `Block`.

use std::collections::{HashMap, HashSet};
use std::hash::Hasher;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use gpui::{BackgroundExecutor, Task};
use monocode_core::block::{Block, BlockRole};
use monocode_core::context_usage::ContextUsage;
use monocode_core::harness::{HARNESSES, HarnessId, RUNTIME_MODES, RuntimeMode};
use monocode_core::inbox::WorkItemKind;
use monocode_core::session::{LinkedWorkItem, Session};
use monocode_core::{Extra, ModelSettings};
use monocode_store::session_store::{
    InFlightSession, SessionRecord, SessionSearchHit, SessionSearchOptions,
    SessionSummary as StoredSummary, SessionUpsert,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::backend::SessionBackend;
use super::reducer::title_from_tool_input;
use super::util::project_path::{is_remote_project_path, normalize_project_path};

/// One task in an orchestration summary row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationSummaryTask {
    pub session_id: String,
    pub title: String,
    pub harness: HarnessId,
    pub model: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_input: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `OrchestrationSummary`: a lead's run as its history row shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationSummary {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<bool>,
    pub tasks: Vec<OrchestrationSummaryTask>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `SessionSummary`: one history row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_lead_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration: Option<OrchestrationSummary>,
    pub id: String,
    pub cwd: String,
    pub harness: HarnessId,
    pub model: String,
    pub runtime_mode: RuntimeMode,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_removed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_work_item: Option<LinkedWorkItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
}

impl SessionSummary {
    /// A row with every optional field unset.
    pub fn new(id: impl Into<String>, cwd: impl Into<String>, harness: HarnessId) -> Self {
        Self {
            orchestration_lead_id: None,
            orchestration: None,
            id: id.into(),
            cwd: cwd.into(),
            harness,
            model: String::new(),
            runtime_mode: RuntimeMode::Supervised,
            title: String::new(),
            provider_session_id: None,
            branch: None,
            worktree_cwd: None,
            worktree_removed: None,
            repo: None,
            additions: None,
            deletions: None,
            created_at: 0,
            updated_at: 0,
            archived: None,
            pinned: None,
            draft: None,
            linked_work_item: None,
            automation_id: None,
        }
    }
}

/// A search result list (`SessionSearchResult`).
pub type SessionSearchResult = monocode_store::session_store::SessionSearchResult;
pub type SearchHit = SessionSearchHit;

/// `InFlightRef`: one `{ sessionId, cwd }` entry of the quit snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InFlightRef {
    pub session_id: String,
    pub cwd: String,
}

impl From<InFlightSession> for InFlightRef {
    fn from(entry: InFlightSession) -> Self {
        Self {
            session_id: entry.session_id,
            cwd: entry.cwd,
        }
    }
}

/// `shouldPersistSession`: only real chats belong in project history. Blank
/// tabs stay ephemeral.
pub fn should_persist_session(session: &Session) -> bool {
    session.inbox_ask.is_none()
        && !is_remote_project_path(&session.cwd)
        && session.cwd != "~"
        && session
            .blocks
            .iter()
            .any(|block| block.role == BlockRole::User)
}

/// `isPersistableId`: matches Rust `validate_id`. A path here fails the whole
/// upsert.
pub fn is_persistable_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn persistable_meta(session: &Session) -> SessionUpsert {
    let linked_work_item = session
        .linked_work_item
        .as_ref()
        .and_then(|item| serde_json::to_value(item).ok())
        .and_then(|value| sanitize_linked_work_item(&value));
    let persistable = |value: &Option<String>| {
        value
            .as_ref()
            .filter(|value| !value.is_empty() && is_persistable_id(value))
            .cloned()
    };
    SessionUpsert {
        id: session.id.clone(),
        cwd: normalize_project_path(&session.cwd),
        harness: session.harness.as_str().to_string(),
        model: session.model.clone(),
        model_settings: serde_json::to_value(&session.model_settings).unwrap_or(json!({})),
        runtime_mode: session.runtime_mode.as_str().to_string(),
        title: session.title.clone(),
        provider_session_id: persistable(&session.provider_session_id),
        provider_account_id: persistable(&session.provider_account_id),
        blocks: Value::Array(Vec::new()),
        context_used: session.context.map(|context| context.used),
        context_window: session
            .context
            .and_then(|context| context.window)
            .filter(|window| *window != 0),
        branch: session.branch.clone().filter(|branch| !branch.is_empty()),
        worktree_cwd: session.worktree_cwd.clone().filter(|cwd| !cwd.is_empty()),
        worktree_removed: session.worktree_removed == Some(true),
        linked_work_item: linked_work_item.and_then(|item| serde_json::to_value(item).ok()),
        automation_id: persistable(&session.automation_id),
    }
}

/// `sanitizeLinkedWorkItem`: a GitHub issue or pull request with a
/// canonical URL, or `None` for anything malformed.
pub fn sanitize_linked_work_item(value: &Value) -> Option<LinkedWorkItem> {
    let item = value.as_object()?;
    let kind = match item.get("kind").and_then(Value::as_str) {
        Some("issue") => WorkItemKind::Issue,
        Some("pr") => WorkItemKind::Pr,
        _ => return None,
    };
    let repo = item
        .get("repo")
        .and_then(Value::as_str)
        .map(monocode_core::js::trim)
        .unwrap_or("");
    let number = safe_integer(item.get("number")?)?;
    if !valid_repo(repo) || number <= 0 {
        return None;
    }
    let path = if kind == WorkItemKind::Pr {
        "pull"
    } else {
        "issues"
    };
    Some(LinkedWorkItem {
        kind,
        repo: repo.to_string(),
        number,
        url: format!("https://github.com/{repo}/{path}/{number}"),
        extra: Extra::new(),
    })
}

/// `^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$`.
fn valid_repo(repo: &str) -> bool {
    let part = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    };
    match repo.split_once('/') {
        Some((owner, name)) => part(owner) && part(name),
        None => false,
    }
}

/// `Number.isSafeInteger`.
fn safe_integer(value: &Value) -> Option<i64> {
    const MAX_SAFE: i64 = 9_007_199_254_740_991;
    if let Some(n) = value.as_i64() {
        return (n.abs() <= MAX_SAFE).then_some(n);
    }
    let n = value.as_f64()?;
    (n.is_finite() && n.fract() == 0.0 && n.abs() <= MAX_SAFE as f64).then_some(n as i64)
}

/// A finite JSON number.
fn finite_number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64).filter(|n| n.is_finite())
}

/// `sanitizeSessionForPersist`: the upsert payload for this session.
pub fn sanitize_session_for_persist(session: &Session) -> SessionUpsert {
    let first_user = session
        .blocks
        .iter()
        .position(|block| block.role == BlockRole::User);
    let blocks = session
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            let mut value = serde_json::to_value(block).ok()?;
            if Some(index) == first_user
                && let Some(lead) = session.orchestration_lead_id.as_ref()
                && let Some(object) = value.as_object_mut()
            {
                object.insert("orchestrationLeadId".into(), Value::String(lead.clone()));
            }
            sanitize_block(&value, false)
        })
        .collect();
    SessionUpsert {
        blocks: Value::Array(blocks),
        ..persistable_meta(session)
    }
}

/// JavaScript truthiness of a JSON value.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// A plain object (not an array).
fn record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

fn trimmed_str<'a>(rec: &'a Map<String, Value>, key: &str) -> &'a str {
    rec.get(key)
        .and_then(Value::as_str)
        .map(monocode_core::js::trim)
        .unwrap_or("")
}

/// `sanitizeBlock`. `hydrate` marks a load from disk rather than a save.
pub fn sanitize_block(block: &Value, hydrate: bool) -> Option<Value> {
    let block = block.as_object()?;
    let role = block.get("role").and_then(Value::as_str).unwrap_or("");
    let is_user = role == "user";
    let text = block.get("text").and_then(Value::as_str).unwrap_or("");
    let mut next = Map::new();
    for key in ["id", "role", "text"] {
        if let Some(value) = block.get(key) {
            next.insert(key.into(), value.clone());
        }
    }
    if let Some(attachments) = block.get("attachments").and_then(Value::as_array)
        && !attachments.is_empty()
    {
        let saved = attachments.iter().map(persistable_attachment).collect();
        next.insert("attachments".into(), Value::Array(saved));
    }
    let image = sanitize_generated_image(block.get("image"));
    if role == "image" && image.is_none() {
        return None;
    }
    if let Some(image) = image {
        next.insert("image".into(), image);
    }
    for key in ["startedAt", "durationMs"] {
        if let Some(value) = block.get(key).filter(|value| !value.is_null()) {
            next.insert(key.into(), value.clone());
        }
    }
    if is_user && let Some(turn_model) = sanitize_turn_model(block.get("turnModel")) {
        next.insert("turnModel".into(), turn_model);
    }
    if is_user && truthy(block.get("draft")) {
        next.insert("draft".into(), Value::Bool(true));
    }
    if is_user && truthy(block.get("monocode")) {
        next.insert("monocode".into(), Value::Bool(true));
    }
    if is_user
        && let Some(intent @ ("plan" | "orchestrate")) = block.get("intent").and_then(Value::as_str)
    {
        next.insert("intent".into(), Value::String(intent.into()));
    }
    if is_user
        && let Some(id) = block.get("appRequestId").and_then(Value::as_str)
        && (1..=512).contains(&id.len())
        && is_persistable_id(id)
    {
        next.insert("appRequestId".into(), Value::String(id.into()));
    }
    for key in ["providerTurnId", "orchestrationLeadId"] {
        if is_user
            && let Some(id) = block.get(key).and_then(Value::as_str)
            && is_persistable_id(id)
        {
            next.insert(key.into(), Value::String(id.into()));
        }
    }
    // Without this the transcript would show the app's orchestration turns
    // as the user's own after a reload.
    if is_user && truthy(block.get("internal")) {
        next.insert("internal".into(), Value::Bool(true));
    }
    if is_user && let Some(metrics) = sanitize_turn_metrics(block.get("turnMetrics")) {
        next.insert("turnMetrics".into(), metrics);
    }
    if truthy(block.get("tool")) {
        next.insert("tool".into(), block["tool"].clone());
    }
    let approval = block.get("approval");
    let decided = approval.and_then(|approval| approval.get("decided"));
    if truthy(decided) {
        let mut saved = Map::new();
        if let Some(request_id) = approval.and_then(|approval| approval.get("requestId")) {
            saved.insert("requestId".into(), request_id.clone());
        }
        saved.insert("decided".into(), decided.cloned().unwrap_or(Value::Null));
        next.insert("approval".into(), Value::Object(saved));
    } else if truthy(approval) && role == "approval" {
        // Drop stale live approval prompts; request ids don't survive restarts.
        return None;
    }
    if let Some(agent_run) = sanitize_agent_run(block.get("agentRun")) {
        next.insert("agentRun".into(), agent_run);
    }
    match sanitize_task_list(block.get("taskList")) {
        Some(task_list) => {
            next.insert("taskList".into(), task_list);
        }
        None if role == "tasks" => return None,
        None => {}
    }
    let plan = sanitize_plan(block.get("plan"), text);
    if truthy(block.get("orchestration")) {
        next.insert(
            "orchestration".into(),
            restore_orchestration_proposal(&block["orchestration"]),
        );
    }
    match plan {
        Some(plan) => {
            next.insert("plan".into(), plan);
        }
        None if role == "plan" => {
            next.insert(
                "plan".into(),
                json!({ "status": "ready", "originalText": text }),
            );
        }
        None => {}
    }
    match sanitize_handoff(block.get("handoff")) {
        Some(handoff) => {
            next.insert("handoff".into(), handoff);
        }
        None if role == "handoff" => return None,
        None => {}
    }
    if let Some(second_opinion) = sanitize_second_opinion(block.get("secondOpinion")) {
        next.insert("secondOpinion".into(), second_opinion);
    }
    if is_user && let Some(threads) = sanitize_btw_threads(block.get("btwThreads"), hydrate) {
        next.insert("btwThreads".into(), threads);
    }
    if let Some(note_card) = sanitize_note_card(block.get("noteCard")) {
        next.insert("noteCard".into(), note_card);
    }
    if is_user
        && let Some(context) = block.get("ciContext").and_then(Value::as_str)
        && !context.is_empty()
    {
        next.insert("ciContext".into(), Value::String(context.into()));
    }
    // Interjection chrome survives restarts only on system blocks; a
    // malformed payload keeps the ordinary system row rather than losing its body.
    if role == "system" {
        if let Some(interjection) = sanitize_interjection(block.get("interjection")) {
            next.insert("interjection".into(), interjection);
        }
        if let Some(notice @ ("error" | "interrupt")) = block.get("notice").and_then(Value::as_str)
        {
            next.insert("notice".into(), Value::String(notice.into()));
        }
    }
    Some(Value::Object(next))
}

/// `persistableAttachment`: the fields worth saving with the transcript.
fn persistable_attachment(file: &Value) -> Value {
    let mut next = Map::new();
    for key in ["id", "name", "mimeType", "kind", "size"] {
        if let Some(value) = file.get(key) {
            next.insert(key.into(), value.clone());
        }
    }
    if truthy(file.get("path")) {
        next.insert("path".into(), file["path"].clone());
    }
    Value::Object(next)
}

/// `restoreOrchestrationProposal`: a reload must never turn a
/// half-generated card into an executable plan.
fn restore_orchestration_proposal(value: &Value) -> Value {
    let mut next = value.clone();
    match value.get("status").and_then(Value::as_str) {
        Some("planning") => {
            if let Some(object) = next.as_object_mut() {
                object.insert("status".into(), Value::String("invalid".into()));
                object.insert(
                    "error".into(),
                    Value::String(
                        "Planning was interrupted. Generate the assignments again.".into(),
                    ),
                );
            }
        }
        Some("starting") => {
            if let Some(object) = next.as_object_mut() {
                object.insert("status".into(), Value::String("ready".into()));
            }
        }
        _ => {}
    }
    next
}

/// `sanitizeNestedId`.
fn sanitize_nested_id(value: Option<&Value>) -> Option<String> {
    let id = monocode_core::js::trim(value?.as_str()?);
    if id.is_empty() || monocode_core::js::len(id) > 256 || id.chars().any(|c| (c as u32) < 0x20) {
        return None;
    }
    Some(id.to_string())
}

fn sanitize_string_record(value: Option<&Value>) -> Option<Value> {
    let rec = record(value)?;
    let mut next = Map::new();
    for (key, raw) in rec {
        let key = sanitize_nested_id(Some(&Value::String(key.clone())));
        let value = sanitize_nested_id(Some(raw));
        if let (Some(key), Some(value)) = (key, value) {
            next.insert(key, Value::String(value));
        }
    }
    (!next.is_empty()).then_some(Value::Object(next))
}

fn sanitize_timestamp(value: Option<&Value>) -> Option<Value> {
    finite_number(value)
        .filter(|n| *n >= 0.0)
        .map(|_| value.cloned().unwrap_or(Value::Null))
}

fn sanitize_btw_message(value: &Value) -> Option<Value> {
    let rec = value.as_object()?;
    let id = sanitize_nested_id(rec.get("id"))?;
    let text = rec.get("text").and_then(Value::as_str)?;
    let created_at = sanitize_timestamp(rec.get("createdAt"))?;
    let role = match rec.get("role").and_then(Value::as_str) {
        Some(role @ ("user" | "assistant")) => role,
        _ => return None,
    };
    let blocks: Vec<Value> = rec
        .get("blocks")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| sanitize_block(block, false))
                .collect()
        })
        .unwrap_or_default();
    let mut next = Map::new();
    next.insert("id".into(), Value::String(id));
    next.insert("role".into(), Value::String(role.into()));
    next.insert("text".into(), Value::String(text.into()));
    next.insert("createdAt".into(), created_at);
    if !blocks.is_empty() {
        next.insert("blocks".into(), Value::Array(blocks));
    }
    Some(Value::Object(next))
}

fn sanitize_btw_threads(value: Option<&Value>, hydrate: bool) -> Option<Value> {
    let entries = value?.as_array()?;
    let threads: Vec<Value> = entries
        .iter()
        .filter_map(|entry| {
            let rec = entry.as_object()?;
            let id = sanitize_nested_id(rec.get("id"));
            let source_end_block_id = sanitize_nested_id(rec.get("sourceEndBlockId"));
            let created_at = sanitize_timestamp(rec.get("createdAt"));
            let updated_at = sanitize_timestamp(rec.get("updatedAt"));
            let status = match rec.get("status").and_then(Value::as_str) {
                Some(status @ ("running" | "ready" | "error")) => Some(status),
                _ => None,
            };
            let messages: Vec<Value> = rec
                .get("messages")
                .and_then(Value::as_array)
                .map(|messages| messages.iter().filter_map(sanitize_btw_message).collect())
                .unwrap_or_default();
            let (
                Some(id),
                Some(source_end_block_id),
                Some(created_at),
                Some(updated_at),
                Some(status),
            ) = (id, source_end_block_id, created_at, updated_at, status)
            else {
                return None;
            };
            if messages.is_empty() {
                return None;
            }
            let error = trimmed_str(rec, "error");
            let model = trimmed_str(rec, "model");
            let harness = rec
                .get("harness")
                .and_then(Value::as_str)
                .filter(|harness| !monocode_core::js::trim(harness).is_empty())
                .and_then(HarnessId::parse)
                .filter(|harness| HARNESSES.contains(harness));
            let model_settings = sanitize_string_record(rec.get("modelSettings"));
            let provider_thread_id = sanitize_nested_id(rec.get("providerThreadId"));
            let interrupted = hydrate && status == "running";
            let mut next = Map::new();
            next.insert("id".into(), Value::String(id));
            next.insert(
                "sourceEndBlockId".into(),
                Value::String(source_end_block_id),
            );
            next.insert("createdAt".into(), created_at);
            next.insert("updatedAt".into(), updated_at);
            next.insert(
                "status".into(),
                Value::String(if interrupted { "error" } else { status }.into()),
            );
            next.insert("messages".into(), Value::Array(messages));
            if let Some(harness) = harness {
                next.insert("harness".into(), Value::String(harness.as_str().into()));
            }
            if !model.is_empty() {
                next.insert("model".into(), Value::String(model.into()));
            }
            if let Some(model_settings) = model_settings {
                next.insert("modelSettings".into(), model_settings);
            }
            if let Some(provider_thread_id) = provider_thread_id {
                next.insert("providerThreadId".into(), Value::String(provider_thread_id));
            }
            if interrupted {
                let message = if error.is_empty() {
                    "This by-the-way request was interrupted before reload."
                } else {
                    error
                };
                next.insert("error".into(), Value::String(message.into()));
            } else if !error.is_empty() {
                next.insert("error".into(), Value::String(error.into()));
            }
            Some(Value::Object(next))
        })
        .collect();
    (!threads.is_empty()).then_some(Value::Array(threads))
}

fn sanitize_generated_image(value: Option<&Value>) -> Option<Value> {
    let rec = record(value)?;
    let path = trimmed_str(rec, "path");
    let name = trimmed_str(rec, "name");
    let mime_type = trimmed_str(rec, "mimeType");
    let size = rec.get("size").and_then(safe_integer)?;
    if path.is_empty() || name.is_empty() || !mime_type.starts_with("image/") || size <= 0 {
        return None;
    }
    let alt = trimmed_str(rec, "alt");
    let mut next = json!({
        "path": path,
        "name": name,
        "mimeType": mime_type,
        "size": size,
    });
    if !alt.is_empty() {
        next["alt"] = Value::String(alt.into());
    }
    Some(next)
}

fn sanitize_turn_metrics(value: Option<&Value>) -> Option<Value> {
    let rec = record(value)?;
    let mut next = Map::new();
    for key in [
        "inputTokens",
        "outputTokens",
        "cacheReadTokens",
        "cacheWriteTokens",
        "cacheHitPercent",
    ] {
        if finite_number(rec.get(key)).is_some_and(|n| n >= 0.0) {
            next.insert(key.into(), rec[key].clone());
        }
    }
    (!next.is_empty()).then_some(Value::Object(next))
}

fn sanitize_turn_model(value: Option<&Value>) -> Option<Value> {
    let rec = record(value)?;
    let harness = rec
        .get("harness")
        .and_then(Value::as_str)
        .and_then(HarnessId::parse)?;
    let id = trimmed_str(rec, "id");
    let name = trimmed_str(rec, "name");
    if id.is_empty() || name.is_empty() {
        return None;
    }
    Some(json!({ "harness": harness.as_str(), "id": id, "name": name }))
}

fn sanitize_interjection(value: Option<&Value>) -> Option<Value> {
    let rec = record(value)?;
    let custom_type = trimmed_str(rec, "customType");
    if custom_type.is_empty() {
        return None;
    }
    let mut next = json!({ "customType": custom_type });
    if let Some(severity @ ("nit" | "concern" | "blocker")) =
        rec.get("severity").and_then(Value::as_str)
    {
        next["severity"] = Value::String(severity.into());
    }
    Some(next)
}

fn sanitize_plan(value: Option<&Value>, text: &str) -> Option<Value> {
    let rec = record(value)?;
    let status = match rec.get("status").and_then(Value::as_str) {
        Some(status @ ("streaming" | "ready" | "building" | "built")) => status,
        _ => return None,
    };
    let key = trimmed_str(rec, "key");
    let original_text = rec
        .get("originalText")
        .and_then(Value::as_str)
        .unwrap_or(text);
    let approved_text = rec
        .get("approvedText")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut next = Map::new();
    if !key.is_empty() {
        next.insert("key".into(), Value::String(key.into()));
    }
    // A restarted app cannot still be executing this approval.
    let status = if matches!(status, "streaming" | "building") {
        "ready"
    } else {
        status
    };
    next.insert("status".into(), Value::String(status.into()));
    if !original_text.is_empty() {
        next.insert("originalText".into(), Value::String(original_text.into()));
    }
    if !approved_text.is_empty() {
        next.insert("approvedText".into(), Value::String(approved_text.into()));
    }
    if rec.get("edited") == Some(&Value::Bool(true)) {
        next.insert("edited".into(), Value::Bool(true));
    }
    Some(Value::Object(next))
}

/// `PERSISTED_AGENT_STEPS`: how much of a delegated run's trail a saved
/// session keeps. Reopening a session is for reading what the subagent
/// concluded, not for replaying every call it made.
pub const PERSISTED_AGENT_STEPS: usize = 100;

fn sanitize_agent_run(value: Option<&Value>) -> Option<Value> {
    let rec = record(value)?;
    let steps = rec.get("steps")?.as_array()?;
    let steps: Vec<Value> = steps
        .iter()
        .filter_map(|entry| {
            let row = entry.as_object()?;
            let id = row.get("id").and_then(Value::as_str).unwrap_or("");
            let kind = match row.get("kind").and_then(Value::as_str) {
                Some(kind @ ("tool" | "message" | "reasoning")) => kind,
                _ => return None,
            };
            if id.is_empty() {
                return None;
            }
            let text = row.get("text").and_then(Value::as_str).unwrap_or("");
            let mut next = json!({ "id": id, "kind": kind, "text": text });
            for key in ["toolKind", "status", "detail"] {
                if let Some(value) = row.get(key).filter(|value| value.is_string()) {
                    next[key] = value.clone();
                }
            }
            if let Some(preview) = row
                .get("preview")
                .filter(|value| value.is_object() || value.is_array())
            {
                next["preview"] = preview.clone();
            }
            Some(next)
        })
        .collect();
    let name = trimmed_str(rec, "name");
    if name.is_empty() && steps.is_empty() {
        return None;
    }
    let mut next = Map::new();
    next.insert(
        "name".into(),
        Value::String(if name.is_empty() { "Subagent" } else { name }.into()),
    );
    let model = trimmed_str(rec, "model");
    if !model.is_empty() {
        next.insert("model".into(), Value::String(model.into()));
    }
    let agent_type = trimmed_str(rec, "agentType");
    if !agent_type.is_empty() {
        next.insert("agentType".into(), Value::String(agent_type.into()));
    }
    let skip = steps.len().saturating_sub(PERSISTED_AGENT_STEPS);
    next.insert(
        "steps".into(),
        Value::Array(steps.into_iter().skip(skip).collect()),
    );
    Some(Value::Object(next))
}

fn sanitize_task_list(value: Option<&Value>) -> Option<Value> {
    let rec = record(value)?;
    let items: Vec<Value> = rec
        .get("items")?
        .as_array()?
        .iter()
        .filter_map(|item| {
            let row = item.as_object()?;
            let text = trimmed_str(row, "text");
            let status = match row.get("status").and_then(Value::as_str) {
                Some(status @ ("pending" | "in_progress" | "completed" | "cancelled")) => status,
                _ => return None,
            };
            if text.is_empty() {
                return None;
            }
            let id = match row.get("id") {
                Some(Value::String(id)) => monocode_core::js::trim(id).to_string(),
                Some(Value::Number(n)) if n.as_f64().is_some_and(f64::is_finite) => {
                    monocode_core::js::number_to_string(n.as_f64().unwrap_or(0.0))
                }
                _ => String::new(),
            };
            let mut next = Map::new();
            if !id.is_empty() {
                next.insert("id".into(), Value::String(id));
            }
            next.insert("text".into(), Value::String(text.into()));
            next.insert("status".into(), Value::String(status.into()));
            Some(Value::Object(next))
        })
        .collect();
    if items.is_empty() {
        return None;
    }
    let mut next = Map::new();
    for key in ["key", "providerSessionId", "explanation"] {
        let value = trimmed_str(rec, key);
        if !value.is_empty() {
            next.insert(key.into(), Value::String(value.into()));
        }
    }
    next.insert("items".into(), Value::Array(items));
    Some(Value::Object(next))
}

fn harness_name(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|name| HarnessId::parse(name).is_some())
}

fn sanitize_handoff(value: Option<&Value>) -> Option<Value> {
    if !truthy(value) {
        return None;
    }
    let value = value?;
    let from = harness_name(value.get("from"))?;
    let to = harness_name(value.get("to"))?;
    let status = value.get("status").and_then(Value::as_str)?;
    if !matches!(status, "preparing" | "ready") {
        return None;
    }
    let interrupted = status == "preparing";
    Some(json!({
        "from": from,
        "to": to,
        "status": "ready",
        "pending": interrupted || truthy(value.get("pending")),
    }))
}

fn sanitize_second_opinion(value: Option<&Value>) -> Option<Value> {
    if !truthy(value) {
        return None;
    }
    let value = value?;
    let from = harness_name(value.get("from"))?;
    let to = harness_name(value.get("to"))?;
    let request = value
        .get("request")
        .and_then(Value::as_str)
        .map(|request| {
            monocode_core::js::slice_prefix(monocode_core::js::trim(request), 240).to_string()
        })
        .unwrap_or_default();
    let files = finite_number(value.get("files"))
        .map(|files| monocode_core::js::round(files).max(0.0))
        .unwrap_or(0.0);
    let mut next = json!({ "from": from, "to": to });
    if !request.is_empty() {
        next["request"] = Value::String(request);
    }
    if files > 0.0 {
        next["files"] = json!(files as i64);
    }
    if value.get("kind").and_then(Value::as_str) == Some("handoff") {
        next["kind"] = Value::String("handoff".into());
    }
    Some(next)
}

fn sanitize_note_card(value: Option<&Value>) -> Option<Value> {
    if !truthy(value) {
        return None;
    }
    let rec = value?.as_object()?;
    let id = trimmed_str(rec, "id");
    if id.is_empty() {
        return None;
    }
    let mut next = json!({
        "id": id,
        "slug": trimmed_str(rec, "slug"),
        "title": trimmed_str(rec, "title"),
    });
    let source_cwd = trimmed_str(rec, "sourceCwd");
    if !source_cwd.is_empty() {
        next["sourceCwd"] = Value::String(source_cwd.into());
    }
    Some(next)
}

/// A `Hasher` fed by `serde_json::to_writer`, so hashing a transcript does
/// not allocate its JSON.
struct HashWriter(std::collections::hash_map::DefaultHasher);

impl std::io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// `persistFingerprint`: equal fingerprints mean an upsert would write the
/// same row.
///
/// The TypeScript gave each block object a token and relied on blocks being
/// replaced, never mutated. Rust blocks have no identity, so this hashes the
/// serialized blocks instead. It runs only when a save is considered, and it
/// is exact where the token version could report a change for an equal copy.
pub fn persist_fingerprint(session: &Session) -> String {
    let meta = serde_json::to_string(&persistable_meta(session)).unwrap_or_default();
    let mut hasher = HashWriter(Default::default());
    let _ = serde_json::to_writer(&mut hasher, &session.blocks);
    format!(
        "{meta}|{}|{:016x}",
        session.orchestration_lead_id.as_deref().unwrap_or(""),
        hasher.0.finish()
    )
}

/// `backfillClaudeShellCommands`: restore Bash commands that older builds
/// saved as a bare "Shell" row. `None` when nothing matched.
pub fn backfill_claude_shell_commands(
    blocks: &[Block],
    commands: &HashMap<String, String>,
) -> Option<Vec<Block>> {
    let mut changed = false;
    let repaired = blocks
        .iter()
        .map(|block| {
            let command = block
                .tool
                .as_ref()
                .and_then(|tool| tool.call_id.as_ref())
                .and_then(|call_id| commands.get(call_id))
                .map(|command| monocode_core::js::trim(command))
                .filter(|command| !command.is_empty());
            let placeholder = block.role == BlockRole::Tool
                && block.tool.as_ref().and_then(|tool| tool.kind.as_deref()) == Some("execute")
                && monocode_core::js::trim(&block.text) == "Shell";
            let (Some(command), true) = (command, placeholder) else {
                return block.clone();
            };
            changed = true;
            let title = title_from_tool_input("Bash", "execute", &json!({ "command": command }));
            let mut next = block.clone();
            next.text = title.clone();
            if let Some(tool) = next.tool.as_mut() {
                tool.title = Some(title);
            }
            next
        })
        .collect();
    changed.then_some(repaired)
}

/// Claude tool calls whose stored row is the bare "Shell" placeholder.
pub fn claude_shell_placeholder_ids(session: &Session) -> Vec<String> {
    session
        .blocks
        .iter()
        .filter(|block| {
            block.role == BlockRole::Tool
                && block.tool.as_ref().and_then(|tool| tool.kind.as_deref()) == Some("execute")
                && monocode_core::js::trim(&block.text) == "Shell"
        })
        .filter_map(|block| block.tool.as_ref()?.call_id.clone())
        .filter(|call_id| !call_id.is_empty())
        .collect()
}

/// `asHarness`: unknown harnesses read as Cursor.
fn as_harness(value: &str) -> HarnessId {
    HarnessId::parse(value).unwrap_or(HarnessId::Cursor)
}

/// `asRuntimeMode`: unknown modes read as supervised.
fn as_runtime_mode(value: &str) -> RuntimeMode {
    RuntimeMode::parse(value)
        .filter(|mode| RUNTIME_MODES.contains(mode))
        .unwrap_or(RuntimeMode::Supervised)
}

/// `normalizeSummary`.
pub fn normalize_summary(summary: StoredSummary) -> SessionSummary {
    let linked_work_item = summary
        .linked_work_item
        .as_ref()
        .and_then(sanitize_linked_work_item);
    SessionSummary {
        orchestration_lead_id: summary.orchestration_lead_id,
        orchestration: summary
            .orchestration
            .and_then(|value| serde_json::from_value(value).ok()),
        id: summary.id,
        cwd: summary.cwd,
        harness: as_harness(&summary.harness),
        model: summary.model,
        runtime_mode: as_runtime_mode(&summary.runtime_mode),
        title: summary.title,
        provider_session_id: summary.provider_session_id.filter(|id| !id.is_empty()),
        branch: summary.branch.filter(|branch| !branch.is_empty()),
        worktree_cwd: summary.worktree_cwd,
        worktree_removed: Some(summary.worktree_removed),
        repo: summary.repo.filter(|repo| !repo.is_empty()),
        additions: Some(summary.additions),
        deletions: Some(summary.deletions),
        created_at: summary.created_at,
        updated_at: summary.updated_at,
        archived: summary.archived.then_some(true),
        pinned: summary.pinned.then_some(true),
        draft: summary.draft.then_some(true),
        linked_work_item,
        automation_id: summary.automation_id.filter(|id| is_persistable_id(id)),
    }
}

/// `recordToSession`: a stored row as a live session, with its blocks
/// sanitized for hydration.
pub fn record_to_session(record: SessionRecord) -> Session {
    let blocks: Vec<Block> = record
        .blocks
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| sanitize_block(block, true))
                // TODO(port): the TypeScript kept blocks whose role it did not
                // know. A `Block` needs a known role, so those are dropped.
                .filter_map(|block| serde_json::from_value::<Block>(block).ok())
                .collect()
        })
        .unwrap_or_default();
    let linked_work_item = record
        .linked_work_item
        .as_ref()
        .and_then(sanitize_linked_work_item);
    let mut session = Session::blank(
        record.id.clone(),
        as_harness(&record.harness),
        record.model.clone(),
        record.cwd.clone(),
    );
    // TODO(port): non-string setting values survived in TypeScript; the
    // typed settings map keeps string values only.
    session.model_settings = record
        .model_settings
        .as_object()
        .map(|settings| {
            settings
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect::<ModelSettings>()
        })
        .unwrap_or_default();
    session.runtime_mode = as_runtime_mode(&record.runtime_mode);
    session.title = record.title.clone();
    session.busy = Some(false);
    session.orchestration_lead_id = record.orchestration_lead_id.clone().or_else(|| {
        blocks
            .iter()
            .filter_map(|block| block.orchestration_lead_id.as_ref())
            .find(|lead| !lead.is_empty() && **lead != record.id)
            .cloned()
    });
    session.provider_session_id = record.provider_session_id.filter(|id| !id.is_empty());
    session.provider_account_id = record.provider_account_id.filter(|id| !id.is_empty());
    session.branch = record.branch.filter(|branch| !branch.is_empty());
    session.worktree_cwd = record.worktree_cwd.filter(|cwd| !cwd.is_empty());
    session.worktree_removed = record.worktree_removed.then_some(true);
    session.linked_work_item = linked_work_item;
    session.automation_id = record.automation_id.filter(|id| is_persistable_id(id));
    session.context = context_from_record(record.context_used, record.context_window);
    session.blocks = blocks;
    session
}

/// `contextFromRecord`: the last reading from a stored session. The harness
/// re-reports on the next turn, so this only has to last until then.
fn context_from_record(used: Option<i64>, window: Option<i64>) -> Option<ContextUsage> {
    let used = used.filter(|used| *used > 0)?;
    Some(ContextUsage {
        used,
        window: window.filter(|window| *window > 0),
    })
}

type Tail = Shared<BoxFuture<'static, ()>>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum QueueKey {
    Session(String),
    InFlight,
    Workspace,
}

#[derive(Default)]
struct WriterState {
    queues: HashMap<QueueKey, (u64, Tail)>,
    lead_by_id: HashMap<String, String>,
    deleted: HashSet<String>,
    next_id: u64,
}

/// Ordered writes to the session store.
///
/// `session_upsert` runs off the main thread, so two writes for the same
/// session could otherwise land in either order and let an older transcript
/// overwrite a newer one. Writes chain per session; different sessions still
/// write concurrently. The in-flight snapshot and the workspace snapshot each
/// have one chain of their own.
#[derive(Clone)]
pub struct SessionWriter {
    backend: Arc<dyn SessionBackend>,
    executor: BackgroundExecutor,
    state: Arc<Mutex<WriterState>>,
}

impl SessionWriter {
    pub fn new(backend: Arc<dyn SessionBackend>, executor: BackgroundExecutor) -> Self {
        Self {
            backend,
            executor,
            state: Arc::default(),
        }
    }

    pub fn backend(&self) -> &Arc<dyn SessionBackend> {
        &self.backend
    }

    pub fn executor(&self) -> &BackgroundExecutor {
        &self.executor
    }

    fn enqueue<T: Send + 'static>(
        &self,
        key: QueueKey,
        operation: impl FnOnce() -> BoxFuture<'static, Result<T, String>> + Send + 'static,
    ) -> Task<Result<T, String>> {
        let (done, finished) = oneshot::channel::<()>();
        let tail: Tail = finished.map(|_| ()).boxed().shared();
        let (previous, id) = {
            let mut state = self.state.lock();
            let id = state.next_id;
            state.next_id += 1;
            let previous = state
                .queues
                .insert(key.clone(), (id, tail))
                .map(|(_, previous)| previous);
            (previous, id)
        };
        let state = self.state.clone();
        self.executor.spawn(async move {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = operation().await;
            drop(done);
            let mut state = state.lock();
            if state
                .queues
                .get(&key)
                .is_some_and(|(queued, _)| *queued == id)
            {
                state.queues.remove(&key);
                if let QueueKey::Session(session_id) = &key {
                    state.lead_by_id.remove(session_id);
                }
            }
            result
        })
    }

    /// The session was deleted in this process (`deletedSessionIds`).
    pub fn is_deleted(&self, session_id: &str) -> bool {
        self.state.lock().deleted.contains(session_id)
    }

    /// `upsertSession`. `Ok(None)` when the session does not persist or was
    /// deleted.
    pub fn upsert_session(
        &self,
        session: &Session,
    ) -> Task<Result<Option<SessionSummary>, String>> {
        if !should_persist_session(session) || self.is_deleted(&session.id) {
            return Task::ready(Ok(None));
        }
        let payload = sanitize_session_for_persist(session);
        {
            let mut state = self.state.lock();
            match session.orchestration_lead_id.as_ref() {
                Some(lead) => state.lead_by_id.insert(session.id.clone(), lead.clone()),
                None => state.lead_by_id.remove(&session.id),
            };
        }
        let backend = self.backend.clone();
        let state = self.state.clone();
        let session_id = session.id.clone();
        self.enqueue(QueueKey::Session(session.id.clone()), move || {
            async move {
                let mut payload = payload;
                {
                    let state = state.lock();
                    if state.deleted.contains(&session_id) {
                        return Ok(None);
                    }
                    if let Some(blocks) = payload.blocks.as_array_mut() {
                        for block in blocks {
                            let lead = block.get("orchestrationLeadId").and_then(Value::as_str);
                            if lead.is_some_and(|lead| state.deleted.contains(lead))
                                && let Some(object) = block.as_object_mut()
                            {
                                object.remove("orchestrationLeadId");
                            }
                        }
                    }
                }
                backend
                    .upsert(payload)
                    .await
                    .map(|summary| Some(normalize_summary(summary)))
            }
            .boxed()
        })
    }

    /// `session_get` without the load-time repairs. `getSession` with the
    /// repairs is `Sessions::get_stored`.
    pub fn get_record(&self, session_id: &str) -> Task<Result<Option<SessionRecord>, String>> {
        let future = self.backend.get(session_id.to_string());
        self.executor.spawn(future)
    }

    /// `listSessionsByProject`.
    pub fn list_sessions_by_project(&self, cwd: &str) -> Task<Result<Vec<SessionSummary>, String>> {
        if cwd.is_empty() || cwd == "~" {
            return Task::ready(Ok(Vec::new()));
        }
        let future = self.backend.list_by_project(normalize_project_path(cwd));
        self.executor
            .spawn(async move { Ok(future.await?.into_iter().map(normalize_summary).collect()) })
    }

    /// `rebaseProjectSessions`.
    pub fn rebase_project_sessions(
        &self,
        from_cwd: &str,
        to_cwd: &str,
    ) -> Task<Result<(), String>> {
        let future = self.backend.rebase_project(
            normalize_project_path(from_cwd),
            normalize_project_path(to_cwd),
        );
        self.executor.spawn(future)
    }

    /// `listLinkedSessions`.
    pub fn list_linked_sessions(&self) -> Task<Result<Vec<SessionSummary>, String>> {
        let future = self.backend.list_linked();
        self.executor
            .spawn(async move { Ok(future.await?.into_iter().map(normalize_summary).collect()) })
    }

    /// `searchSessions`.
    pub fn search_sessions(
        &self,
        query: &str,
        search_owner: &str,
        cwd: Option<&str>,
        include_archived: bool,
    ) -> Task<Result<SessionSearchResult, String>> {
        let query = monocode_core::js::trim(query);
        if query.is_empty() {
            return Task::ready(Ok(SessionSearchResult {
                hits: Vec::new(),
                truncated: false,
            }));
        }
        let options = SessionSearchOptions {
            query: query.to_string(),
            cwd: cwd
                .filter(|cwd| !cwd.is_empty() && *cwd != "~")
                .map(normalize_project_path),
            include_archived,
            search_owner: search_owner.to_string(),
        };
        let future = self.backend.search(options);
        self.executor.spawn(future)
    }

    /// `cancelSessionSearch`.
    pub fn cancel_session_search(&self, search_owner: &str) -> Task<Result<(), String>> {
        let future = self.backend.cancel_search(search_owner.to_string());
        self.executor.spawn(future)
    }

    /// `deleteSession`. Pending saves for the session, and for workers whose
    /// lead it is, finish before the delete runs; later saves are dropped.
    pub fn delete_session(
        &self,
        session_id: &str,
        image_paths: Vec<String>,
    ) -> Task<Result<(), String>> {
        let pending: Vec<Tail> = {
            let mut state = self.state.lock();
            state.deleted.insert(session_id.to_string());
            let mut pending = Vec::new();
            for (key, (_, tail)) in &state.queues {
                let QueueKey::Session(queued) = key else {
                    continue;
                };
                if queued == session_id
                    || state
                        .lead_by_id
                        .get(queued)
                        .is_some_and(|lead| lead == session_id)
                {
                    pending.push(tail.clone());
                }
            }
            pending
        };
        let writer = self.clone();
        let session_id = session_id.to_string();
        self.executor.spawn(async move {
            futures::future::join_all(pending).await;
            let backend = writer.backend.clone();
            let id = session_id.clone();
            let result = writer
                .enqueue(QueueKey::Session(session_id.clone()), move || {
                    backend.delete(id, image_paths)
                })
                .await;
            match result {
                Ok(()) => {
                    let state = writer.state.clone();
                    let timer = writer.executor.timer(Duration::from_secs(60));
                    writer
                        .executor
                        .spawn(async move {
                            timer.await;
                            state.lock().deleted.remove(&session_id);
                        })
                        .detach();
                    Ok(())
                }
                Err(error) => {
                    writer.state.lock().deleted.remove(&session_id);
                    Err(error)
                }
            }
        })
    }

    /// `discardDraftSessionRecord`: delete a draft-only record while its
    /// still-open blank session id may be saved later.
    pub fn discard_draft_session_record(&self, session_id: &str) -> Task<Result<(), String>> {
        let backend = self.backend.clone();
        let id = session_id.to_string();
        self.enqueue(QueueKey::Session(session_id.to_string()), move || {
            backend.delete(id, Vec::new())
        })
    }

    /// `setSessionArchived`.
    pub fn set_session_archived(
        &self,
        session_id: &str,
        archived: bool,
    ) -> Task<Result<(), String>> {
        let backend = self.backend.clone();
        let id = session_id.to_string();
        self.enqueue(QueueKey::Session(session_id.to_string()), move || {
            backend.set_archived(id, archived)
        })
    }

    /// `setSessionPinned`. Not queued, as in TypeScript.
    pub fn set_session_pinned(&self, session_id: &str, pinned: bool) -> Task<Result<(), String>> {
        let future = self.backend.set_pinned(session_id.to_string(), pinned);
        self.executor.spawn(future)
    }

    /// `setSessionLinkedWorkItem`. `None` removes the link.
    pub fn set_session_linked_work_item(
        &self,
        session_id: &str,
        value: Option<&LinkedWorkItem>,
    ) -> Task<Result<(), String>> {
        let item = value
            .and_then(|item| serde_json::to_value(item).ok())
            .and_then(|value| sanitize_linked_work_item(&value))
            .and_then(|item| serde_json::to_value(item).ok());
        let backend = self.backend.clone();
        let id = session_id.to_string();
        self.enqueue(QueueKey::Session(session_id.to_string()), move || {
            backend.set_linked_work_item(id, item)
        })
    }

    /// `flushSessionWrites`: drain pending saves, for example before a
    /// worktree removal changes stored session context.
    pub fn flush_session_writes(&self) -> Task<()> {
        let tails: Vec<Tail> = self
            .state
            .lock()
            .queues
            .iter()
            .filter(|(key, _)| matches!(key, QueueKey::Session(_)))
            .map(|(_, (_, tail))| tail.clone())
            .collect();
        self.executor.spawn(async move {
            futures::future::join_all(tails).await;
        })
    }

    /// `replaceInFlightSessions`. Replaces land in call order, so a stale
    /// busy snapshot cannot win.
    pub fn replace_in_flight_sessions(&self, refs: Vec<InFlightRef>) -> Task<Result<(), String>> {
        let refs = refs
            .into_iter()
            .map(|entry| InFlightSession {
                session_id: entry.session_id,
                cwd: normalize_project_path(&entry.cwd),
            })
            .collect();
        let backend = self.backend.clone();
        self.enqueue(QueueKey::InFlight, move || backend.set_in_flight(refs))
    }

    /// `listInFlightSessions`: kept across reloads; boot must not delete the
    /// only copy.
    pub fn list_in_flight_sessions(&self) -> Task<Result<Vec<InFlightRef>, String>> {
        let future = self.backend.list_in_flight();
        self.executor
            .spawn(async move { Ok(future.await?.into_iter().map(InFlightRef::from).collect()) })
    }

    /// `takeInFlightSessions`: destructive. The first window to boot after a
    /// quit owns these chats.
    pub fn take_in_flight_sessions(&self) -> Task<Result<Vec<InFlightRef>, String>> {
        let future = self.backend.take_in_flight();
        self.executor
            .spawn(async move { Ok(future.await?.into_iter().map(InFlightRef::from).collect()) })
    }

    /// `saveWorkspaceSnapshot`. Saves land in call order.
    pub fn save_workspace_snapshot(&self, snapshot: Value) -> Task<Result<(), String>> {
        let backend = self.backend.clone();
        self.enqueue(QueueKey::Workspace, move || {
            backend.set_workspace_snapshot(snapshot)
        })
    }

    /// `loadWorkspaceSnapshot`.
    pub fn load_workspace_snapshot(&self) -> Task<Result<Option<Value>, String>> {
        let future = self.backend.workspace_snapshot();
        self.executor.spawn(future)
    }

    /// `claudeShellCommands`.
    pub fn claude_shell_commands(
        &self,
        provider_session_id: &str,
        provider_account_id: Option<&str>,
        tool_ids: Vec<String>,
    ) -> Task<Result<HashMap<String, String>, String>> {
        let future = self.backend.claude_shell_commands(
            provider_session_id.to_string(),
            provider_account_id.map(str::to_string),
            tool_ids,
        );
        self.executor.spawn(future)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::block::BlockTool;
    use monocode_core::models::ModelCatalog;
    use monocode_core::reducer::{UserTurnExtra, append_user};

    fn session(harness: HarnessId, cwd: &str) -> Session {
        Session::blank("session-1", harness, "model", cwd)
    }

    fn blocks_of(session: &Session) -> Vec<Value> {
        sanitize_session_for_persist(session)
            .blocks
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn block(value: Value) -> Block {
        serde_json::from_value(value).unwrap()
    }

    fn submitted(text: &str, extra: UserTurnExtra) -> Session {
        append_user(
            &ModelCatalog::new(),
            &session(HarnessId::Codex, "/repo"),
            text,
            &[],
            Some(&extra),
        )
    }

    #[test]
    fn keeps_host_owned_transcripts_out_of_local_session_storage() {
        let mut remote = session(HarnessId::Codex, "remote://env/home/me/repo");
        remote.blocks = vec![Block::new("turn", BlockRole::User, "Continue")];
        assert!(!should_persist_session(&remote));
        let mut local = session(HarnessId::Codex, "/repo");
        assert!(!should_persist_session(&local));
        local.blocks = remote.blocks.clone();
        assert!(should_persist_session(&local));
        local.cwd = "~".into();
        assert!(!should_persist_session(&local));
    }

    #[test]
    fn restores_only_matching_placeholder_rows_and_preserves_tool_output() {
        let blocks = vec![
            Block {
                tool: Some(BlockTool {
                    call_id: Some("toolu_shell".into()),
                    title: Some("Shell".into()),
                    kind: Some("execute".into()),
                    status: Some("completed".into()),
                    detail: Some("tests passed".into()),
                    ..BlockTool::default()
                }),
                ..Block::new("shell", BlockRole::Tool, "Shell")
            },
            Block {
                tool: Some(BlockTool {
                    call_id: Some("toolu_read".into()),
                    kind: Some("read".into()),
                    ..BlockTool::default()
                }),
                ..Block::new("read", BlockRole::Tool, "Read file.ts")
            },
        ];
        let command = format!("npm run check:web {}", "--filter tests ".repeat(20));
        let command = command.trim().to_string();
        let commands: HashMap<String, String> = [
            ("toolu_shell".to_string(), command.clone()),
            ("toolu_read".to_string(), "ignore me".to_string()),
        ]
        .into();
        let repaired = backfill_claude_shell_commands(&blocks, &commands).unwrap();
        assert_eq!(repaired[0].text, command);
        let tool = repaired[0].tool.as_ref().unwrap();
        assert_eq!(tool.title.as_deref(), Some(command.as_str()));
        assert_eq!(tool.status.as_deref(), Some("completed"));
        assert_eq!(tool.detail.as_deref(), Some("tests passed"));
        assert_eq!(repaired[1], blocks[1]);
        assert!(backfill_claude_shell_commands(&repaired, &HashMap::new()).is_none());
    }

    #[test]
    fn accepts_alphanumeric_ids_with_hyphens_and_underscores() {
        assert!(is_persistable_id("acp-session-1"));
        assert!(is_persistable_id("abc_123"));
    }

    #[test]
    fn rejects_filesystem_paths() {
        assert!(!is_persistable_id("/Users/me/.pi/agent/sessions/abc.jsonl"));
        assert!(!is_persistable_id(""));
    }

    fn with_run(agent_run: Value) -> Value {
        let mut s = session(HarnessId::Claude, "/tmp/project");
        s.blocks = vec![block(json!({
            "id": "a1", "role": "tool", "text": "Correctness review",
            "tool": { "callId": "agent-1", "kind": "agent", "status": "completed" }
        }))];
        let mut value = serde_json::to_value(&s.blocks[0]).unwrap();
        value["agentRun"] = agent_run;
        sanitize_block(&value, false).unwrap()["agentRun"].clone()
    }

    #[test]
    fn keeps_the_run_so_a_reopened_session_can_still_be_inspected() {
        let run = json!({
            "name": "Correctness review",
            "agentType": "code-reviewer",
            "steps": [
                { "id": "s1", "kind": "tool", "text": "Read src/App.tsx", "toolKind": "read",
                  "status": "failed", "detail": "File not found" },
                { "id": "s2", "kind": "message", "text": "Nothing to flag." }
            ]
        });
        assert_eq!(with_run(run.clone()), run);
    }

    #[test]
    fn drops_steps_a_provider_left_malformed() {
        let saved = with_run(json!({
            "name": "Correctness review",
            "steps": [
                { "id": "", "kind": "tool", "text": "Read" },
                { "id": "s2", "kind": "bogus", "text": "Read" },
                { "id": "s3", "kind": "tool", "text": "Read src/App.tsx" }
            ]
        }));
        assert_eq!(
            saved["steps"],
            json!([{ "id": "s3", "kind": "tool", "text": "Read src/App.tsx" }])
        );
    }

    #[test]
    fn keeps_only_the_tail_of_a_long_run() {
        let steps: Vec<Value> = (0..260)
            .map(|i| json!({ "id": format!("s{i}"), "kind": "tool", "text": format!("Read file-{i}.ts") }))
            .collect();
        let saved = with_run(json!({ "name": "Correctness review", "steps": steps }));
        let steps = saved["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 100);
        assert_eq!(steps[99]["id"], "s259");
    }

    #[test]
    fn keeps_typed_agent_runs_through_a_session() {
        let mut s = session(HarnessId::Claude, "/tmp/project");
        s.blocks = vec![block(json!({
            "id": "a1", "role": "tool", "text": "Correctness review",
            "tool": { "callId": "agent-1", "kind": "agent", "status": "completed" },
            "agentRun": { "name": "Review", "steps": [{ "id": "s1", "kind": "message", "text": "ok" }] }
        }))];
        assert_eq!(blocks_of(&s)[0]["agentRun"]["steps"][0]["id"], "s1");
    }

    #[test]
    fn keeps_the_stripped_operator_turn_marker_for_later_turns() {
        let s = submitted(
            "list notes",
            UserTurnExtra {
                monocode: true,
                ..UserTurnExtra::default()
            },
        );
        let saved = &blocks_of(&s)[0];
        assert_eq!(saved["role"], "user");
        assert_eq!(saved["text"], "list notes");
        assert_eq!(saved["monocode"], true);
    }

    #[test]
    fn persists_the_request_id_for_an_agent_sent_follow_up() {
        let s = submitted(
            "Continue",
            UserTurnExtra {
                app_request_id: Some("app-source-request-1".into()),
                ..UserTurnExtra::default()
            },
        );
        let saved = &blocks_of(&s)[0];
        assert_eq!(saved["text"], "Continue");
        assert_eq!(saved["appRequestId"], "app-source-request-1");
    }

    #[test]
    fn persists_the_request_id_on_an_unsent_agent_created_draft() {
        let mut s = session(HarnessId::Codex, "/repo");
        s.blocks = vec![block(json!({
            "id": "draft", "role": "user", "text": "Review later", "draft": true,
            "appRequestId": "app-source-draft-1"
        }))];
        let saved = &blocks_of(&s)[0];
        assert_eq!(saved["draft"], true);
        assert_eq!(saved["appRequestId"], "app-source-draft-1");
    }

    #[test]
    fn keeps_an_unsent_user_turn_appended_to_a_started_thread() {
        let mut s = session(HarnessId::Codex, "/repo");
        let blocks = json!([
            { "id": "sent", "role": "user", "text": "Start here" },
            { "id": "reply", "role": "assistant", "text": "Done" },
            { "id": "draft", "role": "user", "text": "Explore this", "draft": true }
        ]);
        s.blocks = serde_json::from_value(blocks.clone()).unwrap();
        assert_eq!(Value::Array(blocks_of(&s)), blocks);
    }

    #[test]
    fn persists_generated_image_metadata_without_binary_payloads() {
        let mut s = session(HarnessId::Codex, "/repo");
        let image = json!({
            "id": "image", "role": "image", "text": "",
            "image": {
                "path": "/app-data/generated-images/image.png", "name": "generated-image",
                "mimeType": "image/png", "size": 8, "alt": "A clean product photo"
            }
        });
        s.blocks = vec![
            Block::new("u", BlockRole::User, "Draw this"),
            block(image.clone()),
        ];
        assert_eq!(blocks_of(&s)[1], image);
    }

    #[test]
    fn drops_malformed_generated_image_metadata() {
        let mut s = session(HarnessId::Codex, "/repo");
        s.blocks = vec![
            Block::new("u", BlockRole::User, "Draw this"),
            block(json!({
                "id": "image", "role": "image", "text": "",
                "image": { "path": "", "name": "generated-image", "mimeType": "image/png", "size": 0 }
            })),
        ];
        assert_eq!(
            blocks_of(&s),
            vec![json!({ "id": "u", "role": "user", "text": "Draw this" })]
        );
    }

    #[test]
    fn persists_a_removed_worktree_as_an_explicit_unselected_working_copy_state() {
        let mut s = session(HarnessId::Codex, "/repo");
        s.worktree_cwd = Some("/repo-worktrees/feature".into());
        s.worktree_removed = Some(true);
        s.blocks = vec![Block::new("u", BlockRole::User, "Build feature")];
        let saved = sanitize_session_for_persist(&s);
        assert_eq!(
            saved.worktree_cwd.as_deref(),
            Some("/repo-worktrees/feature")
        );
        assert!(saved.worktree_removed);
    }

    #[test]
    fn preserves_an_internal_workers_lead_hidden_turns_and_token_metrics() {
        let mut s = session(HarnessId::Claude, "/repo");
        s.orchestration_lead_id = Some("lead".into());
        s.blocks = vec![block(json!({
            "id": "u", "role": "user", "text": "Bounded assignment", "internal": true,
            "turnMetrics": { "inputTokens": 100, "outputTokens": 20 }
        }))];
        let saved = blocks_of(&s)[0].clone();
        assert_eq!(saved["orchestrationLeadId"], "lead");
        assert_eq!(saved["internal"], true);
        assert_eq!(
            saved["turnMetrics"],
            json!({ "inputTokens": 100, "outputTokens": 20 })
        );
        assert!(s.blocks[0].orchestration_lead_id.is_none());
        let mut reloaded = s.clone();
        reloaded.orchestration_lead_id = None;
        reloaded.blocks = vec![block(saved.clone())];
        assert_eq!(blocks_of(&reloaded)[0], saved);
    }

    #[test]
    fn persists_model_provenance_recorded_on_a_user_turn() {
        let mut s = session(HarnessId::Claude, "/tmp/project");
        let turn_model =
            json!({ "harness": "claude", "id": "claude:opus-5", "name": "Claude Opus 5" });
        s.blocks = vec![block(json!({
            "id": "u1", "role": "user", "text": "remember this", "turnModel": turn_model.clone()
        }))];
        assert_eq!(blocks_of(&s)[0]["turnModel"], turn_model);
    }

    #[test]
    fn persists_a_btw_threads_model_and_provider_settings() {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        let thread = json!({
            "id": "btw-1", "sourceEndBlockId": "u1", "createdAt": 1, "updatedAt": 2, "status": "ready",
            "messages": [
                { "id": "m1", "role": "user", "text": "Why?", "createdAt": 1 },
                { "id": "m2", "role": "assistant", "text": "Because.", "createdAt": 2 }
            ],
            "model": "codex:gpt-5.4",
            "modelSettings": { "reasoningEffort": "high", "serviceTier": "fast" }
        });
        s.blocks = vec![block(json!({
            "id": "u1", "role": "user", "text": "Explain this", "btwThreads": [thread.clone()]
        }))];
        assert_eq!(blocks_of(&s)[0]["btwThreads"], json!([thread]));
    }

    #[test]
    fn marks_a_running_btw_thread_interrupted_on_load() {
        let saved = sanitize_block(
            &json!({
                "id": "u1", "role": "user", "text": "q",
                "btwThreads": [{
                    "id": "t", "sourceEndBlockId": "u1", "createdAt": 1, "updatedAt": 2,
                    "status": "running", "messages": [{ "id": "m", "role": "user", "text": "?", "createdAt": 1 }]
                }]
            }),
            true,
        )
        .unwrap();
        assert_eq!(saved["btwThreads"][0]["status"], "error");
        assert_eq!(
            saved["btwThreads"][0]["error"],
            "This by-the-way request was interrupted before reload."
        );
    }

    #[test]
    fn persists_provider_metrics_recorded_on_a_user_turn() {
        let mut s = session(HarnessId::Claude, "/tmp/project");
        let metrics = json!({ "inputTokens": 100, "outputTokens": 20, "cacheReadTokens": 80, "cacheHitPercent": 40.0 });
        s.blocks = vec![block(json!({
            "id": "u1", "role": "user", "text": "remember this", "turnMetrics": metrics.clone()
        }))];
        assert_eq!(blocks_of(&s)[0]["turnMetrics"], metrics);
    }

    #[test]
    fn persists_a_canonical_github_work_item_identity() {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        s.blocks = vec![Block::new("u1", BlockRole::User, "fix PR #42")];
        s.linked_work_item = Some(LinkedWorkItem {
            kind: WorkItemKind::Pr,
            repo: "openai/codex".into(),
            number: 42,
            url: "https://example.com/not-trusted".into(),
            extra: Extra::new(),
        });
        assert_eq!(
            sanitize_session_for_persist(&s).linked_work_item,
            Some(json!({
                "kind": "pr", "repo": "openai/codex", "number": 42,
                "url": "https://github.com/openai/codex/pull/42"
            }))
        );
    }

    #[test]
    fn rejects_malformed_work_items() {
        for value in [
            json!({ "kind": "pr", "repo": "no-slash", "number": 1 }),
            json!({ "kind": "commit", "repo": "a/b", "number": 1 }),
            json!({ "kind": "issue", "repo": "a/b", "number": 0 }),
            json!({ "kind": "issue", "repo": "a/b", "number": 1.5 }),
            json!(["kind"]),
        ] {
            assert!(sanitize_linked_work_item(&value).is_none(), "{value}");
        }
        let issue =
            sanitize_linked_work_item(&json!({ "kind": "issue", "repo": " a/b ", "number": 7 }))
                .unwrap();
        assert_eq!(issue.url, "https://github.com/a/b/issues/7");
    }

    #[test]
    fn persists_the_automation_that_started_a_session() {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        s.blocks = vec![Block::new("u1", BlockRole::User, "review PRs")];
        s.automation_id = Some("automation-1".into());
        assert_eq!(
            sanitize_session_for_persist(&s).automation_id.as_deref(),
            Some("automation-1")
        );
    }

    #[test]
    fn omits_a_path_like_provider_session_id_so_upsert_can_still_snapshot_git() {
        let mut s = session(HarnessId::Pi, "/tmp/project");
        s.provider_session_id = Some("/Users/me/.pi/agent/sessions/abc.jsonl".into());
        s.blocks = vec![Block::new("u1", BlockRole::User, "hey")];
        assert_eq!(sanitize_session_for_persist(&s).provider_session_id, None);
    }

    #[test]
    fn keeps_a_uuid_provider_session_id() {
        let mut s = session(HarnessId::Pi, "/tmp/project");
        s.provider_session_id = Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".into());
        s.blocks = vec![Block::new("u1", BlockRole::User, "hey")];
        assert_eq!(
            sanitize_session_for_persist(&s)
                .provider_session_id
                .as_deref(),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        );
    }

    #[test]
    fn keeps_a_handoff_divider_and_settles_a_preparing_one() {
        let mut s = session(HarnessId::Cursor, "/tmp/project");
        s.blocks = vec![
            Block::new("u1", BlockRole::User, "hey"),
            block(json!({
                "id": "h1", "role": "handoff", "text": "",
                "handoff": { "from": "cursor", "to": "claude", "status": "preparing" }
            })),
        ];
        let saved = &blocks_of(&s)[1];
        assert_eq!(saved["role"], "handoff");
        assert_eq!(
            saved["handoff"],
            json!({ "from": "cursor", "to": "claude", "status": "ready", "pending": true })
        );
    }

    #[test]
    fn keeps_valid_interjection_chrome_only_on_system_blocks() {
        let mut s = session(HarnessId::Pi, "/tmp/project");
        s.blocks = vec![
            block(json!({
                "id": "i1", "role": "system", "text": "Review the fallback.",
                "interjection": { "customType": " advisor ", "severity": "blocker" }
            })),
            block(json!({
                "id": "a1", "role": "assistant", "text": "Not chrome",
                "interjection": { "customType": "advisor", "severity": "nit" }
            })),
        ];
        let saved = blocks_of(&s);
        assert_eq!(
            saved[0]["interjection"],
            json!({ "customType": "advisor", "severity": "blocker" })
        );
        assert!(saved[1].get("interjection").is_none());
    }

    #[test]
    fn drops_malformed_interjection_metadata_without_dropping_its_system_row() {
        let raw = json!({
            "id": "i1", "role": "system", "text": "Still visible",
            "interjection": { "customType": " ", "severity": "unknown" }
        });
        assert_eq!(
            sanitize_block(&raw, false).unwrap(),
            json!({ "id": "i1", "role": "system", "text": "Still visible" })
        );
    }

    #[test]
    fn keeps_a_notice_flag_on_system_blocks_and_drops_anything_else() {
        let blocks = [
            json!({ "id": "e1", "role": "system", "text": "Provider connection lost", "notice": "error" }),
            json!({ "id": "i1", "role": "system", "text": "Turn interrupted when MonoCode quit.", "notice": "interrupt" }),
            json!({ "id": "b1", "role": "system", "text": "Mystery", "notice": "mystery" }),
            json!({ "id": "a1", "role": "assistant", "text": "hi", "notice": "error" }),
        ];
        let saved: Vec<Value> = blocks
            .iter()
            .map(|b| sanitize_block(b, false).unwrap())
            .collect();
        assert_eq!(saved[0]["notice"], "error");
        assert_eq!(saved[1]["notice"], "interrupt");
        assert!(saved[2].get("notice").is_none());
        assert!(saved[3].get("notice").is_none());
    }

    #[test]
    fn keeps_a_second_opinion_card_on_the_user_turn() {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        let card =
            json!({ "from": "claude", "to": "codex", "request": "fix the footer", "files": 2 });
        s.blocks = vec![block(json!({
            "id": "u1", "role": "user", "text": "Second opinion", "secondOpinion": card.clone()
        }))];
        assert_eq!(blocks_of(&s)[0]["secondOpinion"], card);
    }

    #[test]
    fn keeps_a_handoff_card_kind_on_the_user_turn() {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        let card = json!({ "from": "claude", "to": "codex", "kind": "handoff" });
        s.blocks = vec![block(
            json!({ "id": "u1", "role": "user", "text": "Handoff", "secondOpinion": card.clone() }),
        )];
        assert_eq!(blocks_of(&s)[0]["secondOpinion"], card);
    }

    #[test]
    fn keeps_a_note_card_on_the_user_turn_without_the_note_body() {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        let card = json!({ "id": "n1", "slug": "overview", "title": "agent-os project overview", "sourceCwd": "/tmp/project" });
        s.blocks = vec![block(
            json!({ "id": "u1", "role": "user", "text": "hi", "noteCard": card.clone() }),
        )];
        assert_eq!(
            blocks_of(&s)[0],
            json!({ "id": "u1", "role": "user", "text": "hi", "noteCard": card })
        );
    }

    #[test]
    fn keeps_edited_and_approved_plan_metadata() {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        let plan = json!({
            "key": "turn:1", "status": "built", "originalText": "# Original plan",
            "approvedText": "# Edited plan", "edited": true
        });
        s.blocks = vec![
            Block::new("u1", BlockRole::User, "plan this"),
            block(
                json!({ "id": "p1", "role": "plan", "text": "# Edited plan", "plan": plan.clone() }),
            ),
        ];
        let saved = &blocks_of(&s)[1];
        assert_eq!(saved["role"], "plan");
        assert_eq!(saved["plan"], plan);
    }

    #[test]
    fn settles_a_streaming_plan_and_a_bare_plan_row() {
        let streaming = sanitize_block(
            &json!({ "id": "p", "role": "plan", "text": "# P", "plan": { "status": "streaming" } }),
            true,
        )
        .unwrap();
        assert_eq!(
            streaming["plan"],
            json!({ "status": "ready", "originalText": "# P" })
        );
        let bare = sanitize_block(&json!({ "id": "p", "role": "plan", "text": "" }), true).unwrap();
        assert_eq!(
            bare["plan"],
            json!({ "status": "ready", "originalText": "" })
        );
    }

    #[test]
    fn keeps_structured_task_lists() {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        let tasks = json!({
            "id": "tasks1", "role": "tasks", "text": "[x] Inspect\n[~] Implement",
            "taskList": {
                "key": "turn_1", "explanation": "Inspection complete.",
                "items": [
                    { "id": "1", "text": "Inspect", "status": "completed" },
                    { "id": "2", "text": "Implement", "status": "in_progress" }
                ]
            }
        });
        s.blocks = vec![
            Block::new("u1", BlockRole::User, "fix it"),
            block(tasks.clone()),
        ];
        assert_eq!(blocks_of(&s)[1], tasks);
    }

    #[test]
    fn drops_live_approval_prompts_and_empty_task_rows() {
        assert!(sanitize_block(&json!({ "id": "a", "role": "approval", "text": "?", "approval": { "requestId": 1 } }), false).is_none());
        let decided = sanitize_block(
            &json!({ "id": "a", "role": "approval", "text": "?", "approval": { "requestId": 1, "decided": "allow", "x": 1 } }),
            false,
        )
        .unwrap();
        assert_eq!(
            decided["approval"],
            json!({ "requestId": 1, "decided": "allow" })
        );
        assert!(
            sanitize_block(
                &json!({ "id": "t", "role": "tasks", "text": "", "taskList": { "items": [] } }),
                false
            )
            .is_none()
        );
        let restored = sanitize_block(
            &json!({ "id": "o", "role": "plan", "text": "", "orchestration": { "status": "planning", "tasks": [] } }),
            true,
        )
        .unwrap();
        assert_eq!(restored["orchestration"]["status"], "invalid");
    }

    fn fingerprint_base(blocks: Vec<Block>) -> Session {
        let mut s = session(HarnessId::Codex, "/tmp/project");
        s.blocks = blocks;
        s
    }

    fn user() -> Block {
        Block::new("u1", BlockRole::User, "hi")
    }

    fn answer() -> Block {
        Block::new("a1", BlockRole::Assistant, "done")
    }

    #[test]
    fn fingerprint_is_stable_while_nothing_changes() {
        let s = fingerprint_base(vec![user(), answer()]);
        assert_eq!(persist_fingerprint(&s), persist_fingerprint(&s));
        assert_eq!(persist_fingerprint(&s.clone()), persist_fingerprint(&s));
    }

    #[test]
    fn fingerprint_changes_when_an_automation_origin_is_stamped() {
        let before = fingerprint_base(vec![user(), answer()]);
        let mut after = before.clone();
        after.automation_id = Some("automation-1".into());
        assert_ne!(persist_fingerprint(&after), persist_fingerprint(&before));
    }

    #[test]
    fn fingerprint_changes_when_a_block_in_the_middle_is_replaced() {
        let tool = |status: &str| Block {
            tool: Some(BlockTool {
                status: Some(status.into()),
                ..BlockTool::default()
            }),
            ..Block::new("t1", BlockRole::Tool, "run")
        };
        let before = fingerprint_base(vec![user(), tool("running"), answer()]);
        let after = fingerprint_base(vec![user(), tool("completed"), answer()]);
        assert_ne!(persist_fingerprint(&after), persist_fingerprint(&before));
    }

    #[test]
    fn fingerprint_changes_when_an_approval_is_decided() {
        let approval = |decided: Option<&str>| {
            let mut value = json!({ "id": "p1", "role": "approval", "text": "allow?", "approval": { "requestId": 1 } });
            if let Some(decided) = decided {
                value["approval"]["decided"] = json!(decided);
            }
            block(value)
        };
        let before = fingerprint_base(vec![user(), approval(None)]);
        let after = fingerprint_base(vec![user(), approval(Some("allow"))]);
        assert_ne!(persist_fingerprint(&after), persist_fingerprint(&before));
    }

    #[test]
    fn fingerprint_changes_when_a_block_is_appended_or_a_field_changes() {
        let before = fingerprint_base(vec![user()]);
        let appended = fingerprint_base(vec![user(), answer()]);
        assert_ne!(persist_fingerprint(&appended), persist_fingerprint(&before));
        let mut renamed = before.clone();
        renamed.title = "Renamed".into();
        assert_ne!(persist_fingerprint(&renamed), persist_fingerprint(&before));
    }

    #[test]
    fn fingerprint_ignores_state_that_is_never_written() {
        let before = fingerprint_base(vec![user(), answer()]);
        let mut busy = before.clone();
        busy.busy = Some(true);
        assert_eq!(persist_fingerprint(&busy), persist_fingerprint(&before));
        let mut path_id = before.clone();
        path_id.provider_session_id = Some("/Users/me/.pi/agent/sessions/abc.jsonl".into());
        assert_eq!(persist_fingerprint(&path_id), persist_fingerprint(&before));
    }

    #[test]
    fn fingerprint_matches_persist_for_a_zero_context_window() {
        let mut zero = fingerprint_base(vec![user()]);
        zero.context = Some(ContextUsage {
            used: 10,
            window: Some(0),
        });
        let mut unknown = zero.clone();
        unknown.context = Some(ContextUsage {
            used: 10,
            window: None,
        });
        assert_eq!(persist_fingerprint(&zero), persist_fingerprint(&unknown));
    }

    #[test]
    fn saves_the_exact_ci_context_alongside_the_compact_user_message() {
        let context = "Checked commit: abc123\n\nRun tests failed at src/app.test.ts:42\nExpected 2, received 1";
        let s = submitted(
            "Fix 1 failed CI check for acme/web PR #42.",
            UserTurnExtra {
                ci_context: Some(context.into()),
                ..UserTurnExtra::default()
            },
        );
        let saved = serde_json::to_value(sanitize_session_for_persist(&s)).unwrap();
        assert_eq!(saved["blocks"][0]["role"], "user");
        assert_eq!(
            saved["blocks"][0]["text"],
            "Fix 1 failed CI check for acme/web PR #42."
        );
        assert_eq!(saved["blocks"][0]["ciContext"], context);
    }

    fn record(blocks: Value) -> SessionRecord {
        SessionRecord {
            id: "s1".into(),
            orchestration_lead_id: None,
            cwd: "/repo".into(),
            harness: "mystery".into(),
            model: "m".into(),
            model_settings: json!({ "effort": "high", "bad": 1 }),
            runtime_mode: "bogus".into(),
            title: "codex · Fix".into(),
            provider_session_id: Some(String::new()),
            provider_account_id: Some("acct".into()),
            blocks,
            context_used: Some(120),
            context_window: Some(0),
            branch: Some("main".into()),
            worktree_cwd: None,
            worktree_removed: true,
            linked_work_item: Some(json!({ "kind": "pr", "repo": "a/b", "number": 3, "url": "x" })),
            automation_id: Some("../bad".into()),
            created_at: 1,
            updated_at: 2,
        }
    }

    #[test]
    fn record_to_session_hydrates_and_falls_back() {
        let session = record_to_session(record(json!([
            { "id": "u", "role": "user", "text": "go", "orchestrationLeadId": "lead" },
            { "id": "a", "role": "approval", "text": "?", "approval": { "requestId": 2 } },
            { "id": "x", "role": "mystery", "text": "?" },
            "not a block"
        ])));
        assert_eq!(session.harness, HarnessId::Cursor);
        assert_eq!(session.runtime_mode, RuntimeMode::Supervised);
        assert_eq!(session.busy, Some(false));
        assert_eq!(session.blocks.len(), 1);
        assert_eq!(session.orchestration_lead_id.as_deref(), Some("lead"));
        assert_eq!(
            session.model_settings.get("effort").map(String::as_str),
            Some("high")
        );
        assert!(!session.model_settings.contains_key("bad"));
        assert_eq!(session.provider_session_id, None);
        assert_eq!(session.provider_account_id.as_deref(), Some("acct"));
        assert_eq!(
            session.context,
            Some(ContextUsage {
                used: 120,
                window: None
            })
        );
        assert_eq!(session.worktree_removed, Some(true));
        assert_eq!(
            session.linked_work_item.unwrap().url,
            "https://github.com/a/b/pull/3"
        );
        assert_eq!(session.automation_id, None);
        assert_eq!(session.title, "codex · Fix");
    }

    #[test]
    fn normalizes_stored_summaries() {
        let summary = normalize_summary(StoredSummary {
            id: "s".into(),
            orchestration_lead_id: None,
            orchestration: Some(json!({ "status": "active", "tasks": [] })),
            cwd: "/repo".into(),
            harness: "codex".into(),
            model: "m".into(),
            runtime_mode: "auto".into(),
            title: "t".into(),
            provider_session_id: Some(String::new()),
            branch: Some(String::new()),
            worktree_cwd: None,
            worktree_removed: false,
            repo: Some("repo".into()),
            additions: 3,
            deletions: 1,
            created_at: 1,
            updated_at: 2,
            archived: false,
            pinned: true,
            draft: false,
            linked_work_item: None,
            automation_id: Some("auto-1".into()),
        });
        assert_eq!(summary.harness, HarnessId::Codex);
        assert_eq!(summary.runtime_mode, RuntimeMode::Auto);
        assert_eq!((summary.provider_session_id, summary.branch), (None, None));
        assert_eq!(
            (summary.archived, summary.pinned, summary.draft),
            (None, Some(true), None)
        );
        assert_eq!(summary.orchestration.unwrap().status, "active");
        assert_eq!(summary.automation_id.as_deref(), Some("auto-1"));
    }
}
