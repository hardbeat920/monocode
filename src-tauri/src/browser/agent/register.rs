//! Registering `monocode browser-mcp` with the providers that take MCP
//! servers (spec 12.2), and the text for those that do not (spec 12.5).
//!
//! The entry is user-scoped, named [`SERVER_NAME`], and holds only the
//! executable and `browser-mcp`: no token, because these files are readable
//! by other processes. Endpoint and token reach the server by inheritance
//! (`harness_spawn` -> provider CLI -> MCP child). Codex filters the
//! environment of MCP children, so its entry also lists the two variables in
//! `env_vars`, which makes Codex pass them through.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::State;

use super::mcp::SERVER_NAME;
use crate::browser::protocol::{ENV_ENDPOINT, ENV_TOKEN};

/// Providers whose MCP configuration MonoCode can write (see `mcp::mcp_add`).
pub const MCP_PROVIDERS: [&str; 4] = ["claude", "codex", "cursor", "opencode"];
const SUBCOMMAND: &str = "browser-mcp";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserMcpRegistration {
    pub provider: String,
    /// The provider takes MCP servers; otherwise the agent uses the CLI.
    pub supported: bool,
    /// An entry named `monocode-browser` exists in the user configuration.
    pub registered: bool,
    /// That entry starts this executable (and, for Codex, passes the
    /// credentials through). A registered but stale entry is replaced by
    /// `browser_mcp_register`.
    pub current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
}

/// Where each provider keeps its user-scoped MCP servers.
struct Locations {
    home: PathBuf,
    codex_home: PathBuf,
    opencode_override: Option<PathBuf>,
}

impl Locations {
    fn from_env() -> Result<Locations, String> {
        let home = PathBuf::from(crate::dirs_home().ok_or("Home directory not found")?);
        Ok(Locations {
            codex_home: std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
            opencode_override: std::env::var_os("OPENCODE_CONFIG").map(PathBuf::from),
            home,
        })
    }

    fn config_path(&self, provider: &str) -> Option<PathBuf> {
        Some(match provider {
            "claude" => self.home.join(".claude.json"),
            "cursor" => self.home.join(".cursor/mcp.json"),
            "codex" => self.codex_home.join("config.toml"),
            "opencode" => crate::mcp::opencode_config_path(
                &self.home,
                &self.home,
                "user",
                self.opencode_override.as_deref(),
            ),
            _ => return None,
        })
    }
}

/// The command line the entry should run.
fn expected_command(exe: &str) -> Vec<String> {
    vec![exe.to_string(), SUBCOMMAND.to_string()]
}

fn current_exe() -> Result<String, String> {
    std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

/// Command line of the existing entry, and whether it passes the
/// credentials through (always true except for Codex without `env_vars`).
fn existing_entry(provider: &str, path: &Path) -> Option<(Vec<String>, bool)> {
    let raw = std::fs::read_to_string(path).ok()?;
    if provider == "codex" {
        let root: toml::Table = toml::from_str(&raw).ok()?;
        let server = root.get("mcp_servers")?.get(SERVER_NAME)?;
        let mut command = vec![server.get("command")?.as_str()?.to_string()];
        for arg in server
            .get("args")
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
        {
            command.push(arg.as_str()?.to_string());
        }
        let passes = server
            .get("env_vars")
            .and_then(|vars| vars.as_array())
            .is_some_and(|vars| {
                [ENV_ENDPOINT, ENV_TOKEN]
                    .iter()
                    .all(|name| vars.iter().any(|v| v.as_str() == Some(name)))
            });
        return Some((command, passes));
    }
    let root: Value = serde_json::from_str(&raw)
        .or_else(|_| serde_json::from_str(&crate::mcp::strip_jsonc(&raw)))
        .ok()?;
    let server = match provider {
        "opencode" => root.get("mcp").and_then(|mcp| {
            mcp.get("servers")
                .and_then(|s| s.get(SERVER_NAME))
                .or_else(|| mcp.get(SERVER_NAME))
        })?,
        _ => root.get("mcpServers")?.get(SERVER_NAME)?,
    };
    let mut command: Vec<String> = match server.get("command")? {
        Value::String(command) => vec![command.clone()],
        Value::Array(parts) => parts
            .iter()
            .map(|part| part.as_str().map(str::to_owned))
            .collect::<Option<_>>()?,
        _ => return None,
    };
    for arg in server
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        command.push(arg.as_str()?.to_string());
    }
    Some((command, true))
}

fn status_at(locations: &Locations, provider: &str, exe: &str) -> BrowserMcpRegistration {
    let path = locations.config_path(provider);
    let entry = path
        .as_deref()
        .and_then(|path| existing_entry(provider, path));
    BrowserMcpRegistration {
        provider: provider.to_string(),
        supported: path.is_some(),
        registered: entry.is_some(),
        current: entry.is_some_and(|(command, passes)| passes && command == expected_command(exe)),
        config_path: path.map(|path| path.to_string_lossy().into_owned()),
    }
}

