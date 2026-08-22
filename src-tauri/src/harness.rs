use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::dirs_home;
use crate::fs::expand_home;

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessHttpResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorBinary {
    pub path: String,
}

struct LiveChild {
    stdin: Mutex<ChildStdin>,
    pid: u32,
}

struct LiveSse {
    stop: Arc<AtomicBool>,
}

pub struct HarnessHost {
    children: Mutex<HashMap<String, Arc<LiveChild>>>,
    sse: Mutex<HashMap<String, Arc<LiveSse>>>,
}

impl HarnessHost {
    pub fn new() -> Self {
        Self {
            children: Mutex::new(HashMap::new()),
            sse: Mutex::new(HashMap::new()),
        }
    }

    fn insert(&self, session_id: String, live: Arc<LiveChild>) -> Option<Arc<LiveChild>> {
        self.children
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id, live)
    }

    fn get(&self, session_id: &str) -> Option<Arc<LiveChild>> {
        self.children
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .cloned()
    }

    fn remove(&self, session_id: &str) -> Option<Arc<LiveChild>> {
        self.children
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id)
    }

    pub(crate) fn kill_all(&self) {
        let kids: Vec<Arc<LiveChild>> = {
            let mut map = self.children.lock().unwrap_or_else(|e| e.into_inner());
            map.drain().map(|(_, child)| child).collect()
        };
        for live in kids {
            terminate(live.pid);
        }
        self.stop_all_sse();
    }

    fn insert_sse(&self, session_id: String, live: Arc<LiveSse>) -> Option<Arc<LiveSse>> {
        self.sse
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id, live)
    }

    fn stop_sse(&self, session_id: &str) {
        if let Some(live) = self
            .sse
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id)
        {
            live.stop.store(true, Ordering::SeqCst);
        }
    }

    fn stop_all_sse(&self) {
        let streams: Vec<Arc<LiveSse>> = {
            let mut map = self.sse.lock().unwrap_or_else(|e| e.into_inner());
            map.drain().map(|(_, live)| live).collect()
        };
        for live in streams {
            live.stop.store(true, Ordering::SeqCst);
        }
    }
}

impl Drop for HarnessHost {
    fn drop(&mut self) {
        self.kill_all();
    }
}

/// Resolve the Cursor CLI (`cursor-agent`), never Grok's `agent` shim.
#[tauri::command]
pub fn harness_resolve_cursor() -> Result<CursorBinary, String> {
    resolve_cursor_agent()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| "Cursor CLI not found. Install it and run `agent login`, then retry.".into())
}

/// Resolve the Codex CLI (`codex`).
#[tauri::command]
pub fn harness_resolve_codex() -> Result<CursorBinary, String> {
    resolve_codex()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| {
            "Codex CLI not found. Install it from https://developers.openai.com/codex/cli and run `codex login`, then retry."
                .into()
        })
}

/// Resolve the OpenCode CLI (`opencode`).
#[tauri::command]
pub fn harness_resolve_opencode() -> Result<CursorBinary, String> {
    resolve_opencode()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| {
            "OpenCode CLI not found. Install it from https://opencode.ai and run `opencode auth login`, then retry."
                .into()
        })
}

/// Resolve the Claude Code CLI (`claude`).
#[tauri::command]
pub fn harness_resolve_claude() -> Result<CursorBinary, String> {
    resolve_claude()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| {
            "Claude Code CLI not found. Install it from https://claude.com/product/claude-code and run `claude auth login`, then retry."
                .into()
        })
}

/// Resolve the Pi coding agent CLI (`pi`).
#[tauri::command]
pub fn harness_resolve_pi() -> Result<CursorBinary, String> {
    resolve_pi()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| {
            "Pi CLI not found. Install it with `npm install -g @earendil-works/pi-coding-agent` and authenticate, then retry."
                .into()
        })
}

