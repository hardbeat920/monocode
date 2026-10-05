//! Tauri commands over `monocode_process::harness`.
use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use monocode_process::harness::{
    self as host, AntigravityBinary, ConfiguredBinary, CursorBinary, HarnessAccount, HarnessEvents,
    HarnessHttpResponse,
};
pub(crate) use monocode_process::harness::{reap_orphaned_harness_processes, HarnessHost};

const STDOUT_EVENT: &str = "harness-stdout";
const STDERR_EVENT: &str = "harness-stderr";
const EXIT_EVENT: &str = "harness-exit";
const SSE_EVENT: &str = "harness-sse";
const SSE_END_EVENT: &str = "harness-sse-end";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HarnessLine {
    session_id: String,
    line: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HarnessExit {
    session_id: String,
    code: Option<i32>,
    pid: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HarnessSse {
    session_id: String,
    data: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HarnessSseEnd {
    session_id: String,
    error: Option<String>,
}

/// Emits harness output to every webview, as the commands did before the move.
pub(crate) struct TauriHarnessEvents(pub AppHandle);

impl HarnessEvents for TauriHarnessEvents {
    fn stdout(&self, session_id: &str, line: String) {
        let _ = self.0.emit(
            STDOUT_EVENT,
            HarnessLine {
                session_id: session_id.to_string(),
                line,
            },
        );
    }

    fn stderr(&self, session_id: &str, line: String) {
        let _ = self.0.emit(
            STDERR_EVENT,
            HarnessLine {
                session_id: session_id.to_string(),
                line,
            },
        );
    }

    fn exit(&self, session_id: &str, code: Option<i32>, pid: u32) {
        let _ = self.0.emit(
            EXIT_EVENT,
            HarnessExit {
                session_id: session_id.to_string(),
                code,
                pid,
            },
        );
    }

    fn sse(&self, session_id: &str, data: String) {
        let _ = self.0.emit(
            SSE_EVENT,
            HarnessSse {
                session_id: session_id.to_string(),
                data,
            },
        );
    }

    fn sse_end(&self, session_id: &str, error: Option<String>) {
        let _ = self.0.emit(
            SSE_END_EVENT,
            HarnessSseEnd {
                session_id: session_id.to_string(),
                error,
            },
        );
    }
}

/// Resolve the Cursor CLI (`cursor-agent`), never Grok's `agent` shim.
#[tauri::command(async)]
pub fn harness_resolve_cursor() -> Result<CursorBinary, String> {
    host::harness_resolve_cursor()
}

/// Resolve the Codex CLI (`codex`).
#[tauri::command(async)]
pub fn harness_resolve_codex() -> Result<CursorBinary, String> {
    host::harness_resolve_codex()
}

/// Resolve the OpenCode CLI (`opencode`).
#[tauri::command(async)]
pub fn harness_resolve_opencode() -> Result<CursorBinary, String> {
    host::harness_resolve_opencode()
}

#[tauri::command(async)]
pub fn harness_resolve_configured(
    provider: String,
    binary_path: String,
) -> Result<ConfiguredBinary, String> {
    host::harness_resolve_configured(provider, binary_path)
}

#[tauri::command]
pub fn harness_runtime_binary_paths(
    host: State<'_, HarnessHost>,
    paths: HashMap<String, String>,
) -> HashMap<String, String> {
    host::harness_runtime_binary_paths(&host, paths)
}

/// Resolve the Claude Code CLI (`claude`).
#[tauri::command(async)]
pub fn harness_resolve_claude() -> Result<CursorBinary, String> {
    host::harness_resolve_claude()
}

#[tauri::command]
pub async fn claude_mcp_list(host: State<'_, HarnessHost>, cwd: String) -> Result<String, String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host::claude_mcp_list(&host, cwd))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn claude_mcp_add(
    host: State<'_, HarnessHost>,
    cwd: String,
    name: String,
    config: String,
    scope: String,
) -> Result<(), String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        host::claude_mcp_add(&host, cwd, name, config, scope)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn claude_mcp_remove(
    host: State<'_, HarnessHost>,
    cwd: String,
    name: String,
    scope: String,
) -> Result<(), String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host::claude_mcp_remove(&host, cwd, name, scope))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn mcp_provider_login(
    host: State<'_, HarnessHost>,
    cwd: String,
    provider: String,
    name: String,
) -> Result<(), String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        host::mcp_provider_login(&host, cwd, provider, name)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Resolve the Pi coding agent CLI (`pi`).
#[tauri::command(async)]
pub fn harness_resolve_pi() -> Result<CursorBinary, String> {
    host::harness_resolve_pi()
}

/// Resolve the omp (oh-my-pi) coding agent CLI.
#[tauri::command(async)]
pub fn harness_resolve_omp() -> Result<CursorBinary, String> {
    host::harness_resolve_omp()
}

/// Resolve the Vercel fx coding agent CLI (`fx`), never the JSON viewer of the same name.
#[tauri::command(async)]
pub fn harness_resolve_fx() -> Result<CursorBinary, String> {
    host::harness_resolve_fx()
}

/// Resolve xAI Grok Build (`grok`).
#[tauri::command(async)]
pub fn harness_resolve_grok() -> Result<CursorBinary, String> {
    host::harness_resolve_grok()
}

/// Resolve Nous Research Hermes Agent (`hermes`).
#[tauri::command(async)]
pub fn harness_resolve_hermes() -> Result<CursorBinary, String> {
    host::harness_resolve_hermes()
}

/// Resolve Factory Droid (`droid`), driven over ACP via `droid exec --output-format acp`.
#[tauri::command(async)]
pub fn harness_resolve_droid() -> Result<CursorBinary, String> {
    host::harness_resolve_droid()
}

/// Antigravity's ACP server is separate from the interactive agy CLI.
#[tauri::command(async)]
pub fn harness_resolve_antigravity() -> Result<AntigravityBinary, String> {
    host::harness_resolve_antigravity()
}

/// Bind an ephemeral loopback port for `opencode serve`.
#[tauri::command]
pub fn harness_free_port() -> Result<u16, String> {
    host::harness_free_port()
}

/// Off the main thread: fork/exec, and `apply_gui_env` can wait on the first
/// login-shell read. Callers await this before writing to the child. Kill can
/// still race the fork, so a cancelled spawn must not reinsert the child.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn harness_spawn(
    app: AppHandle,
    host: State<'_, HarnessHost>,
    session_id: String,
    command: String,
    args: Vec<String>,
    cwd: String,
    account: Option<HarnessAccount>,
    binary_provider: Option<String>,
    binary_path: Option<String>,
) -> Result<u32, String> {
    let data_dir = crate::app_data_dir(&app)?;
    let control = app.try_state::<monocode_process::control::ControlHost>();
    host::harness_spawn(
        &host,
        &data_dir,
        control.as_deref(),
        session_id,
        command,
        args,
        cwd,
        account,
        binary_provider,
        binary_path,
    )
}

#[tauri::command(async)]
pub fn provider_account_remove(
    app: AppHandle,
    host: State<'_, HarnessHost>,
    provider: String,
    account_id: String,
) -> Result<(), String> {
    host::provider_account_remove(&host, &crate::app_data_dir(&app)?, provider, account_id)
}

/// A child that stops draining stdin can block `write_all` for minutes, so the
/// write runs on the blocking pool — never on an async worker or the IPC path,
/// where it would starve `harness_kill` and make the wedged child unrecoverable.
#[tauri::command]
pub async fn harness_write(
    host: State<'_, HarnessHost>,
    session_id: String,
    line: String,
) -> Result<(), String> {
    let host = host.inner().clone();
    tauri::async_runtime::spawn_blocking(move || host::harness_write(&host, session_id, line))
        .await
        .map_err(|e| format!("Harness write task failed: {e}"))?
}

/// `async` dispatch keeps kill executable while a sibling `harness_write` is
/// blocked on a wedged child's stdin.
#[tauri::command(async)]
pub fn harness_kill(host: State<'_, HarnessHost>, session_id: String) -> Result<(), String> {
    host::harness_kill(&host, session_id)
}

/// Off the main thread: `kill_all` waits for the children to die before it
/// returns, and a window close calls this while the app keeps running.
#[tauri::command(async)]
pub fn harness_kill_all(host: State<'_, HarnessHost>) -> Result<(), String> {
    host::harness_kill_all(&host)
}

#[tauri::command]
pub async fn harness_http(
    url: String,
    method: String,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<HarnessHttpResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        host::harness_http(url, method, headers, body, timeout_ms)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn harness_sse_open(
    host: State<HarnessHost>,
    session_id: String,
    url: String,
    headers: Option<HashMap<String, String>>,
) -> Result<(), String> {
    host::harness_sse_open(&host, session_id, url, headers)
}

#[tauri::command]
pub fn harness_sse_close(host: State<HarnessHost>, session_id: String) -> Result<(), String> {
    host::harness_sse_close(&host, session_id)
}

/// One-shot capture of stdout (used for `cursor-agent --list-models`).
#[tauri::command]
pub async fn harness_exec(
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
    binary_provider: Option<String>,
    binary_path: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        host::harness_exec(command, args, cwd, binary_provider, binary_path)
    })
    .await
    .map_err(|e| e.to_string())?
}
