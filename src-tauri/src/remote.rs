// Companion link (thin iPad client) — host-side pairing, token store,
// Tailscale tailnet status, and `tailscale serve` lifecycle.
//
// MERGE NOTE (upstream-friendly): this module is self-contained and only
// *calls* existing commands — it changes none of them. Desktop behavior is
// untouched until the user enables the companion link in Settings.
//
// LAN and Tailscale can both be live at once: the TCP listener is always
// 0.0.0.0, and the iPad keeps both hosts so it can switch without re-pairing.
// Tailscale path: each app embeds a userspace node and signs in with Google.
// The system Tailscale app is optional on the Mac (`systemTailscale`).

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

/// Default host port. Mirrors COMPANION_PORT_DEFAULT in protocol.ts.
pub const COMPANION_PORT_DEFAULT: u16 = 17233;
/// Protocol version. Mirrors COMPANION_PROTO_VERSION in protocol.ts.
pub const COMPANION_PROTO_VERSION: u32 = 1;
/// WebSocket path. Mirrors COMPANION_WS_PATH in protocol.ts.
pub const COMPANION_WS_PATH: &str = "/v1/connect";

const TOKEN_DIR: &str = "companion";
const TOKEN_FILE: &str = "token";
const TOKEN_BYTES: usize = 32;

/// Commands the companion link may forward. Mirrors the inverse of
/// LOCAL_ONLY_COMMANDS in protocol.ts: window chrome stays host-local,
/// everything below is safe to serve remotely. The WS server enforces this.
pub const REMOTE_COMMAND_ALLOWLIST: &[&str] = &[
    // Agent harnesses (spawning stays on the host; the iPad never runs CLIs).
    "harness_resolve_cursor",
    "harness_resolve_codex",
    "harness_resolve_opencode",
    "harness_resolve_claude",
    "harness_resolve_pi",
    "harness_resolve_omp",
    "harness_resolve_fx",
    "harness_resolve_grok",
    "harness_free_port",
    "harness_spawn",
    "harness_write",
    "harness_kill",
    "harness_kill_all",
    "harness_http",
    "harness_sse_open",
    "harness_sse_close",
    "harness_exec",
    // Terminals.
    "pty_spawn",
    "pty_write",
    "pty_resize",
    "pty_status",
    "pty_kill",
    "pty_kill_all",
    // Sessions / workspace.
    "session_upsert",
    "session_list_by_project",
    "session_search",
    "session_get",
    "session_delete",
    "session_set_archived",
    "session_set_pinned",
    "session_set_in_flight",
    "session_list_in_flight",
    "session_take_in_flight",
    "workspace_set_snapshot",
    "workspace_get_snapshot",
    // Filesystem / git.
    "list_dir",
    "list_project_files",
    "git_diff_stats",
    "git_diff_index",
    "git_diff_files",
    "git_file_diff",
    "git_history",
    "git_commit_files",
    "git_commit_file_diff",
    "git_stage_file",
    "git_stage_contents",
    "git_unstage_file",
    "git_discard_file",
    "git_discard_all",
    "git_stage_all",
    "git_unstage_all",
    "git_commit",
    "git_staged_context",
    "git_push",
    "git_pull",
    "git_sync",
    "git_range_context",
    "git_pr_status",
    "git_pr_create",
    "git_github_repo",
    "git_github_work_items",
    "git_github_work_item_details",
    "git_github_work_item_thread",
    "git_github_work_item_comment",
    "git_github_pr_diff",
    "git_branches",
    "git_checkout",
    "git_create_branch",
    "git_stash",
    "create_path",
    "rename_path",
    "delete_path",
    "copy_path",
    "move_path",
    "reveal_path",
    "clone_repo",
    "read_file_preview",
    "stat_files",
    "inspect_paths",
    "read_file_base64",
    "read_binary_file",
    "write_attachment",
    "read_text_file",
    "write_text_file",
    // Search / skills / misc host data.
    "search_project",
    "list_skills",
    "cursor_tool_calls",
    "fetch_claude_usage",
    "fetch_inbox_media",
    "linear_status",
    "linear_set_token",
    "linear_list_teams",
    "linear_list_issues",
    "linear_issue_details",
    "linear_issue_thread",
    "linear_issue_comment",
    "notes_list",
    "notes_get",
    "notes_upsert",
    "notes_delete",
    "session_checkpoint_ensure",
    "session_checkpoint_prepare",
    "session_checkpoint_capture",
    "session_checkpoint_status",
    "session_checkpoint_file_diff",
    "session_checkpoint_undo",
    "session_checkpoint_keep",
    "save_project_logo",
    "remove_project_logo",
    "default_cwd",
    "home_dir",
    "remote_peers",
    // Read-only node state (the iPad pairing screen benefits later too).
    // Start/stop stay host-only. remote_status lets a paired iPad learn
    // the other live route (LAN vs Tailscale) without re-pairing.
    // Embed start/stop/status stay on each device (iPad tsnet is local).
    "remote_status",
    "stage_window_transfer",
    "take_window_transfer",
];

/// Future WS server gate: unknown/new commands default to denied until they
/// are reviewed into the list above.
pub fn is_remote_command(command: &str) -> bool {
    REMOTE_COMMAND_ALLOWLIST.contains(&command)
}

/// One companion route. LAN and Tailscale can both be on; the TCP listener
/// is always `0.0.0.0` so LAN keeps working while the embedded node is up.
#[derive(Serialize, serde::Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RemoteMode {
    #[default]
    Tailscale,
    Lan,
}

struct RemoteConfig {
    enabled: bool,
    lan: bool,
    tailscale: bool,
    /// Also run `tailscale serve` through the system Tailscale app. Off by
    /// default — the embedded node is the Google-login path.
    system_tailscale: bool,
    port: u16,
    token: Option<String>,
    server: Option<ServerHandle>,
    forwarding_registered: bool,
    embed_task: Option<tauri::async_runtime::JoinHandle<()>>,
    embed_snapshot: EmbedSnapshot,
    pair_code: Option<PairingCode>,
}

