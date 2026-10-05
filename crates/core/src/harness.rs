//! Port of the provider and runtime-mode tables in
//! src/features/sessions/model/session.ts.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// An agent CLI MonoCode can drive. The variant order is `HARNESSES`, which
/// is also the order the model picker and catalogs use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HarnessId {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "grok")]
    Grok,
    #[serde(rename = "opencode")]
    Opencode,
    #[serde(rename = "pi")]
    Pi,
    #[serde(rename = "omp")]
    Omp,
    #[serde(rename = "fx")]
    Fx,
    #[serde(rename = "hermes")]
    Hermes,
    #[serde(rename = "droid")]
    Droid,
    #[serde(rename = "antigravity")]
    Antigravity,
}

/// `HARNESSES`.
pub const HARNESSES: [HarnessId; 11] = [
    HarnessId::Claude,
    HarnessId::Codex,
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

impl HarnessId {
    /// The id as stored in JSON and in the `sessions.harness` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            HarnessId::Claude => "claude",
            HarnessId::Codex => "codex",
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

    /// Parse a stored id. Returns `None` for ids this build does not know.
    pub fn parse(value: &str) -> Option<Self> {
        HARNESSES.into_iter().find(|id| id.as_str() == value)
    }

    /// `HARNESS_LABEL`: the short name used as a tab title prefix.
    pub const fn label(self) -> &'static str {
        self.as_str()
    }

    /// `HARNESS_TITLE`: the product name.
    pub const fn title(self) -> &'static str {
        match self {
            HarnessId::Claude => "Claude Code",
            HarnessId::Codex => "Codex",
            HarnessId::Cursor => "Cursor",
            HarnessId::Grok => "Grok Build",
            HarnessId::Opencode => "OpenCode",
            HarnessId::Pi => "Pi",
            HarnessId::Omp => "omp",
            HarnessId::Fx => "fx",
            HarnessId::Hermes => "Hermes Agent",
            HarnessId::Droid => "Factory Droid",
            HarnessId::Antigravity => "Antigravity",
        }
    }
}

impl fmt::Display for HarnessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for HarnessId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        HarnessId::parse(value).ok_or_else(|| format!("Unknown harness: {value}"))
    }
}

/// `harnessSupportsAttachments`: fx ACP rejects attachment prompt blocks.
pub fn harness_supports_attachments(id: HarnessId) -> bool {
    id != HarnessId::Fx
}

/// How much the agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RuntimeMode {
    #[default]
    #[serde(rename = "supervised")]
    Supervised,
    #[serde(rename = "auto-accept-edits")]
    AutoAcceptEdits,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "full-access")]
    FullAccess,
}

/// `RUNTIME_MODES`.
pub const RUNTIME_MODES: [RuntimeMode; 4] = [
    RuntimeMode::Supervised,
    RuntimeMode::AutoAcceptEdits,
    RuntimeMode::Auto,
    RuntimeMode::FullAccess,
];

/// `DEFAULT_RUNTIME_MODE`.
pub const DEFAULT_RUNTIME_MODE: RuntimeMode = RuntimeMode::Supervised;

impl RuntimeMode {
    /// The mode as stored in JSON and in the `sessions.runtime_mode` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            RuntimeMode::Supervised => "supervised",
            RuntimeMode::AutoAcceptEdits => "auto-accept-edits",
            RuntimeMode::Auto => "auto",
            RuntimeMode::FullAccess => "full-access",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        RUNTIME_MODES
            .into_iter()
            .find(|mode| mode.as_str() == value)
    }

    /// `RUNTIME_MODE_LABEL`.
    pub const fn label(self) -> &'static str {
        match self {
            RuntimeMode::Supervised => "Supervised",
            RuntimeMode::AutoAcceptEdits => "Auto-accept edits",
            RuntimeMode::Auto => "Auto",
            RuntimeMode::FullAccess => "Full access",
        }
    }

    /// `RUNTIME_MODE_HINT`.
    pub const fn hint(self) -> &'static str {
        match self {
            RuntimeMode::Supervised => "Ask before commands and file changes.",
            RuntimeMode::AutoAcceptEdits => "Auto-approve edits, ask before other actions.",
            RuntimeMode::Auto => "An AI reviewer can approve or deny actions.",
            RuntimeMode::FullAccess => {
                "Allow commands, edits, and supported MCP confirmations in non-plan turns without prompts."
            }
        }
    }
}

impl fmt::Display for RuntimeMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RuntimeMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        RuntimeMode::parse(value).ok_or_else(|| format!("Unknown runtime mode: {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_ids_round_trip_through_json() {
        for id in HARNESSES {
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, format!("\"{}\"", id.as_str()));
            assert_eq!(serde_json::from_str::<HarnessId>(&json).unwrap(), id);
            assert_eq!(HarnessId::parse(id.as_str()), Some(id));
        }
        assert!(HARNESSES.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn runtime_modes_round_trip_through_json() {
        for mode in RUNTIME_MODES {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, format!("\"{}\"", mode.as_str()));
            assert_eq!(serde_json::from_str::<RuntimeMode>(&json).unwrap(), mode);
        }
    }
}
