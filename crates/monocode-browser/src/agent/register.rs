//! Registering `monocode browser-mcp` with the providers that take MCP
//! servers, and the text for those that do not.
//!
//! The entry is user-scoped, named [`SERVER_NAME`], and holds only the
//! executable and `browser-mcp`: no token, because these files are readable
//! by other processes. Endpoint and token reach the server by inheritance
//! (`harness_spawn` -> provider CLI -> MCP child). Codex filters the
//! environment of MCP children, so its entry also lists the two variables in
//! `env_vars`, which makes Codex pass them through, and raises Codex's 60 s
//! tool-call limit, which a navigation or a wait behind an access prompt
//! can exceed.
//!
//! Sessions of MonoCode's own Claude and Codex accounts run with
//! `CLAUDE_CONFIG_DIR` / `CODEX_HOME` set to the account's directory and read
//! their MCP servers from there, so the entry is written to every account
//! directory as well as to the default configuration. The provider CLI only
//! edits the default one; account files are written directly.
//!
//! A configuration file that is a symlink (dotfiles kept with stow or
//! chezmoi) is written through to its target, so the link survives.
//!
//! The executable named in the entry comes from [`super::server_binary`]: on
//! Windows a per-build copy in the app data directory, so a session holding
//! `browser-mcp` open never locks `MonoCode.exe` against a rebuild or update.
//!
//! What differs between the apps (their MCP file writers, the provider
//! CLIs, where their accounts live) comes from a [`ProviderConfig`].
//!
//! OpenCode 2 is not supported: it starts MCP servers (and runs shell
//! commands) from its shared background service, which MonoCode starts
//! without a session's environment, so neither the MCP server nor the CLI
//! ever sees the session token. OpenCode 1 runs one server per session and
//! inherits it like the other providers.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use super::mcp::SERVER_NAME;
use crate::protocol::{ENV_ENDPOINT, ENV_TOKEN};

/// The app's side of registering: its MCP configuration writers, the
/// provider CLIs, and where its provider accounts live.
pub trait ProviderConfig: Sync {
    /// The user's home directory.
    fn home_dir(&self) -> Option<PathBuf>;
    /// OpenCode's user configuration file (`OPENCODE_CONFIG` when set).
    fn opencode_config_path(&self, home: &Path, override_path: Option<&Path>) -> PathBuf;
    /// JSON with comments and trailing commas, as plain JSON.
    fn strip_jsonc(&self, raw: &str) -> String;
    /// Put (or replace) server `name` in the `mcpServers` of a JSON file,
    /// keeping everything else.
    fn put_json_server(&self, path: &Path, name: &str, server: Value) -> Result<(), String>;
    /// Put (or replace) server `name` in an OpenCode configuration.
    fn put_opencode_server(
        &self,
        path: &Path,
        name: &str,
        server: Value,
        major: u32,
    ) -> Result<(), String>;
    /// `claude mcp add --scope user …` and the like, through the CLI.
    fn add_mcp_via_cli(
        &self,
        provider: &str,
        cwd: &str,
        name: &str,
        server: &Value,
        binary_path: Option<&str>,
    ) -> Result<(), String>;
    /// `claude mcp remove --scope user …` and the like.
    fn remove_mcp_via_cli(
        &self,
        provider: &str,
        cwd: &str,
        name: &str,
        binary_path: Option<&str>,
    ) -> Result<(), String>;
    /// `opencode --version`'s major version.
    fn opencode_major_version(&self, cwd: &str, binary_path: Option<&str>) -> Result<u32, String>;
    /// The directories of the app's own accounts of `provider` (Claude and
    /// Codex): every one that exists, plus `account_id`'s, created when
    /// needed. Their sessions run with `CLAUDE_CONFIG_DIR` / `CODEX_HOME`
    /// set to it.
    fn account_dirs(
        &self,
        provider: &str,
        account_id: Option<&str>,
    ) -> Result<Vec<PathBuf>, String>;
    /// Where Windows keeps the copies `browser-mcp` runs from (the app data
    /// directory's `bin`).
    fn copies_dir(&self) -> Result<PathBuf, String>;
}

