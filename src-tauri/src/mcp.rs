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

fn server_from_json(provider: &str, name: &str, config: &str) -> Result<(String, Value), String> {
    let value: Value = serde_json::from_str(config).map_err(|e| format!("Invalid JSON: {e}"))?;
    let (name, mut server) = if let Some(servers) = value.get("mcpServers") {
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
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err("Server name cannot be empty or contain control characters".into());
    }
    if provider != "opencode"
        && !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Server name must use letters, numbers, hyphens, or underscores".into());
    }
    if !server.is_object() {
        return Err("Server configuration must be an object".into());
    }
    if provider == "opencode" {
        if let Some(parts) = server.get("command").and_then(Value::as_array) {
            let parts = parts
                .iter()
                .map(|part| {
                    part.as_str()
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned)
                })
                .collect::<Option<Vec<_>>>()
                .ok_or("OpenCode command must contain non-empty strings")?;
            let (command, args) = parts
                .split_first()
                .ok_or("OpenCode command cannot be empty")?;
            let object = server.as_object_mut().unwrap();
            if object.contains_key("args") {
                return Err("Use either a command array or separate args".into());
            }
            object.insert("command".into(), Value::String(command.clone()));
            object.insert("args".into(), serde_json::json!(args));
        }
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
    let (name, server) = server_from_json(&provider, &name, &config)?;
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
            "opencode" => {
                if !matches!(scope.as_str(), "user" | "project") {
                    return Err("Invalid OpenCode MCP scope".into());
                }
                let major = crate::harness::opencode_major_version(&cwd)?;
                let override_path = std::env::var_os("OPENCODE_CONFIG").map(PathBuf::from);
                let path = opencode_config_path(
                    Path::new(&home),
                    &project,
                    &scope,
                    override_path.as_deref(),
                );
                write_opencode_server(&path, &name, server, major)
            }
            "claude" | "codex" => {
                crate::harness::add_mcp_via_cli(&provider, &scope, &cwd, &name, &server)
            }
            _ => Err("Unsupported MCP provider".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn opencode_config_path(
    home: &Path,
    project: &Path,
    scope: &str,
    override_path: Option<&Path>,
) -> PathBuf {
    if scope == "user" {
        if let Some(path) = override_path {
            return path.to_path_buf();
        }
    }
    let directory = if scope == "user" {
        home.join(".config/opencode")
    } else {
        project.to_path_buf()
    };
    let candidates: &[&str] = if scope == "user" {
        &["opencode.json", "opencode.jsonc"]
    } else {
        &[
            "opencode.json",
            "opencode.jsonc",
            ".opencode/opencode.json",
            ".opencode/opencode.jsonc",
        ]
    };
    candidates
        .iter()
        .map(|name| directory.join(name))
        .find(|path| path.exists())
        .unwrap_or_else(|| directory.join("opencode.json"))
}

fn normalize_opencode_server(server: Value, major: u32) -> Result<Value, String> {
    let mut object = server
        .as_object()
        .ok_or("Server configuration must be an object")?
        .clone();
    let local = object.get("command").is_some();
    let allowed: &[&str] = if local {
        &[
            "type",
            "command",
            "args",
            "env",
            "environment",
            "enabled",
            "disabled",
            "cwd",
            "timeout",
            "codemode",
            "protocol",
        ]
    } else {
        &[
            "type", "url", "headers", "enabled", "disabled", "oauth", "timeout", "codemode",
            "protocol",
        ]
    };
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("OpenCode configuration cannot preserve '{key}'"));
    }
    let kind = object
        .remove("type")
        .and_then(|value| value.as_str().map(str::to_owned));
    if local {
        if !matches!(kind.as_deref(), None | Some("stdio") | Some("local")) {
            return Err("OpenCode local server type must be local".into());
        }
        let command = object
            .remove("command")
            .ok_or("Local server needs a command")?;
        let mut parts = if let Some(command) = command.as_str() {
            vec![Value::String(command.to_owned())]
        } else {
            command
                .as_array()
                .cloned()
                .ok_or("OpenCode command must be an array or string")?
        };
        if let Some(args) = object.remove("args") {
            parts.extend(
                args.as_array()
                    .ok_or("args must be an array")?
                    .iter()
                    .cloned(),
            );
        }
        if parts.is_empty()
            || parts
                .iter()
                .any(|part| part.as_str().is_none_or(str::is_empty))
        {
            return Err("OpenCode command must contain non-empty strings".into());
        }
        if object.contains_key("env") && object.contains_key("environment") {
            return Err("Use either env or environment".into());
        }
        if let Some(environment) = object
            .remove("env")
            .or_else(|| object.remove("environment"))
        {
            let values = environment
                .as_object()
                .ok_or("environment must be an object")?;
            if values.values().any(|value| !value.is_string()) {
                return Err("environment values must be strings".into());
            }
            object.insert("environment".into(), environment);
        }
        object.insert("command".into(), Value::Array(parts));
        object.insert("type".into(), Value::String("local".into()));
    } else {
        if !matches!(
            kind.as_deref(),
            None | Some("http") | Some("sse") | Some("remote")
        ) {
            return Err("OpenCode remote server type must be remote".into());
        }
        let url = object
            .get("url")
            .and_then(Value::as_str)
            .ok_or("Remote server needs a URL")?;
        let parsed = url::Url::parse(url).map_err(|_| "Invalid server URL")?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("MCP URL must use HTTP or HTTPS".into());
        }
        object.insert("type".into(), Value::String("remote".into()));
    }
    if major == 1 {
        if let Some(disabled) = object.remove("disabled") {
            object.insert(
                "enabled".into(),
                Value::Bool(!disabled.as_bool().ok_or("disabled must be a boolean")?),
            );
        }
    } else if let Some(enabled) = object.remove("enabled") {
        object.insert(
            "disabled".into(),
            Value::Bool(!enabled.as_bool().ok_or("enabled must be a boolean")?),
        );
    }
    Ok(Value::Object(object))
}

