//! What the MCP settings page reads and changes. The connection shapes
//! mirror `monocode_engine::submit::mcp` and `mcp_settings_cache` (ports of
//! src/features/settings/model/mcp.ts and mcpSettingsCache.ts), with the
//! same JSON names.
//!
//! The engine's `McpSettingsCache` shares discovery between this page and
//! the composer's MCP picker, and Claude's slower health check never delays
//! the list. [`McpData`] exposes that cache plus the commands the page
//! runs (`mcp_add`, `mcp_provider_login`, `claude_mcp_remove`, reveal, and
//! the confirm dialog).

use std::collections::HashMap;

use gpui::{App, AppContext as _, Entity, Subscription, Task, Window};
use serde::{Deserialize, Serialize};

use crate::data::{DataTask, Listener};

/// `McpConnection.provider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum McpProvider {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "claude_desktop")]
    ClaudeDesktop,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "opencode")]
    Opencode,
}

impl McpProvider {
    /// `PROVIDERS`, in chip order.
    pub const ALL: [McpProvider; 5] = [
        McpProvider::Claude,
        McpProvider::ClaudeDesktop,
        McpProvider::Codex,
        McpProvider::Cursor,
        McpProvider::Opencode,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            McpProvider::Claude => "claude",
            McpProvider::ClaudeDesktop => "claude_desktop",
            McpProvider::Codex => "codex",
            McpProvider::Cursor => "cursor",
            McpProvider::Opencode => "opencode",
        }
    }

    /// `MCP_PROVIDER_LABELS`.
    pub fn label(self) -> &'static str {
        match self {
            McpProvider::Claude => "Claude Code",
            McpProvider::ClaudeDesktop => "Claude Desktop",
            McpProvider::Codex => "Codex",
            McpProvider::Cursor => "Cursor",
            McpProvider::Opencode => "OpenCode",
        }
    }

    /// `SCOPES[provider]`: where the provider can add a server.
    pub fn scopes(self) -> &'static [McpScope] {
        match self {
            McpProvider::Claude => &[McpScope::Local, McpScope::Project, McpScope::User],
            McpProvider::ClaudeDesktop | McpProvider::Codex => &[McpScope::User],
            McpProvider::Cursor | McpProvider::Opencode => &[McpScope::Project, McpScope::User],
        }
    }

    /// The harness whose logo stands for the provider (`ProviderIcon`).
    pub fn harness_id(self) -> &'static str {
        match self {
            McpProvider::Claude | McpProvider::ClaudeDesktop => "claude",
            McpProvider::Codex => "codex",
            McpProvider::Cursor => "cursor",
            McpProvider::Opencode => "opencode",
        }
    }
}

/// `McpConnection.scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum McpScope {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "project")]
    Project,
    #[serde(rename = "user")]
    User,
}

impl McpScope {
    pub fn as_str(self) -> &'static str {
        match self {
            McpScope::Local => "local",
            McpScope::Project => "project",
            McpScope::User => "user",
        }
    }

    /// `option[0].toUpperCase() + option.slice(1)`.
    pub fn label(self) -> &'static str {
        match self {
            McpScope::Local => "Local",
            McpScope::Project => "Project",
            McpScope::User => "User",
        }
    }
}

/// `McpConnection`: one configured MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConnection {
    pub provider: McpProvider,
    pub name: String,
    pub scope: McpScope,
    pub config_path: String,
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// `McpServerRow`: a connection with its status text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerRow {
    pub connection: McpConnection,
    pub status: String,
}

/// `McpSettingsSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpSettingsSnapshot {
    pub servers: Vec<McpServerRow>,
    pub error: String,
    pub claude_error: String,
}

/// The MCP cache and commands. See the module docs.
pub trait McpData: 'static {
    /// `getCachedMcpSettings`.
    fn cached(&self, cwd: &str, cx: &App) -> Option<McpSettingsSnapshot>;
    /// `subscribeMcpSettings`: runs when `cwd`'s snapshot changes, such as
    /// when Claude's health check lands.
    fn subscribe(&self, cwd: &str, listener: Listener, cx: &mut App) -> Subscription;
    /// `loadMcpSettings(cwd, force)`.
    fn load(&self, cwd: &str, force: bool, cx: &mut App) -> Task<McpSettingsSnapshot>;
    /// `mcp_provider_login`: opens the browser when the provider supports it.
    fn login(&self, cwd: &str, provider: McpProvider, name: &str, cx: &mut App) -> DataTask<()>;
    /// `claude_mcp_remove`.
    fn remove(&self, cwd: &str, name: &str, scope: McpScope, cx: &mut App) -> DataTask<()>;
    /// `mcp_add` with the pasted JSON.
    fn add(
        &self,
        cwd: &str,
        provider: McpProvider,
        scope: McpScope,
        name: &str,
        config: &str,
        cx: &mut App,
    ) -> DataTask<()>;
    /// `revealPath`.
    fn reveal(&self, path: &str, cx: &mut App) -> DataTask<()>;
    /// The `ask` dialog: resolves to whether the user agreed.
    fn confirm(&self, message: &str, title: &str, window: &mut Window, cx: &mut App) -> Task<bool>;
}