/// Providers whose MCP configuration MonoCode can write (see `mcp::mcp_add`).
pub const MCP_PROVIDERS: [&str; 4] = ["claude", "codex", "cursor", "opencode"];
const SUBCOMMAND: &str = "browser-mcp";
/// Codex `tool_timeout_sec` (its default is 60): a call may wait up to two
/// minutes for a load or a condition, plus the user's answer to the access
/// prompt and a cold browser start. Claude Code's default limit is far
/// longer; Cursor and OpenCode have no per-server setting for tool calls.
const CODEX_TOOL_TIMEOUT_SECS: i64 = 300;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserMcpRegistration {
    pub provider: String,
    /// The provider takes MCP servers; otherwise the agent uses the CLI.
    pub supported: bool,
    /// An entry named `monocode-browser` exists in the user configuration.
    pub registered: bool,
    /// That entry starts this executable (and, for Codex, passes the
    /// credentials through and allows long calls), in the default
    /// configuration and in every MonoCode account of the provider. A
    /// registered but stale entry is replaced by `browser_mcp_register`.
    pub current: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    /// Why browser tools cannot work with this provider at all (neither MCP
    /// nor CLI), when `supported` is false for a provider in [`MCP_PROVIDERS`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

/// Why OpenCode 2 cannot use the browser tools, for the UI and the agent.
pub const OPENCODE_2_UNSUPPORTED: &str = "OpenCode 2 runs MCP servers and shell commands from its shared background service, which does not carry this session's MonoCode credential, so browser tools cannot work there yet. OpenCode 1 is supported.";

/// The installed OpenCode's major version, cached per binary: asking runs
/// `opencode --version`, and the instructions are needed on every turn.
fn opencode_major(config: &dyn ProviderConfig, binary_path: Option<&str>) -> Option<u32> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<Option<String>, u32>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = binary_path.map(str::to_owned);
    if let Some(major) = cache.lock().ok()?.get(&key) {
        return Some(*major);
    }
    let home = config.home_dir()?;
    let major = config
        .opencode_major_version(&home.to_string_lossy(), binary_path)
        .ok()?;
    cache.lock().ok()?.insert(key, major);
    Some(major)
}

/// Why `provider` (at `opencode_major`, when known) cannot use the browser.
pub fn unsupported_reason(provider: &str, opencode_major: Option<u32>) -> Option<&'static str> {
    (provider == "opencode" && opencode_major == Some(2)).then_some(OPENCODE_2_UNSUPPORTED)
}

/// Where each provider keeps its user-scoped MCP servers.
pub struct Locations {
    pub home: PathBuf,
    /// The directory of Claude's `.claude.json`.
    pub claude_dir: PathBuf,
    pub codex_home: PathBuf,
    pub opencode_override: Option<PathBuf>,
    /// A MonoCode account's directory rather than the default configuration:
    /// written directly, because the provider CLI would edit the default.
    pub account: bool,
}

