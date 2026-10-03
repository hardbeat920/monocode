//! Port of src/features/connections/model/protocol.ts.
//!
//! The wire types between a desktop and MonoCode Host. The host server in
//! this module and the desktop client share them, so both sides agree on the
//! JSON shapes the TypeScript host and renderer used.

use std::collections::BTreeMap;

use monocode_core::block::Extra;
use monocode_core::session::LinkedWorkItem;
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{AgentModel, AttachmentKind, Block, HarnessId, RuntimeMode, Session};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

pub const HOST_PROTOCOL_VERSION: i64 = 1;
/// The npm package a machine runs to host sessions for this desktop.
pub const HOST_PACKAGE: &str = "monocode-host";

/// The command that installs, updates, or pairs a host for this desktop.
pub fn host_connect_command(desktop_version: Option<&str>) -> String {
    match desktop_version.filter(|version| !version.is_empty()) {
        Some(version) => format!("npx {HOST_PACKAGE}@{version} connect"),
        None => format!("npx {HOST_PACKAGE} connect"),
    }
}

/// Compares `a.b.c` versions, ignoring prerelease suffixes.
pub fn compare_versions(a: &str, b: &str) -> i32 {
    let parts = |value: &str| -> Vec<f64> {
        value
            .split('-')
            .next()
            .unwrap_or("")
            .split('.')
            .map(js_number_or_zero)
            .collect()
    };
    let left = parts(a);
    let right = parts(b);
    for i in 0..3 {
        let delta = left.get(i).copied().unwrap_or(0.0) - right.get(i).copied().unwrap_or(0.0);
        if delta != 0.0 {
            return if delta > 0.0 { 1 } else { -1 };
        }
    }
    0
}

/// `Number(part) || 0` for one version component.
pub(crate) fn js_number_or_zero(part: &str) -> f64 {
    let value = super::js::number_from_str(part);
    if value.is_nan() { 0.0 } else { value }
}

/// Hosts before this desktop's version, or without pushed changes, should
/// be updated.
pub fn host_needs_update(host: &HostDescriptor, desktop_version: Option<&str>) -> bool {
    if !host
        .capabilities
        .iter()
        .any(|entry| entry == "changes.wait")
    {
        return true;
    }
    match (desktop_version, host.host_version.as_deref()) {
        (Some(desktop), Some(hosted)) if !desktop.is_empty() && !hosted.is_empty() => {
            compare_versions(hosted, desktop) < 0
        }
        _ => false,
    }
}

/// A provider a host can run. Every harness is remote-capable today; the
/// list below keeps the protocol's order.
pub type RemoteProvider = HarnessId;

pub const REMOTE_PROVIDERS: [RemoteProvider; 11] = [
    HarnessId::Codex,
    HarnessId::Claude,
    HarnessId::Cursor,
    HarnessId::Grok,
    HarnessId::Opencode,
    HarnessId::Pi,
    HarnessId::Omp,
    HarnessId::Fx,
    HarnessId::Hermes,
    HarnessId::Droid,
    HarnessId::Antigravity,
];

/// The provider's wire name, such as `codex`.
pub fn provider_name(provider: RemoteProvider) -> &'static str {
    match provider {
        HarnessId::Codex => "codex",
        HarnessId::Claude => "claude",
        HarnessId::Cursor => "cursor",
        HarnessId::Grok => "grok",
        HarnessId::Opencode => "opencode",
        HarnessId::Pi => "pi",
        HarnessId::Omp => "omp",
        HarnessId::Fx => "fx",
        HarnessId::Hermes => "hermes",
        HarnessId::Droid => "droid",
        HarnessId::Antigravity => "antigravity",
    }
}

