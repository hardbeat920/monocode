use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
#[cfg(not(windows))]
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::dirs_home;
use crate::fs::expand_home;
use crate::passwd_identity;

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

struct HarnessInner {
    children: HashMap<String, Arc<LiveChild>>,
    epochs: HashMap<String, u64>,
}

pub struct HarnessHost {
    inner: Mutex<HarnessInner>,
    sse: Mutex<HashMap<String, Arc<LiveSse>>>,
    /// Bumped by `kill_all` so a spawn that started before quit cannot reinsert.
    kill_all_gen: AtomicU64,
}

impl HarnessHost {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HarnessInner {
                children: HashMap::new(),
                epochs: HashMap::new(),
            }),
            sse: Mutex::new(HashMap::new()),
            kill_all_gen: AtomicU64::new(0),
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, HarnessInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn get(&self, session_id: &str) -> Option<Arc<LiveChild>> {
        self.lock_inner().children.get(session_id).cloned()
    }

    /// Stamp this spawn and drop any child already registered under the id.
    fn begin_spawn(&self, session_id: &str) -> (u64, u64, Option<Arc<LiveChild>>) {
        let mut inner = self.lock_inner();
        let kill_all = self.kill_all_gen.load(Ordering::SeqCst);
        let epoch = inner.epochs.entry(session_id.to_string()).or_insert(0);
        *epoch += 1;
        let epoch = *epoch;
        let prev = inner.children.remove(session_id);
        (epoch, kill_all, prev)
    }

    #[cfg(all(test, unix))]
    fn spawn_stamp_current(&self, session_id: &str, epoch: u64, kill_all: u64) -> bool {
        let inner = self.lock_inner();
        self.kill_all_gen.load(Ordering::SeqCst) == kill_all
            && inner.epochs.get(session_id) == Some(&epoch)
    }

    /// Keep the child only if nothing cancelled this spawn while it was forking.
    fn install_spawn(
        &self,
        session_id: String,
        epoch: u64,
        kill_all: u64,
        live: Arc<LiveChild>,
    ) -> Option<Arc<LiveChild>> {
        let mut inner = self.lock_inner();
        if self.kill_all_gen.load(Ordering::SeqCst) != kill_all {
            return Some(live);
        }
        if inner.epochs.get(&session_id) != Some(&epoch) {
            return Some(live);
        }
        if let Some(prev) = inner.children.insert(session_id, live) {
            terminate(prev.pid);
        }
        None
    }

    fn kill_session(&self, session_id: &str) -> Option<Arc<LiveChild>> {
        let mut inner = self.lock_inner();
        *inner.epochs.entry(session_id.to_string()).or_insert(0) += 1;
        inner.children.remove(session_id)
    }

    fn remove_if_pid(&self, session_id: &str, pid: u32) -> Option<Arc<LiveChild>> {
        let mut inner = self.lock_inner();
        if inner.children.get(session_id).map(|live| live.pid) != Some(pid) {
            return None;
        }
        inner.children.remove(session_id)
    }

    pub(crate) fn kill_all(&self) {
        let kids: Vec<Arc<LiveChild>> = {
            let mut inner = self.lock_inner();
            self.kill_all_gen.fetch_add(1, Ordering::SeqCst);
            inner.children.drain().map(|(_, child)| child).collect()
        };
        self.stop_all_sse();
        let pids: Vec<u32> = kids.iter().map(|live| live.pid).collect();
        // Drop stdin before signaling so ACP CLIs that watch the pipe can exit.
        drop(kids);
        terminate_all(&pids);
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
#[tauri::command(async)]
pub fn harness_resolve_cursor() -> Result<CursorBinary, String> {
    resolve_cursor_agent()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| "Cursor CLI not found. Install it and run `agent login`, then retry.".into())
}

/// Resolve the Codex CLI (`codex`).
#[tauri::command(async)]
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
#[tauri::command(async)]
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
#[tauri::command(async)]
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
#[tauri::command(async)]
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

/// Resolve the omp (oh-my-pi) coding agent CLI.
#[tauri::command(async)]
pub fn harness_resolve_omp() -> Result<CursorBinary, String> {
    resolve_omp()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| {
            "omp CLI not found. Install it with `curl -fsSL https://omp.sh/install | sh` and authenticate, then retry."
                .into()
        })
}

/// Resolve the Vercel fx coding agent CLI (`fx`), never the JSON viewer of the same name.
#[tauri::command(async)]
pub fn harness_resolve_fx() -> Result<CursorBinary, String> {
    resolve_fx()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| {
            "fx CLI not found. Install it from https://fx.sh and run `fx login`, then retry.".into()
        })
}