/// One recorded command, for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpCall {
    Load {
        cwd: String,
        force: bool,
    },
    Login {
        cwd: String,
        provider: McpProvider,
        name: String,
    },
    Remove {
        cwd: String,
        name: String,
        scope: McpScope,
    },
    Add {
        cwd: String,
        provider: McpProvider,
        scope: McpScope,
        name: String,
        config: String,
    },
    Reveal {
        path: String,
    },
}

/// The state behind [`LocalMcp`].
#[derive(Default)]
pub struct LocalMcpState {
    /// What a load of each project discovers.
    pub discovered: HashMap<String, McpSettingsSnapshot>,
    cache: HashMap<String, McpSettingsSnapshot>,
    pub calls: Vec<McpCall>,
    /// The answer `confirm` gives.
    pub confirm: bool,
    /// Make the next command fail with this message.
    pub fail_next: Option<String>,
}

/// An in-memory MCP cache for the gallery and tests. Loads answer at once.
#[derive(Clone)]
pub struct LocalMcp {
    state: Entity<LocalMcpState>,
}

impl LocalMcp {
    pub fn new(cx: &mut App) -> Self {
        Self {
            state: cx.new(|_| LocalMcpState {
                confirm: true,
                ..Default::default()
            }),
        }
    }

    pub fn state(&self) -> &Entity<LocalMcpState> {
        &self.state
    }

    /// What a load of `cwd` will discover.
    pub fn set_discovered(&self, cwd: &str, snapshot: McpSettingsSnapshot, cx: &mut App) {
        self.state.update(cx, |state, _| {
            state.discovered.insert(cwd.to_string(), snapshot);
        });
    }

    /// Replace the cached snapshot and tell subscribers, as a health check
    /// landing does.
    pub fn publish(&self, cwd: &str, snapshot: McpSettingsSnapshot, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            state.cache.insert(cwd.to_string(), snapshot);
            cx.notify();
        });
    }

    pub fn calls(&self, cx: &App) -> Vec<McpCall> {
        self.state.read(cx).calls.clone()
    }

    fn command(&self, call: McpCall, cx: &mut App) -> DataTask<()> {
        let failure = self.state.update(cx, |state, _| {
            state.calls.push(call);
            state.fail_next.take()
        });
        Task::ready(match failure {
            Some(message) => Err(message),
            None => Ok(()),
        })
    }
}

impl McpData for LocalMcp {
    fn cached(&self, cwd: &str, cx: &App) -> Option<McpSettingsSnapshot> {
        self.state.read(cx).cache.get(cwd).cloned()
    }

    fn subscribe(&self, _cwd: &str, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.state, move |_, cx| listener(cx))
    }

    fn load(&self, cwd: &str, force: bool, cx: &mut App) -> Task<McpSettingsSnapshot> {
        let snapshot = self.state.update(cx, |state, _| {
            state.calls.push(McpCall::Load {
                cwd: cwd.to_string(),
                force,
            });
            if !force && let Some(cached) = state.cache.get(cwd) {
                return cached.clone();
            }
            let snapshot = state.discovered.get(cwd).cloned().unwrap_or_default();
            state.cache.insert(cwd.to_string(), snapshot.clone());
            snapshot
        });
        Task::ready(snapshot)
    }

    fn login(&self, cwd: &str, provider: McpProvider, name: &str, cx: &mut App) -> DataTask<()> {
        self.command(
            McpCall::Login {
                cwd: cwd.into(),
                provider,
                name: name.into(),
            },
            cx,
        )
    }

    fn remove(&self, cwd: &str, name: &str, scope: McpScope, cx: &mut App) -> DataTask<()> {
        self.command(
            McpCall::Remove {
                cwd: cwd.into(),
                name: name.into(),
                scope,
            },
            cx,
        )
    }

    fn add(
        &self,
        cwd: &str,
        provider: McpProvider,
        scope: McpScope,
        name: &str,
        config: &str,
        cx: &mut App,
    ) -> DataTask<()> {
        self.command(
            McpCall::Add {
                cwd: cwd.into(),
                provider,
                scope,
                name: name.into(),
                config: config.into(),
            },
            cx,
        )
    }

    fn reveal(&self, path: &str, cx: &mut App) -> DataTask<()> {
        self.command(McpCall::Reveal { path: path.into() }, cx)
    }

    fn confirm(&self, _: &str, _: &str, _: &mut Window, cx: &mut App) -> Task<bool> {
        Task::ready(self.state.read(cx).confirm)
    }
}