fn write_opencode_server(path: &Path, name: &str, server: Value, major: u32) -> Result<(), String> {
    let server = normalize_opencode_server(server, major)?;
    write_json_config(path, |root| {
        let object = root
            .as_object_mut()
            .ok_or("Existing config must be a JSON object")?;
        let mcp = object.entry("mcp").or_insert_with(|| serde_json::json!({}));
        let mcp = mcp
            .as_object_mut()
            .ok_or("Existing mcp must be an object")?;
        let servers = if major == 1 {
            if mcp
                .get("servers")
                .and_then(Value::as_object)
                .is_some_and(|nested| nested.values().all(Value::is_object))
            {
                return Err("Existing OpenCode config uses the 2.x MCP layout".into());
            }
            mcp
        } else {
            if mcp
                .iter()
                .any(|(key, value)| key != "servers" && value.is_object())
            {
                return Err("Existing OpenCode config uses the 1.x MCP layout".into());
            }
            mcp.entry("servers")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
                .ok_or("Existing mcp.servers must be an object")?
        };
        if servers.contains_key(name) {
            return Err(format!("{name} is already configured in this file"));
        }
        servers.insert(name.to_owned(), server);
        Ok(())
    })
}

fn write_json_server(path: &Path, name: &str, server: Value) -> Result<(), String> {
    write_json_config(path, |root| {
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
        Ok(())
    })
}

