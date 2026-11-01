//! Composer settings and approval decisions shared by the native view and transports.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessMode {
    ReadOnly,
    #[default]
    Supervised,
    AutoAcceptEdits,
    Auto,
    FullAccess,
}
impl AccessMode {
    pub fn id(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Supervised => "supervised",
            Self::AutoAcceptEdits => "auto-accept-edits",
            Self::Auto => "auto",
            Self::FullAccess => "full-access",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read-only",
            Self::Supervised => "Supervised",
            Self::AutoAcceptEdits => "Auto-accept edits",
            Self::Auto => "Auto",
            Self::FullAccess => "Full access",
        }
    }
    pub fn from_id(id: &str) -> Self {
        match id {
            "read-only" => Self::ReadOnly,
            "auto-accept-edits" => Self::AutoAcceptEdits,
            "auto" => Self::Auto,
            "full-access" => Self::FullAccess,
            _ => Self::Supervised,
        }
    }
    /// codexProtocol.ts::runtimeModeToCodexConfig.
    pub fn codex(self) -> (Value, &'static str, Value, &'static str) {
        match self {
            Self::ReadOnly => (
                json!("never"),
                "read-only",
                json!({"type":"readOnly"}),
                "user",
            ),
            Self::Supervised => (
                json!("untrusted"),
                "read-only",
                json!({"type":"readOnly"}),
                "user",
            ),
            Self::AutoAcceptEdits => (
                json!("on-request"),
                "workspace-write",
                json!({"type":"workspaceWrite"}),
                "user",
            ),
            Self::Auto => (
                json!("on-request"),
                "workspace-write",
                json!({"type":"workspaceWrite"}),
                "auto_review",
            ),
            Self::FullAccess => (
                json!("on-request"),
                "danger-full-access",
                json!({"type":"dangerFullAccess"}),
                "user",
            ),
        }
    }
    pub fn claude(self) -> &'static str {
        match self {
            Self::ReadOnly => "plan",
            Self::Supervised => "default",
            Self::AutoAcceptEdits => "acceptEdits",
            Self::Auto => "auto",
            Self::FullAccess => "bypassPermissions",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Native remote control invocation, never saved as provider configuration.
    #[serde(skip)]
    pub remote_action: Option<serde_json::Value>,
    pub account: crate::accounts::Profile,
    pub usage_limit: Option<crate::usage::Limit>,
    /// Isolated background text jobs; never persisted as conversation settings.
    #[serde(skip)]
    pub text_only: bool,
    /// Transport-only context (for example selected MCP servers). The raw user
    /// message remains the transcript and queue label.
    #[serde(skip)]
    pub prepared_prompt: Option<String>,
    /// Immutable identity captured for this newly submitted turn.
    #[serde(skip)]
    pub turn_model: Option<TurnInfo>,
    #[serde(skip)]
    pub disable_claude_hooks: bool,
    #[serde(skip)]
    pub environment: std::collections::HashMap<String, String>,
    #[serde(skip)]
    pub mono: Option<MonoContext>,
    #[serde(skip)]
    pub mono_message_id: Option<String>,
    #[serde(skip)]
    pub mono_sent_at: Option<u64>,
    #[serde(skip)]
    pub internal_message: bool,
    #[serde(skip)]
    pub attachments: Vec<std::path::PathBuf>,
    pub revision: u64,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access: AccessMode,
    pub fast: bool,
    pub service_tier: Option<String>,
    pub thinking: Option<bool>,
    /// Provider-advertised controls beyond the common effort/speed switches.
    pub model_settings: std::collections::HashMap<String, String>,
    pub plan: bool,
    pub operator: bool,
    /// A submitted /operator grants this thread access independently of its next draft.
    pub operator_enabled: bool,
    pub orchestrator: bool,
    pub new_worktree: bool,
    pub worktree_base: Option<String>,
    pub worktree_cwd: Option<String>,
    pub worktree_removed: bool,
    pub handoff: Option<Handoff>,
    pub pending_provider_switch: Option<ProviderSwitch>,
    pub turns: std::collections::HashMap<usize, TurnInfo>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderSwitch {
    pub from: String,
    /// The outgoing model's display name, for the transfer status.
    pub model_label: String,
    pub cwd: String,
    pub model: Option<String>,
    pub model_settings: std::collections::HashMap<String, String>,
    pub effort: Option<String>,
    pub fast: bool,
    pub service_tier: Option<String>,
    pub thinking: Option<bool>,
    pub skip_outgoing_recap: bool,
    pub usage_limit: Option<crate::usage::Limit>,
    pub account: crate::accounts::Profile,
    pub provider_id: Option<String>,
}
#[derive(Clone, Debug)]
pub struct MonoContext {
    pub mono: crate::mono::Mono,
    pub root: std::path::PathBuf,
    pub executable: std::path::PathBuf,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnInfo {
    /// An accepted steer belongs to the provider run that opened this receipt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined_turn: Option<usize>,
    pub provider_turn_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub model_id: Option<String>,
    pub duration_ms: u64,
    /// Distinguishes a measured zero from a legacy turn without timing.
    pub duration_known: bool,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub metrics: Option<TurnMetrics>,
}
impl TurnInfo {
    pub fn has_duration(&self) -> bool {
        self.duration_known || self.duration_ms > 0
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Handoff {
    pub from: String,
    pub brief: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnMetrics {
    #[serde(alias = "inputTokens")]
    pub input_tokens: u64,
    #[serde(alias = "outputTokens")]
    pub output_tokens: u64,
    #[serde(alias = "cacheReadTokens")]
    pub cache_read_tokens: u64,
    #[serde(alias = "cacheWriteTokens")]
    pub cache_write_tokens: u64,
    #[serde(alias = "cacheHitPercent")]
    pub cache_hit_percent: Option<f64>,
}
impl TurnMetrics {
    pub fn reported(self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_write_tokens > 0
            || self.cache_hit_percent.is_some()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Approval {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub can_remember: bool,
}
#[derive(Clone, Debug)]
pub enum Decision {
    Steer(crate::follow_up::Message),
    Allow,
    Session,
    Deny,
    Answer(Vec<Vec<String>>),
    KeepOpen,
}
#[derive(Clone, Debug, PartialEq)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    pub description: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct QuestionItem {
    pub id: String,
    pub prompt: String,
    pub multiple: bool,
    pub allow_custom: bool,
    pub options: Vec<QuestionOption>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Question {
    pub id: String,
    pub title: String,
    pub items: Vec<QuestionItem>,
    pub auto_resolve_at: Option<u64>,
}
pub struct Control {
    pub id: String,
    pub decision: Decision,
}
