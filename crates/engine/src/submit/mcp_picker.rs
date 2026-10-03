//! Port of src/features/sessions/model/mcpPicker.ts: the `/mcp` picker rows,
//! the `@mcp/...` tags a draft carries, and the context line those tags add
//! to an outgoing prompt.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use monocode_core::{HarnessId, js};
use regex::Regex;

use super::mcp::McpConnection;
use super::text::json_string;

/// `McpPickerServer.availability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum McpAvailability {
    Available,
    Authentication,
    Unavailable,
}

impl McpAvailability {
    fn priority(self) -> u8 {
        match self {
            McpAvailability::Available => 0,
            McpAvailability::Authentication => 1,
            McpAvailability::Unavailable => 2,
        }
    }
}

/// `McpPickerServer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpPickerServer {
    pub server: McpConnection,
    pub availability: McpAvailability,
    pub detail: &'static str,
}

/// `McpTag`: a server and the `@mcp/...` token that names it in a draft.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct McpTag {
    pub server: McpConnection,
    pub token: String,
}

/// One piece of draft text, tagged when it is an MCP token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpTagPart {
    pub text: String,
    pub tag: Option<McpTag>,
}

static AUTHENTICATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)auth|sign.?in|log.?in|unauthorized").unwrap());
static FAILED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)failed|error|offline|unreachable|disconnected").unwrap());

/// `newMcpTag`: the shortest token no existing tag uses.
pub fn new_mcp_tag(server: &McpConnection, existing: &[McpTag]) -> McpTag {
    let used: HashSet<&str> = existing.iter().map(|tag| tag.token.as_str()).collect();
    let provider = server.provider.as_str();
    let candidates = [
        format!("@mcp/{}", server.name),
        format!("@mcp/{provider}/{}", server.name),
        format!("@mcp/{provider}/{}/{}", server.scope.as_str(), server.name),
    ];
    let token = match candidates
        .iter()
        .find(|candidate| !used.contains(candidate.as_str()))
    {
        Some(token) => token.clone(),
        None => {
            let mut suffix = 2;
            while used.contains(format!("{}-{suffix}", candidates[2]).as_str()) {
                suffix += 1;
            }
            format!("{}-{suffix}", candidates[2])
        }
    };
    McpTag {
        server: server.clone(),
        token,
    }
}

fn opens_token(c: Option<char>) -> bool {
    c.is_none_or(|c| js::is_space(c) || matches!(c, '(' | '[' | '{'))
}

fn closes_token(c: Option<char>) -> bool {
    c.is_none_or(|c| js::is_space(c) || matches!(c, ')' | ']' | '}' | '.' | '!' | '?' | ';' | ','))
}

/// `mcpTagParts`: split draft text at the tokens of `tags`. Offsets are
/// bytes.
pub fn mcp_tag_parts(text: &str, tags: &[McpTag]) -> Vec<McpTagPart> {
    if text.is_empty() || tags.is_empty() {
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![McpTagPart {
                text: text.to_string(),
                tag: None,
            }]
        };
    }
    let mut parts = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let mut hit: Option<(usize, &McpTag)> = None;
        for tag in tags {
            let mut start = text[cursor..]
                .find(&tag.token)
                .map(|offset| cursor + offset);
            while let Some(found) = start {
                let end = found + tag.token.len();
                let before = opens_token(text[..found].chars().next_back());
                let after = closes_token(text[end..].chars().next());
                if before && after {
                    break;
                }
                let from = found + 1;
                start = text[from..].find(&tag.token).map(|offset| from + offset);
            }
            if let Some(found) = start {
                let better = match hit {
                    None => true,
                    Some((best, best_tag)) => {
                        found < best || (found == best && tag.token.len() > best_tag.token.len())
                    }
                };
                if better {
                    hit = Some((found, tag));
                }
            }
        }
        let Some((start, tag)) = hit else {
            break;
        };
        if start > cursor {
            parts.push(McpTagPart {
                text: text[cursor..start].to_string(),
                tag: None,
            });
        }
        parts.push(McpTagPart {
            text: tag.token.clone(),
            tag: Some(tag.clone()),
        });
        cursor = start + tag.token.len();
    }
    if cursor < text.len() {
        parts.push(McpTagPart {
            text: text[cursor..].to_string(),
            tag: None,
        });
    }
    parts
}

/// `taggedMcpServers`: the servers whose tags are still in the draft.
pub fn tagged_mcp_servers(text: &str, tags: &[McpTag]) -> Vec<McpConnection> {
    let present: HashSet<String> = mcp_tag_parts(text, tags)
        .into_iter()
        .filter_map(|part| part.tag.map(|tag| tag.token))
        .collect();
    tags.iter()
        .filter(|tag| present.contains(&tag.token))
        .map(|tag| tag.server.clone())
        .collect()
}