fn write_json_config(
    path: &Path,
    update: impl FnOnce(&mut Value) -> Result<(), String>,
) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid config path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let lock_path = parent.join(format!(
        ".{}.lock",
        path.file_name()
            .and_then(|part| part.to_str())
            .unwrap_or("mcp")
    ));
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let mut root: Value = if path.exists() {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw)
            .or_else(|_| serde_json::from_str(&strip_jsonc(&raw)))
            .map_err(|e| format!("Existing config is invalid JSON: {e}"))?
    } else {
        serde_json::json!({})
    };
    update(&mut root)?;
    let encoded = serde_json::to_vec_pretty(&root).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("mcp"),
        uuid::Uuid::new_v4()
    ));
    let result = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|e| e.to_string())
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
        let desktop_config = claude_desktop_config(Path::new(&home));
        let opencode_config = std::env::var_os("OPENCODE_CONFIG").map(PathBuf::from);
        Ok(discover(
            Path::new(&home),
            &project,
            codex_home.as_deref(),
            &desktop_config,
            opencode_config.as_deref(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn discover(
    home: &Path,
    project: &Path,
    codex_home_override: Option<&Path>,
    desktop_config: &Path,
    opencode_config: Option<&Path>,
) -> Vec<McpConnection> {
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
        desktop_config,
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
    if let Some(custom) = opencode_config {
        add_json_file(&mut connections, "opencode", "user", custom, "mcp");
    }

    // Project configuration is inherited from parent directories. Stop at the
    // repository boundary so an unrelated parent project is not shown.
    for directory in project.ancestors() {
        if directory == home {
            break;
        }
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
    // A server can itself be named "servers", so inspect the nested shape.
    let servers = servers.map(|value| {
        if provider == "opencode" {
            if let Some(nested) = value.get("servers").and_then(Value::as_object) {
                if nested.values().all(Value::is_object) {
                    return &value["servers"];
                }
            }
        }
        value
    });
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
            "claude",
            "",
            r#"{"mcpServers":{"docs":{"command":"npx","args":["server"]}}}"#,
        )
        .unwrap();
        assert_eq!(name, "docs");
        assert_eq!(server["command"], "npx");
        assert!(server_from_json(
            "claude",
            "",
            r#"{"mcpServers":{"one":{"command":"npx"},"two":{"command":"node"}}}"#
        )
        .is_err());
        assert!(server_from_json(
            "claude",
            "different",
            r#"{"mcpServers":{"docs":{"command":"npx"}}}"#
        )
        .is_err());
    }

    #[test]
    fn accepts_opencode_names_and_command_arrays() {
        let (name, server) = server_from_json(
            "opencode",
            "docs server",
            r#"{"type":"local","command":["npx","-y","server"]}"#,
        )
        .unwrap();
        assert_eq!(name, "docs server");
        assert_eq!(server["command"], "npx");
        assert_eq!(server["args"], serde_json::json!(["-y", "server"]));
        assert!(server_from_json("claude", "docs server", r#"{"command":"npx"}"#).is_err());
        assert!(server_from_json("opencode", "docs", r#"{"command":[]}"#).is_err());
    }

    #[test]
    fn writes_opencode_one_config_with_environment() {
        let root =
            std::env::temp_dir().join(format!("monocode-opencode-one-{}", uuid::Uuid::new_v4()));
        let path = root.join("opencode.jsonc");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, "{\n // keep this setting\n \"theme\": \"dark\", \"mcp\": {\"existing\": {\"type\": \"remote\", \"url\": \"https://example.com\"},},\n}").unwrap();
        let (_, server) = server_from_json(
            "opencode",
            "docs",
            r#"{"type":"local","command":["npx","-y","docs"],"environment":{"TOKEN":"{env:DOCS_TOKEN}"},"enabled":true}"#,
        ).unwrap();
        write_opencode_server(&path, "docs", server, 1).unwrap();
        let config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(config["theme"], "dark");
        assert_eq!(config["mcp"]["existing"]["url"], "https://example.com");
        assert_eq!(
            config["mcp"]["docs"]["command"],
            serde_json::json!(["npx", "-y", "docs"])
        );
        assert_eq!(
            config["mcp"]["docs"]["environment"]["TOKEN"],
            "{env:DOCS_TOKEN}"
        );
        assert_eq!(config["mcp"]["docs"]["enabled"], true);
        assert!(
            write_opencode_server(&path, "docs", serde_json::json!({"command":"npx"}), 1).is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writes_opencode_two_config_and_normalizes_standard_env() {
        let root =
            std::env::temp_dir().join(format!("monocode-opencode-two-{}", uuid::Uuid::new_v4()));
        let path = root.join("opencode.json");
        let (_, server) = server_from_json(
            "opencode",
            "docs",
            r#"{"mcpServers":{"docs":{"command":"npx","args":["docs"],"env":{"TOKEN":"{env:DOCS_TOKEN}"},"enabled":false}}}"#,
        ).unwrap();
        write_opencode_server(&path, "docs", server, 2).unwrap();
        let config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            config["mcp"]["servers"]["docs"]["command"],
            serde_json::json!(["npx", "docs"])
        );
        assert_eq!(
            config["mcp"]["servers"]["docs"]["environment"]["TOKEN"],
            "{env:DOCS_TOKEN}"
        );
        assert_eq!(config["mcp"]["servers"]["docs"]["disabled"], true);
        assert!(config["mcp"]["docs"].is_null());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chooses_existing_opencode_jsonc_and_custom_user_config() {
        let root =
            std::env::temp_dir().join(format!("monocode-opencode-path-{}", uuid::Uuid::new_v4()));
        let home = root.join("home");
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("opencode.jsonc"), "{}").unwrap();
        assert_eq!(
            opencode_config_path(&home, &project, "project", None),
            project.join("opencode.jsonc")
        );
        std::fs::remove_file(project.join("opencode.jsonc")).unwrap();
        std::fs::create_dir_all(project.join(".opencode")).unwrap();
        std::fs::write(project.join(".opencode/opencode.jsonc"), "{}").unwrap();
        assert_eq!(
            opencode_config_path(&home, &project, "project", None),
            project.join(".opencode/opencode.jsonc")
        );
        let custom = root.join("custom.jsonc");
        assert_eq!(
            opencode_config_path(&home, &project, "user", Some(&custom)),
            custom
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_additions_keep_every_server() {
        let root = std::env::temp_dir().join(format!("monocode-mcp-lock-{}", uuid::Uuid::new_v4()));
        let path = root.join(".cursor/mcp.json");
        std::thread::scope(|scope| {
            for index in 0..8 {
                let path = path.clone();
                scope.spawn(move || {
                    write_json_server(
                        &path,
                        &format!("server-{index}"),
                        serde_json::json!({"command":"npx"}),
                    )
                    .unwrap();
                });
            }
        });
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["mcpServers"].as_object().unwrap().len(), 8);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacing_config_restricts_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("monocode-mcp-mode-{}", uuid::Uuid::new_v4()));
        let path = root.join("mcp.json");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, r#"{"mcpServers":{}}"#).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_json_server(&path, "new", serde_json::json!({"command":"npx"})).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::remove_dir_all(root).unwrap();
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
    fn adds_to_existing_cursor_jsonc() {
        let root =
            std::env::temp_dir().join(format!("monocode-mcp-jsonc-{}", uuid::Uuid::new_v4()));
        let path = root.join(".cursor/mcp.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "{\n // existing server\n \"mcpServers\": {\"old\": {\"command\": \"node\",},},\n}",
        )
        .unwrap();
        write_json_server(&path, "new", serde_json::json!({"command":"npx"})).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["mcpServers"]["old"]["command"], "node");
        assert_eq!(value["mcpServers"]["new"]["command"], "npx");
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
        let desktop = root.join("claude_desktop_config.json");
        std::fs::create_dir_all(desktop.parent().unwrap()).unwrap();
        std::fs::write(&desktop, r#"{"mcpServers":{"desktop":{"command":"npx"}}}"#).unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            "[mcp_servers.three]\nurl = 'https://example.com'\n",
        )
        .unwrap();
        let codex_raw = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        let codex_config: toml::Value = toml::from_str(&codex_raw).unwrap();
        assert!(codex_config.get("mcp_servers").is_some());
        std::fs::write(home.join(".config/opencode/opencode.jsonc"), "{\"mcp\": {\"servers\": {\"four\": {\"type\": \"remote\", \"url\": \"https://example.com\",},},}}").unwrap();
        let found = discover(&home, &project, None, &desktop, None);
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

    #[test]
    fn does_not_duplicate_home_configs_or_hide_a_server_named_servers() {
        let root = std::env::temp_dir().join(format!("monocode-mcp-{}", uuid::Uuid::new_v4()));
        let project = root.join("work/notes");
        std::fs::create_dir_all(root.join(".cursor")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            root.join(".cursor/mcp.json"),
            r#"{"mcpServers":{"servers":{"command":"npx"},"docs":{"command":"node"}}}"#,
        )
        .unwrap();
        let found = discover(&root, &project, None, &root.join("desktop.json"), None);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|server| server.scope == "user"));
        assert!(found.iter().any(|server| server.name == "servers"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
