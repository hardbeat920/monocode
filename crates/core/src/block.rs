//! Port of the transcript block types in src/features/sessions/model/session.ts.
//!
//! These structs are what `sessions.blocks_json` holds, so their JSON shape
//! must match what the TypeScript app wrote. Every persisted struct keeps the
//! fields it does not model in `extra`, so a load and save loses nothing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::attachment::Attachment;
use crate::harness::HarnessId;
use crate::notes::NoteCardMeta;
use crate::orchestration::OrchestrationProposal;

/// Fields a persisted struct did not model, kept for the next save.
pub type Extra = Map<String, Value>;

/// `Record<string, string>` provider settings, such as `{ "effort": "high" }`.
pub type ModelSettings = BTreeMap<String, String>;

/// What kind of row a block is in the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum BlockRole {
    #[default]
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "reasoning")]
    Reasoning,
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "approval")]
    Approval,
    #[serde(rename = "tasks")]
    Tasks,
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "system")]
    System,
    #[serde(rename = "handoff")]
    Handoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TaskListItemStatus {
    #[default]
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListItem {
    /// Stable provider identity, when available, for merging status-only updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub text: String,
    pub status: TaskListItemStatus,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListMeta {
    /// Provider identity for replacing later snapshots of the same list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Provider conversation that produced this list, when the provider scopes task ids to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    pub items: Vec<TaskListItem>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One-shot behavior selected in the composer for the next harness turn.
///
/// `Block.intent` only ever holds `Plan` or `Orchestrate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TurnIntent {
    #[default]
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "build")]
    Build,
    #[serde(rename = "orchestrate")]
    Orchestrate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PlanStatus {
    #[default]
    #[serde(rename = "streaming")]
    Streaming,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "building")]
    Building,
    #[serde(rename = "built")]
    Built,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanBlockMeta {
    /// Provider or turn identity used to merge streamed snapshots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub status: PlanStatus,
    /// Provider-authored plan before any user edits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
    /// Exact markdown the user approved with Build.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A provider and model pair, such as the one a plan is built with
/// (`PlanBuildTarget` in TypeScript).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTarget {
    pub harness: HarnessId,
    pub model: String,
    pub model_settings: ModelSettings,
}

/// `PlanBuildTarget`.
pub type PlanBuildTarget = ModelTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HandoffStatus {
    #[serde(rename = "preparing")]
    Preparing,
    #[serde(rename = "ready")]
    Ready,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffMeta {
    pub from: HarnessId,
    pub to: HarnessId,
    pub status: HandoffStatus,
    /// Inject this brief into prompts to `to` until that harness accepts a turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BtwMessageRole {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
}

/// One persisted question and answer in a completed turn's side conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtwMessage {
    pub id: String,
    pub role: BtwMessageRole,
    pub text: String,
    pub created_at: i64,
    /// Rich harness activity for assistant replies, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<Block>>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BtwThreadStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "error")]
    Error,
}

/// Independent, read-only "by the way" conversation anchored to a turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtwThread {
    pub id: String,
    pub source_end_block_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: BtwThreadStatus,
    pub messages: Vec<BtwMessage>,
    /// Provider that answered this side thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<HarnessId>,
    /// Selected harness model for this side thread. `None` means the session default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Provider settings selected for this side thread's model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    /// Provider-specific side-thread id when the text runner supports resume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Live harness blocks for the in-flight reply. The app does not persist them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_blocks: Option<Vec<Block>>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecondOpinionKind {
    /// Split-pane continue. Without a kind the card is a second-opinion review.
    #[serde(rename = "handoff")]
    Handoff,
}

/// Compact transcript card for a second-opinion or split-pane handoff turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecondOpinionMeta {
    pub from: HarnessId,
    pub to: HarnessId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<SecondOpinionKind>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// How serious a mid-turn interjection is, such as an OMP advisor note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum InterjectionSeverity {
    #[serde(rename = "nit")]
    Nit,
    #[serde(rename = "concern")]
    Concern,
    #[serde(rename = "blocker")]
    Blocker,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterjectionMeta {
    pub custom_type: String,
    /// Highest severity among this interjection's retained notes, when any is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<InterjectionSeverity>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolPreviewKind {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "write")]
    Write,
    #[serde(rename = "shell")]
    Shell,
    #[serde(rename = "search")]
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolPreviewLineKind {
    #[serde(rename = "add")]
    Add,
    #[serde(rename = "del")]
    Del,
    #[serde(rename = "context")]
    Context,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPreviewLine {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<i64>,
    pub kind: ToolPreviewLineKind,
    pub text: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPreview {
    pub kind: ToolPreviewKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additions: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i64>,
    /// Write supplied new contents without the previous file to compare.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<ToolPreviewLine>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ToolPreview {
    /// A preview of `kind` with every optional field unset.
    pub fn new(kind: ToolPreviewKind) -> Self {
        Self {
            kind,
            title: None,
            path: None,
            file_name: None,
            start_line: None,
            additions: None,
            deletions: None,
            content_only: None,
            query: None,
            lines: None,
            output: None,
            extra: Extra::new(),
        }
    }
}

/// One thing a subagent did, mirrored into the parent transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentStepKind {
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "message")]
    Message,
    #[serde(rename = "reasoning")]
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStep {
    /// Provider step identity, so repeats merge instead of stacking up.
    pub id: String,
    pub kind: AgentStepKind,
    /// Tool label, or the prose the subagent wrote.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<ToolPreview>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The inside of a delegated run: what the subagent is called, and the trail
/// it left. Held on the parent Agent tool block so the transcript can open it
/// without a second session.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunMeta {
    /// What the subagent is called, such as "Correctness review".
    pub name: String,
    /// Provider agent type, such as "code-reviewer".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// Model reported for the child, which may differ from its parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub steps: Vec<AgentStep>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedImageMeta {
    pub path: String,
    pub name: String,
    pub mime_type: String,
    pub size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Provider and model provenance captured when a user turn is submitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnModel {
    pub harness: HarnessId,
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Provider-reported token accounting for one user turn.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<i64>,
    /// Provider-normalized share of input served from cache, as a percentage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_hit_percent: Option<f64>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `Block.tool`: a tool call row.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockTool {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<ToolPreview>,
    /// Left running by the agent when it yielded. The turn waits on it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// How an approval request ended. `Cancelled` means a PermissionRequest hook
/// decided before the user could.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApprovalDecided {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// `Block.approval`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockApproval {
    pub request_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided: Option<ApprovalDecided>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A system row the reader must not miss, rather than turn chrome like a
/// status ping. It never folds into the trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlockNotice {
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "interrupt")]
    Interrupt,
}