/// Short-lived 6-digit claim code for easy manual pairing. The code itself
/// never grants session access: it is single-use and only exchanges for the
/// real pairing token over the same TLS/trusted link. Brute force is
/// pointless — a handful of wrong guesses burns the code.
struct PairingCode {
    code: String,
    expires_at: std::time::SystemTime,
    attempts: u8,
}

const PAIR_CODE_TTL_SECS: u64 = 600;
const PAIR_CODE_MAX_ATTEMPTS: u8 = 10;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PairingCodeView {
    /// Displayed grouped as "123 456".
    pub code: String,
    /// Seconds until expiry (capped, informational).
    pub expires_in: u64,
}

/// A running accept loop. Aborted on disable / port change / re-enable.
/// Uses the Tauri runtime handle (not tokio's) so it can be spawned from
/// setup(), which runs outside the async runtime on the main thread.
struct ServerHandle {
    port: u16,
    task: tauri::async_runtime::JoinHandle<()>,
}

pub struct RemoteState {
    inner: Mutex<RemoteConfig>,
    shared: std::sync::Arc<crate::remote_server::ServerShared>,
}

impl RemoteState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(RemoteConfig {
                enabled: false,
                lan: false,
                tailscale: false,
                system_tailscale: false,
                port: COMPANION_PORT_DEFAULT,
                token: None,
                server: None,
                forwarding_registered: false,
                embed_task: None,
                embed_snapshot: EmbedSnapshot::default(),
                pair_code: None,
            }),
            shared: std::sync::Arc::new(crate::remote_server::ServerShared::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RemoteConfig> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Companion listener port (shared by the TCP and embedded listeners).
    /// Desktop-only today (mobile shells never serve); the method stays
    /// un-gated so a future mobile use needs no refactor.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn companion_port(&self) -> u16 {
        self.lock().port
    }

    /// Pairing token, generating and persisting it on first use.
    pub fn pairing_token(&self, app: &AppHandle) -> Result<String, String> {
        let mut config = self.lock();
        match &config.token {
            Some(token) => Ok(token.clone()),
            None => {
                let token = load_or_create_token(app)?;
                config.token = Some(token.clone());
                Ok(token)
            }
        }
    }

    pub fn companion_shared(&self) -> std::sync::Arc<crate::remote_server::ServerShared> {
        std::sync::Arc::clone(&self.shared)
    }

    /// Hosts the iPad should remember for this pairing. Only currently
    /// enabled routes are advertised, so a LAN-only link never plants a
    /// stale tailnet name as the fallback.
    pub fn advertised_hosts(&self) -> AdvertisedHosts {
        let (lan, tailscale, system, embed_ip) = {
            let config = self.lock();
            (
                config.lan,
                config.tailscale,
                config.system_tailscale,
                config.embed_snapshot.tailnet_ip.clone(),
            )
        };
        AdvertisedHosts {
            lan_ip: if lan { lan_ip() } else { None },
            tailnet_host: if tailscale {
                embed_ip.filter(|ip| !ip.is_empty()).or_else(|| {
                    // MagicDNS from the system client is only useful when
                    // that client is also serving the companion port.
                    if system {
                        tailnet_hostname()
                    } else {
                        None
                    }
                })
            } else {
                None
            },
        }
    }

    /// Swap the embedded-node task, returning the previous one to abort.
    pub fn embed_replace_task(
        &self,
        task: tauri::async_runtime::JoinHandle<()>,
    ) -> Option<tauri::async_runtime::JoinHandle<()>> {
        self.lock().embed_task.replace(task)
    }

    pub fn embed_take_task(&self) -> Option<tauri::async_runtime::JoinHandle<()>> {
        self.lock().embed_task.take()
    }

    pub fn embed_snapshot(&self) -> EmbedSnapshot {
        self.lock().embed_snapshot.clone()
    }

    pub fn embed_update_snapshot(&self, snapshot: EmbedSnapshot) {
        self.lock().embed_snapshot = snapshot;
    }
}

