//! Port of src/features/settings/model/mcp.ts: MCP connection rows and the
//! parser for `claude mcp list`.

use serde::{Deserialize, Serialize};

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

/// `McpServer`: a name and its health text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub status: String,
}

/// `parseClaudeMcpList`: Claude's list output is for humans; keep only names
/// and health text.
pub fn parse_claude_mcp_list(output: &str) -> Vec<McpServer> {
    output
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter_map(|line| {
            // `/^([A-Za-z0-9_-]+):\s+(.+)$/`
            let colon = line.find(':')?;
            let name = &line[..colon];
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return None;
            }
            let rest = &line[colon + 1..];
            let detail = rest.trim_start_matches(monocode_core::js::is_space);
            if detail.len() == rest.len() || detail.is_empty() {
                return None;
            }
            let status = split_detail(detail).pop().unwrap_or(detail);
            Some(McpServer {
                name: name.to_string(),
                status: status.to_string(),
            })
        })
        .collect()
}

/// `detail.split(/ - | — /)`.
fn split_detail(detail: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < detail.len() {
        let rest = &detail[index..];
        let separator = [" - ", " — "].into_iter().find(|sep| rest.starts_with(sep));
        match separator {
            Some(sep) => {
                parts.push(&detail[start..index]);
                index += sep.len();
                start = index;
            }
            None => index += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    parts.push(&detail[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_server_names_and_health_while_omitting_launch_details() {
        assert_eq!(
            parse_claude_mcp_list(
                "Checking MCP server health...\nnotion: https://example.com/mcp - ! Needs authentication\nlocal_tools: npx tools - --token secret - ✔ Connected\n"
            ),
            vec![
                McpServer {
                    name: "notion".into(),
                    status: "! Needs authentication".into()
                },
                McpServer {
                    name: "local_tools".into(),
                    status: "✔ Connected".into()
                },
            ]
        );
    }
}