/// One transcript row.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    pub role: BlockRole,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<GeneratedImageMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    /// Epoch ms when this user turn started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    /// How long the agent worked on this user turn, in ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// Stable model label for this turn. Newly created user blocks have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_model: Option<TurnModel>,
    /// Provider turn boundary used to replace this user message, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_turn_id: Option<String>,
    /// User turn saved to the session but not submitted to the harness yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    /// This user turn activated MonoCode app access for its thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monocode: Option<bool>,
    /// The Plan or Orchestrator mode this user turn was sent in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<TurnIntent>,
    /// Stable CLI request that submitted this turn, for safe retries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_request_id: Option<String>,
    /// Provider-reported token metrics for this user turn, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_metrics: Option<TurnMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<BlockTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval: Option<BlockApproval>,
    /// Inner activity of a delegated run. Agent and Task tool blocks have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_run: Option<AgentRunMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_list: Option<TaskListMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanBlockMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration: Option<OrchestrationProposal>,
    /// Parent conversation for an internal orchestration worker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration_lead_id: Option<String>,
    /// A turn the app wrote on the user's behalf to keep an orchestration
    /// moving. The harness needs it and the transcript hides it, so a run
    /// reads as one conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff: Option<HandoffMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub second_opinion: Option<SecondOpinionMeta>,
    /// Independent read-only side conversations anchored to this user turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub btw_threads: Option<Vec<BtwThread>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_card: Option<NoteCardMeta>,
    /// Exact CI repair instructions and evidence supplied with this user turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ci_context: Option<String>,
    /// Mid-turn interjection chrome, on system blocks only. The body is in `text`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interjection: Option<InterjectionMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<BlockNotice>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Block {
    /// A block with only `id`, `role`, and `text` set.
    pub fn new(id: impl Into<String>, role: BlockRole, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role,
            text: text.into(),
            ..Self::default()
        }
    }

    /// `block.draft` is truthy.
    pub fn is_draft(&self) -> bool {
        self.draft == Some(true)
    }

    /// `block.streaming` is truthy.
    pub fn is_streaming(&self) -> bool {
        self.streaming == Some(true)
    }

    /// `block.internal` is truthy.
    pub fn is_internal(&self) -> bool {
        self.internal == Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_unknown_fields_and_explicit_false() {
        let raw = json!({
            "id": "b1",
            "role": "handoff",
            "text": "brief",
            "handoff": { "from": "codex", "to": "droid", "status": "ready", "pending": false },
            "futureField": { "nested": [1, 2] },
            "tool": { "callId": "c", "kind": "edit", "laterToolField": true }
        });
        let block: Block = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(block.handoff.as_ref().unwrap().pending, Some(false));
        assert!(block.extra.contains_key("futureField"));
        assert_eq!(serde_json::to_value(&block).unwrap(), raw);
    }

    #[test]
    fn omits_unset_optional_fields() {
        let block = Block::new("a", BlockRole::Assistant, "hi");
        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            json!({ "id": "a", "role": "assistant", "text": "hi" })
        );
    }

    #[test]
    fn nests_side_thread_blocks() {
        let raw = json!({
            "id": "u",
            "role": "user",
            "text": "q",
            "btwThreads": [{
                "id": "t",
                "sourceEndBlockId": "r",
                "createdAt": 1,
                "updatedAt": 2,
                "status": "ready",
                "harness": "claude",
                "messages": [{
                    "id": "m",
                    "role": "assistant",
                    "text": "a",
                    "createdAt": 3,
                    "blocks": [{ "id": "x", "role": "reasoning", "text": "r" }]
                }]
            }],
            "turnMetrics": { "inputTokens": 10, "cacheHitPercent": 12.5 }
        });
        let block: Block = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&block).unwrap(), raw);
    }
}
