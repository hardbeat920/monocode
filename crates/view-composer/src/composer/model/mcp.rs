//! Port of the tag half of src/features/sessions/model/mcpPicker.ts: the
//! inline `@mcp/name` references the composer inserts and highlights, the
//! context line they add to a sent message, and the picker's ordering.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

/// `McpConnection` from src/features/settings/model/mcp.ts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct McpConnection {
    /// `claude`, `claude_desktop`, `codex`, `cursor`, or `opencode`.
    pub provider: String,
    pub name: String,
    /// `local`, `project`, or `user`.
    pub scope: String,
    pub config_path: String,
    pub transport: String,
    /// `None` reads as enabled.
    pub enabled: Option<bool>,
}

/// `McpTag`: a server and the token that names it in the draft.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct McpTag {
    pub server: McpConnection,
    pub token: String,
}

/// `newMcpTag`: the shortest token no other tag uses.
pub fn new_mcp_tag(server: &McpConnection, existing: &[McpTag]) -> McpTag {
    let used: Vec<&str> = existing.iter().map(|tag| tag.token.as_str()).collect();
    let candidates = [
        format!("@mcp/{}", server.name),
        format!("@mcp/{}/{}", server.provider, server.name),
        format!("@mcp/{}/{}/{}", server.provider, server.scope, server.name),
    ];
    let token = candidates
        .iter()
        .find(|candidate| !used.contains(&candidate.as_str()))
        .cloned()
        .unwrap_or_else(|| {
            let mut suffix = 2;
            while used.contains(&format!("{}-{suffix}", candidates[2]).as_str()) {
                suffix += 1;
            }
            format!("{}-{suffix}", candidates[2])
        });
    McpTag {
        server: server.clone(),
        token,
    }
}

/// One piece of `mcpTagParts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpTagPart {
    pub range: Range<usize>,
    pub tag: Option<McpTag>,
}

fn before_ok(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(c) => c.is_whitespace() || matches!(c, '(' | '[' | '{'),
    }
}

fn after_ok(c: Option<char>) -> bool {
    match c {
        None => true,
        Some(c) => c.is_whitespace() || matches!(c, ')' | ']' | '}' | '.' | '!' | '?' | ';' | ','),
    }
}

/// `mcpTagParts`: split `text` around tag tokens that stand on their own.
/// Parts carry byte ranges; a tie at one position goes to the longer token.
pub fn mcp_tag_parts(text: &str, tags: &[McpTag]) -> Vec<McpTagPart> {
    if text.is_empty() {
        return Vec::new();
    }
    if tags.is_empty() {
        return vec![McpTagPart {
            range: 0..text.len(),
            tag: None,
        }];
    }
    let mut parts = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let mut hit: Option<(usize, &McpTag)> = None;
        for tag in tags {
            if tag.token.is_empty() {
                continue;
            }
            let mut from = cursor;
            let mut found = None;
            while let Some(offset) = text[from..].find(&tag.token) {
                let start = from + offset;
                let end = start + tag.token.len();
                let before = before_ok(text[..start].chars().next_back());
                let after = after_ok(text[end..].chars().next());
                if before && after {
                    found = Some(start);
                    break;
                }
                // `indexOf(token, start + 1)`.
                from = start + text[start..].chars().next().map_or(1, char::len_utf8);
            }
            if let Some(start) = found
                && hit.is_none_or(|(at, current)| {
                    start < at || (start == at && tag.token.len() > current.token.len())
                })
            {
                hit = Some((start, tag));
            }
        }
        let Some((start, tag)) = hit else {
            break;
        };
        if start > cursor {
            parts.push(McpTagPart {
                range: cursor..start,
                tag: None,
            });
        }
        let end = start + tag.token.len();
        parts.push(McpTagPart {
            range: start..end,
            tag: Some(tag.clone()),
        });
        cursor = end;
    }
    if cursor < text.len() {
        parts.push(McpTagPart {
            range: cursor..text.len(),
            tag: None,
        });
    }
    parts
}