impl Locations {
    pub fn from_env(config: &dyn ProviderConfig) -> Result<Locations, String> {
        let home = config.home_dir().ok_or("Home directory not found")?;
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Ok(Locations {
            // The provider CLIs MonoCode runs inherit these variables too.
            claude_dir: var("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.clone()),
            codex_home: var("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            opencode_override: var("OPENCODE_CONFIG"),
            home,
            account: false,
        })
    }

    /// The configuration of one MonoCode account (Claude or Codex), the
    /// directory its sessions get as `CLAUDE_CONFIG_DIR` / `CODEX_HOME`.
    pub fn account(dir: PathBuf) -> Locations {
        Locations {
            home: dir.clone(),
            claude_dir: dir.clone(),
            codex_home: dir,
            opencode_override: None,
            account: true,
        }
    }

    pub fn config_path(&self, config: &dyn ProviderConfig, provider: &str) -> Option<PathBuf> {
        Some(match provider {
            "claude" => self.claude_dir.join(".claude.json"),
            "cursor" => self.home.join(".cursor/mcp.json"),
            "codex" => self.codex_home.join("config.toml"),
            "opencode" => {
                config.opencode_config_path(&self.home, self.opencode_override.as_deref())
            }
            _ => return None,
        })
    }
}

/// The command line the entry should run.
fn expected_command(exe: &str) -> Vec<String> {
    vec![exe.to_string(), SUBCOMMAND.to_string()]
}

/// The executable the entry should name; `install` creates it when needed.
fn server_exe(dir: &Path, install: bool) -> Result<String, String> {
    use super::server_binary;
    let source = server_binary::source_executable()?;
    let path = if install {
        server_binary::install(&source, dir, server_binary::uses_copy())?
    } else {
        server_binary::planned(&source, dir, server_binary::uses_copy())?
    };
    Ok(path.to_string_lossy().into_owned())
}

/// Command line of the existing entry, and whether its other settings are
/// current (always true except for Codex: `env_vars` passing the
/// credentials through, and [`CODEX_TOOL_TIMEOUT_SECS`]).
fn existing_entry(
    config: &dyn ProviderConfig,
    provider: &str,
    path: &Path,
) -> Option<(Vec<String>, bool)> {
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
        let patient = server
            .get("tool_timeout_sec")
            .and_then(|t| t.as_float().or_else(|| t.as_integer().map(|i| i as f64)))
            .is_some_and(|t| t >= CODEX_TOOL_TIMEOUT_SECS as f64);
        return Some((command, passes && patient));
    }
    let root: Value = serde_json::from_str(&raw)
        .or_else(|_| serde_json::from_str(&config.strip_jsonc(&raw)))
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

pub fn status_at(
    config: &dyn ProviderConfig,
    locations: &Locations,
    provider: &str,
    exe: &str,
    opencode_major: Option<u32>,
) -> BrowserMcpRegistration {
    let path = locations.config_path(config, provider);
    let entry = path
        .as_deref()
        .and_then(|path| existing_entry(config, provider, path));
    let unsupported = unsupported_reason(provider, opencode_major);
    BrowserMcpRegistration {
        provider: provider.to_string(),
        supported: path.is_some() && unsupported.is_none(),
        unsupported_reason: unsupported.map(str::to_owned),
        registered: entry.is_some(),
        current: entry.is_some_and(|(command, passes)| passes && command == expected_command(exe)),
        config_path: path.map(|path| path.to_string_lossy().into_owned()),
    }
}

/// The `[mcp_servers.monocode-browser]` table for Codex.
fn codex_table(exe: &str) -> String {
    let quote = |value: &str| toml::Value::String(value.to_string()).to_string();
    format!(
        "[mcp_servers.{SERVER_NAME}]\ncommand = {}\nargs = [{}]\nenv_vars = [{}, {}]\ntool_timeout_sec = {CODEX_TOOL_TIMEOUT_SECS}\n",
        quote(exe),
        quote(SUBCOMMAND),
        quote(ENV_ENDPOINT),
        quote(ENV_TOKEN)
    )
}

fn read_or_empty(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.to_string()),
    }
}

/// Our entry's command in a parsed Codex configuration.
fn codex_command(parsed: &toml::Table) -> Option<&str> {
    parsed
        .get("mcp_servers")
        .and_then(|servers| servers.get(SERVER_NAME))
        .and_then(|server| server.get("command"))
        .and_then(|command| command.as_str())
}

/// Append our table to a Codex `config.toml` without rewriting the user's
/// file: the text is only appended, after checking that the result still
/// parses and holds exactly our entry.
fn append_codex_table(path: &Path, exe: &str) -> Result<(), String> {
    let mut text = read_or_empty(path)?;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&codex_table(exe));
    let parsed: toml::Table = toml::from_str(&text)
        .map_err(|e| format!("Cannot add the browser server to {}: {e}", path.display()))?;
    if codex_command(&parsed) != Some(exe) {
        return Err(format!(
            "Cannot add the browser server to {}: its mcp_servers layout is not supported",
            path.display()
        ));
    }
    write_replacing(path, &text)
}