/// The `[mcp_servers.monocode-browser]` table for Codex.
fn codex_table(exe: &str) -> String {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    format!(
        "[mcp_servers.{SERVER_NAME}]\ncommand = {}\nargs = [{}]\nenv_vars = [{}, {}]\n",
        quote(exe),
        quote(SUBCOMMAND),
        quote(ENV_ENDPOINT),
        quote(ENV_TOKEN)
    )
}

/// Append our table to a Codex `config.toml` without rewriting the user's
/// file: the text is only appended, after checking that the result still
/// parses and holds exactly our entry.
fn append_codex_table(path: &Path, exe: &str) -> Result<(), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.to_string()),
    };
    let mut text = existing.clone();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&codex_table(exe));
    let parsed: toml::Table = toml::from_str(&text)
        .map_err(|e| format!("Cannot add the browser server to {}: {e}", path.display()))?;
    let entry = parsed
        .get("mcp_servers")
        .and_then(|servers| servers.get(SERVER_NAME))
        .and_then(|server| server.get("command"))
        .and_then(|command| command.as_str());
    if entry != Some(exe) {
        return Err(format!(
            "Cannot add the browser server to {}: its mcp_servers layout is not supported",
            path.display()
        ));
    }
    let parent = path.parent().ok_or("Invalid Codex config path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".config.toml.{}.tmp", uuid::Uuid::new_v4()));
    let written = std::fs::write(&temporary, &text).and_then(|_| {
        if let Ok(metadata) = std::fs::metadata(path) {
            std::fs::set_permissions(&temporary, metadata.permissions())?;
        }
        std::fs::rename(&temporary, path)
    });
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written.map_err(|e| e.to_string())
}

fn register_at(
    locations: &Locations,
    provider: &str,
    cwd: &str,
    exe: &str,
    binary_path: Option<&str>,
) -> Result<BrowserMcpRegistration, String> {
    let before = status_at(locations, provider, exe);
    if !before.supported {
        return Err(format!(
            "{provider} does not take MCP servers from MonoCode; it uses the browser CLI instead"
        ));
    }
    if before.current {
        return Ok(before);
    }
    let path = locations
        .config_path(provider)
        .ok_or("Unsupported MCP provider")?;
    let server = json!({"command": exe, "args": [SUBCOMMAND]});
    match provider {
        "claude" => {
            if before.registered {
                crate::harness::remove_mcp_via_cli("claude", cwd, SERVER_NAME, binary_path)?;
            }
            crate::harness::add_mcp_via_cli(
                "claude",
                "user",
                cwd,
                SERVER_NAME,
                &server,
                binary_path,
            )?;
        }
        "codex" => {
            if before.registered {
                crate::harness::remove_mcp_via_cli("codex", cwd, SERVER_NAME, binary_path)?;
            }
            append_codex_table(&path, exe)?;
        }
        "cursor" => crate::mcp::put_json_server(&path, SERVER_NAME, server, true)?,
        "opencode" => {
            let major = crate::harness::opencode_major_version(cwd, binary_path)?;
            crate::mcp::put_opencode_server(&path, SERVER_NAME, server, major, true)?;
        }
        _ => return Err("Unsupported MCP provider".into()),
    }
    Ok(status_at(locations, provider, exe))
}