/// Status of the embedded tailnet node. Same shape on Mac (tailscale-rs)
/// and iPad (userspace tsnet) so the Companion page can share one UI.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmbedSnapshot {
    pub running: bool,
    pub authorized: bool,
    pub tailnet_ip: Option<String>,
    /// Browser URL for interactive (Google SSO) authorization.
    pub login_url: Option<String>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailnet_name: Option<String>,
    /// Tailscale node hostname (this Mac or this iPad).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub enabled: bool,
    pub lan: bool,
    pub tailscale: bool,
    /// Preferred label when a caller still thinks in one mode: Tailscale if
    /// that path is on, otherwise LAN. Both flags can be true at once.
    pub mode: RemoteMode,
    pub port: u16,
    /// Protocol version the host speaks.
    pub version: u32,
    pub lan_ip: Option<String>,
    pub tailnet_host: Option<String>,
    /// True when the host also forwards through the system Tailscale app.
    #[serde(default)]
    pub system_tailscale: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdvertisedHosts {
    pub lan_ip: Option<String>,
    pub tailnet_host: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RemotePairing {
    pub port: u16,
    pub token: String,
    /// Best-effort LAN IP for the QR payload (Tailscale users type their
    /// tailnet hostname instead). Null when it cannot be determined.
    pub lan_ip: Option<String>,
    /// Best-effort MagicDNS name (`machine.tailnet.ts.net`) when the Tailscale
    /// CLI is installed and logged in. Prefill for the `tailscale serve` path:
    /// `tailscale serve --bg --https=443 http://localhost:<port>`.
    pub tailnet_host: Option<String>,
    pub version: u32,
}

#[tauri::command]
pub fn remote_status(state: State<RemoteState>) -> RemoteStatus {
    let hosts = state.advertised_hosts();
    let config = state.lock();
    RemoteStatus {
        enabled: config.enabled,
        lan: config.lan,
        tailscale: config.tailscale,
        mode: if config.tailscale {
            RemoteMode::Tailscale
        } else {
            RemoteMode::Lan
        },
        port: config.port,
        version: COMPANION_PROTO_VERSION,
        lan_ip: hosts.lan_ip,
        tailnet_host: hosts.tailnet_host,
        system_tailscale: config.system_tailscale,
    }
}

/// Turn a route on (or, with no mode, both). Does not disable the other
/// route. Persist the pairing token, bind the listener, start Tailscale
/// when that path is live.
#[tauri::command]
pub async fn remote_enable(
    app: AppHandle,
    state: State<'_, RemoteState>,
    port: Option<u16>,
    mode: Option<RemoteMode>,
) -> Result<RemotePairing, String> {
    if let Some(port) = port {
        if port == 0 {
            return Err("Port must be non-zero".into());
        }
    }
    // Resolve token + port without holding the lock across awaits.
    let (port, token, restart, lan, tailscale) = {
        let mut config = state.lock();
        if let Some(port) = port {
            config.port = port;
        }
        match mode {
            Some(RemoteMode::Lan) => config.lan = true,
            Some(RemoteMode::Tailscale) => config.tailscale = true,
            None if !config.lan && !config.tailscale => {
                config.lan = true;
                config.tailscale = true;
            }
            None => {}
        }
        let token = match &config.token {
            Some(token) => token.clone(),
            None => {
                let token = load_or_create_token(&app)?;
                config.token = Some(token.clone());
                token
            }
        };
        config.enabled = config.lan || config.tailscale;
        let restart = config.server.as_ref().map(|s| s.port) != Some(config.port);
        (config.port, token, restart, config.lan, config.tailscale)
    };
    write_enabled(&app, Some((port, lan, tailscale)))?;

    if restart {
        start_server(&app, &state, port, &token).await?;
    }

    apply_routes(&app, &state, port, tailscale).await;

    Ok(RemotePairing {
        port,
        token,
        lan_ip: lan_ip(),
        tailnet_host: tailnet_hostname(),
        version: COMPANION_PROTO_VERSION,
    })
}

/// Turn one route on or off without dropping the other. Disabling the last
/// live route tears the whole link down.
#[tauri::command]
pub async fn remote_set_route(
    app: AppHandle,
    state: State<'_, RemoteState>,
    route: RemoteMode,
    enabled: bool,
) -> Result<RemotePairing, String> {
    if enabled {
        return remote_enable(app, state, None, Some(route)).await;
    }
    let (port, token, lan, tailscale, empty) = {
        let mut config = state.lock();
        match route {
            RemoteMode::Lan => config.lan = false,
            RemoteMode::Tailscale => config.tailscale = false,
        }
        config.enabled = config.lan || config.tailscale;
        let token = config.token.clone().unwrap_or_default();
        (
            config.port,
            token,
            config.lan,
            config.tailscale,
            !config.enabled,
        )
    };
    if empty {
        remote_disable(app, state).await?;
        return Ok(RemotePairing {
            port,
            token,
            lan_ip: lan_ip(),
            tailnet_host: tailnet_hostname(),
            version: COMPANION_PROTO_VERSION,
        });
    }
    write_enabled(&app, Some((port, lan, tailscale)))?;
    apply_routes(&app, &state, port, tailscale).await;
    Ok(RemotePairing {
        port,
        token,
        lan_ip: lan_ip(),
        tailnet_host: tailnet_hostname(),
        version: COMPANION_PROTO_VERSION,
    })
}

async fn apply_routes(app: &AppHandle, state: &RemoteState, port: u16, tailscale: bool) {
    let system = state.lock().system_tailscale;
    if tailscale {
        start_embedded_best_effort(app).await;
        if system {
            serve_best_effort(port, true).await;
        }
    } else {
        stop_embedded(app, state).await;
        if system {
            serve_best_effort(port, false).await;
        }
    }
}

/// Bind with retries on AddrInUse: a previous instance's socket can still
/// be draining when the app restarts quickly.
async fn bind_with_retry(port: u16) -> Result<tokio::net::TcpListener, String> {
    let mut attempt = 0;
    loop {
        match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && attempt < 8 => {
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            Err(error) => {
                return Err(format!("Failed to listen on port {port}: {error}"));
            }
        }
    }
}

/// Bind the listener and start the accept loop, replacing any previous one.
/// Shared by `remote_enable` and boot-time autostart.
async fn start_server(
    app: &AppHandle,
    state: &RemoteState,
    port: u16,
    token: &str,
) -> Result<(), String> {
    // 0.0.0.0 so direct-LAN companions and `tailscale serve` (which dials
    // localhost) both reach it. Every frame still needs the token.
    // Retried: a previous instance's socket can still be draining when the
    // app restarts quickly (or two windows race at boot).
    let listener = bind_with_retry(port).await?;
    let task = tauri::async_runtime::spawn(crate::remote_server::run(
        app.clone(),
        listener,
        token.to_string(),
        state.companion_shared(),
    ));
    let mut config = state.lock();
    if let Some(previous) = config.server.take() {
        previous.task.abort();
    }
    if !config.forwarding_registered {
        config.forwarding_registered = true;
        crate::remote_server::register_event_forwarding(app, &state.companion_shared());
    }
    config.server = Some(ServerHandle { port, task });
    Ok(())
}

/// Best-effort embedded node start for link enable. Saved keys/auth carry
/// over, so after the first Google login this is silent. Never fails.
async fn start_embedded_best_effort(app: &AppHandle) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = app;
        return;
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if let Err(error) = crate::tailnet_embed::start(app.clone(), None, Vec::new()).await {
            eprintln!("companion: embedded node did not start: {error}");
        }
    }
}