/// `taggedMcpServers`: the servers whose tags are still in the text.
pub fn tagged_mcp_servers(text: &str, tags: &[McpTag]) -> Vec<McpConnection> {
    let present: Vec<String> = mcp_tag_parts(text, tags)
        .into_iter()
        .filter_map(|part| part.tag.map(|tag| tag.token))
        .collect();
    tags.iter()
        .filter(|tag| present.contains(&tag.token))
        .map(|tag| tag.server.clone())
        .collect()
}

/// `mcpContextText`: tells the agent which configured servers to use.
pub fn mcp_context_text(servers: &[McpConnection], text: &str) -> String {
    if servers.is_empty() {
        return text.to_string();
    }
    let names = servers
        .iter()
        .map(|server| {
            format!(
                "{} ({})",
                serde_json::to_string(&server.name).unwrap_or_default(),
                server.provider
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let plural = if servers.len() == 1 { "" } else { "s" };
    format!(
        "MCP context: Use the configured server{plural} {names} when relevant to this request.\n\n{text}"
    )
}

/// `McpPickerServer.availability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum McpAvailability {
    Available,
    Authentication,
    Unavailable,
}

/// `McpPickerServer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpPickerServer {
    pub server: McpConnection,
    pub availability: McpAvailability,
    pub detail: &'static str,
}

/// `MCP_PROVIDER_LABELS`.
pub fn mcp_provider_label(provider: &str) -> &str {
    match provider {
        "claude" => "Claude Code",
        "claude_desktop" => "Claude Desktop",
        "codex" => "Codex",
        "cursor" => "Cursor",
        "opencode" => "OpenCode",
        other => other,
    }
}

static AUTH_STATUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)auth|sign.?in|log.?in|unauthorized").unwrap());
static FAILED_STATUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)failed|error|offline|unreachable|disconnected").unwrap());

