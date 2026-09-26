use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::dirs_home;
use crate::fs::expand_home;

fn claude_desktop_config(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/Claude/claude_desktop_config.json")
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("Claude/claude_desktop_config.json")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        home.join(".config/Claude/claude_desktop_config.json")
    }
}

fn server_from_json(name: &str, config: &str) -> Result<(String, Value), String> {
    let value: Value = serde_json::from_str(config).map_err(|e| format!("Invalid JSON: {e}"))?;
    let (name, server) = if let Some(servers) = value.get("mcpServers") {
        let servers = servers.as_object().ok_or("mcpServers must be an object")?;
        if servers.len() != 1 {
            return Err("Add one server at a time".into());
        }
        let (server_name, server) = servers.iter().next().unwrap();
        if !name.trim().is_empty() && name.trim() != server_name {
            return Err("Name does not match the mcpServers entry".into());
        }
        (server_name.clone(), server.clone())
    } else {
        (name.trim().to_owned(), value)
    };
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Server name must use letters, numbers, hyphens, or underscores".into());
    }
    if !server.is_object() {
        return Err("Server configuration must be an object".into());
    }
    let command = server
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty());
    let url = server
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty());
    if command == url {
        return Err("Server needs either a command or a URL".into());
    }
    Ok((name, server))
}

#[tauri::command]
pub async fn mcp_add(
    cwd: String,
    provider: String,
    scope: String,
    name: String,
    config: String,
) -> Result<(), String> {
    let (name, server) = server_from_json(&name, &config)?;
    tauri::async_runtime::spawn_blocking(move || {
        let home = dirs_home().ok_or("Home directory not found")?;
        let project = expand_home(&cwd);
        if !project.is_dir() {
            return Err("Project directory does not exist".into());
        }
        match provider.as_str() {
            "cursor" | "claude_desktop" => {
                let path = match (provider.as_str(), scope.as_str()) {
                    ("cursor", "user") => Path::new(&home).join(".cursor/mcp.json"),
                    ("cursor", "project") => project.join(".cursor/mcp.json"),
                    ("claude_desktop", "user") => claude_desktop_config(Path::new(&home)),
                    _ => return Err("Unsupported scope for this provider".into()),
                };
                if provider == "claude_desktop" && server.get("command").is_none() {
                    return Err("Claude Desktop local configuration requires a command".into());
                }
                write_json_server(&path, &name, server)
            }
            "claude" | "codex" | "opencode" => {
                crate::harness::add_mcp_via_cli(&provider, &scope, &cwd, &name, &server)
            }
            _ => Err("Unsupported MCP provider".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn write_json_server(path: &Path, name: &str, server: Value) -> Result<(), String> {
    let mut root: Value = if path.exists() {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| format!("Existing config is invalid JSON: {e}"))?
    } else {
        serde_json::json!({})
    };
    let object = root
        .as_object_mut()
        .ok_or("Existing config must be a JSON object")?;
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or("Existing mcpServers must be an object")?;
    if servers.contains_key(name) {
        return Err(format!("{name} is already configured in this file"));
    }
    servers.insert(name.to_owned(), server);
    let encoded = serde_json::to_vec_pretty(&root).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, encoded).map_err(|e| e.to_string())
}

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
    add_json_file(
        &mut connections,
        "claude_desktop",
        "user",
        &claude_desktop_config(home),
        "mcpServers",
    );

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
    fn parses_single_server_from_standard_json() {
        let (name, server) = server_from_json(
            "",
            r#"{"mcpServers":{"docs":{"command":"npx","args":["server"]}}}"#,
        )
        .unwrap();
        assert_eq!(name, "docs");
        assert_eq!(server["command"], "npx");
        assert!(server_from_json(
            "",
            r#"{"mcpServers":{"one":{"command":"npx"},"two":{"command":"node"}}}"#
        )
        .is_err());
        assert!(
            server_from_json("different", r#"{"mcpServers":{"docs":{"command":"npx"}}}"#).is_err()
        );
    }

    #[test]
    fn writes_server_without_discarding_other_configuration() {
        let root =
            std::env::temp_dir().join(format!("monocode-mcp-write-{}", uuid::Uuid::new_v4()));
        let path = root.join(".cursor/mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"otherSetting":true,"mcpServers":{"existing":{"command":"node"}}}"#,
        )
        .unwrap();
        write_json_server(&path, "new", serde_json::json!({"command":"npx"})).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["otherSetting"], true);
        assert_eq!(value["mcpServers"]["existing"]["command"], "node");
        assert_eq!(value["mcpServers"]["new"]["command"], "npx");
        assert!(write_json_server(&path, "new", serde_json::json!({"command":"npx"})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

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
        let desktop = claude_desktop_config(&home);
        std::fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        std::fs::write(desktop, r#"{"mcpServers":{"desktop":{"command":"npx"}}}"#).unwrap();
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
                ("claude_desktop", "desktop"),
                ("codex", "three"),
                ("cursor", "two"),
                ("opencode", "four")
            ]
        );
        assert!(!serde_json::to_string(&found).unwrap().contains("secret"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