/// Bind an ephemeral loopback port for `opencode serve`.
#[tauri::command]
pub fn harness_free_port() -> Result<u16, String> {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .map_err(|e| format!("Failed to reserve a local port: {e}"))
}

#[tauri::command]
pub fn harness_spawn(
    app: AppHandle,
    host: State<HarnessHost>,
    session_id: String,
    command: String,
    args: Vec<String>,
    cwd: String,
) -> Result<(), String> {
    if let Some(prev) = host.remove(&session_id) {
        terminate(prev.pid);
    }

    let workdir = expand_home(&cwd);
    if !workdir.is_dir() {
        return Err(format!(
            "Working directory does not exist: {}",
            workdir.display()
        ));
    }

    let mut cmd = Command::new(&command);
    cmd.args(&args)
        .current_dir(&workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_gui_path(&mut cmd);
    isolate_child(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start {command}: {e}"))?;
    let pid = child.id();

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed to open harness stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to open harness stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to open harness stderr".to_string())?;

    let live = Arc::new(LiveChild {
        stdin: Mutex::new(stdin),
        pid,
    });
    host.insert(session_id.clone(), live);

    let stdout_app = app.clone();
    let stdout_id = session_id.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let _ = stdout_app.emit(
                STDOUT_EVENT,
                HarnessLine {
                    session_id: stdout_id.clone(),
                    line,
                },
            );
        }
    });

    let stderr_app = app.clone();
    let stderr_id = session_id.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let _ = stderr_app.emit(
                STDERR_EVENT,
                HarnessLine {
                    session_id: stderr_id.clone(),
                    line,
                },
            );
        }
    });

    let wait_app = app.clone();
    let wait_id = session_id;
    thread::spawn(move || {
        let code = child.wait().ok().and_then(|status| status.code());
        if let Some(host) = wait_app.try_state::<HarnessHost>() {
            host.stop_sse(&wait_id);
            host.remove(&wait_id);
        }
        let _ = wait_app.emit(
            EXIT_EVENT,
            HarnessExit {
                session_id: wait_id,
                code,
            },
        );
    });

    Ok(())
}

#[tauri::command]
pub fn harness_write(
    host: State<HarnessHost>,
    session_id: String,
    line: String,
) -> Result<(), String> {
    let live = host
        .get(&session_id)
        .ok_or_else(|| "Harness process is not running".to_string())?;
    let mut stdin = live.stdin.lock().unwrap_or_else(|e| e.into_inner());
    stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.write_all(b"\n"))
        .and_then(|_| stdin.flush())
        .map_err(|e| format!("Failed to write to harness: {e}"))
}

#[tauri::command]
pub fn harness_kill(host: State<HarnessHost>, session_id: String) -> Result<(), String> {
    host.stop_sse(&session_id);
    if let Some(live) = host.remove(&session_id) {
        terminate(live.pid);
    }
    Ok(())
}