/// `mcpPickerServers`: servers matching `query`, usable ones first, each
/// with why it can or cannot be picked for `harness`.
pub fn mcp_picker_servers(
    connections: &[McpConnection],
    harness: &str,
    claude_status: &HashMap<String, String>,
    query: &str,
) -> Vec<McpPickerServer> {
    let search = monocode_core::js::trim(query).to_lowercase();
    let mut out: Vec<McpPickerServer> = connections
        .iter()
        .filter(|server| {
            format!(
                "{} {} {}",
                server.name,
                mcp_provider_label(&server.provider),
                server.scope
            )
            .to_lowercase()
            .contains(&search)
        })
        .map(|server| {
            let status = (server.provider == "claude")
                .then(|| claude_status.get(&server.name))
                .flatten();
            let matches = server.provider == harness;
            let disabled = server.enabled == Some(false);
            let authentication = matches && status.is_some_and(|s| AUTH_STATUS.is_match(s));
            let failed = matches && status.is_some_and(|s| FAILED_STATUS.is_match(s));
            let availability = if !matches || disabled || failed {
                McpAvailability::Unavailable
            } else if authentication {
                McpAvailability::Authentication
            } else {
                McpAvailability::Available
            };
            let detail = if !matches {
                "Different provider"
            } else if disabled {
                "Disabled in provider configuration"
            } else if failed {
                "Connection unavailable"
            } else {
                "Configured for this provider"
            };
            McpPickerServer {
                server: server.clone(),
                availability,
                detail,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        a.availability
            .cmp(&b.availability)
            .then_with(|| locale_compare(&a.server.name, &b.server.name))
            .then_with(|| locale_compare(&a.server.provider, &b.server.provider))
    });
    out
}

fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(provider: &str, name: &str, scope: &str, config: &str) -> McpConnection {
        McpConnection {
            provider: provider.into(),
            name: name.into(),
            scope: scope.into(),
            config_path: config.into(),
            transport: "stdio".into(),
            enabled: None,
        }
    }

    fn docs() -> McpConnection {
        server("claude", "docs", "project", "/repo/.mcp.json")
    }

    fn servers() -> Vec<McpConnection> {
        vec![
            McpConnection {
                transport: "stdio".into(),
                ..server("cursor", "other", "user", "/cursor")
            },
            McpConnection {
                transport: "http".into(),
                ..server("claude", "needs-login", "user", "/claude")
            },
            docs(),
        ]
    }

    #[test]
    fn prioritizes_usable_servers_while_retaining_authentication_and_other_providers() {
        let status: HashMap<String, String> = [(
            "needs-login".to_string(),
            "Needs authentication".to_string(),
        )]
        .into();
        let ranked: Vec<(String, McpAvailability)> =
            mcp_picker_servers(&servers(), "claude", &status, "")
                .into_iter()
                .map(|row| (row.server.name, row.availability))
                .collect();
        assert_eq!(
            ranked,
            vec![
                ("docs".to_string(), McpAvailability::Available),
                ("needs-login".to_string(), McpAvailability::Authentication),
                ("other".to_string(), McpAvailability::Unavailable),
            ]
        );
        let names: Vec<String> =
            mcp_picker_servers(&servers(), "claude", &HashMap::new(), "cursor")
                .into_iter()
                .map(|row| row.server.name)
                .collect();
        assert_eq!(names, ["other"]);
    }

    #[test]
    fn makes_disabled_provider_entries_unselectable_even_when_health_says_connected() {
        for provider in ["claude", "codex", "opencode"] {
            let mut entry = docs();
            entry.provider = provider.into();
            entry.enabled = Some(false);
            let status: HashMap<String, String> =
                [("docs".to_string(), "Connected".to_string())].into();
            let rows = mcp_picker_servers(&[entry], provider, &status, "");
            assert_eq!(rows[0].availability, McpAvailability::Unavailable);
            assert_eq!(rows[0].detail, "Disabled in provider configuration");
        }
    }

    #[test]
    fn adds_only_selected_server_names_to_outgoing_context() {
        assert!(mcp_context_text(&[docs()], "Find the docs").contains("\"docs\" (claude)"));
        assert_eq!(mcp_context_text(&[], "Find the docs"), "Find the docs");
    }

    #[test]
    fn keeps_mcp_references_inline_and_only_uses_tags_still_in_the_draft() {
        let docs_tag = new_mcp_tag(&docs(), &[]);
        let mut cursor_docs = docs();
        cursor_docs.provider = "cursor".into();
        let another = new_mcp_tag(&cursor_docs, std::slice::from_ref(&docs_tag));
        assert_eq!(docs_tag.token, "@mcp/docs");
        assert_eq!(another.token, "@mcp/cursor/docs");
        let tags = [docs_tag.clone(), another.clone()];
        let text = format!("Ask {} about this, then {}.", docs_tag.token, another.token);
        assert_eq!(
            mcp_tag_parts(&text, &tags)
                .iter()
                .filter(|part| part.tag.is_some())
                .count(),
            2
        );
        assert_eq!(tagged_mcp_servers(&text, &tags).len(), 2);
        assert!(
            tagged_mcp_servers(
                &format!("Ask {}2 about this", docs_tag.token),
                std::slice::from_ref(&docs_tag)
            )
            .is_empty()
        );
        assert!(tagged_mcp_servers("Ask about this", &[docs_tag]).is_empty());
    }

    #[test]
    fn falls_back_to_a_numbered_token_when_every_name_is_taken() {
        let first = new_mcp_tag(&docs(), &[]);
        let second = new_mcp_tag(&docs(), std::slice::from_ref(&first));
        let third = new_mcp_tag(&docs(), &[first.clone(), second.clone()]);
        let fourth = new_mcp_tag(&docs(), &[first, second, third.clone()]);
        assert_eq!(third.token, "@mcp/claude/project/docs");
        assert_eq!(fourth.token, "@mcp/claude/project/docs-2");
    }

    #[test]
    fn prefers_the_longer_token_at_the_same_position() {
        let short = McpTag {
            server: docs(),
            token: "@mcp/docs".into(),
        };
        let long = McpTag {
            server: docs(),
            token: "@mcp/docs/extra".into(),
        };
        let parts = mcp_tag_parts("use @mcp/docs/extra now", &[short, long.clone()]);
        assert_eq!(parts[1].tag.as_ref(), Some(&long));
        assert_eq!(parts[1].range, 4..19);
    }
}