/// Remove our table (and its subtables) from an account's `config.toml`,
/// where `codex mcp remove` cannot reach: only the layout [`codex_table`]
/// writes is recognised, everything else is kept verbatim, and a result that
/// still holds the entry (another layout) is refused unwritten.
fn remove_codex_table(path: &Path) -> Result<(), String> {
    let raw = read_or_empty(path)?;
    let header = format!("[mcp_servers.{SERVER_NAME}]");
    let nested = format!("[mcp_servers.{SERVER_NAME}.");
    let mut kept = String::with_capacity(raw.len());
    let mut ours = false;
    for line in raw.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            ours = trimmed == header || trimmed.starts_with(&nested);
        }
        if !ours {
            kept.push_str(line);
        }
    }
    let mut text = kept.trim_end().to_string();
    if !text.is_empty() {
        text.push('\n');
    }
    let parsed: toml::Table = toml::from_str(&text).map_err(|e| {
        format!(
            "Cannot update the browser server in {}: {e}",
            path.display()
        )
    })?;
    if codex_command(&parsed).is_some() {
        return Err(format!(
            "Cannot update the browser server in {}: its mcp_servers layout is not supported",
            path.display()
        ));
    }
    write_replacing(path, &text)
}

/// Write `text` to a temporary file next to `path` and rename it over
/// `path`, keeping the file's permissions. `path` must already be the
/// symlink's target ([`write_target`]): a rename onto a link replaces it.
fn write_replacing(path: &Path, text: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid config path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let temporary = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
    let written = std::fs::write(&temporary, text).and_then(|_| {
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

/// The file a configuration path names, following symlinks: dotfile
/// managers (stow, chezmoi) keep these files as links, and writing a
/// temporary file and renaming it over the link would replace the link
/// with a plain copy. A dangling link resolves to its (missing) target.
fn write_target(path: &Path) -> PathBuf {
    let mut path = path.to_path_buf();
    // The bound stops a symlink loop.
    for _ in 0..32 {
        let Ok(target) = std::fs::read_link(&path) else {
            break;
        };
        path = match path.parent() {
            Some(parent) => parent.join(target),
            None => target,
        };
    }
    path
}

pub fn register_at(
    config: &dyn ProviderConfig,
    locations: &Locations,
    provider: &str,
    cwd: &str,
    exe: &str,
    binary_path: Option<&str>,
    opencode_major: Option<u32>,
) -> Result<BrowserMcpRegistration, String> {
    let before = status_at(config, locations, provider, exe, opencode_major);
    // A provider that cannot use the tools at all is an answer, not an
    // error: the reply says why, and nothing is written.
    if before.unsupported_reason.is_some() {
        return Ok(before);
    }
    if !before.supported {
        return Err(format!(
            "{provider} does not take MCP servers from MonoCode; it uses the browser CLI instead"
        ));
    }
    if before.current {
        return Ok(before);
    }
    let path = write_target(
        &locations
            .config_path(config, provider)
            .ok_or("Unsupported MCP provider")?,
    );
    let server = json!({"command": exe, "args": [SUBCOMMAND]});
    match provider {
        // User-scoped Claude servers are the `mcpServers` of `.claude.json`.
        "claude" if locations.account => config.put_json_server(&path, SERVER_NAME, server)?,
        "codex" if locations.account => {
            if before.registered {
                remove_codex_table(&path)?;
            }
            append_codex_table(&path, exe)?;
        }
        "claude" => {
            if before.registered {
                config.remove_mcp_via_cli("claude", cwd, SERVER_NAME, binary_path)?;
            }
            config.add_mcp_via_cli("claude", cwd, SERVER_NAME, &server, binary_path)?;
        }
        "codex" => {
            if before.registered {
                config.remove_mcp_via_cli("codex", cwd, SERVER_NAME, binary_path)?;
            }
            append_codex_table(&path, exe)?;
        }
        "cursor" => config.put_json_server(&path, SERVER_NAME, server)?,
        "opencode" => {
            let major = match opencode_major {
                Some(major) => major,
                None => config.opencode_major_version(cwd, binary_path)?,
            };
            if unsupported_reason(provider, Some(major)).is_some() {
                return Ok(status_at(config, locations, provider, exe, Some(major)));
            }
            config.put_opencode_server(&path, SERVER_NAME, server, major)?;
        }
        _ => return Err("Unsupported MCP provider".into()),
    }
    Ok(status_at(config, locations, provider, exe, opencode_major))
}

/// Registration state of the browser MCP server for every provider.
/// `opencode_binary` is the OpenCode the app runs, when it has its own.
pub fn mcp_status(
    config: &dyn ProviderConfig,
    opencode_binary: Option<&str>,
) -> Result<Vec<BrowserMcpRegistration>, String> {
    let locations = Locations::from_env(config)?;
    let exe = server_exe(&config.copies_dir()?, false)?;
    // An entry naming a copy that is not there yet (a new build) is
    // stale; registering makes the copy.
    let present = Path::new(&exe).is_file();
    let major = opencode_major(config, opencode_binary);
    Ok(MCP_PROVIDERS
        .iter()
        .map(|provider| {
            let mut status = status_at(config, &locations, provider, &exe, major);
            status.current &= present;
            // An account without the entry is fixed by registering.
            for account in account_locations(config, provider, None).unwrap_or_default() {
                status.current &= status_at(config, &account, provider, &exe, major).current;
            }
            status
        })
        .collect())
}

/// The MonoCode accounts of `provider` (Claude and Codex have them): every
/// account directory that exists, plus `account_id`'s, created when needed.
fn account_locations(
    config: &dyn ProviderConfig,
    provider: &str,
    account_id: Option<&str>,
) -> Result<Vec<Locations>, String> {
    if !matches!(provider, "claude" | "codex") {
        return Ok(Vec::new());
    }
    let mut dirs = config.account_dirs(provider, account_id)?;
    dirs.sort();
    dirs.dedup();
    Ok(dirs.into_iter().map(Locations::account).collect())
}

/// Make sure `provider` starts `monocode browser-mcp`, in its default
/// configuration and in every MonoCode account of it (`account_id`, the
/// session's account, is included even when its directory is new). Called
/// when a session of that provider is first granted `/browser`;
/// idempotent, and replaces an entry left by a moved or updated app.
/// Sessions already running see the tools from their next start. A provider
/// that cannot use the tools (OpenCode 2) gets `supported: false` and why.
/// `binary_path` is the provider's CLI the app runs, when it has its own.
pub fn mcp_register(
    config: &dyn ProviderConfig,
    provider: &str,
    cwd: &str,
    account_id: Option<&str>,
    binary_path: Option<&str>,
) -> Result<BrowserMcpRegistration, String> {
    let locations = Locations::from_env(config)?;
    let major = (provider == "opencode")
        .then(|| opencode_major(config, binary_path))
        .flatten();
    // Answer before copying anything for a provider that cannot use it.
    if unsupported_reason(provider, major).is_some() {
        return Ok(status_at(config, &locations, provider, "", major));
    }
    if !MCP_PROVIDERS.contains(&provider) {
        return Err(format!(
            "{provider} does not take MCP servers from MonoCode; it uses the browser CLI instead"
        ));
    }
    let exe = server_exe(&config.copies_dir()?, true)?;
    let status = register_at(config, &locations, provider, cwd, &exe, binary_path, major)?;
    for account in account_locations(config, provider, account_id)? {
        register_at(config, &account, provider, cwd, &exe, binary_path, major)?;
    }
    Ok(status)
}

/// Text appended to a turn while the session has browser access. With MCP
/// the tools describe themselves, so a short pointer is enough; a provider
/// without MCP gets the CLI instructions; a provider that can use neither
/// is told so, to pass on to the user. `checkout` is the thread's working
/// directory: where its branch serves its app is named too.
pub fn agent_instructions(
    config: &dyn ProviderConfig,
    provider: &str,
    checkout: Option<&Path>,
    binary_path: Option<&str>,
) -> String {
    let major = (provider == "opencode")
        .then(|| opencode_major(config, binary_path))
        .flatten();
    // Where the thread's branch serves its app (one browser for every
    // worktree, each thread on its own port).
    let app = match checkout {
        Some(dir) if unsupported_reason(provider, major).is_none() => Some(
            crate::devservers::agent_note(dir, &crate::devservers::for_dir(dir)),
        ),
        _ => None,
    };
    instructions_for(provider, major, app.as_deref())
}

pub fn instructions_for(provider: &str, opencode_major: Option<u32>, app: Option<&str>) -> String {
    let tools = if let Some(reason) = unsupported_reason(provider, opencode_major) {
        Tools::Unsupported(reason)
    } else if MCP_PROVIDERS.contains(&provider) {
        Tools::Mcp
    } else {
        Tools::Cli
    };
    instructions_block(tools, app)
}

/// How an agent reaches the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tools<'a> {
    /// The `browser_*` tools of the MCP server.
    Mcp,
    /// The `monocode browser` CLI, through the agent's shell.
    Cli,
    /// Neither works with this provider, and why.
    Unsupported(&'a str),
}

/// The `<monocode_browser>` block for an agent that reaches the browser
/// through `tools`; `app` names where the thread's branch serves its app.
pub fn instructions_block(tools: Tools<'_>, app: Option<&str>) -> String {
    let body = match tools {
        Tools::Unsupported(reason) => format!("The user enabled browser access in this thread, but MonoCode's browser tools do not work with this provider: {reason} Do not try the browser tools or the MonoCode browser CLI; tell the user if the task needs a browser."),
        Tools::Mcp => "Browser access is enabled in this thread. Use the browser_* tools (MonoCode browser); start with browser_tabs to open your own tab, and act on the refs in the snapshot each result brings. If the tools are missing, the provider has not loaded MonoCode's browser server yet; tell the user it appears after the session restarts.".to_string(),
        Tools::Cli => super::cli::instructions(),
    };
    match app {
        Some(app) => format!("<monocode_browser>\n{body}\n{app}\n</monocode_browser>"),
        None => format!("<monocode_browser>\n{body}\n</monocode_browser>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's side, as plain as it gets: no provider CLI, JSON files
    /// edited in place.
    struct TestConfig;

    impl ProviderConfig for TestConfig {
        fn home_dir(&self) -> Option<PathBuf> {
            None
        }
        fn opencode_config_path(&self, home: &Path, override_path: Option<&Path>) -> PathBuf {
            override_path
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".config/opencode/opencode.json"))
        }
        fn strip_jsonc(&self, raw: &str) -> String {
            raw.to_string()
        }
        fn put_json_server(&self, path: &Path, name: &str, server: Value) -> Result<(), String> {
            let mut root: Value = std::fs::read_to_string(path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_else(|| json!({}));
            root["mcpServers"][name] = server;
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(path, root.to_string()).map_err(|e| e.to_string())
        }
        fn put_opencode_server(
            &self,
            path: &Path,
            name: &str,
            server: Value,
            _major: u32,
        ) -> Result<(), String> {
            let mut command = vec![server["command"].clone()];
            command.extend(server["args"].as_array().cloned().unwrap_or_default());
            let mut root = json!({});
            root["mcp"][name] = json!({"type": "local", "command": command});
            std::fs::write(path, root.to_string()).map_err(|e| e.to_string())
        }
        fn add_mcp_via_cli(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &Value,
            _: Option<&str>,
        ) -> Result<(), String> {
            Err("no provider CLI in tests".into())
        }
        fn remove_mcp_via_cli(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: Option<&str>,
        ) -> Result<(), String> {
            Err("no provider CLI in tests".into())
        }
        fn opencode_major_version(&self, _: &str, _: Option<&str>) -> Result<u32, String> {
            Ok(1)
        }
        fn account_dirs(&self, _: &str, _: Option<&str>) -> Result<Vec<PathBuf>, String> {
            Ok(Vec::new())
        }
        fn copies_dir(&self) -> Result<PathBuf, String> {
            Err("no copies in tests".into())
        }
    }

    fn sandbox() -> (Locations, PathBuf) {
        let root = std::env::temp_dir().join(format!("monocode-reg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        (
            Locations {
                home: root.clone(),
                claude_dir: root.clone(),
                codex_home: root.join(".codex"),
                opencode_override: None,
                account: false,
            },
            root,
        )
    }

    #[test]
    fn json_providers_are_registered_once_and_repaired_when_the_app_moves() {
        let (locations, root) = sandbox();
        let cursor = locations.config_path(&TestConfig, "cursor").unwrap();
        std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
        std::fs::write(
            &cursor,
            r#"{"mcpServers":{"other":{"command":"x"}},"theme":"dark"}"#,
        )
        .unwrap();
        let status = status_at(
            &TestConfig,
            &locations,
            "cursor",
            "/Apps/MonoCode/monocode",
            None,
        );
        assert!(status.supported && !status.registered && !status.current);

        let first = register_at(
            &TestConfig,
            &locations,
            "cursor",
            "/",
            "/Apps/MonoCode/monocode",
            None,
            None,
        )
        .unwrap();
        assert!(first.registered && first.current);
        // Idempotent: a second call changes nothing.
        let raw = std::fs::read_to_string(&cursor).unwrap();
        register_at(
            &TestConfig,
            &locations,
            "cursor",
            "/",
            "/Apps/MonoCode/monocode",
            None,
            None,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&cursor).unwrap(), raw);

        let moved = status_at(
            &TestConfig,
            &locations,
            "cursor",
            "/Applications/MonoCode/monocode",
            None,
        );
        assert!(moved.registered && !moved.current);
        let repaired = register_at(
            &TestConfig,
            &locations,
            "cursor",
            "/",
            "/Applications/MonoCode/monocode",
            None,
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
        let path = locations.config_path(&TestConfig, "codex").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "# my settings\nmodel = \"o4\"\n\n[mcp_servers.docs]\ncommand = \"npx\"\n";
        std::fs::write(&path, original).unwrap();
        let exe = r"C:\Program Files\MonoCode\monocode.exe";
        let registered =
            register_at(&TestConfig, &locations, "codex", "/", exe, None, None).unwrap();
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
            server["tool_timeout_sec"].as_integer(),
            Some(CODEX_TOOL_TIMEOUT_SECS)
        );
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
        let stale = status_at(&TestConfig, &locations, "codex", "/m", None);
        assert!(stale.registered && !stale.current);
        // Nor would one keeping Codex's 60 s limit on tool calls.
        std::fs::write(
            &path,
            format!(
                "[mcp_servers.{SERVER_NAME}]\ncommand = \"/m\"\nargs = [\"browser-mcp\"]\nenv_vars = [\"{ENV_ENDPOINT}\", \"{ENV_TOKEN}\"]\n"
            ),
        )
        .unwrap();
        assert!(!status_at(&TestConfig, &locations, "codex", "/m", None).current);

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
    fn accounts_are_written_directly_and_repaired_without_the_provider_cli() {
        let (_, root) = sandbox();
        let account = Locations::account(root.join("acct"));
        // Claude: the account's own `.claude.json`, never the default one.
        let claude = account.config_path(&TestConfig, "claude").unwrap();
        assert_eq!(claude, root.join("acct/.claude.json"));
        assert!(
            register_at(&TestConfig, &account, "claude", "/", "/m", None, None)
                .unwrap()
                .current
        );
        let config: Value =
            serde_json::from_str(&std::fs::read_to_string(&claude).unwrap()).unwrap();
        assert_eq!(config["mcpServers"][SERVER_NAME]["command"], "/m");
        assert!(!root.join(".claude.json").exists());

        // Codex: a stale table is replaced in place; the rest is kept.
        let codex = account.config_path(&TestConfig, "codex").unwrap();
        let mine = "model = \"o4\"\n\n[mcp_servers.docs]\ncommand = \"npx\"\n";
        std::fs::write(
            &codex,
            format!("{mine}\n[mcp_servers.{SERVER_NAME}]\ncommand = \"/old\"\nargs = [\"browser-mcp\"]\n\n[mcp_servers.{SERVER_NAME}.tools.browser_click]\napproval_mode = \"auto\"\n"),
        )
        .unwrap();
        assert!(
            register_at(&TestConfig, &account, "codex", "/", "/m", None, None)
                .unwrap()
                .current
        );
        let text = std::fs::read_to_string(&codex).unwrap();
        assert!(text.starts_with(mine), "{text}");
        assert!(
            !text.contains("/old") && !text.contains("approval_mode"),
            "{text}"
        );
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["docs"]["command"].as_str(),
            Some("npx")
        );
        // Repairing again does not pile up blank lines.
        std::fs::write(&codex, text.replace("\"/m\"", "\"/old\"")).unwrap();
        register_at(&TestConfig, &account, "codex", "/", "/m", None, None).unwrap();
        assert_eq!(std::fs::read_to_string(&codex).unwrap(), text);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_configs_are_written_through_to_their_target() {
        let (locations, root) = sandbox();
        let dotfiles = root.join("dotfiles");
        std::fs::create_dir_all(&dotfiles).unwrap();
        std::fs::create_dir_all(root.join(".cursor")).unwrap();
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::write(dotfiles.join("mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
        std::fs::write(dotfiles.join("config.toml"), "model = \"o4\"\n").unwrap();
        let cursor = locations.config_path(&TestConfig, "cursor").unwrap();
        let codex = locations.config_path(&TestConfig, "codex").unwrap();
        // One absolute link, one relative.
        std::os::unix::fs::symlink(dotfiles.join("mcp.json"), &cursor).unwrap();
        std::os::unix::fs::symlink("../dotfiles/config.toml", &codex).unwrap();
        register_at(&TestConfig, &locations, "cursor", "/", "/m", None, None).unwrap();
        register_at(&TestConfig, &locations, "codex", "/", "/m", None, None).unwrap();
        for (link, target) in [(&cursor, "mcp.json"), (&codex, "config.toml")] {
            assert!(std::fs::symlink_metadata(link)
                .unwrap()
                .file_type()
                .is_symlink());
            assert!(std::fs::read_to_string(dotfiles.join(target))
                .unwrap()
                .contains("/m"));
        }
        assert!(status_at(&TestConfig, &locations, "cursor", "/m", None).current);
        assert!(status_at(&TestConfig, &locations, "codex", "/m", None).current);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opencode_layouts_are_read_and_unknown_providers_use_the_cli() {
        let (locations, root) = sandbox();
        let path = locations.config_path(&TestConfig, "opencode").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            json!({"mcp": {SERVER_NAME: {"type": "local", "command": ["/m", "browser-mcp"]}}})
                .to_string(),
        )
        .unwrap();
        assert!(status_at(&TestConfig, &locations, "opencode", "/m", None).current);
        std::fs::write(
            &path,
            json!({"mcp": {"servers": {SERVER_NAME: {"type": "local", "command": ["/old", "browser-mcp"]}}}}).to_string(),
        )
        .unwrap();
        let stale = status_at(&TestConfig, &locations, "opencode", "/m", None);
        assert!(stale.registered && !stale.current);

        let pi = status_at(&TestConfig, &locations, "pi", "/m", None);
        assert!(!pi.supported && !pi.registered);
        assert!(register_at(&TestConfig, &locations, "pi", "/", "/m", None, None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opencode_2_is_reported_unsupported_and_never_registered() {
        let (locations, root) = sandbox();
        let path = locations.config_path(&TestConfig, "opencode").unwrap();
        let v2 = status_at(&TestConfig, &locations, "opencode", "/m", Some(2));
        assert!(!v2.supported && !v2.registered);
        assert_eq!(
            v2.unsupported_reason.as_deref(),
            Some(OPENCODE_2_UNSUPPORTED)
        );
        let reply = register_at(
            &TestConfig,
            &locations,
            "opencode",
            "/",
            "/m",
            None,
            Some(2),
        )
        .unwrap();
        assert!(!reply.supported && !reply.registered);
        assert_eq!(
            reply.unsupported_reason.as_deref(),
            Some(OPENCODE_2_UNSUPPORTED)
        );
        assert!(!path.exists(), "nothing is written for OpenCode 2");
        let v1 = status_at(&TestConfig, &locations, "opencode", "/m", Some(1));
        assert!(v1.supported && v1.unsupported_reason.is_none());
        let json = serde_json::to_value(&v2).unwrap();
        assert_eq!(json["unsupportedReason"], OPENCODE_2_UNSUPPORTED);
        assert!(serde_json::to_value(&v1)
            .unwrap()
            .get("unsupportedReason")
            .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn instructions_point_mcp_providers_at_tools_and_others_at_the_cli() {
        let mcp = instructions_for("claude", None, None);
        assert!(mcp.contains("browser_tabs") && !mcp.contains(" browser --help"));
        let cli = instructions_for("pi", None, None);
        assert!(cli.contains("browser --help") && cli.starts_with("<monocode_browser>"));
        // OpenCode 1 takes the MCP server; OpenCode 2 can use neither path.
        assert!(instructions_for("opencode", Some(1), None).contains("browser_tabs"));
        let opencode2 = instructions_for("opencode", Some(2), None);
        assert!(opencode2.contains("OpenCode 2") && !opencode2.contains("browser --help"));
        // The thread's app goes inside the block, after the instructions.
        let with_app = instructions_for("claude", None, Some("APP NOTE"));
        assert!(with_app.ends_with("\nAPP NOTE\n</monocode_browser>"));
    }
}