/// Best-effort embedded node stop: abort the task, reset the snapshot, and
/// clear the boot flag so a stopped link stays stopped. Never fails.
async fn stop_embedded(app: &AppHandle, state: &RemoteState) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = (app, state);
        return;
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        if let Some(task) = state.embed_take_task() {
            task.abort();
        }
        crate::tailnet_embed::set_wanted(app, false);
        state.embed_update_snapshot(EmbedSnapshot::default());
    }
}

/// Best-effort `tailscale serve` management for link enable/disable.
/// No CLI, or a CLI that refuses — both fine, LAN pairing is unaffected.
async fn serve_best_effort(port: u16, on: bool) {
    let args: Vec<String> = if on {
        vec![
            "serve".into(),
            "--bg".into(),
            format!("--tcp={port}"),
            format!("tcp://localhost:{port}"),
        ]
    } else {
        vec!["serve".into(), format!("--tcp={port}"), "off".into()]
    };
    let owned = args.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
        tailscale_cli(&refs, STD_CLI_TIMEOUT)
    })
    .await;
    match result {
        Ok(Some(output)) if output.status.success() => {}
        Ok(Some(output)) => eprintln!("companion: tailscale serve: {}", serve_error(&output)),
        Ok(None) => {}
        Err(error) => eprintln!("companion: tailscale serve: {error}"),
    }
}

const ENABLED_FILE: &str = "enabled";
fn enabled_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(data_dir.join(TOKEN_DIR).join(ENABLED_FILE))
}

/// Stored as `port:routes` (`17233:lan,tailscale`). A bare port, or the
/// legacy `17233:tailscale` / `17233:lan` tokens, still load.
fn parse_route_flags(raw: Option<&str>) -> (bool, bool) {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return (false, true);
    };
    if raw.eq_ignore_ascii_case("both") {
        return (true, true);
    }
    let lan = raw.split(',').any(|part| part.trim().eq_ignore_ascii_case("lan"));
    let tailscale = raw
        .split(',')
        .any(|part| part.trim().eq_ignore_ascii_case("tailscale"));
    if !lan && !tailscale {
        return (false, true);
    }
    (lan, tailscale)
}

fn format_route_flags(lan: bool, tailscale: bool) -> String {
    match (lan, tailscale) {
        (true, true) => "lan,tailscale".into(),
        (true, false) => "lan".into(),
        (false, true) => "tailscale".into(),
        (false, false) => String::new(),
    }
}

fn read_enabled(app: &AppHandle) -> Option<(u16, bool, bool)> {
    let raw = std::fs::read_to_string(enabled_path(app).ok()?).ok()?;
    let (port_raw, routes_raw) = match raw.trim().split_once(':') {
        Some((port, routes)) => (port, Some(routes)),
        None => (raw.trim(), None),
    };
    let port: u16 = port_raw.parse().ok().filter(|port| *port != 0)?;
    let (lan, tailscale) = parse_route_flags(routes_raw);
    if !lan && !tailscale {
        return None;
    }
    Some((port, lan, tailscale))
}

fn write_enabled(
    app: &AppHandle,
    enabled: Option<(u16, bool, bool)>,
) -> Result<(), String> {
    let path = enabled_path(app)?;
    match enabled {
        Some((port, lan, tailscale)) => {
            let routes = format_route_flags(lan, tailscale);
            std::fs::write(&path, format!("{port}:{routes}")).map_err(|e| e.to_string())
        }
        None => {
            let _ = std::fs::remove_file(&path);
            Ok(())
        }
    }
}

const SYSTEM_FILE: &str = "system";

fn system_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(data_dir.join(TOKEN_DIR).join(SYSTEM_FILE))
}

fn read_system(app: &AppHandle) -> bool {
    let Ok(path) = system_path(app) else {
        return false;
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let trimmed = raw.trim();
    trimmed == "1" || trimmed.eq_ignore_ascii_case("true")
}

fn write_system(app: &AppHandle, on: bool) -> Result<(), String> {
    let path = system_path(app)?;
    if on {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, "1\n").map_err(|e| e.to_string())
    } else {
        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}

/// Opt-in: also expose the companion port through the system Tailscale app
/// (`tailscale serve`). The embedded Google-login node stays the default.
#[tauri::command]
pub async fn remote_set_system_tailscale(
    app: AppHandle,
    state: State<'_, RemoteState>,
    enabled: bool,
) -> Result<RemoteStatus, String> {
    {
        let mut config = state.lock();
        config.system_tailscale = enabled;
    }
    write_system(&app, enabled)?;
    let (port, tailscale) = {
        let config = state.lock();
        (config.port, config.tailscale)
    };
    if tailscale {
        serve_best_effort(port, enabled).await;
    }
    Ok(remote_status(state))
}

/// Boot-time autostart: if the link was enabled when the app last ran,
/// bring the listener back in the same mode without opening Settings.
pub fn autostart(app: &AppHandle) {
    let system = read_system(app);
    {
        let state: State<'_, RemoteState> = app.state();
        state.lock().system_tailscale = system;
    }
    let Some((port, lan, tailscale)) = read_enabled(app) else {
        return;
    };
    let Ok(token) = load_or_create_token(app) else {
        return;
    };
    let state: State<'_, RemoteState> = app.state();
    {
        let mut config = state.lock();
        config.enabled = true;
        config.port = port;
        config.lan = lan;
        config.tailscale = tailscale;
        config.system_tailscale = read_system(&app);
        config.token = Some(token.clone());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state: State<'_, RemoteState> = app.state();
        if let Err(error) = start_server(&app, &state, port, &token).await {
            eprintln!("companion autostart failed: {error}");
            return;
        }
        apply_routes(&app, &state, port, tailscale).await;
    });
}