#[tauri::command]
pub fn harness_kill_all(host: State<HarnessHost>) -> Result<(), String> {
    host.kill_all();
    Ok(())
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
        assert_loopback(&url)?;
        let timeout = Duration::from_millis(timeout_ms.unwrap_or(30_000).max(1));
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        let mut request = agent.request(&method, &url);
        if let Some(headers) = &headers {
            for (key, value) in headers {
                request = request.set(key, value);
            }
        }
        let result = match body {
            Some(payload) => request.send_string(&payload),
            None => request.call(),
        };
        match result {
            Ok(response) => read_http_response(response),
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_default();
                Ok(HarnessHttpResponse { status, body })
            }
            Err(error) => Err(format!("OpenCode HTTP failed: {error}")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn harness_sse_open(
    app: AppHandle,
    host: State<HarnessHost>,
    session_id: String,
    url: String,
    headers: Option<HashMap<String, String>>,
) -> Result<(), String> {
    assert_loopback(&url)?;
    host.stop_sse(&session_id);
    let stop = Arc::new(AtomicBool::new(false));
    host.insert_sse(
        session_id.clone(),
        Arc::new(LiveSse {
            stop: Arc::clone(&stop),
        }),
    );

    thread::spawn(move || {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(60 * 60 * 6))
            .timeout_write(Duration::from_secs(30))
            .build();
        let mut request = agent.get(&url).set("Accept", "text/event-stream");
        if let Some(headers) = &headers {
            for (key, value) in headers {
                request = request.set(key, value);
            }
        }
        let result = request.call();
        if stop.load(Ordering::SeqCst) {
            emit_sse_end(&app, &session_id, None);
            return;
        }
        match result {
            Ok(response) => {
                let reader = BufReader::new(response.into_reader());
                read_sse(reader, &app, &session_id, &stop);
                emit_sse_end(&app, &session_id, None);
            }
            Err(error) => {
                emit_sse_end(
                    &app,
                    &session_id,
                    Some(format!("OpenCode event stream failed: {error}")),
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn harness_sse_close(host: State<HarnessHost>, session_id: String) -> Result<(), String> {
    host.stop_sse(&session_id);
    Ok(())
}

fn read_http_response(response: ureq::Response) -> Result<HarnessHttpResponse, String> {
    let status = response.status();
    let body = response
        .into_string()
        .map_err(|e| format!("Failed to read OpenCode response: {e}"))?;
    Ok(HarnessHttpResponse { status, body })
}

fn read_sse<R: BufRead>(reader: R, app: &AppHandle, session_id: &str, stop: &AtomicBool) {
    let mut data = String::new();
    for line in reader.lines() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(line) = line else { break };
        if line.starts_with(':') {
            continue;
        }
        if line.is_empty() {
            if data.is_empty() {
                continue;
            }
            let payload = std::mem::take(&mut data);
            let _ = app.emit(
                SSE_EVENT,
                HarnessSse {
                    session_id: session_id.to_string(),
                    data: payload,
                },
            );
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            let piece = rest.strip_prefix(' ').unwrap_or(rest);
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(piece);
        }
    }
}

fn emit_sse_end(app: &AppHandle, session_id: &str, error: Option<String>) {
    let _ = app.emit(
        SSE_END_EVENT,
        HarnessSseEnd {
            session_id: session_id.to_string(),
            error,
        },
    );
}

fn assert_loopback(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://127.0.0.1:")
        || lower.starts_with("http://127.0.0.1/")
        || lower.starts_with("http://localhost:")
        || lower.starts_with("http://localhost/")
    {
        return Ok(());
    }
    Err("OpenCode HTTP is limited to localhost".into())
}

const EXEC_ALLOWED_ARGS: &[&[&str]] = &[
    &["--version"],
    &["--list-models"],
    &["models", "--verbose"],
    &["agent", "list"],
];

fn exec_args_allowed(args: &[String]) -> bool {
    EXEC_ALLOWED_ARGS
        .iter()
        .any(|a| a.len() == args.len() && a.iter().zip(args).all(|(x, y)| x == y))
}

/// Must be a path a resolver would hand back, not an arbitrary binary
/// that merely shares a file name.
fn is_resolved_harness_binary(command: &str) -> bool {
    let path = PathBuf::from(command);
    [
        resolve_cursor_agent(),
        resolve_codex(),
        resolve_opencode(),
        resolve_claude(),
        resolve_pi(),
    ]
    .into_iter()
    .flatten()
    .any(|resolved| resolved == path)
}

/// One-shot capture of stdout (used for `cursor-agent --list-models`).
#[tauri::command]
pub async fn harness_exec(
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
) -> Result<String, String> {
    if !exec_args_allowed(&args) {
        return Err("harness_exec: unsupported arguments".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        if !is_resolved_harness_binary(&command) {
            return Err("harness_exec: not a resolved harness CLI".to_string());
        }
        exec_capture(&command, &args, cwd.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn exec_capture(command: &str, args: &[String], cwd: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new(command);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_gui_path(&mut cmd);
    isolate_child(&mut cmd);
    if let Some(dir) = cwd {
        let workdir = expand_home(dir);
        if workdir.is_dir() {
            cmd.current_dir(workdir);
        }
    }

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to run {command}: {e}"))?;
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            if output.status.success() || !stdout.trim().is_empty() {
                return Ok(stdout);
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(stderr.trim().to_string())
        }
        Ok(Err(e)) => Err(format!("Failed to run {command}: {e}")),
        Err(_) => {
            terminate(pid);
            Err(format!("{command} timed out"))
        }
    }
}

const KILL_ESCALATE: Duration = Duration::from_secs(2);

fn isolate_child(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = cmd;
    }
}

fn terminate(pid: u32) {
    terminate_after(pid, KILL_ESCALATE);
}

fn terminate_after(pid: u32, escalate: Duration) {
    if pid == 0 || pid == 1 {
        return;
    }
    signal_tree(pid, TreeSignal::Term);
    thread::spawn(move || {
        thread::sleep(escalate);
        if tree_alive(pid) {
            signal_tree(pid, TreeSignal::Kill);
        }
    });
}

enum TreeSignal {
    Term,
    Kill,
}

fn signal_tree(pid: u32, signal: TreeSignal) {
    #[cfg(unix)]
    {
        let sig = match signal {
            TreeSignal::Term => libc::SIGTERM,
            TreeSignal::Kill => libc::SIGKILL,
        };
        let ipid = pid as i32;
        unsafe {
            libc::kill(ipid, sig);
            libc::kill(-ipid, sig);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = signal;
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

fn tree_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let ipid = pid as i32;
        unsafe { libc::kill(ipid, 0) == 0 || libc::kill(-ipid, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn resolve_cursor_agent() -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Stable shims first. `command -v` often returns a versioned path
    // (`…/versions/<build>/cursor-agent`); macOS TCC then treats each
    // upgrade as a new binary.
    if let Some(home) = &home {
        candidates.push(home.join(".local/bin/cursor-agent"));
        candidates.push(home.join(".local/bin/agent"));
        candidates.push(home.join(".cargo/bin/cursor-agent"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/cursor-agent"));
    candidates.push(PathBuf::from("/usr/local/bin/cursor-agent"));
    candidates.push(PathBuf::from("/usr/bin/cursor-agent"));
    candidates.push(PathBuf::from("/snap/bin/cursor-agent"));
    if let Some(from_shell) = which_via_login_shell("cursor-agent") {
        candidates.push(from_shell);
    }

    candidates.into_iter().find(|path| is_cursor_agent(path))
}

fn resolve_codex() -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(home) = &home {
        candidates.push(home.join(".local/bin/codex"));
        candidates.push(home.join(".npm-global/bin/codex"));
        candidates.push(home.join(".cargo/bin/codex"));
        candidates.push(home.join("n/bin/codex"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/codex"));
    candidates.push(PathBuf::from("/usr/local/bin/codex"));
    candidates.push(PathBuf::from("/usr/bin/codex"));
    candidates.push(PathBuf::from("/snap/bin/codex"));
    if let Some(from_shell) = which_via_login_shell("codex") {
        candidates.push(from_shell);
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn resolve_opencode() -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(home) = &home {
        candidates.push(home.join(".opencode/bin/opencode"));
        candidates.push(home.join(".local/bin/opencode"));
        candidates.push(home.join(".npm-global/bin/opencode"));
        candidates.push(home.join(".cargo/bin/opencode"));
        candidates.push(home.join("n/bin/opencode"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/opencode"));
    candidates.push(PathBuf::from("/usr/local/bin/opencode"));
    candidates.push(PathBuf::from("/usr/bin/opencode"));
    candidates.push(PathBuf::from("/snap/bin/opencode"));
    if let Some(from_shell) = which_via_login_shell("opencode") {
        candidates.push(from_shell);
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn resolve_claude() -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(home) = &home {
        candidates.push(home.join(".local/bin/claude"));
        candidates.push(home.join(".claude/local/claude"));
        candidates.push(home.join(".local/share/claude/claude"));
        candidates.push(home.join(".npm-global/bin/claude"));
        candidates.push(home.join(".cargo/bin/claude"));
        candidates.push(home.join("n/bin/claude"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/claude"));
    candidates.push(PathBuf::from("/usr/local/bin/claude"));
    candidates.push(PathBuf::from("/usr/bin/claude"));
    candidates.push(PathBuf::from("/snap/bin/claude"));
    if let Some(from_shell) = which_via_login_shell("claude") {
        candidates.push(from_shell);
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn resolve_pi() -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(home) = &home {
        for name in ["pi-coding-agent", "pi"] {
            candidates.push(home.join(".local/bin").join(name));
            candidates.push(home.join(".npm-global/bin").join(name));
            candidates.push(home.join(".cargo/bin").join(name));
            candidates.push(home.join("n/bin").join(name));
        }
    }
    for name in ["pi-coding-agent", "pi"] {
        #[cfg(target_os = "macos")]
        candidates.push(PathBuf::from("/opt/homebrew/bin").join(name));
        candidates.push(PathBuf::from("/usr/local/bin").join(name));
        candidates.push(PathBuf::from("/usr/bin").join(name));
        candidates.push(PathBuf::from("/snap/bin").join(name));
    }
    if let Some(from_shell) = which_via_login_shell("pi-coding-agent") {
        candidates.push(from_shell);
    }
    if let Some(from_shell) = which_via_login_shell("pi") {
        candidates.push(from_shell);
    }

    candidates.into_iter().find(|path| is_pi_coding_agent(path))
}

fn is_pi_coding_agent(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name != "pi" && name != "pi-coding-agent" {
        return false;
    }
    if name == "pi-coding-agent" {
        return true;
    }
    file_mentions_pi_coding_agent(path) || pi_help_mentions_rpc(path)
}

fn file_mentions_pi_coding_agent(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 64 * 1024];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    let text = String::from_utf8_lossy(&buf[..n]);
    text.contains("pi-coding-agent")
        || text.contains("@earendil-works/pi")
        || text.contains("@mariozechner/pi-coding-agent")
        || text.contains("PI_CODING_AGENT")
}

fn pi_help_mentions_rpc(path: &Path) -> bool {
    let mut cmd = Command::new(path);
    cmd.arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_child(&mut cmd);
    let Ok(child) = cmd.spawn() else {
        return false;
    };
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(output)) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .to_ascii_lowercase();
            text.contains("--mode") && text.contains("rpc")
        }
        _ => {
            terminate(pid);
            false
        }
    }
}

fn is_cursor_agent(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let text = path.to_string_lossy();
    if text.contains("/.grok/") {
        return false;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "cursor-agent" {
        return true;
    }
    if name == "agent" {
        // One symlink hop. canonicalize() can walk into another .app
        // and trip macOS "data from other apps" TCC.
        if let Ok(target) = std::fs::read_link(path) {
            let resolved = if target.is_absolute() {
                target
            } else {
                path.parent().unwrap_or(path).join(target)
            };
            return resolved.to_string_lossy().contains("cursor-agent");
        }
    }
    false
}

fn which_via_login_shell(name: &str) -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/bash".into()
        }
    });
    let output = Command::new(&shell)
        .args(["-lc", &format!("command -v {name}")])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    let p = PathBuf::from(path);
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

fn apply_gui_path(cmd: &mut Command) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(home) = dirs_home() {
        parts.push(format!("{home}/.local/bin"));
        parts.push(format!("{home}/.cargo/bin"));
        parts.push(format!("{home}/.claude/local"));
        parts.push(format!("{home}/.local/share/claude"));
        parts.push(format!("{home}/.opencode/bin"));
        parts.push(format!("{home}/.npm-global/bin"));
    }
    parts.push("/opt/homebrew/bin".into());
    parts.push("/usr/local/bin".into());
    parts.push("/usr/bin".into());
    parts.push("/bin".into());
    parts.push("/snap/bin".into());
    if let Ok(existing) = std::env::var("PATH") {
        parts.push(existing);
    }
    cmd.env("PATH", parts.join(":"));
    if let Some(home) = dirs_home() {
        cmd.env("HOME", home);
    }
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    fn spawn_group(script: &str) -> std::process::Child {
        Command::new("sh")
            .args(["-c", script])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn test process")
    }

    fn wait_dead(pid: u32, child: &mut std::process::Child) -> bool {
        for _ in 0..40 {
            let _ = child.try_wait();
            let leader_gone = unsafe { libc::kill(pid as i32, 0) != 0 };
            let group_gone = unsafe { libc::kill(-(pid as i32), 0) != 0 };
            if leader_gone && group_gone {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn terminate_reaps_the_process_group() {
        let mut child = spawn_group("sleep 30 & sleep 30");
        let pid = child.id();
        assert!(tree_alive(pid));
        terminate_after(pid, Duration::from_millis(100));
        if !wait_dead(pid, &mut child) {
            let _ = child.kill();
            panic!("process group survived terminate");
        }
    }

    #[test]
    fn terminate_escalates_to_sigkill() {
        let mut child = spawn_group("trap '' TERM; while true; do sleep 1; done");
        let pid = child.id();
        assert!(tree_alive(pid));
        terminate_after(pid, Duration::from_millis(150));
        if !wait_dead(pid, &mut child) {
            let _ = child.kill();
            panic!("SIGTERM-ignoring process survived SIGKILL escalate");
        }
    }

    #[test]
    fn cursor_agent_accepts_symlink_named_agent() {
        let dir = std::env::temp_dir().join(format!("monocode-agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cursor-agent-pack")).unwrap();
        let target = dir.join("cursor-agent-pack/cursor-agent");
        std::fs::write(&target, b"#!/bin/sh\n").unwrap();
        let agent = dir.join("agent");
        std::os::unix::fs::symlink(&target, &agent).unwrap();
        assert!(is_cursor_agent(&agent));
        assert!(!is_cursor_agent(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pi_accepts_coding_agent_and_rejects_other_pi() {
        let dir = std::env::temp_dir().join(format!("monocode-pi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let named = dir.join("pi-coding-agent");
        std::fs::write(&named, b"#!/bin/sh\n").unwrap();
        assert!(is_pi_coding_agent(&named));

        let shim = dir.join("pi");
        std::fs::write(
            &shim,
            b"#!/usr/bin/env node\nrequire('@earendil-works/pi-coding-agent/cli.js');\n",
        )
        .unwrap();
        assert!(is_pi_coding_agent(&shim));

        let other = dir.join("pi");
        std::fs::write(&other, b"#!/bin/sh\necho 3.14159\n").unwrap();
        // Overwrite the shim: a calculator named `pi` must not match.
        assert!(!file_mentions_pi_coding_agent(&other));
        assert!(!is_pi_coding_agent(&other));

        assert!(!is_pi_coding_agent(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod exec_allowlist_tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn allows_known_catalog_args() {
        assert!(exec_args_allowed(&args(&["--version"])));
        assert!(exec_args_allowed(&args(&["--list-models"])));
        assert!(exec_args_allowed(&args(&["models", "--verbose"])));
        assert!(exec_args_allowed(&args(&["agent", "list"])));
    }

    #[test]
    fn rejects_other_args() {
        assert!(!exec_args_allowed(&args(&[])));
        assert!(!exec_args_allowed(&args(&["--help"])));
        assert!(!exec_args_allowed(&args(&["--version", "--json"])));
        assert!(!exec_args_allowed(&args(&["models"])));
        assert!(!exec_args_allowed(&args(&["-c", "id"])));
        assert!(!exec_args_allowed(&args(&["agent", "list", "--json"])));
    }
}
