use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::dirs_home;
use crate::fs::expand_home;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConnection {
    provider: String,
    name: String,
    scope: String,
    config_path: String,
    transport: String,
}

#[tauri::command]
pub async fn mcp_discover(cwd: String) -> Result<Vec<McpConnection>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = dirs_home().ok_or("Home directory not found")?;
        let project = expand_home(&cwd);
        let codex_home = std::env::var_os("CODEX_HOME").map(PathBuf::from);
        Ok(discover(Path::new(&home), &project, codex_home.as_deref()))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn discover(home: &Path, project: &Path, codex_home_override: Option<&Path>) -> Vec<McpConnection> {
    let mut connections = Vec::new();
    let claude = home.join(".claude.json");
    if let Some(config) = read_json(&claude) {
        add_json_servers(
            &mut connections,
            "claude",
            "user",
            &claude,
            config.get("mcpServers"),
        );
        add_json_servers(
            &mut connections,
            "claude",
            "local",
            &claude,
            config
                .get("projects")
                .and_then(|projects| projects.get(project.to_string_lossy().as_ref()))
                .and_then(|entry| entry.get("mcpServers")),
        );
    }
    let cursor = home.join(".cursor/mcp.json");
    add_json_file(&mut connections, "cursor", "user", &cursor, "mcpServers");

    let codex_home = codex_home_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".codex"));
    add_toml_file(
        &mut connections,
        "codex",
        "user",
        &codex_home.join("config.toml"),
    );

    for file in ["opencode.json", "opencode.jsonc"] {
        add_json_file(
            &mut connections,
            "opencode",
            "user",
            &home.join(".config/opencode").join(file),
            "mcp",
        );
    }
    if let Some(custom) = std::env::var_os("OPENCODE_CONFIG") {
        add_json_file(
            &mut connections,
            "opencode",
            "user",
            &PathBuf::from(custom),
            "mcp",
        );
    }

    // Project configuration is inherited from parent directories. Stop at the
    // repository boundary so an unrelated parent project is not shown.
    for directory in project.ancestors() {
        add_json_file(
            &mut connections,
            "claude",
            "project",
            &directory.join(".mcp.json"),
            "mcpServers",
        );
        add_json_file(
            &mut connections,
            "cursor",
            "project",
            &directory.join(".cursor/mcp.json"),
            "mcpServers",
        );
        add_toml_file(
            &mut connections,
            "codex",
            "project",
            &directory.join(".codex/config.toml"),
        );
        for file in [
            "opencode.json",
            "opencode.jsonc",
            ".opencode/opencode.json",
            ".opencode/opencode.jsonc",
        ] {
            add_json_file(
                &mut connections,
                "opencode",
                "project",
                &directory.join(file),
                "mcp",
            );
        }
        if directory.join(".git").exists() {
            break;
        }
    }
    connections.sort_by(|a, b| {
        (&a.provider, &a.name, &a.scope, &a.config_path).cmp(&(
            &b.provider,
            &b.name,
            &b.scope,
            &b.config_path,
        ))
    });
    connections
}

fn add_json_file(
    connections: &mut Vec<McpConnection>,
    provider: &str,
    scope: &str,
    path: &Path,
    key: &str,
) {
    if let Some(config) = read_json(path) {
        add_json_servers(connections, provider, scope, path, config.get(key));
    }
}

fn add_json_servers(
    connections: &mut Vec<McpConnection>,
    provider: &str,
    scope: &str,
    path: &Path,
    servers: Option<&Value>,
) {
    // OpenCode 2.x nests the map under mcp.servers; older versions use mcp.
    let servers = servers.and_then(|value| value.get("servers").or(Some(value)));
    let Some(servers) = servers.and_then(Value::as_object) else {
        return;
    };
    for (name, config) in servers {
        if !config.is_object() {
            continue;
        }
        connections.push(McpConnection {
            provider: provider.into(),
            name: name.clone(),
            scope: scope.into(),
            config_path: path.to_string_lossy().into_owned(),
            transport: transport(config).into(),
        });
    }
}