/// Resolve xAI Grok Build (`grok`).
#[tauri::command(async)]
pub fn harness_resolve_grok() -> Result<CursorBinary, String> {
    resolve_grok()
        .map(|path| CursorBinary {
            path: path.to_string_lossy().into_owned(),
        })
        .ok_or_else(|| {
            "Grok Build CLI not found. Install it with `curl -fsSL https://x.ai/cli/install.sh | bash` and run `grok login`, then retry.".into()
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

/// Off the main thread: fork/exec, and `apply_gui_env` can wait on the first
/// login-shell read. Callers await this before writing to the child. Kill can
/// still race the fork, so a cancelled spawn must not reinsert the child.
#[tauri::command(async)]
pub fn harness_spawn(
    app: AppHandle,
    host: State<'_, HarnessHost>,
    session_id: String,
    command: String,
    args: Vec<String>,
    cwd: String,
) -> Result<u32, String> {
    let (epoch, kill_all, prev) = host.begin_spawn(&session_id);
    if let Some(prev) = prev {
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
    prepare_child(&mut cmd, &command);

    let mut child =
        spawn_managed(&mut cmd).map_err(|e| format!("Failed to start {command}: {e}"))?;
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
    if let Some(rejected) = host.install_spawn(session_id.clone(), epoch, kill_all, live) {
        // A kill, or a newer spawn, won the race while this one was forking.
        // Returning `Ok` here would hand the caller a dead pid to store as the
        // session's live child, and this child's stdout would be parsed as the
        // stream that replaced it. Reap it without emitting anything.
        terminate(rejected.pid);
        thread::spawn(move || {
            let _ = child.wait();
        });
        return Err(SPAWN_CANCELLED.to_string());
    }

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
    let wait_pid = pid;
    thread::spawn(move || {
        let code = child.wait().ok().and_then(|status| status.code());
        if let Some(host) = wait_app.try_state::<HarnessHost>() {
            if host.remove_if_pid(&wait_id, wait_pid).is_some() {
                host.stop_sse(&wait_id);
            }
        }
        let _ = wait_app.emit(
            EXIT_EVENT,
            HarnessExit {
                session_id: wait_id,
                code,
                pid: wait_pid,
            },
        );
    });

    Ok(pid)
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
    if let Some(live) = host.kill_session(&session_id) {
        terminate(live.pid);
    }
    Ok(())
}

/// Off the main thread: `kill_all` waits for the children to die before it
/// returns, and a window close calls this while the app keeps running.
#[tauri::command(async)]
pub fn harness_kill_all(host: State<'_, HarnessHost>) -> Result<(), String> {
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
    &["models", "--json"],
    &["models"],
    &["status", "--json"],
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
        resolve_omp(),
        resolve_fx(),
        resolve_grok(),
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
    prepare_child(&mut cmd, command);
    if let Some(dir) = cwd {
        let workdir = expand_home(dir);
        if workdir.is_dir() {
            cmd.current_dir(workdir);
        }
    }

    let child = spawn_managed(&mut cmd).map_err(|e| format!("Failed to run {command}: {e}"))?;
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
/// Quit and `Drop` cannot wait on a detached escalate thread — the process
/// exits first and isolated harness groups stay behind as PID-1 orphans.
#[cfg(not(windows))]
const KILL_ALL_GRACE: Duration = Duration::from_millis(300);
#[cfg(not(windows))]
const KILL_ALL_KILL_WAIT: Duration = Duration::from_millis(150);
const HARNESS_PARENT_ENV: &str = "MONOCODE_HARNESS_PARENT";

/// An interactive shell has to source the user's whole rc file; nvm alone can
/// take a second.
#[cfg(not(windows))]
const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(5);

/// A spawn that was cancelled mid-fork. The session it was starting is already
/// gone or already replaced, so callers must not register this child.
const SPAWN_CANCELLED: &str = "Harness start was cancelled";

/// Its own process group, so one signal reaches the whole tree, plus the
/// marker a later launch reads to recognise what this run left behind. Every
/// harness spawn goes through here, probes included: a `--help` probe that
/// hangs is a `node` process too, and an unmarked one is unreapable.
fn isolate_child(cmd: &mut Command) {
    cmd.env(HARNESS_PARENT_ENV, std::process::id().to_string());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
}

fn spawn_managed(cmd: &mut Command) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        crate::windows::spawn_managed(cmd)
    }
    #[cfg(not(windows))]
    {
        cmd.spawn()
    }
}

fn terminate(pid: u32) {
    terminate_after(pid, KILL_ESCALATE);
}

fn terminate_after(pid: u32, escalate: Duration) {
    if pid == 0 || pid == 1 {
        return;
    }
    #[cfg(windows)]
    {
        let _ = escalate;
        signal_tree(pid, TreeSignal::Kill);
    }
    #[cfg(not(windows))]
    {
        signal_tree(pid, TreeSignal::Term);
        thread::spawn(move || {
            thread::sleep(escalate);
            if tree_alive(pid) {
                signal_tree(pid, TreeSignal::Kill);
            }
        });
    }
}

/// SIGTERM every tree, then SIGKILL whatever is still standing, before return.
pub(crate) fn terminate_all(pids: &[u32]) {
    let pids: Vec<u32> = pids.iter().copied().filter(|pid| *pid > 1).collect();
    #[cfg(windows)]
    for pid in pids {
        signal_tree(pid, TreeSignal::Kill);
    }
    #[cfg(not(windows))]
    {
        if pids.is_empty() {
            return;
        }
        for pid in &pids {
            signal_tree(*pid, TreeSignal::Term);
        }
        wait_until_dead(&pids, Instant::now() + KILL_ALL_GRACE);
        let remaining: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|pid| tree_alive(*pid))
            .collect();
        if remaining.is_empty() {
            return;
        }
        for pid in &remaining {
            signal_tree(*pid, TreeSignal::Kill);
        }
        wait_until_dead(&remaining, Instant::now() + KILL_ALL_KILL_WAIT);
    }
}

/// The thread that owns each `Child` reaps it, so a killed leader stops
/// answering `kill(pid, 0)` within a poll or two. Reaping here instead would
/// race that thread for the exit status and free the pid while we still signal
/// it.
#[cfg(not(windows))]
fn wait_until_dead(pids: &[u32], until: Instant) {
    while Instant::now() < until {
        if pids.iter().all(|pid| !tree_alive(*pid)) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

enum TreeSignal {
    #[cfg(not(windows))]
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
            // Every child is isolated with process_group(0), so its pid is the
            // stable group id even after the leader exits. Signal the group
            // first; looking it up through a dead leader loses descendants
            // that ignored SIGTERM and prevents the SIGKILL escalation.
            libc::kill(-ipid, sig);
            libc::kill(ipid, sig);
        }
    }
    #[cfg(windows)]
    {
        let _ = signal;
        let mut cmd = Command::new("taskkill");
        crate::hide_window_console(&mut cmd);
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let _ = cmd.status();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = signal;
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

#[cfg(not(windows))]
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

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(any(unix, test))]
#[derive(Debug, Clone)]
struct ProcessSnapshot {
    pid: u32,
    ppid: u32,
    args: String,
    harness_parent: Option<u32>,
}

/// Kill harness trees left behind by a previous MonoCode that exited
/// before SIGKILL ran (crash, force-quit, or the detached escalate thread).
/// Off-thread: the sweep shells out to `ps` and then waits on a SIGKILL, and
/// launch would otherwise hold the first window for both. Nothing this run
/// spawns can be caught by it — our own children carry our pid as the marker.
pub(crate) fn reap_orphaned_harness_processes() {
    #[cfg(unix)]
    {
        let our_pid = std::process::id();
        thread::spawn(move || reap_snapshots(&snapshot_processes(), our_pid));
    }
}

#[cfg(unix)]
fn reap_snapshots(rows: &[ProcessSnapshot], our_pid: u32) {
    let pids: Vec<u32> = rows
        .iter()
        .filter(|row| should_reap_process(row, our_pid, process_alive))
        .map(|row| row.pid)
        .collect();
    terminate_all(&pids);
}

#[cfg(any(unix, test))]
fn should_reap_process(
    proc: &ProcessSnapshot,
    our_pid: u32,
    parent_alive: impl Fn(u32) -> bool,
) -> bool {
    if proc.pid == our_pid || proc.pid <= 1 || proc.ppid == our_pid {
        return false;
    }
    if let Some(parent) = proc.harness_parent {
        return looks_like_harness_argv(&proc.args) && parent != our_pid && !parent_alive(parent);
    }
    proc.ppid == 1 && is_legacy_orphaned_cursor_acp(&proc.args)
}

/// Pre-marker leftovers: `cursor-agent acp` reparented to launchd.
#[cfg(any(unix, test))]
fn is_legacy_orphaned_cursor_acp(args: &str) -> bool {
    if !args.contains("cursor-agent") {
        return false;
    }
    args.split_whitespace().any(|part| part == "acp")
}

/// Argv of an agent CLI we spawned — not a shell, tmux, or `npm start`.
/// Used to decide whose environment is worth opening; the marker still
/// decides what actually dies.
#[cfg(any(unix, test))]
fn looks_like_harness_argv(args: &str) -> bool {
    if is_legacy_orphaned_cursor_acp(args) {
        return true;
    }
    args.split_whitespace().any(is_harness_argv_token)
}

#[cfg(any(unix, test))]
fn is_harness_argv_token(part: &str) -> bool {
    let name = Path::new(part)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(part);
    matches!(
        name,
        "cursor-agent"
            | "pi-coding-agent"
            | "claude"
            | "codex"
            | "opencode"
            | "grok"
            | "omp"
            | "fx"
            | "pi"
            | "worker-server"
            | "app-server"
    )
}

#[cfg(any(all(unix, not(target_os = "linux")), test))]
fn parse_ps_row(line: &str) -> Option<ProcessSnapshot> {
    let s = line.trim();
    let pid_end = s.find(char::is_whitespace)?;
    let pid: u32 = s[..pid_end].parse().ok()?;
    let rest = s[pid_end..].trim_start();
    let ppid_end = rest.find(char::is_whitespace)?;
    let ppid: u32 = rest[..ppid_end].parse().ok()?;
    let args = rest[ppid_end..].trim_start();
    if args.is_empty() {
        return None;
    }
    Some(ProcessSnapshot {
        pid,
        ppid,
        args: args.to_string(),
        harness_parent: harness_parent_from_bytes(args.as_bytes()),
    })
}

#[cfg(any(unix, test))]
fn harness_parent_from_bytes(buf: &[u8]) -> Option<u32> {
    let mut needle = Vec::with_capacity(HARNESS_PARENT_ENV.len() + 1);
    needle.extend_from_slice(HARNESS_PARENT_ENV.as_bytes());
    needle.push(b'=');
    let pos = buf
        .windows(needle.len())
        .position(|chunk| chunk == needle)?;
    let start = pos + needle.len();
    let digits = buf[start..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    std::str::from_utf8(&buf[start..start + digits])
        .ok()?
        .parse()
        .ok()
}

/// List processes by argv, then open environ only for agent CLIs. Linux
/// orphans sit under `systemd --user`, not pid 1, so the marker (not ppid)
/// is what identifies them.
#[cfg(unix)]
fn snapshot_processes() -> Vec<ProcessSnapshot> {
    #[cfg(target_os = "linux")]
    {
        snapshot_from_proc()
    }
    #[cfg(not(target_os = "linux"))]
    {
        snapshot_from_ps()
    }
}

/// `ps -E`/`-e` dumps every process environment; skip that. List argv only,
/// then open environ for agent CLIs. Linux orphans sit under `systemd --user`,
/// not pid 1, so the marker (not ppid) is what identifies them.
#[cfg(all(unix, not(target_os = "linux")))]
fn snapshot_from_ps() -> Vec<ProcessSnapshot> {
    let mut cmd = Command::new("ps");
    #[cfg(target_os = "macos")]
    {
        cmd.args(["-axww", "-o", "pid=", "-o", "ppid=", "-o", "command="]);
    }
    #[cfg(not(target_os = "macos"))]
    {
        cmd.args(["-axww", "-o", "pid=", "-o", "ppid=", "-o", "args="]);
    }
    let Ok(output) = cmd.output() else {
        return Vec::new();
    };
    let mut rows: Vec<ProcessSnapshot> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_ps_row)
        .collect();
    attach_markers_from_env(&mut rows);
    rows
}

#[cfg(target_os = "linux")]
fn snapshot_from_proc() -> Vec<ProcessSnapshot> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut rows: Vec<ProcessSnapshot> = entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let dir = entry.path();
            let cmdline = std::fs::read(dir.join("cmdline")).ok()?;
            if cmdline.is_empty() {
                return None;
            }
            Some(ProcessSnapshot {
                pid,
                ppid: proc_ppid(&dir)?,
                args: String::from_utf8_lossy(&cmdline).replace('\0', " "),
                harness_parent: None,
            })
        })
        .collect();
    attach_markers_from_env(&mut rows);
    rows
}