/// Connected companion count, for Settings ("1 iPad connected") and
/// headless verification.
#[tauri::command]
pub fn remote_peers(state: State<RemoteState>) -> PeerCount {
    PeerCount {
        connected: state.companion_shared().peer_count(),
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PeerCount {
    pub connected: usize,
}

#[tauri::command]
pub async fn remote_disable(app: AppHandle, state: State<'_, RemoteState>) -> Result<(), String> {
    let (port, system) = {
        let config = state.lock();
        (config.port, config.system_tailscale)
    };
    stop_embedded(&app, &state).await;
    {
        let mut config = state.lock();
        config.enabled = false;
        config.lan = false;
        config.tailscale = false;
        // Disabling burns the manual code too — a stale code must never
        // outlive the link it was generated for.
        config.pair_code = None;
        if let Some(server) = config.server.take() {
            server.task.abort();
        }
    }
    write_enabled(&app, None)?;
    if system {
        serve_best_effort(port, false).await;
    }
    Ok(())
}

/// Pairing payload for the Settings QR screen. Does not flip `enabled` by
/// itself; the link only serves once `remote_enable` has run.
#[tauri::command]
pub fn remote_pairing(app: AppHandle, state: State<RemoteState>) -> Result<RemotePairing, String> {
    let mut config = state.lock();
    let token = match &config.token {
        Some(token) => token.clone(),
        None => {
            let token = load_or_create_token(&app)?;
            config.token = Some(token.clone());
            token
        }
    };
    Ok(RemotePairing {
        port: config.port,
        token,
        lan_ip: lan_ip(),
        tailnet_host: tailnet_hostname(),
        version: COMPANION_PROTO_VERSION,
    })
}

/// Mint (or re-mint) the 6-digit manual pairing code. Shown big on the
/// host; typed on the iPad instead of the full token. Single-use, expiring,
/// attempt-limited — see PairingCode.
#[tauri::command]
pub fn remote_pairing_code(state: State<RemoteState>) -> Result<PairingCodeView, String> {
    if !state.lock().enabled {
        return Err("Enable the companion link first.".into());
    }
    let code = mint_pair_digits()?;
    let view = PairingCodeView {
        code: format!("{} {}", &code[..3], &code[3..]),
        expires_in: PAIR_CODE_TTL_SECS,
    };
    state.lock().pair_code = Some(PairingCode {
        code,
        expires_at: std::time::SystemTime::now()
            + std::time::Duration::from_secs(PAIR_CODE_TTL_SECS),
        attempts: 0,
    });
    Ok(view)
}

fn mint_pair_digits() -> Result<String, String> {
    let mut bytes = [0u8; 6];
    read_secure_random(&mut bytes).map_err(|_| "No random source".to_string())?;
    Ok(bytes.iter().map(|b| (b % 10).to_string()).collect())
}

fn normalize_pair_code(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Verify a claimed code. Single-use: success and abuse both burn it.
pub(crate) fn verify_pair_code(state: &RemoteState, claimed: &str) -> Result<(), String> {
    let mut config = state.lock();
    let Some(entry) = config.pair_code.take() else {
        return Err("No pairing code is active — generate one on the host.".into());
    };
    if entry.expires_at < std::time::SystemTime::now() {
        return Err("That code expired — generate a fresh one on the host.".into());
    }
    if normalize_pair_code(claimed) == entry.code {
        return Ok(());
    }
    let attempts = entry.attempts + 1;
    if attempts >= PAIR_CODE_MAX_ATTEMPTS {
        return Err("Too many wrong guesses — code burned. Generate a fresh one.".into());
    }
    config.pair_code = Some(PairingCode { attempts, ..entry });
    Err("Wrong code — check the host screen and retry.".into())
}

fn token_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(data_dir.join(TOKEN_DIR).join(TOKEN_FILE))
}

fn load_or_create_token(app: &AppHandle) -> Result<String, String> {
    let path = token_path(app)?;
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let token = raw.trim().to_string();
        if is_pairing_token(&token) {
            return Ok(token);
        }
    }
    let token = generate_token();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, format!("{token}\n")).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(token)
}

fn generate_token() -> String {
    let mut bytes = [0u8; TOKEN_BYTES];
    if read_secure_random(&mut bytes).is_err() {
        // Fallback only (non-unix): still unique per host+time, but the unix
        // path above is the real token source on every supported host OS.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            .wrapping_add(std::process::id() as u128);
        let mut state = seed | 1;
        for chunk in bytes.chunks_mut(8) {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            let rand = state.wrapping_mul(0x2545F4914F6CDD1D);
            chunk.copy_from_slice(&rand.to_le_bytes()[..chunk.len()]);
        }
    }
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(unix)]
fn read_secure_random(buf: &mut [u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

#[cfg(not(unix))]
fn read_secure_random(_buf: &mut [u8]) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no secure random source",
    ))
}