fn add_toml_file(connections: &mut Vec<McpConnection>, provider: &str, scope: &str, path: &Path) {
    let Some(raw) = std::fs::read_to_string(path).ok() else {
        return;
    };
    let Some(config) = toml::from_str::<toml::Value>(&raw).ok() else {
        return;
    };
    let Some(servers) = config.get("mcp_servers").and_then(toml::Value::as_table) else {
        return;
    };
    for (name, entry) in servers {
        connections.push(McpConnection {
            provider: provider.into(),
            name: name.clone(),
            scope: scope.into(),
            config_path: path.to_string_lossy().into_owned(),
            transport: if entry.get("url").is_some() {
                "http"
            } else {
                "stdio"
            }
            .into(),
        });
    }
}

fn transport(config: &Value) -> &str {
    config
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if config.get("url").is_some() {
                "http"
            } else {
                "stdio"
            }
        })
}

fn read_json(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw)
        .or_else(|_| serde_json::from_str(&strip_jsonc(&raw)))
        .ok()
}

/// Remove JSONC comments and trailing commas without touching quoted text.
fn strip_jsonc(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut clean = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut quoted = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if quoted {
            clean.push(byte);
            if byte == b'\\' && index + 1 < bytes.len() {
                index += 1;
                clean.push(bytes[index]);
            } else if byte == b'"' {
                quoted = false;
            }
        } else if byte == b'"' {
            quoted = true;
            clean.push(byte);
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            clean.push(b'\n');
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 1).min(bytes.len() - 1);
        } else {
            clean.push(byte);
        }
        index += 1;
    }
    let mut result = Vec::with_capacity(clean.len());
    quoted = false;
    let mut escaped = false;
    for (index, byte) in clean.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
        } else if *byte == b'"' {
            quoted = true;
        }
        if !quoted
            && *byte == b','
            && clean[index + 1..]
                .iter()
                .find(|b| !b.is_ascii_whitespace())
                .is_some_and(|b| *b == b'}' || *b == b']')
        {
            continue;
        }
        result.push(*byte);
    }
    String::from_utf8(result).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_preserves_urls_and_removes_comments_and_trailing_commas() {
        let input = r#"{"mcp":{"servers":{"docs":{"url":"https://example.com/mcp",},},}, // comment
        }"#;
        let value: Value = serde_json::from_str(&strip_jsonc(input)).unwrap();
        assert_eq!(
            value["mcp"]["servers"]["docs"]["url"],
            "https://example.com/mcp"
        );
    }

    #[test]
    fn discovers_provider_configs_without_exposing_credentials() {
        let root = std::env::temp_dir().join(format!("monocode-mcp-{}", uuid::Uuid::new_v4()));
        let home = root.join("home");
        let project = root.join("project");
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(home.join(".config/opencode")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(home.join(".claude.json"), r#"{"mcpServers":{"one":{"type":"http","url":"https://example.com","headers":{"Authorization":"secret"}}}}"#).unwrap();
        std::fs::write(
            home.join(".cursor/mcp.json"),
            r#"{"mcpServers":{"two":{"command":"npx"}}}"#,
        )
        .unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            "[mcp_servers.three]\nurl = 'https://example.com'\n",
        )
        .unwrap();
        let codex_raw = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        let codex_config: toml::Value = toml::from_str(&codex_raw).unwrap();
        assert!(codex_config.get("mcp_servers").is_some());
        std::fs::write(home.join(".config/opencode/opencode.jsonc"), "{\"mcp\": {\"servers\": {\"four\": {\"type\": \"remote\", \"url\": \"https://example.com\",},},}}").unwrap();
        let found = discover(&home, &project, None);
        let names: Vec<_> = found
            .iter()
            .map(|entry| (entry.provider.as_str(), entry.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("claude", "one"),
                ("codex", "three"),
                ("cursor", "two"),
                ("opencode", "four")
            ]
        );
        assert!(!serde_json::to_string(&found).unwrap().contains("secret"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