#[cfg(unix)]
fn attach_markers_from_env(rows: &mut [ProcessSnapshot]) {
    let pids: Vec<u32> = rows
        .iter()
        .filter(|row| row.harness_parent.is_none() && looks_like_harness_argv(&row.args))
        .map(|row| row.pid)
        .collect();
    if pids.is_empty() {
        return;
    }
    let parents = read_harness_parents(&pids);
    for row in rows {
        if row.harness_parent.is_some() {
            continue;
        }
        if let Some(parent) = parents.get(&row.pid).copied() {
            row.harness_parent = Some(parent);
        }
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn read_harness_parents(pids: &[u32]) -> HashMap<u32, u32> {
    let mut found = HashMap::new();
    if pids.is_empty() {
        return found;
    }
    let list = pids
        .iter()
        .map(|pid| pid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut cmd = Command::new("ps");
    #[cfg(target_os = "macos")]
    {
        cmd.args(["-Eww", "-p", &list, "-o", "pid=", "-o", "command="]);
    }
    #[cfg(not(target_os = "macos"))]
    {
        cmd.args(["-eww", "-p", &list, "-o", "pid=", "-o", "args="]);
    }
    let Ok(output) = cmd.output() else {
        return found;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((pid, command)) = parse_ps_pid_command(line) else {
            continue;
        };
        if let Some(parent) = harness_parent_from_bytes(command.as_bytes()) {
            found.insert(pid, parent);
        }
    }
    found
}

#[cfg(any(all(unix, not(target_os = "linux")), test))]
fn parse_ps_pid_command(line: &str) -> Option<(u32, String)> {
    let s = line.trim();
    let pid_end = s.find(char::is_whitespace)?;
    let pid: u32 = s[..pid_end].parse().ok()?;
    let command = s[pid_end..].trim_start();
    if command.is_empty() {
        return None;
    }
    Some((pid, command.to_string()))
}

#[cfg(target_os = "linux")]
fn read_harness_parents(pids: &[u32]) -> HashMap<u32, u32> {
    pids.iter()
        .filter_map(|pid| {
            let buf = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
            Some((*pid, harness_parent_from_bytes(&buf)?))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn proc_ppid(dir: &Path) -> Option<u32> {
    parse_proc_ppid(&std::fs::read_to_string(dir.join("stat")).ok()?)
}

/// Field 4 of `stat`, counted from the last closing paren: `comm` is unquoted
/// and can hold spaces and parens of its own.
#[cfg(any(target_os = "linux", test))]
fn parse_proc_ppid(stat: &str) -> Option<u32> {
    stat.get(stat.rfind(')')? + 1..)?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
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

    first_binary_matching(candidates, is_cursor_agent)
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

    // Last resort: the Codex app bundles its own CLI, but never puts it on
    // PATH. It is pinned to the app release (often a prerelease), so a real
    // CLI install always wins.
    if let Some(home) = &home {
        candidates.push(home.join("Applications/Codex.app/Contents/Resources/codex"));
    }
    candidates.push(PathBuf::from(
        "/Applications/Codex.app/Contents/Resources/codex",
    ));

    first_binary(candidates)
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

    first_binary(candidates)
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

    first_binary(candidates)
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

    first_binary_matching(candidates, is_pi_coding_agent)
}

fn resolve_omp() -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Installer default first, then bun/brew, then anything on the login PATH.
    if let Some(home) = &home {
        candidates.push(home.join(".local/bin/omp"));
        candidates.push(home.join(".bun/bin/omp"));
        candidates.push(home.join(".npm-global/bin/omp"));
        candidates.push(home.join(".cargo/bin/omp"));
        candidates.push(home.join("n/bin/omp"));
    }
    #[cfg(target_os = "macos")]
    candidates.push(PathBuf::from("/opt/homebrew/bin/omp"));
    candidates.push(PathBuf::from("/usr/local/bin/omp"));
    candidates.push(PathBuf::from("/usr/bin/omp"));
    candidates.push(PathBuf::from("/snap/bin/omp"));
    if let Some(from_shell) = which_via_login_shell("omp") {
        candidates.push(from_shell);
    }

    first_binary_matching(candidates, is_omp_agent)
}

/// omp ships as a ~126MB compiled binary, so the cheap string scan that
/// identifies the npm-installed Pi CLI finds nothing in its Mach-O header.
/// Identify it by name plus a `--help` probe instead.
fn is_omp_agent(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if !binary_name_eq(path, "omp") {
        return false;
    }
    help_mentions_rpc_mode(path)
}

fn resolve_fx() -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Installer default first so a Homebrew JSON-viewer `fx` does not win.
    if let Some(home) = &home {
        candidates.push(home.join(".local/bin/fx"));
        candidates.push(home.join(".fx/bin/fx"));
        candidates.push(home.join(".npm-global/bin/fx"));
        candidates.push(home.join(".cargo/bin/fx"));
        candidates.push(home.join("n/bin/fx"));
    }
    #[cfg(target_os = "macos")]
    candidates.push(PathBuf::from("/opt/homebrew/bin/fx"));
    candidates.push(PathBuf::from("/usr/local/bin/fx"));
    candidates.push(PathBuf::from("/usr/bin/fx"));
    candidates.push(PathBuf::from("/snap/bin/fx"));
    if let Some(from_shell) = which_via_login_shell("fx") {
        candidates.push(from_shell);
    }

    first_binary_matching(candidates, is_fx_agent)
}

fn resolve_grok() -> Option<PathBuf> {
    let home = dirs_home().map(PathBuf::from);
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(home) = &home {
        candidates.push(home.join(".grok/bin/grok"));
        candidates.push(home.join(".local/bin/grok"));
        candidates.push(home.join(".npm-global/bin/grok"));
        candidates.push(home.join(".cargo/bin/grok"));
        candidates.push(home.join("n/bin/grok"));
    }
    #[cfg(target_os = "macos")]
    candidates.push(PathBuf::from("/opt/homebrew/bin/grok"));
    candidates.push(PathBuf::from("/usr/local/bin/grok"));
    candidates.push(PathBuf::from("/usr/bin/grok"));
    candidates.push(PathBuf::from("/snap/bin/grok"));
    if let Some(from_shell) = which_via_login_shell("grok") {
        candidates.push(from_shell);
    }

    first_binary_matching(candidates, is_grok_agent)
}

fn is_pi_coding_agent(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if binary_name_eq(path, "pi-coding-agent") {
        return true;
    }
    if !binary_name_eq(path, "pi") {
        return false;
    }
    file_mentions_pi_coding_agent(path) || help_mentions_rpc_mode(path)
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

fn help_mentions_rpc_mode(path: &Path) -> bool {
    let mut cmd = Command::new(path);
    cmd.arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // npm-installed harnesses are `#!/usr/bin/env node` scripts, so this probe
    // fails outright without a PATH that has node on it.
    apply_gui_env(&mut cmd);
    isolate_child(&mut cmd);
    let Ok(child) = spawn_managed(&mut cmd) else {
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

fn is_fx_agent(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if !binary_name_eq(path, "fx") {
        return false;
    }
    file_mentions_fx_agent(path) || fx_help_mentions_acp(path)
}

fn is_grok_agent(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    if !binary_name_eq(path, "grok") {
        return false;
    }
    // Official installer: ~/.grok/bin/grok
    if path_has_component(path, ".grok") {
        return true;
    }
    file_mentions_grok_agent(path) || grok_help_mentions_agent(path)
}

/// The fx markers sit megabytes into the compiled binary, so a small head-read
/// never matched and every resolve fell through to spawning `fx --help`. Scan
/// the whole file in chunks instead, overlapping enough to catch a marker that
/// straddles a boundary.
fn file_mentions_fx_agent(path: &Path) -> bool {
    const MARKERS: [&str; 4] = ["vercel-labs/fx", "FX_MODEL", "createFxAgent", "fx acp"];
    const CHUNK: usize = 1024 * 1024;
    const OVERLAP: usize = 64;

    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file);
    let mut buf = vec![0u8; CHUNK + OVERLAP];
    let mut carry = 0usize;
    loop {
        let Ok(n) = reader.read(&mut buf[carry..]) else {
            return false;
        };
        if n == 0 {
            return false;
        }
        let filled = carry + n;
        let text = String::from_utf8_lossy(&buf[..filled]);
        if MARKERS.iter().any(|marker| text.contains(marker)) {
            return true;
        }
        carry = filled.min(OVERLAP);
        buf.copy_within(filled - carry..filled, 0);
    }
}

fn fx_help_mentions_acp(path: &Path) -> bool {
    let mut cmd = Command::new(path);
    cmd.arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // npm-installed harnesses are `#!/usr/bin/env node` scripts, so this probe
    // fails outright without a PATH that has node on it.
    apply_gui_env(&mut cmd);
    isolate_child(&mut cmd);
    let Ok(child) = spawn_managed(&mut cmd) else {
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
            text.contains("acp") && (text.contains("ask") || text.contains("gateway"))
        }
        _ => {
            terminate(pid);
            false
        }
    }
}

fn file_mentions_grok_agent(path: &Path) -> bool {
    const MARKERS: [&str; 4] = ["xai-grok", "Grok Build", "docs.x.ai/build", "grok agent"];
    const CHUNK: usize = 1024 * 1024;
    const OVERLAP: usize = 64;

    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut reader = BufReader::new(file);
    let mut buf = vec![0u8; CHUNK + OVERLAP];
    let mut carry = 0usize;
    loop {
        let Ok(n) = reader.read(&mut buf[carry..]) else {
            return false;
        };
        if n == 0 {
            return false;
        }
        let filled = carry + n;
        let text = String::from_utf8_lossy(&buf[..filled]);
        if MARKERS.iter().any(|marker| text.contains(marker)) {
            return true;
        }
        carry = filled.min(OVERLAP);
        buf.copy_within(filled - carry..filled, 0);
    }
}

fn grok_help_mentions_agent(path: &Path) -> bool {
    let mut cmd = Command::new(path);
    cmd.arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_gui_env(&mut cmd);
    isolate_child(&mut cmd);
    let Ok(child) = spawn_managed(&mut cmd) else {
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
            text.contains("grok build") || (text.contains("agent") && text.contains("stdio"))
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
    if path_has_component(path, ".grok") {
        return false;
    }
    if binary_name_eq(path, "cursor-agent") {
        return true;
    }
    if binary_name_eq(path, "agent") {
        // One symlink hop. canonicalize() can walk into another .app
        // and trip macOS "data from other apps" TCC.
        if let Ok(target) = std::fs::read_link(path) {
            let resolved = if target.is_absolute() {
                target
            } else {
                path.parent().unwrap_or(path).join(target)
            };
            return path_has_component(&resolved, "cursor-agent")
                || resolved
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .contains("cursor-agent");
        }
    }
    false
}

/// Look `name` up in the interactive login shell's PATH.
///
/// Reads the cached PATH rather than spawning a shell per lookup: six
/// resolvers each asking `command -v` meant six shell startups per probe.
fn which_via_login_shell(name: &str) -> Option<PathBuf> {
    which_in_path(&gui_search_path(), name)
}

fn which_in_path(path: &str, name: &str) -> Option<PathBuf> {
    std::env::split_paths(std::ffi::OsStr::new(path)).find_map(|dir| {
        if dir.as_os_str().is_empty() {
            return None;
        }
        existing_binary(dir.join(name))
    })
}

fn first_binary(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find_map(existing_binary)
}

fn first_binary_matching(
    candidates: Vec<PathBuf>,
    pred: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    candidates.into_iter().find_map(|path| {
        let path = existing_binary(path)?;
        pred(&path).then_some(path)
    })
}

fn existing_binary(path: PathBuf) -> Option<PathBuf> {
    #[cfg(windows)]
    if path.extension().is_none() {
        for ext in ["exe", "cmd", "bat", "com"] {
            let candidate = path.with_extension(ext);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    is_executable_file(&path).then_some(path)
}

#[cfg(all(test, windows))]
mod windows_launcher_tests {
    use super::*;

    #[test]
    fn npm_shell_shim_does_not_hide_windows_launcher() {
        let dir = std::env::temp_dir().join(format!("monocode-launcher-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bare = dir.join("agent");
        let cmd = dir.join("agent.cmd");
        std::fs::write(&bare, b"#!/bin/sh\n").unwrap();
        std::fs::write(&cmd, b"@echo off\n").unwrap();
        assert_eq!(existing_binary(bare.clone()), Some(cmd.clone()));
        std::fs::remove_file(&cmd).unwrap();
        assert_eq!(existing_binary(bare.clone()), None);
        std::fs::remove_file(bare).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}

fn binary_name_eq(path: &Path, expected: &str) -> bool {
    path.file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            if cfg!(windows) {
                name.eq_ignore_ascii_case(expected)
            } else {
                name == expected
            }
        })
}

fn path_has_component(path: &Path, needle: &str) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| name == needle)
    })
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ["exe", "cmd", "bat", "com"]
                        .iter()
                        .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                })
    }
}

/// Resolve `name` the way a terminal would, then fall back to common install
/// dirs. Finder-launched apps inherit launchd's PATH (`/usr/bin:/bin/…`), so
/// Homebrew / mise / `~/.local/bin` tools look missing unless we search here.
pub(crate) fn resolve_gui_binary(name: &str) -> Option<PathBuf> {
    which_in_path(&gui_search_path(), name)
}

pub(crate) fn gui_search_path() -> String {
    gui_search_path_from(login_shell_path(), dirs_home(), std::env::var("PATH").ok())
}

fn gui_search_path_from(
    login_path: Option<String>,
    home: Option<String>,
    existing: Option<String>,
) -> String {
    let mut parts: Vec<PathBuf> = Vec::new();
    // Login-shell PATH first so Homebrew, mise, nvm, and custom dirs match
    // the user's terminal. The fixed list is a fallback when that read fails.
    if let Some(path) = login_path {
        parts.extend(std::env::split_paths(&path));
    }
    if let Some(home) = home {
        parts.push(format!("{home}/.local/bin").into());
        parts.push(format!("{home}/.cargo/bin").into());
        parts.push(format!("{home}/.claude/local").into());
        parts.push(format!("{home}/.local/share/claude").into());
        parts.push(format!("{home}/.opencode/bin").into());
        parts.push(format!("{home}/.grok/bin").into());
        parts.push(format!("{home}/.npm-global/bin").into());
        parts.push(format!("{home}/.bun/bin").into());
        parts.push(format!("{home}/AppData/Roaming/npm").into());
        parts.push(format!("{home}/AppData/Local/Yarn/bin").into());
        parts.push(format!("{home}/scoop/shims").into());
    }
    parts.push("/opt/homebrew/bin".into());
    parts.push("/usr/local/bin".into());
    parts.push("/usr/bin".into());
    parts.push("/bin".into());
    parts.push("/snap/bin".into());
    #[cfg(windows)]
    {
        parts.push(r"C:\Program Files\Git\cmd".into());
        parts.push(r"C:\Program Files\nodejs".into());
    }
    if let Some(existing) = existing {
        parts.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(parts)
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn apply_gui_path(cmd: &mut Command) {
    cmd.env("PATH", gui_search_path());
}

pub(crate) fn apply_gui_env(cmd: &mut Command) {
    apply_gui_path(cmd);
    crate::hide_window_console(cmd);
    if let Some(id) = passwd_identity() {
        if std::env::var_os("HOME").is_none() {
            cmd.env("HOME", &id.home);
        }
        if std::env::var_os("USER").is_none() {
            cmd.env("USER", &id.user);
            cmd.env("LOGNAME", &id.user);
        }
        if std::env::var_os("SHELL").is_none() && !id.shell.is_empty() {
            cmd.env("SHELL", &id.shell);
        }
    } else if let Some(home) = dirs_home() {
        cmd.env("HOME", &home);
        if std::env::var_os("USERPROFILE").is_none() {
            cmd.env("USERPROFILE", &home);
        }
        if std::env::var_os("USER").is_none() {
            if let Ok(username) = std::env::var("USERNAME") {
                cmd.env("USER", username);
            }
        }
    }
    if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
}

fn prepare_child(cmd: &mut Command, command: &str) {
    apply_gui_env(cmd);
    if command_basename(command) == "fx" {
        apply_fx_env(cmd);
    }
    if command_basename(command) == "grok" {
        apply_grok_env(cmd);
    }
    isolate_child(cmd);
}

/// fx keeps its Gateway credential in the macOS Keychain and reads it by
/// shelling out to `osascript`. From a bundled app that read can block on a
/// SecurityAgent prompt nobody ever sees, and fx then rejects `initialize`
/// outright. Forwarding an API key from the login shell skips the Keychain
/// entirely for users who have one set.
fn apply_fx_env(cmd: &mut Command) {
    for key in [
        "AI_GATEWAY_API_KEY",
        "FX_AI_GATEWAY_API_KEY",
        "VERCEL_OIDC_TOKEN",
    ] {
        if std::env::var_os(key).is_some() {
            continue;
        }
        if let Some(value) = login_shell_env(key) {
            cmd.env(key, value);
        }
    }
}

fn apply_grok_env(cmd: &mut Command) {
    for key in ["XAI_API_KEY", "GROK_CODE_XAI_API_KEY"] {
        if std::env::var_os(key).is_some() {
            continue;
        }
        if let Some(value) = login_shell_env(key) {
            cmd.env(key, value);
        }
    }
}

static LOGIN_SHELL_ENV: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Keys worth keeping out of `printenv`. PATH is the important one: a
/// Finder-launched app inherits only launchd's bare PATH.
const LOGIN_SHELL_KEYS: [&str; 6] = [
    "PATH",
    "AI_GATEWAY_API_KEY",
    "FX_AI_GATEWAY_API_KEY",
    "VERCEL_OIDC_TOKEN",
    "XAI_API_KEY",
    "GROK_CODE_XAI_API_KEY",
];

fn login_shell_path() -> Option<String> {
    login_shell_env("PATH")
}

fn login_shell_env(name: &str) -> Option<String> {
    let mut cache = LOGIN_SHELL_ENV.lock().ok()?;
    if cache.is_none() {
        *cache = Some(load_login_shell_env());
    }
    cache
        .as_ref()
        .and_then(|map| map.get(name).cloned())
        .filter(|value| !value.is_empty())
}

fn load_login_shell_env() -> HashMap<String, String> {
    #[cfg(windows)]
    {
        LOGIN_SHELL_KEYS
            .into_iter()
            .filter_map(|key| {
                let value = std::env::var(key).ok()?;
                (!value.is_empty()).then(|| (key.to_string(), value))
            })
            .collect()
    }
    #[cfg(not(windows))]
    load_unix_login_shell_env()
}

/// Read the environment the user actually gets in a terminal.
///
/// `-lic`, not `-lc`: zsh reads `.zshrc` only for *interactive* shells, and
/// version managers (nvm, fnm, mise, volta) all initialize from there. A
/// login-but-not-interactive shell sees `.zshenv`/`.zprofile` only, so every
/// nvm-managed CLI looks uninstalled.
#[cfg(not(windows))]
fn load_unix_login_shell_env() -> HashMap<String, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/bash".into()
        }
    });
    let mut cmd = Command::new(&shell);
    cmd.args(["-lic", "printenv"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    isolate_child(&mut cmd);
    let Ok(child) = cmd.spawn() else {
        return HashMap::new();
    };
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = match rx.recv_timeout(LOGIN_SHELL_TIMEOUT) {
        Ok(Ok(output)) => output,
        _ => {
            terminate(pid);
            return HashMap::new();
        }
    };
    let mut map = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if LOGIN_SHELL_KEYS.contains(&key) && !value.is_empty() {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

fn command_basename(command: &str) -> &str {
    Path::new(command)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
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

    /// A real child so `install_spawn` can be exercised directly, rather than
    /// through a helper that re-states its condition.
    fn live_child() -> (Arc<LiveChild>, std::process::Child) {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn test process");
        let pid = child.id();
        let stdin = child.stdin.take().expect("test child stdin");
        (
            Arc::new(LiveChild {
                stdin: Mutex::new(stdin),
                pid,
            }),
            child,
        )
    }

    fn reap(mut child: std::process::Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn install_spawn_keeps_a_child_nothing_cancelled() {
        let host = HarnessHost::new();
        let (epoch, kill_all, _) = host.begin_spawn("s1");
        let (live, child) = live_child();
        let pid = live.pid;
        assert!(host
            .install_spawn("s1".into(), epoch, kill_all, live)
            .is_none());
        assert_eq!(host.get("s1").map(|live| live.pid), Some(pid));
        reap(child);
    }

    #[test]
    fn install_spawn_rejects_a_child_killed_mid_spawn() {
        let host = HarnessHost::new();
        let (epoch, kill_all, _) = host.begin_spawn("s1");
        host.kill_session("s1");
        let (live, child) = live_child();
        assert!(host
            .install_spawn("s1".into(), epoch, kill_all, live)
            .is_some());
        assert!(host.get("s1").is_none());
        reap(child);
    }

    #[test]
    fn install_spawn_rejects_a_child_after_kill_all() {
        let host = HarnessHost::new();
        let (epoch, kill_all, _) = host.begin_spawn("s1");
        host.kill_all();
        let (live, child) = live_child();
        assert!(host
            .install_spawn("s1".into(), epoch, kill_all, live)
            .is_some());
        assert!(host.get("s1").is_none());
        reap(child);
    }

    #[test]
    fn kill_during_spawn_invalidates_the_stamp() {
        let host = HarnessHost::new();
        let (epoch, kill_all, prev) = host.begin_spawn("s1");
        assert!(prev.is_none());
        assert!(host.spawn_stamp_current("s1", epoch, kill_all));
        host.kill_session("s1");
        assert!(!host.spawn_stamp_current("s1", epoch, kill_all));
    }

    #[test]
    fn overlapping_spawn_invalidates_the_earlier_one() {
        let host = HarnessHost::new();
        let first = host.begin_spawn("s1");
        let second = host.begin_spawn("s1");
        assert!(!host.spawn_stamp_current("s1", first.0, first.1));
        assert!(host.spawn_stamp_current("s1", second.0, second.1));
    }

    #[test]
    fn kill_all_rejects_an_in_flight_spawn() {
        let host = HarnessHost::new();
        let (epoch, kill_all, _) = host.begin_spawn("s1");
        host.kill_all();
        assert!(!host.spawn_stamp_current("s1", epoch, kill_all));
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
    fn terminate_escalates_after_group_leader_exits() {
        let mut child = spawn_group(
            "trap 'exit 0' TERM; sh -c 'trap \"\" TERM; while true; do sleep 1; done' & wait",
        );
        let pid = child.id();
        assert!(tree_alive(pid));
        terminate_after(pid, Duration::from_millis(150));
        if !wait_dead(pid, &mut child) {
            let _ = child.kill();
            panic!("process group survived after its leader exited");
        }
    }

    fn live_group(script: &str) -> (Arc<LiveChild>, std::process::Child) {
        let mut child = Command::new("sh")
            .args(["-c", script])
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn grouped child");
        let pid = child.id();
        let stdin = child.stdin.take().expect("grouped child stdin");
        (
            Arc::new(LiveChild {
                stdin: Mutex::new(stdin),
                pid,
            }),
            child,
        )
    }

    #[test]
    fn kill_all_reaps_term_ignoring_children_before_return() {
        let host = HarnessHost::new();
        let (epoch, kill_all, _) = host.begin_spawn("s1");
        let (live, child) = live_group("trap '' TERM; while true; do sleep 1; done");
        let pid = live.pid;
        // `harness_spawn` always leaves a thread owning the `Child`. Without one
        // the SIGKILLed leader lingers as a zombie that `kill(pid, 0)` still
        // answers, so the test would not be exercising the real shutdown.
        let waiter = thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });
        assert!(host
            .install_spawn("s1".into(), epoch, kill_all, live)
            .is_none());
        host.kill_all();
        let alive = tree_alive(pid);
        let _ = waiter.join();
        assert!(
            !alive,
            "kill_all returned while the harness tree was still alive"
        );
    }

    #[test]
    fn reap_snapshots_kills_a_marked_orphan() {
        let mut child = Command::new("sh")
            .args(["-c", "trap '' TERM; while true; do sleep 1; done"])
            .process_group(0)
            .env(HARNESS_PARENT_ENV, "2147483646")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn marked orphan");
        let pid = child.id();
        reap_snapshots(
            &[ProcessSnapshot {
                pid,
                ppid: 1,
                args: "/Users/n/.local/bin/cursor-agent acp".into(),
                harness_parent: Some(2_147_483_646),
            }],
            std::process::id(),
        );
        if !wait_dead(pid, &mut child) {
            let _ = child.kill();
            panic!("marked orphan survived reap_snapshots");
        }
    }

    #[test]
    fn reap_snapshots_spares_children_of_this_process() {
        let mut child = Command::new("sleep")
            .arg("30")
            .process_group(0)
            .env(HARNESS_PARENT_ENV, std::process::id().to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn live child");
        let pid = child.id();
        reap_snapshots(
            &[ProcessSnapshot {
                pid,
                ppid: std::process::id(),
                args: "sleep 30".into(),
                harness_parent: Some(std::process::id()),
            }],
            std::process::id(),
        );
        if !tree_alive(pid) {
            let _ = child.try_wait();
            panic!("reap_snapshots killed a child of this process");
        }
        reap(child);
    }

    /// The marker is what lets the next launch tell a crashed run's leftovers
    /// from a live instance's children. Every spawn funnels through
    /// `isolate_child`, so losing it here silently un-reaps probes and shells.
    #[test]
    fn isolate_child_stamps_the_reap_marker() {
        let pid = std::process::id().to_string();
        let mut cmd = Command::new("true");
        isolate_child(&mut cmd);
        assert!(cmd.get_envs().any(|(key, value)| {
            key == std::ffi::OsStr::new(HARNESS_PARENT_ENV)
                && value == Some(std::ffi::OsStr::new(pid.as_str()))
        }));
    }

    #[test]
    fn which_in_path_takes_the_first_executable_hit() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("monocode-which-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (empty, unreadable, real) = (dir.join("a"), dir.join("b"), dir.join("c"));
        for sub in [&empty, &unreadable, &real] {
            std::fs::create_dir_all(sub).unwrap();
        }

        // A same-named file that is not executable must not win.
        let decoy = unreadable.join("claude");
        std::fs::write(&decoy, b"not a program\n").unwrap();
        std::fs::set_permissions(&decoy, std::fs::Permissions::from_mode(0o644)).unwrap();

        let target = real.join("claude");
        std::fs::write(&target, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();

        let path = format!(
            "{}::{}:{}",
            empty.display(),
            unreadable.display(),
            real.display()
        );
        assert_eq!(which_in_path(&path, "claude"), Some(target));
        assert_eq!(which_in_path(&path, "codex"), None);
        assert_eq!(which_in_path("", "claude"), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gui_search_path_puts_login_path_ahead_of_fallbacks() {
        let path = gui_search_path_from(
            Some("/custom/gh-dir:/usr/bin".into()),
            Some("/tmp/home".into()),
            Some("/bin".into()),
        );
        let parts: Vec<&str> = path.split(':').collect();
        assert_eq!(parts[0], "/custom/gh-dir");
        assert!(parts.contains(&"/tmp/home/.local/bin"));
        assert!(parts.contains(&"/tmp/home/.grok/bin"));
        assert!(parts.contains(&"/opt/homebrew/bin"));
        assert!(parts.contains(&"/usr/local/bin"));
        assert_eq!(*parts.last().unwrap(), "/bin");
    }

    #[test]
    fn resolve_gui_binary_finds_a_binary_on_the_gui_path() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("monocode-gui-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("gh");
        std::fs::write(&target, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();

        let path = gui_search_path_from(Some(dir.to_string_lossy().into_owned()), None, None);
        assert_eq!(which_in_path(&path, "gh"), Some(target));

        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn omp_accepts_rpc_capable_binary_and_rejects_other_names() {
        let dir = std::env::temp_dir().join(format!("monocode-omp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // omp is a compiled binary, so identification leans on the --help probe.
        // A shell stub that answers `--help` the same way stands in for it here.
        let agent = dir.join("omp");
        std::fs::write(
            &agent,
            b"#!/bin/sh\necho '--mode=<value> Output mode: text, json, rpc, or rpc-ui'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(unix)]
        assert!(is_omp_agent(&agent));

        // oh-my-posh and friends must not win the name.
        let other = dir.join("oh-my-posh");
        std::fs::write(&other, b"#!/bin/sh\necho prompt theme engine\n").unwrap();
        assert!(!is_omp_agent(&other));

        assert!(!is_omp_agent(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fx_accepts_vercel_agent_and_rejects_json_viewer() {
        let dir = std::env::temp_dir().join(format!("monocode-fx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let agent = dir.join("fx");
        std::fs::write(&agent, b"#!/bin/sh\necho vercel-labs/fx\n# FX_MODEL\n").unwrap();
        assert!(is_fx_agent(&agent));

        let viewer = dir.join("fx-viewer");
        std::fs::write(&viewer, b"#!/bin/sh\necho Terminal JSON viewer\n").unwrap();
        assert!(!is_fx_agent(&viewer));

        let other = dir.join("fx");
        std::fs::write(&other, b"#!/bin/sh\necho json viewer\n").unwrap();
        assert!(!file_mentions_fx_agent(&other));
        assert!(!is_fx_agent(&other));

        assert!(!is_fx_agent(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real fx binary carries its markers megabytes in. A head-only read
    /// missed them and silently fell back to spawning `fx --help`.
    #[test]
    fn fx_marker_is_found_past_the_first_chunk() {
        let dir = std::env::temp_dir().join(format!("monocode-fx-deep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let agent = dir.join("fx");
        let mut blob = vec![b'\0'; 6 * 1024 * 1024];
        blob.extend_from_slice(b"https://github.com/vercel-labs/fx");
        std::fs::write(&agent, &blob).unwrap();
        assert!(file_mentions_fx_agent(&agent));

        // A marker straddling a chunk boundary must still be caught.
        let split = dir.join("fx-split");
        let mut edge = vec![b'\0'; 1024 * 1024 - 4];
        edge.extend_from_slice(b"vercel-labs/fx");
        std::fs::write(&split, &edge).unwrap();
        assert!(file_mentions_fx_agent(&split));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grok_accepts_official_install_path_and_markers() {
        let dir = std::env::temp_dir().join(format!("monocode-grok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join(".grok/bin");
        std::fs::create_dir_all(&home).unwrap();

        let agent = home.join("grok");
        std::fs::write(&agent, b"#!/bin/sh\necho other grok\n").unwrap();
        assert!(is_grok_agent(&agent));

        let marked = dir.join("grok");
        std::fs::write(&marked, b"#!/bin/sh\n# Grok Build\n# xai-grok\n").unwrap();
        assert!(is_grok_agent(&marked));

        let other = dir.join("grok-cli");
        std::fs::write(&other, b"#!/bin/sh\necho not grok\n").unwrap();
        assert!(!is_grok_agent(&other));

        assert!(!is_grok_agent(&dir.join("missing")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_basename_strips_path() {
        assert_eq!(command_basename("/Users/me/.local/bin/fx"), "fx");
        assert_eq!(command_basename("fx"), "fx");
        assert_eq!(command_basename("/Users/me/.grok/bin/grok"), "grok");
        assert_eq!(command_basename("fx.exe"), "fx");
    }

    #[test]
    fn passwd_identity_resolves_the_current_user() {
        let id = passwd_identity().expect("passwd");
        assert!(!id.user.is_empty());
        assert!(PathBuf::from(&id.home).is_dir());
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
        assert!(exec_args_allowed(&args(&["models", "--json"])));
        assert!(exec_args_allowed(&args(&["models"])));
        assert!(exec_args_allowed(&args(&["status", "--json"])));
        assert!(exec_args_allowed(&args(&["agent", "list"])));
    }

    #[test]
    fn rejects_other_args() {
        assert!(!exec_args_allowed(&args(&[])));
        assert!(!exec_args_allowed(&args(&["--help"])));
        assert!(!exec_args_allowed(&args(&["--version", "--json"])));
        assert!(!exec_args_allowed(&args(&["-c", "id"])));
        assert!(!exec_args_allowed(&args(&["agent", "list", "--json"])));
    }
}

#[cfg(test)]
mod reap_logic_tests {
    use super::*;

    fn row(pid: u32, ppid: u32, args: &str, parent: Option<u32>) -> ProcessSnapshot {
        ProcessSnapshot {
            pid,
            ppid,
            args: args.into(),
            harness_parent: parent,
        }
    }

    #[test]
    fn parse_ps_row_reads_pid_ppid_and_args() {
        let parsed = parse_ps_row(
            " 2436     1 /Users/n/.local/bin/cursor-agent --use-system-ca index.js acp",
        )
        .unwrap();
        assert_eq!(parsed.pid, 2436);
        assert_eq!(parsed.ppid, 1);
        assert!(parsed.args.contains("cursor-agent"));
        assert!(parsed.args.ends_with(" acp") || parsed.args.ends_with("acp"));
        assert_eq!(parsed.harness_parent, None);
    }

    #[test]
    fn parse_ps_row_reads_harness_parent_from_env_tail() {
        let parsed = parse_ps_row(
            " 27129 21504 /Users/n/cursor-agent acp PATH=/usr/bin MONOCODE_HARNESS_PARENT=21504 HOME=/tmp",
        )
        .unwrap();
        assert_eq!(parsed.pid, 27129);
        assert_eq!(parsed.ppid, 21504);
        assert_eq!(parsed.harness_parent, Some(21504));
    }

    #[test]
    fn parse_proc_ppid_skips_a_comm_holding_spaces_and_parens() {
        assert_eq!(
            parse_proc_ppid("2436 (cursor-agent) S 1 2436 2436 0 -1"),
            Some(1)
        );
        assert_eq!(
            parse_proc_ppid("2436 (Web Content (x)) S 21504 2436 0 -1"),
            Some(21504)
        );
        assert_eq!(parse_proc_ppid("2436 no-parens S 1"), None);
    }

    #[test]
    fn harness_parent_from_bytes_reads_the_marker() {
        let mut buf = b"PATH=/usr/bin\0".to_vec();
        buf.extend_from_slice(HARNESS_PARENT_ENV.as_bytes());
        buf.extend_from_slice(b"=21504\0HOME=/tmp\0");
        assert_eq!(harness_parent_from_bytes(&buf), Some(21504));
    }

    #[test]
    fn should_reap_marked_child_of_a_dead_parent() {
        let proc = row(
            10,
            1,
            "/Users/n/.local/bin/cursor-agent --use-system-ca index.js acp",
            Some(999),
        );
        assert!(should_reap_process(&proc, 42, |_| false));
        assert!(!should_reap_process(&proc, 42, |_| true));
    }

    #[test]
    fn should_not_reap_our_own_marked_child() {
        let proc = row(10, 42, "/Users/n/.local/bin/cursor-agent acp", Some(42));
        assert!(!should_reap_process(&proc, 42, |_| true));
    }

    #[test]
    fn should_not_reap_a_terminal_program_even_if_marked() {
        assert!(!should_reap_process(
            &row(10, 1, "tmux new -s work", Some(999)),
            42,
            |_| false,
        ));
        assert!(!should_reap_process(
            &row(11, 1, "npm start", Some(999)),
            42,
            |_| false,
        ));
        assert!(!should_reap_process(
            &row(12, 1, "-zsh", Some(999)),
            42,
            |_| false,
        ));
    }

    #[test]
    fn looks_like_harness_argv_accepts_agent_clis_only() {
        assert!(looks_like_harness_argv(
            "/Users/n/.local/bin/cursor-agent --use-system-ca index.js acp"
        ));
        assert!(looks_like_harness_argv(
            "/opt/homebrew/bin/node /Users/n/.local/share/cursor-agent/versions/x/index.js worker-server"
        ));
        assert!(looks_like_harness_argv("/Users/n/.local/bin/claude --help"));
        assert!(!looks_like_harness_argv("tmux new -s work"));
        assert!(!looks_like_harness_argv("npm start"));
        assert!(!looks_like_harness_argv(
            "node /Users/n/code/app/node_modules/typescript/lib/tsserver.js"
        ));
    }

    #[test]
    fn parse_ps_pid_command_does_not_treat_the_binary_as_ppid() {
        let (pid, command) = parse_ps_pid_command(
            " 27129 /Users/n/cursor-agent acp PATH=/usr/bin MONOCODE_HARNESS_PARENT=21504 HOME=/tmp",
        )
        .unwrap();
        assert_eq!(pid, 27129);
        assert_eq!(harness_parent_from_bytes(command.as_bytes()), Some(21504));
    }

    #[test]
    fn should_reap_legacy_cursor_acp_orphaned_to_launchd() {
        let args = "/Users/n/.local/bin/cursor-agent --use-system-ca /Users/n/index.js acp";
        assert!(should_reap_process(&row(10, 1, args, None), 42, |_| false));
        assert!(!should_reap_process(&row(10, 42, args, None), 42, |_| true));
        assert!(!is_legacy_orphaned_cursor_acp(
            "node /usr/local/bin/typescript-language-server --stdio"
        ));
    }
}