/// Registration state of the browser MCP server for every provider.
#[tauri::command]
pub async fn browser_mcp_status() -> Result<Vec<BrowserMcpRegistration>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let locations = Locations::from_env()?;
        let exe = current_exe()?;
        Ok(MCP_PROVIDERS
            .iter()
            .map(|provider| status_at(&locations, provider, &exe))
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Make sure `provider` starts `monocode browser-mcp` (spec 12.2). Called by
/// the UI when a session of that provider is first granted `/browser`;
/// idempotent, and replaces an entry left by a moved or updated app.
/// Sessions already running see the tools from their next start.
#[tauri::command]
pub async fn browser_mcp_register(
    host: State<'_, crate::harness::HarnessHost>,
    provider: String,
    cwd: String,
) -> Result<BrowserMcpRegistration, String> {
    let binary_path = host.runtime_binary_path(&provider);
    tauri::async_runtime::spawn_blocking(move || {
        let locations = Locations::from_env()?;
        let exe = current_exe()?;
        register_at(&locations, &provider, &cwd, &exe, binary_path.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Text the UI appends to a turn while the session has browser access.
/// With MCP the tools describe themselves, so a short pointer is enough; a
/// provider without MCP gets the CLI instructions (spec 12.5).
#[tauri::command]
pub fn browser_agent_instructions(provider: String) -> String {
    let body = if MCP_PROVIDERS.contains(&provider.as_str()) {
        "Browser access is enabled in this thread. Use the browser_* tools (MonoCode browser); start with browser_tabs to open your own tab, and take a browser_snapshot before acting on refs. If the tools are missing, the provider has not loaded MonoCode's browser server yet; tell the user it appears after the session restarts.".to_string()
    } else {
        super::cli::instructions()
    };
    format!("<monocode_browser>\n{body}\n</monocode_browser>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> (Locations, PathBuf) {
        let root = std::env::temp_dir().join(format!("monocode-reg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        (
            Locations {
                home: root.clone(),
                codex_home: root.join(".codex"),
                opencode_override: None,
            },
            root,
        )
    }

    #[test]
    fn json_providers_are_registered_once_and_repaired_when_the_app_moves() {
        let (locations, root) = sandbox();
        let cursor = locations.config_path("cursor").unwrap();
        std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
        std::fs::write(
            &cursor,
            r#"{"mcpServers":{"other":{"command":"x"}},"theme":"dark"}"#,
        )
        .unwrap();
        let status = status_at(&locations, "cursor", "/Apps/MonoCode/monocode");
        assert!(status.supported && !status.registered && !status.current);

        let first =
            register_at(&locations, "cursor", "/", "/Apps/MonoCode/monocode", None).unwrap();
        assert!(first.registered && first.current);
        // Idempotent: a second call changes nothing.
        let raw = std::fs::read_to_string(&cursor).unwrap();
        register_at(&locations, "cursor", "/", "/Apps/MonoCode/monocode", None).unwrap();
        assert_eq!(std::fs::read_to_string(&cursor).unwrap(), raw);

        let moved = status_at(&locations, "cursor", "/Applications/MonoCode/monocode");
        assert!(moved.registered && !moved.current);
        let repaired = register_at(
            &locations,
            "cursor",
            "/",
            "/Applications/MonoCode/monocode",
            None,
        )
        .unwrap();
        assert!(repaired.current);
        let config: Value =
            serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
        assert_eq!(config["theme"], "dark");
        assert_eq!(config["mcpServers"]["other"]["command"], "x");
        assert_eq!(
            config["mcpServers"][SERVER_NAME],
            json!({"command": "/Applications/MonoCode/monocode", "args": ["browser-mcp"]})
        );
        // No credential is ever written to a provider's configuration.
        assert!(!std::fs::read_to_string(&cursor)
            .unwrap()
            .contains("TOKEN\":"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_entries_are_appended_and_pass_the_credentials_through() {
        let (locations, root) = sandbox();
        let path = locations.config_path("codex").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "# my settings\nmodel = \"o4\"\n\n[mcp_servers.docs]\ncommand = \"npx\"\n";
        std::fs::write(&path, original).unwrap();
        let exe = r"C:\Program Files\MonoCode\monocode.exe";
        let registered = register_at(&locations, "codex", "/", exe, None).unwrap();
        assert!(registered.current, "{registered:?}");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with(original),
            "the user's text is kept verbatim"
        );
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        let server = &parsed["mcp_servers"][SERVER_NAME];
        assert_eq!(server["command"].as_str(), Some(exe));
        assert_eq!(server["env_vars"].as_array().unwrap().len(), 2);
        assert_eq!(
            parsed["mcp_servers"]["docs"]["command"].as_str(),
            Some("npx")
        );

        // An entry without env_vars would start but never see the token.
        std::fs::write(
            &path,
            format!(
                "[mcp_servers.{SERVER_NAME}]\ncommand = \"{}\"\nargs = [\"browser-mcp\"]\n",
                "/m"
            ),
        )
        .unwrap();
        let stale = status_at(&locations, "codex", "/m");
        assert!(stale.registered && !stale.current);

        // A layout appending cannot extend is refused, not corrupted.
        std::fs::write(&path, "mcp_servers = { docs = { command = \"npx\" } }\n").unwrap();
        assert!(append_codex_table(&path, "/m").is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "mcp_servers = { docs = { command = \"npx\" } }\n"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opencode_layouts_are_read_and_unknown_providers_use_the_cli() {
        let (locations, root) = sandbox();
        let path = locations.config_path("opencode").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            json!({"mcp": {SERVER_NAME: {"type": "local", "command": ["/m", "browser-mcp"]}}})
                .to_string(),
        )
        .unwrap();
        assert!(status_at(&locations, "opencode", "/m").current);
        std::fs::write(
            &path,
            json!({"mcp": {"servers": {SERVER_NAME: {"type": "local", "command": ["/old", "browser-mcp"]}}}}).to_string(),
        )
        .unwrap();
        let stale = status_at(&locations, "opencode", "/m");
        assert!(stale.registered && !stale.current);

        let pi = status_at(&locations, "pi", "/m");
        assert!(!pi.supported && !pi.registered);
        assert!(register_at(&locations, "pi", "/", "/m", None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn instructions_point_mcp_providers_at_tools_and_others_at_the_cli() {
        let mcp = browser_agent_instructions("claude".into());
        assert!(mcp.contains("browser_tabs") && !mcp.contains(" browser --help"));
        let cli = browser_agent_instructions("pi".into());
        assert!(cli.contains("browser --help") && cli.starts_with("<monocode_browser>"));
    }
}