fn is_pairing_token(value: &str) -> bool {
    // 32 bytes base64url-no-pad render as 43 chars.
    value.len() == 43
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Best-effort MagicDNS name for the Tailscale pairing path. Only reported
/// while the tailnet is actually running — a stale name is worse than none.
fn tailnet_hostname() -> Option<String> {
    let status = tailnet_status();
    if !status.running {
        return None;
    }
    status.dns_name
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TailnetStatus {
    pub installed: bool,
    /// BackendState == "Running": SSO complete, tailnet reachable.
    pub running: bool,
    /// SSO login, e.g. `user@company.dev` (Google Workspace login lands here).
    pub login_name: Option<String>,
    pub display_name: Option<String>,
    /// Tailnet name as reported by the control plane.
    pub tailnet_name: Option<String>,
    /// This machine's MagicDNS name without trailing dot.
    pub dns_name: Option<String>,
    pub magic_dns: bool,
}

/// Tailnet login + identity for the Settings pairing screen. Never fails:
/// a logged-out or missing client reports as such.
#[tauri::command]
pub fn remote_tailnet() -> TailnetStatus {
    tailnet_status()
}

fn tailnet_status() -> TailnetStatus {
    let down = TailnetStatus {
        installed: false,
        running: false,
        login_name: None,
        display_name: None,
        tailnet_name: None,
        dns_name: None,
        magic_dns: false,
    };
    let output = match tailscale_cli(&["status", "--json"], STD_CLI_TIMEOUT) {
        Some(output) if output.status.success() => output,
        Some(_) => {
            return TailnetStatus {
                installed: true,
                ..down
            }
        }
        None => return down,
    };
    let json: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(json) => json,
        Err(_) => {
            return TailnetStatus {
                installed: true,
                ..down
            }
        }
    };
    parse_tailnet_status(&json)
}

#[cfg(test)]
pub(crate) fn parse_tailnet_status_for_test(json: &serde_json::Value) -> TailnetStatus {
    parse_tailnet_status(json)
}

fn parse_tailnet_status(json: &serde_json::Value) -> TailnetStatus {
    let running = json
        .get("BackendState")
        .and_then(|v| v.as_str())
        .is_some_and(|state| state == "Running");
    let dns_name = json
        .get("Self")
        .and_then(|s| s.get("DNSName"))
        .and_then(|v| v.as_str())
        .map(|name| name.trim_end_matches('.').to_string())
        .filter(|name| !name.is_empty());
    let user_id = json
        .get("Self")
        .and_then(|s| s.get("UserID"))
        .and_then(|v| v.as_u64())
        .map(|id| id.to_string());
    let user = user_id.as_deref().and_then(|id| json.get("User")?.get(id));
    TailnetStatus {
        installed: true,
        running,
        login_name: user
            .and_then(|u| u.get("LoginName"))
            .and_then(|v| v.as_str())
            .filter(|name| !name.is_empty() && *name != "tagged-devices")
            .map(str::to_string),
        display_name: user
            .and_then(|u| u.get("DisplayName"))
            .and_then(|v| v.as_str())
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        tailnet_name: json
            .get("CurrentTailnet")
            .and_then(|t| t.get("Name"))
            .and_then(|v| v.as_str())
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        dns_name,
        magic_dns: json
            .get("CurrentTailnet")
            .and_then(|t| t.get("MagicDNSEnabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServeStatus {
    pub active: bool,
}

/// Expose the companion listener on the tailnet via
/// `tailscale serve --tcp`. Raw-TCP mode (not `--https`) on purpose: the
/// tailnet is already WireGuard-encrypted end to end, and TCP forwarding
/// needs no provisioned certificates. The pairing token still gates every
/// frame.
#[tauri::command]
pub async fn remote_serve_on(state: State<'_, RemoteState>) -> Result<ServeStatus, String> {
    let port = state.lock().port;
    let tcp = format!("--tcp={port}");
    let target = format!("tcp://localhost:{port}");
    let tcp_ref = tcp.clone();
    let target_ref = target.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        tailscale_cli(&["serve", "--bg", &tcp_ref, &target_ref], STD_CLI_TIMEOUT)
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Tailscale CLI not found. Install it and log in, then retry.".to_string())?;
    if !output.status.success() {
        return Err(serve_error(&output));
    }
    Ok(ServeStatus { active: true })
}

/// Remove the tailnet forwarding again. `--bg` persistence means it would
/// otherwise survive restarts.
#[tauri::command]
pub async fn remote_serve_off(state: State<'_, RemoteState>) -> Result<ServeStatus, String> {
    let port = state.lock().port;
    let tcp = format!("--tcp={port}");
    let output = tauri::async_runtime::spawn_blocking(move || {
        tailscale_cli(&["serve", &tcp, "off"], STD_CLI_TIMEOUT)
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Tailscale CLI not found.".to_string())?;
    if !output.status.success() {
        return Err(serve_error(&output));
    }
    Ok(ServeStatus { active: false })
}

/// Whether our TCP forwarder is currently served. Parsed defensively: serve
/// status JSON shapes drift between client versions, so this looks for our
/// target address anywhere in the payload instead of a fixed schema.
#[tauri::command]
pub async fn remote_serve_status(state: State<'_, RemoteState>) -> Result<ServeStatus, String> {
    let port = state.lock().port;
    let output = tauri::async_runtime::spawn_blocking(move || {
        tailscale_cli(&["serve", "status", "--json"], STD_CLI_TIMEOUT)
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Tailscale CLI not found.".to_string())?;
    if !output.status.success() {
        return Err(serve_error(&output));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    Ok(ServeStatus {
        active: serve_target_present(&json, port),
    })
}

/// Embedded tailnet node status. Mac uses tailscale-rs; iPad uses userspace
/// tsnet. Android has no node yet.
#[tauri::command]
pub async fn remote_embed_status(
    app: AppHandle,
    state: State<'_, RemoteState>,
) -> Result<EmbedSnapshot, String> {
    #[cfg(target_os = "android")]
    {
        let _ = (&app, &state);
        return Ok(EmbedSnapshot::default());
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(state.embed_snapshot())
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(any(target_os = "android", target_os = "ios"), allow(dead_code))]
pub struct EmbedStartInput {
    #[serde(default)]
    pub auth_key: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Start (or restart) the embedded node. Saved key, fresh key, or
/// interactive Google SSO via the login URL the snapshot returns.
#[tauri::command]
pub async fn remote_embed_start(
    app: AppHandle,
    state: State<'_, RemoteState>,
    input: Option<EmbedStartInput>,
) -> Result<EmbedSnapshot, String> {
    let input = input.unwrap_or(EmbedStartInput {
        auth_key: None,
        tags: Vec::new(),
    });
    #[cfg(target_os = "ios")]
    {
        let _ = &state;
        crate::tsnet_mobile::start(app, input.auth_key).await
    }
    #[cfg(target_os = "android")]
    {
        let _ = (&app, &state, &input);
        Err("The embedded tailnet node is not available on Android.".into())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = state;
        crate::tailnet_embed::start(app, input.auth_key, input.tags).await
    }
}

/// Stop the embedded node. Saved keys survive; starting again rejoins.
#[tauri::command]
pub async fn remote_embed_stop(
    app: AppHandle,
    state: State<'_, RemoteState>,
) -> Result<EmbedSnapshot, String> {
    #[cfg(target_os = "ios")]
    {
        let _ = &state;
        crate::tsnet_mobile::stop(app).await
    }
    #[cfg(target_os = "android")]
    {
        let _ = (&app, &state);
        Err("The embedded tailnet node is not available on Android.".into())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = state;
        crate::tailnet_embed::stop(app).await
    }
}

/// Wipe this device's tailnet keys and restart so Google sign-in can pick
/// a different account. Pairing with the Mac is unchanged.
#[tauri::command]
pub async fn remote_embed_logout(
    app: AppHandle,
    state: State<'_, RemoteState>,
) -> Result<EmbedSnapshot, String> {
    #[cfg(target_os = "ios")]
    {
        let _ = &state;
        crate::tsnet_mobile::logout(app).await
    }
    #[cfg(target_os = "android")]
    {
        let _ = (&app, &state);
        Err("The embedded tailnet node is not available on Android.".into())
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = state;
        crate::tailnet_embed::logout(app).await
    }
}

/// DNS-safe Tailscale hostname for this device (Mac computer name / iPad name).
pub fn node_hostname() -> String {
    #[cfg(unix)]
    {
        let mut buf = vec![0u8; 256];
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if rc == 0 {
            if let Some(end) = buf.iter().position(|&b| b == 0) {
                buf.truncate(end);
            }
            if let Ok(raw) = String::from_utf8(buf) {
                let trimmed = raw.trim_end_matches(".local");
                let sanitized = sanitize_ts_hostname(trimmed);
                // iOS gethostname is often "localhost", which Tailscale
                // then shows as the device name. Use the platform default.
                if !sanitized.is_empty() && sanitized != "localhost" {
                    return sanitized;
                }
            }
        }
    }
    default_node_hostname().to_string()
}

pub fn default_node_hostname() -> &'static str {
    #[cfg(target_os = "ios")]
    {
        "monocode-ipad"
    }
    #[cfg(not(target_os = "ios"))]
    {
        "monocode"
    }
}

pub fn sanitize_ts_hostname(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else if ch == '-' || ch == '_' || ch == '.' || ch == ' ' {
            '-'
        } else {
            continue;
        };
        if next == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(next);
        if out.len() >= 63 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Build an embed snapshot from `tailscale status --json` / tsnet status JSON.
#[cfg_attr(not(any(test, target_os = "ios")), allow(dead_code))]
pub(crate) fn embed_from_status_json(json: &serde_json::Value) -> EmbedSnapshot {
    let tailnet = parse_tailnet_status(json);
    let login_url = json
        .get("AuthURL")
        .and_then(|v| v.as_str())
        .filter(|url| !url.is_empty())
        .map(str::to_string);
    let tailnet_ip = json
        .get("Self")
        .and_then(|s| s.get("TailscaleIPs"))
        .and_then(|v| v.as_array())
        .and_then(|ips| {
            ips.iter()
                .filter_map(|v| v.as_str())
                .find(|ip| ip.contains('.'))
                .map(str::to_string)
        });
    let hostname = json
        .get("Self")
        .and_then(|s| s.get("HostName"))
        .and_then(|v| v.as_str())
        .map(sanitize_ts_hostname)
        .filter(|name| !name.is_empty())
        .or_else(|| Some(node_hostname()));
    EmbedSnapshot {
        running: true,
        authorized: tailnet.running,
        tailnet_ip,
        login_url: if tailnet.running { None } else { login_url },
        error: None,
        login_name: tailnet.login_name,
        display_name: tailnet.display_name,
        tailnet_name: tailnet.tailnet_name,
        hostname,
    }
}

fn serve_target_present(json: &serde_json::Value, port: u16) -> bool {
    let needle_local = format!("localhost:{port}");
    let needle_loop = format!("127.0.0.1:{port}");
    let mut stack = vec![json];
    while let Some(value) = stack.pop() {
        match value {
            serde_json::Value::String(text) => {
                if text.contains(&needle_local) || text.contains(&needle_loop) {
                    return true;
                }
            }
            serde_json::Value::Array(items) => stack.extend(items),
            serde_json::Value::Object(map) => stack.extend(map.values()),
            _ => {}
        }
    }
    false
}

fn serve_error(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        return "tailscale serve failed".into();
    }
    // CLI errors can be multi-line; keep the last line (the actionable one).
    stderr
        .lines()
        .last()
        .unwrap_or(&stderr)
        .chars()
        .take(300)
        .collect()
}

const STD_CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Run the Tailscale CLI off-thread with a timeout. None = not installed or
/// hung; Some(output) may still be a non-zero exit (logged out, etc.).
fn tailscale_cli(args: &[&str], timeout: std::time::Duration) -> Option<std::process::Output> {
    let owned: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let output = std::process::Command::new("tailscale")
            .args(&owned)
            .output();
        let _ = tx.send(output);
    });
    rx.recv_timeout(timeout).ok()?.ok()
}

/// Best-effort LAN IP without new dependencies: "connecting" a UDP socket
/// sends nothing but reveals the interface route to the LAN.
fn lan_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // TEST-NET-1; no packet ever leaves — connect() only picks a route.
    socket.connect("192.0.2.1:80").ok()?;
    let addr = socket.local_addr().ok()?;
    let ip = addr.ip().to_string();
    if ip == "0.0.0.0" {
        return None;
    }
    Some(ip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn google_workspace_status() -> serde_json::Value {
        // Shape of `tailscale status --json` on a Google-SSO tailnet.
        json!({
            "BackendState": "Running",
            "Self": {
                "DNSName": "macbook.tail9a5.ts.net.",
                "UserID": 5,
            },
            "User": {
                "5": { "ID": 5, "LoginName": "user@company.dev", "DisplayName": "User" },
                "999": { "ID": 999, "LoginName": "tagged-devices", "DisplayName": "Tagged Devices" },
            },
            "CurrentTailnet": {
                "Name": "company",
                "MagicDNSSuffix": "tail9a5.ts.net",
                "MagicDNSEnabled": true,
            },
        })
    }

    #[test]
    fn tailnet_reports_google_login_and_dns() {
        let status = parse_tailnet_status_for_test(&google_workspace_status());
        assert!(status.running);
        assert_eq!(status.login_name.as_deref(), Some("user@company.dev"));
        assert_eq!(status.display_name.as_deref(), Some("User"));
        assert_eq!(status.tailnet_name.as_deref(), Some("company"));
        assert_eq!(status.dns_name.as_deref(), Some("macbook.tail9a5.ts.net"));
        assert!(status.magic_dns);
    }

    #[test]
    fn hostname_sanitizes_machine_names() {
        assert_eq!(sanitize_ts_hostname("MJ iPad"), "mj-ipad");
        assert_eq!(sanitize_ts_hostname("Aleksandrs-Mac-Studio.local"), "aleksandrs-mac-studio-local");
        assert_eq!(sanitize_ts_hostname("---Wow---"), "wow");
        assert!(sanitize_ts_hostname("***").is_empty());
    }

    #[test]
    fn embed_snapshot_reads_auth_url_and_google_login() {
        let json = json!({
            "AuthURL": "https://login.tailscale.com/a/example",
            "BackendState": "NeedsLogin",
            "Self": { "DNSName": "", "UserID": 0, "TailscaleIPs": [] },
        });
        let snap = embed_from_status_json(&json);
        assert!(snap.running);
        assert!(!snap.authorized);
        assert_eq!(
            snap.login_url.as_deref(),
            Some("https://login.tailscale.com/a/example")
        );
    }

    #[test]
    fn embed_snapshot_reads_identity_when_running() {
        let json = google_workspace_status();
        let snap = embed_from_status_json(&json);
        assert!(snap.authorized);
        assert_eq!(snap.login_name.as_deref(), Some("user@company.dev"));
        assert_eq!(snap.login_url, None);
    }

    #[test]
    fn tailnet_logged_out_is_not_running() {
        let status = parse_tailnet_status_for_test(&json!({
            "BackendState": "NoState",
            "Self": { "DNSName": "", "UserID": 0 },
        }));
        assert!(!status.running);
        assert_eq!(status.login_name, None);
        assert_eq!(status.dns_name, None);
    }

    #[test]
    fn serve_target_found_anywhere_in_status() {
        assert!(serve_target_present(
            &json!({ "TCP": { "17233": { "TCPForward": "tcp://localhost:17233" } } }),
            17233
        ));
        assert!(serve_target_present(
            &json!({ "TCP": { "17233": { "TCPForward": "tcp://127.0.0.1:17233" } } }),
            17233
        ));
        assert!(!serve_target_present(
            &json!({ "TCP": { "443": { "TCPForward": "tcp://localhost:443" } } }),
            17233
        ));
        assert!(!serve_target_present(&json!({}), 17233));
    }

    #[test]
    fn route_flags_read_legacy_and_combined() {
        assert_eq!(parse_route_flags(None), (false, true));
        assert_eq!(parse_route_flags(Some("")), (false, true));
        assert_eq!(parse_route_flags(Some("lan")), (true, false));
        assert_eq!(parse_route_flags(Some("tailscale")), (false, true));
        assert_eq!(parse_route_flags(Some("lan,tailscale")), (true, true));
        assert_eq!(parse_route_flags(Some("both")), (true, true));
        assert_eq!(format_route_flags(true, true), "lan,tailscale");
        assert_eq!(format_route_flags(true, false), "lan");
        assert_eq!(format_route_flags(false, true), "tailscale");
        assert_eq!(format_route_flags(false, false), "");
    }

    fn live_pair_state(code: &str) -> RemoteState {
        let state = RemoteState::new();
        state.lock().pair_code = Some(PairingCode {
            code: code.to_string(),
            expires_at: std::time::SystemTime::now() + std::time::Duration::from_secs(600),
            attempts: 0,
        });
        state
    }

    #[test]
    fn pair_code_accepts_with_spacing_and_burns_on_use() {
        let state = live_pair_state("123456");
        assert!(verify_pair_code(&state, "123 456").is_ok());
        // Single-use: the very next claim finds nothing.
        assert!(verify_pair_code(&state, "123456").is_err());
    }

    #[test]
    fn pair_code_burns_after_too_many_guesses() {
        let state = live_pair_state("123456");
        for _ in 0..PAIR_CODE_MAX_ATTEMPTS - 1 {
            assert!(verify_pair_code(&state, "000000").is_err());
        }
        // Last allowed guess burns it; even the right code fails after.
        assert!(verify_pair_code(&state, "000000").is_err());
        assert!(verify_pair_code(&state, "123456").is_err());
    }

    #[test]
    fn pair_code_rejects_expired() {
        let state = RemoteState::new();
        state.lock().pair_code = Some(PairingCode {
            code: "123456".to_string(),
            expires_at: std::time::SystemTime::now() - std::time::Duration::from_secs(1),
            attempts: 0,
        });
        assert!(verify_pair_code(&state, "123456").is_err());
    }
}