/// The provider with this wire name.
pub fn parse_provider(value: &str) -> Option<RemoteProvider> {
    REMOTE_PROVIDERS
        .into_iter()
        .find(|provider| provider_name(*provider) == value)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDescriptor {
    pub protocol_version: i64,
    pub environment_id: String,
    pub name: String,
    pub providers: Vec<RemoteProvider>,
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// The MonoCode Host package version. Hosts before 0.5 omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_version: Option<String>,
    /// Network addresses the host listens on, such as `https://10.0.0.5:3774`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProject {
    pub id: String,
    pub cwd: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDirectoryEntry {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDirectory {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<HostDirectoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HostModelCatalog {
    pub models: BTreeMap<RemoteProvider, Vec<AgentModel>>,
    pub errors: BTreeMap<RemoteProvider, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostWorktree {
    pub path: String,
    pub branch: Option<String>,
    pub head: String,
    pub is_main: bool,
    pub missing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum HostSessionStatus {
    #[default]
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "interrupted")]
    Interrupted,
}

/// One host-owned session, as the host saves it and as a snapshot sync
/// sends it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSession {
    pub session: Session,
    pub project_id: String,
    pub revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub status: HostSessionStatus,
    /// Missing from snapshots written before creation time was stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    /// Temporary branch created by the composer for automatic first-turn naming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_worktree_branch: Option<String>,
    /// Host-only: the revision at which each block last changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_revisions: Option<BTreeMap<String, i64>>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `HostSessionSummary`: the session list row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSessionSummary {
    pub project_id: String,
    pub revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub status: HostSessionStatus,
    pub updated_at: i64,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub title: String,
    pub harness: RemoteProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<RuntimeMode>,
    /// Always written, as `null` when unknown. Older cached summaries lack it.
    #[serde(default)]
    pub provider_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_worktree_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_work_item: Option<LinkedWorkItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_input: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteAttachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub kind: AttachmentKind,
    pub size: i64,
}

/// The session fields of a delta: everything but `blocks`.
pub type SessionMeta = Map<String, Value>;

/// A delta's session value: `HostSession` without `session.blocks` and
/// `blockRevisions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSessionDelta {
    pub session: SessionMeta,
    pub project_id: String,
    pub revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub status: HostSessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_worktree_branch: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `sessions.sync` sends only the blocks that changed after the client's
/// revision, so a long transcript is not re-downloaded on every poll.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SessionSync {
    #[serde(rename = "unchanged")]
    Unchanged { revision: i64 },
    #[serde(rename = "snapshot")]
    Snapshot { value: Box<HostSession> },
    #[serde(rename = "delta", rename_all = "camelCase")]
    Delta {
        base: i64,
        value: Box<HostSessionDelta>,
        block_ids: Vec<String>,
        blocks: Vec<Block>,
    },
}

/// A sync too large for one response. Its serialized JSON is read in bounded
/// pieces with `sessions.syncChunk`, so every piece describes one revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSyncTransfer {
    pub transfer: String,
    /// UTF-16 length of the serialized `SessionSync`.
    pub length: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSyncChunk {
    pub data: String,
}

/// `SessionSync | SessionSyncTransfer`, told apart by `kind: "chunked"`.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionSyncResponse {
    Sync(SessionSync),
    Chunked(SessionSyncTransfer),
}

impl Serialize for SessionSyncResponse {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Chunked<'a> {
            kind: &'static str,
            transfer: &'a str,
            length: i64,
        }
        match self {
            Self::Sync(sync) => sync.serialize(serializer),
            Self::Chunked(transfer) => Chunked {
                kind: "chunked",
                transfer: &transfer.transfer,
                length: transfer.length,
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for SessionSyncResponse {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.get("kind").and_then(Value::as_str) == Some("chunked") {
            serde_json::from_value(value)
                .map(Self::Chunked)
                .map_err(serde::de::Error::custom)
        } else {
            serde_json::from_value(value)
                .map(Self::Sync)
                .map_err(serde::de::Error::custom)
        }
    }
}

/// Fails when the delta does not apply to `known`; request a snapshot then.
pub fn apply_session_sync(
    known: Option<&HostSession>,
    sync: SessionSync,
) -> Result<HostSession, String> {
    let base = match &sync {
        SessionSync::Snapshot { value } => return Ok((**value).clone()),
        SessionSync::Unchanged { revision } => *revision,
        SessionSync::Delta { base, .. } => *base,
    };
    let Some(known) = known.filter(|known| known.revision == base) else {
        return Err("Session sync base does not match".into());
    };
    let SessionSync::Delta {
        value,
        block_ids,
        blocks,
        ..
    } = sync
    else {
        return Ok(known.clone());
    };
    let mut by_id: std::collections::HashMap<&str, &Block> = known
        .session
        .blocks
        .iter()
        .map(|block| (block.id.as_str(), block))
        .collect();
    for block in &blocks {
        by_id.insert(block.id.as_str(), block);
    }
    let blocks = block_ids
        .iter()
        .map(|id| {
            by_id
                .get(id.as_str())
                .map(|block| (*block).clone())
                .ok_or_else(|| "Session sync is missing a block".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let HostSessionDelta {
        mut session,
        project_id,
        revision,
        run_id,
        status,
        created_at,
        updated_at,
        archived,
        pinned,
        auto_worktree_branch,
        extra,
    } = *value;
    session.insert("blocks".into(), Value::Array(Vec::new()));
    let mut session: Session =
        serde_json::from_value(Value::Object(session)).map_err(|error| error.to_string())?;
    session.blocks = blocks;
    Ok(HostSession {
        session,
        project_id,
        revision,
        run_id,
        status,
        created_at,
        updated_at,
        archived,
        pinned,
        auto_worktree_branch,
        block_revisions: None,
        extra,
    })
}

/// `HostCommand`: what `commands.dispatch` carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HostCommand {
    #[serde(rename = "create", rename_all = "camelCase")]
    Create {
        command_id: String,
        project_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worktree_cwd: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auto_worktree_branch: Option<String>,
        harness: RemoteProvider,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_settings: Option<BTreeMap<String, String>>,
        runtime_mode: RuntimeMode,
    },
    #[serde(rename = "configure", rename_all = "camelCase")]
    Configure {
        command_id: String,
        session_id: String,
        model: String,
        model_settings: BTreeMap<String, String>,
        runtime_mode: RuntimeMode,
    },
    #[serde(rename = "compact", rename_all = "camelCase")]
    Compact {
        command_id: String,
        session_id: String,
    },
    #[serde(rename = "send", rename_all = "camelCase")]
    Send {
        command_id: String,
        session_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<RemoteAttachment>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<SendIntent>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft_block_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan_block_id: Option<String>,
    },
    #[serde(rename = "draft", rename_all = "camelCase")]
    Draft {
        command_id: String,
        session_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attachments: Option<Vec<RemoteAttachment>>,
    },
    #[serde(rename = "removeDraft", rename_all = "camelCase")]
    RemoveDraft {
        command_id: String,
        session_id: String,
        draft_block_id: String,
    },
    #[serde(rename = "cancel", rename_all = "camelCase")]
    Cancel {
        command_id: String,
        session_id: String,
        run_id: String,
    },
    #[serde(rename = "approve", rename_all = "camelCase")]
    Approve {
        command_id: String,
        session_id: String,
        run_id: String,
        request_id: i64,
        decision: ApprovalDecision,
    },
    #[serde(rename = "answer", rename_all = "camelCase")]
    Answer {
        command_id: String,
        session_id: String,
        run_id: String,
        request_id: i64,
        reply: UserQuestionReply,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendIntent {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "build")]
    Build,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
}

/// One session write, as reported by `changes.wait`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionChange {
    pub id: String,
    pub project_id: String,
    pub revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<bool>,
    /// Whether a turn is running after this write. Lets a desktop notice a
    /// finished turn in a tab it is not showing. Absent from older hosts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<HostSessionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busy: Option<bool>,
}

/// `reset` means the desktop's cursor is from another host run or too old;
/// it reloads what it shows and continues from `cursor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionChanges {
    pub boot: String,
    pub cursor: i64,
    pub sessions: Vec<SessionChange>,
    pub reset: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReceipt {
    pub command_id: String,
    pub session_id: String,
    pub revision: i64,
}

/// Credentials never leave the desktop's native connection store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMachine {
    pub id: String,
    pub name: String,
    /// How the desktop reaches the host, for display.
    pub endpoint: String,
    /// Direct TLS addresses; the desktop pins the host certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<Vec<String>>,
    pub environment_id: String,
    /// A fallback route through an SSH forward to the host's loopback port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<RemoteMachineSsh>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMachineSsh {
    pub target: String,
    #[serde(default)]
    pub port: Option<u16>,
    pub remote_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshSetupPrompt {
    pub id: String,
    pub message: String,
    pub confirm: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshSetup {
    pub id: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<SshSetupPrompt>,
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<RemoteMachine>,
}

pub fn is_remote_provider(value: &Value) -> bool {
    value.as_str().and_then(parse_provider).is_some()
}

pub fn require_host_descriptor(value: &Value) -> Result<HostDescriptor, String> {
    let incompatible = || "This machine is running an incompatible MonoCode Host".to_string();
    let compatible = value.get("protocolVersion").and_then(Value::as_f64)
        == Some(HOST_PROTOCOL_VERSION as f64)
        && value
            .get("environmentId")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        && value
            .get("providers")
            .and_then(Value::as_array)
            .is_some_and(|providers| providers.iter().all(is_remote_provider));
    if !compatible {
        return Err(incompatible());
    }
    serde_json::from_value(value.clone()).map_err(|_| incompatible())
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::BlockRole;
    use serde_json::json;

    fn known() -> HostSession {
        serde_json::from_value(json!({
            "projectId": "project",
            "revision": 4,
            "status": "running",
            "updatedAt": 0,
            "session": {
                "id": "session",
                "harness": "codex",
                "model": "codex:test",
                "modelSettings": {},
                "runtimeMode": "supervised",
                "cwd": "/host/repo",
                "title": "Work",
                "busy": true,
                "blocks": [
                    { "id": "user", "role": "user", "text": "Do it" },
                    { "id": "reply", "role": "assistant", "text": "Work", "streaming": true }
                ]
            }
        }))
        .unwrap()
    }

    fn delta_value(known: &HostSession, revision: i64, busy: bool) -> Box<HostSessionDelta> {
        let mut session = serde_json::to_value(&known.session).unwrap();
        let session = session.as_object_mut().unwrap();
        session.remove("blocks");
        session.insert("busy".into(), json!(busy));
        Box::new(HostSessionDelta {
            session: session.clone(),
            project_id: known.project_id.clone(),
            revision,
            run_id: None,
            status: known.status,
            created_at: None,
            updated_at: known.updated_at,
            archived: None,
            pinned: None,
            auto_worktree_branch: None,
            extra: Extra::new(),
        })
    }

    #[test]
    fn applies_changed_blocks_and_keeps_unchanged_ones() {
        let known = known();
        let next = apply_session_sync(
            Some(&known),
            SessionSync::Delta {
                base: 4,
                value: delta_value(&known, 6, false),
                block_ids: vec!["user".into(), "reply".into(), "done".into()],
                blocks: vec![
                    Block::new("reply", BlockRole::Assistant, "Work done"),
                    Block::new("done", BlockRole::System, "Finished"),
                ],
            },
        )
        .unwrap();
        assert_eq!(next.revision, 6);
        assert_eq!(next.session.busy, Some(false));
        assert_eq!(
            next.session
                .blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["Do it", "Work done", "Finished"]
        );
        assert_eq!(next.session.blocks[0], known.session.blocks[0]);
    }

    #[test]
    fn returns_the_known_value_when_nothing_changed() {
        let known = known();
        assert_eq!(
            apply_session_sync(Some(&known), SessionSync::Unchanged { revision: 4 }).unwrap(),
            known
        );
    }

    #[test]
    fn rejects_deltas_that_do_not_apply_so_the_caller_loads_a_snapshot() {
        let known = known();
        assert!(apply_session_sync(Some(&known), SessionSync::Unchanged { revision: 3 }).is_err());
        assert!(
            apply_session_sync(
                Some(&known),
                SessionSync::Delta {
                    base: 4,
                    value: delta_value(&known, 5, true),
                    block_ids: vec!["user".into(), "unknown".into()],
                    blocks: vec![],
                },
            )
            .is_err()
        );
        assert!(apply_session_sync(None, SessionSync::Unchanged { revision: 4 }).is_err());
    }

    #[test]
    fn sync_responses_round_trip_with_their_kind() {
        let chunked: SessionSyncResponse =
            serde_json::from_value(json!({ "kind": "chunked", "transfer": "t", "length": 9 }))
                .unwrap();
        assert_eq!(
            chunked,
            SessionSyncResponse::Chunked(SessionSyncTransfer {
                transfer: "t".into(),
                length: 9
            })
        );
        assert_eq!(
            serde_json::to_value(&chunked).unwrap(),
            json!({ "kind": "chunked", "transfer": "t", "length": 9 })
        );
        let unchanged: SessionSyncResponse =
            serde_json::from_value(json!({ "kind": "unchanged", "revision": 2 })).unwrap();
        assert_eq!(
            unchanged,
            SessionSyncResponse::Sync(SessionSync::Unchanged { revision: 2 })
        );
    }

    #[test]
    fn versions_compare_by_number_and_hosts_without_pushed_changes_need_updates() {
        assert_eq!(compare_versions("0.10.0", "0.9.9"), 1);
        assert_eq!(compare_versions("1.2.3-beta", "1.2.3"), 0);
        assert_eq!(compare_versions("1.2", "1.2.1"), -1);
        let mut host: HostDescriptor = serde_json::from_value(json!({
            "protocolVersion": 1,
            "environmentId": "env",
            "name": "box",
            "providers": ["codex"],
            "capabilities": ["sessions"],
            "hostVersion": "0.5.0"
        }))
        .unwrap();
        assert!(host_needs_update(&host, None));
        host.capabilities.push("changes.wait".into());
        assert!(!host_needs_update(&host, None));
        assert!(host_needs_update(&host, Some("0.6.0")));
        assert!(!host_needs_update(&host, Some("0.5.0")));
        assert_eq!(
            host_connect_command(Some("0.6.0")),
            "npx monocode-host@0.6.0 connect"
        );
        assert_eq!(host_connect_command(None), "npx monocode-host connect");
    }

    #[test]
    fn descriptors_must_name_this_protocol_and_known_providers() {
        let valid = json!({
            "protocolVersion": 1,
            "environmentId": "env",
            "name": "box",
            "providers": ["codex", "antigravity"],
            "capabilities": []
        });
        assert_eq!(
            require_host_descriptor(&valid).unwrap().providers,
            [HarnessId::Codex, HarnessId::Antigravity]
        );
        for bad in [
            json!({ "protocolVersion": 2, "environmentId": "env", "name": "", "providers": [], "capabilities": [] }),
            json!({ "protocolVersion": 1, "environmentId": "", "name": "", "providers": [], "capabilities": [] }),
            json!({ "protocolVersion": 1, "environmentId": "e", "name": "", "providers": ["vim"], "capabilities": [] }),
        ] {
            assert!(require_host_descriptor(&bad).is_err(), "{bad}");
        }
    }
}