/// `localeCompare` for server and provider names.
fn locale_compare(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

/// `mcpPickerServers`: matching rows, usable ones first.
pub fn mcp_picker_servers(
    connections: &[McpConnection],
    harness: HarnessId,
    claude_status: &HashMap<String, String>,
    query: &str,
) -> Vec<McpPickerServer> {
    let search = js::trim(query).to_lowercase();
    let mut rows: Vec<McpPickerServer> = connections
        .iter()
        .filter(|server| {
            format!(
                "{} {} {}",
                server.name,
                server.provider.label(),
                server.scope.as_str()
            )
            .to_lowercase()
            .contains(&search)
        })
        .map(|server| {
            let status = (server.provider == super::mcp::McpProvider::Claude)
                .then(|| claude_status.get(&server.name))
                .flatten();
            let matches = server.provider.as_str() == harness.as_str();
            let disabled = server.enabled == Some(false);
            let authentication =
                matches && status.is_some_and(|status| AUTHENTICATION.is_match(status));
            let failed = matches && status.is_some_and(|status| FAILED.is_match(status));
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
    rows.sort_by(|a, b| {
        a.availability
            .priority()
            .cmp(&b.availability.priority())
            .then_with(|| locale_compare(&a.server.name, &b.server.name))
            .then_with(|| locale_compare(a.server.provider.as_str(), b.server.provider.as_str()))
    });
    rows
}

/// `mcpContextText`: name the selected servers ahead of the prompt.
pub fn mcp_context_text(servers: &[McpConnection], text: &str) -> String {
    if servers.is_empty() {
        return text.to_string();
    }
    let names = servers
        .iter()
        .map(|server| {
            format!(
                "{} ({})",
                json_string(&server.name),
                server.provider.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let plural = if servers.len() == 1 { "" } else { "s" };
    format!(
        "MCP context: Use the configured server{plural} {names} when relevant to this request.\n\n{text}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::submit::mcp::{McpProvider, McpScope};

    fn connection(
        provider: McpProvider,
        name: &str,
        scope: McpScope,
        config: &str,
        transport: &str,
    ) -> McpConnection {
        McpConnection {
            provider,
            name: name.into(),
            scope,
            config_path: config.into(),
            transport: transport.into(),
            enabled: None,
        }
    }

    fn servers() -> Vec<McpConnection> {
        vec![
            connection(
                McpProvider::Cursor,
                "other",
                McpScope::User,
                "/cursor",
                "stdio",
            ),
            connection(
                McpProvider::Claude,
                "needs-login",
                McpScope::User,
                "/claude",
                "http",
            ),
            connection(
                McpProvider::Claude,
                "docs",
                McpScope::Project,
                "/repo/.mcp.json",
                "stdio",
            ),
        ]
    }

    #[test]
    fn prioritizes_usable_servers_while_retaining_authentication_and_other_providers() {
        let status = HashMap::from([(
            "needs-login".to_string(),
            "Needs authentication".to_string(),
        )]);
        let ranked: Vec<(String, McpAvailability)> =
            mcp_picker_servers(&servers(), HarnessId::Claude, &status, "")
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
        let filtered: Vec<String> =
            mcp_picker_servers(&servers(), HarnessId::Claude, &HashMap::new(), "cursor")
                .into_iter()
                .map(|row| row.server.name)
                .collect();
        assert_eq!(filtered, ["other"]);
    }

    #[test]
    fn adds_only_selected_server_names_to_outgoing_context() {
        assert!(mcp_context_text(&servers()[2..], "Find the docs").contains("\"docs\" (claude)"));
        assert_eq!(mcp_context_text(&[], "Find the docs"), "Find the docs");
    }

    #[test]
    fn makes_disabled_provider_entries_unselectable_even_when_health_says_connected() {
        for (provider, harness) in [
            (McpProvider::Claude, HarnessId::Claude),
            (McpProvider::Codex, HarnessId::Codex),
            (McpProvider::Opencode, HarnessId::Opencode),
        ] {
            let disabled = McpConnection {
                provider,
                enabled: Some(false),
                ..servers()[2].clone()
            };
            let status = HashMap::from([("docs".to_string(), "Connected".to_string())]);
            let rows = mcp_picker_servers(&[disabled], harness, &status, "");
            assert_eq!(rows[0].availability, McpAvailability::Unavailable);
            assert_eq!(rows[0].detail, "Disabled in provider configuration");
        }
    }

    #[test]
    fn keeps_mcp_references_inline_and_only_uses_tags_still_in_the_draft() {
        let docs = new_mcp_tag(&servers()[2], &[]);
        let cursor_docs = McpConnection {
            provider: McpProvider::Cursor,
            ..servers()[2].clone()
        };
        let another_docs = new_mcp_tag(&cursor_docs, std::slice::from_ref(&docs));
        assert_eq!(docs.token, "@mcp/docs");
        assert_eq!(another_docs.token, "@mcp/cursor/docs");
        let tags = vec![docs.clone(), another_docs.clone()];
        let text = format!(
            "Ask {} about this, then {}.",
            docs.token, another_docs.token
        );
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
                &format!("Ask {}2 about this", docs.token),
                std::slice::from_ref(&docs)
            )
            .is_empty()
        );
        assert!(tagged_mcp_servers("Ask about this", &[docs]).is_empty());
    }
}
