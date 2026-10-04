//! Port of src/integrations/harness/core/child.ts: the `ChildBackend` seam
//! between provider protocols and whoever owns their processes, and the
//! bridge that routes each session's stdout, stderr, and exit.
//!
//! The TypeScript reached the backend through Tauri `invoke` and `listen`, or
//! a headless host's replacement. Here [`ChildBackend`] is a trait with one
//! typed method per command, [`HostChildBackend`] implements it over
//! `monocode_process::harness::HarnessHost`, and [`ChildRouter`] receives the
//! host's events (it implements `HarnessEvents`) and routes them to async
//! channels. [`Children`] is the handle providers use, with the same
//! functions `child.ts` exported.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use anyhow::{Result, anyhow};
use futures::FutureExt;
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};

use monocode_core::harness::HarnessId;
use monocode_process::control::ControlHost;
use monocode_process::harness::{self as host, HarnessEvents, HarnessHost};

use super::task::{BoxFuture, SharedSpawner};

/// A backend call. Errors are the plain strings the commands returned.
pub type ChildFuture<T> = BoxFuture<'static, Result<T, String>>;

/// `account` on `spawnChild`: run the child under a named provider profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildAccount {
    pub provider: HarnessId,
    pub id: String,
}

/// The arguments of `harness_spawn`.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct SpawnRequest {
    pub session_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub account: Option<ChildAccount>,
    pub binary_provider: Option<HarnessId>,
    pub binary_path: Option<String>,
    /// Per-process overrides. Debug output includes names only.
    pub environment: HashMap<String, String>,
}

impl std::fmt::Debug for SpawnRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names: Vec<_> = self.environment.keys().collect();
        names.sort();
        f.debug_struct("SpawnRequest")
            .field("session_id", &self.session_id)
            .field("command", &self.command)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("account", &self.account)
            .field("binary_provider", &self.binary_provider)
            .field("binary_path", &self.binary_path)
            .field("environment_names", &names)
            .finish()
    }
}

/// The arguments of `harness_exec`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecRequest {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub binary_provider: Option<HarnessId>,
    pub binary_path: Option<String>,
}

/// The input of `harnessHttp`.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<String>,
    pub timeout_ms: Option<i64>,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names: Vec<_> = self
            .headers
            .as_ref()
            .into_iter()
            .flat_map(|headers| headers.keys())
            .collect();
        names.sort();
        f.debug_struct("HttpRequest")
            .field("url", &self.url)
            .field("method", &self.method)
            .field("header_names", &names)
            .field("body_bytes", &self.body.as_ref().map(String::len))
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

/// The result of `harnessHttp`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// `ResolvedHarnessBinary`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ResolvedHarnessBinary {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

/// `HarnessBinaryInspection`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HarnessBinaryInspection {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The `binaryPath?: string | null` argument of the resolvers and
/// `execChild`. `undefined` meant "the path MonoCode launched with", `null`
/// meant "no configured path".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BinaryPathChoice {
    /// `undefined`: use the runtime path for the provider, if any.
    #[default]
    Runtime,
    /// A given path, or `None` for the provider's default resolver.
    Given(Option<String>),
}

/// `ChildBackend`: process I/O supplied by the desktop app or a headless
/// host. Provider protocols never need to know which process owns their
/// children. Events come back through a [`ChildRouter`].
pub trait ChildBackend: Send + Sync {
    /// `harness_spawn`. Returns the child's pid.
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32>;
    /// `harness_write`: one line, newline added by the host. May block for as
    /// long as the child does not drain stdin.
    fn write(&self, session_id: String, line: String) -> ChildFuture<()>;
    /// `harness_kill`.
    fn kill(&self, session_id: String) -> ChildFuture<()>;
    /// `harness_kill_all`.
    fn kill_all(&self) -> ChildFuture<()>;
    /// `runtimeProviderBinaryPath`: the configured path this process launched
    /// with, trimmed, or `None`.
    fn runtime_binary_path(&self, provider: HarnessId) -> Option<String>;
    /// `harness_resolve_<provider>`.
    fn resolve_default(&self, provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary>;
    /// `harness_resolve_configured`.
    fn resolve_configured(
        &self,
        provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary>;
    /// `harness_exec`.
    fn exec(&self, request: ExecRequest) -> ChildFuture<String>;
    /// `harness_free_port`.
    fn free_port(&self) -> ChildFuture<u16>;
    /// `harness_http`.
    fn http(&self, request: HttpRequest) -> ChildFuture<HttpResponse>;
    /// `harness_sse_open`.
    fn sse_open(
        &self,
        session_id: String,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()>;
    /// `harness_sse_close`.
    fn sse_close(&self, session_id: String) -> ChildFuture<()>;
    /// `read_text_file`, or `harness_read_text_file` on a headless host:
    /// provider-owned transcript files are read on the machine running the child.
    fn read_text_file(&self, path: String) -> ChildFuture<String>;
    /// `harness_update`: run the CLI's own self-update.
    fn update_cli(
        &self,
        command: String,
        provider: HarnessId,
        binary_path: Option<String>,
    ) -> ChildFuture<()>;
    /// `homeDir()` on the machine running the child.
    fn home_dir(&self) -> ChildFuture<String>;
    /// `hasHeadlessChildBackend`: true for a headless host's backend.
    fn is_headless(&self) -> bool {
        false
    }
}

/// What a watched child produced, in arrival order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildEvent {
    Stdout(String),
    Stderr(String),
    /// The current child exited. Exits of replaced children never arrive.
    Exit(Option<i32>),
}

/// What a watched OpenCode event stream produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    Data(String),
    /// The stream ended, with the error that ended it, if any.
    End(Option<String>),
}

/// The receiving side of `watchChild`. It closes when the watch is replaced
/// or removed, after any events already queued.
pub type ChildEvents = async_channel::Receiver<ChildEvent>;

/// The receiving side of `watchSse`.
pub type SseEvents = async_channel::Receiver<SseEvent>;

/// `MAX_BUFFERED`: stdout lines and SSE frames kept for a session that has no
/// watcher yet.
pub const MAX_BUFFERED: usize = 1000;

/// `isCurrentChildExit`: true when this exit belongs to the child we have spawned.
pub fn is_current_child_exit(expected_pid: Option<i64>, exited_pid: Option<i64>) -> bool {
    let Some(expected) = expected_pid.filter(|pid| *pid > 0) else {
        return false;
    };
    let Some(exited) = exited_pid.filter(|pid| *pid > 0) else {
        return false;
    };
    exited == expected
}

#[derive(Default)]
struct RouterState {
    watchers: HashMap<String, async_channel::Sender<ChildEvent>>,
    line_buffer: HashMap<String, VecDeque<String>>,
    sse_watchers: HashMap<String, async_channel::Sender<SseEvent>>,
    sse_buffer: HashMap<String, VecDeque<String>>,
    /// Ids this router spawned or opened and has not stopped. Only these may
    /// buffer while unwatched. Output for any other id, such as a killed
    /// generation's late lines, would sit in a buffer until the router clears
    /// and then replay into the next child that reuses the id.
    owned_children: HashSet<String>,
    owned_sse: HashSet<String>,
    live_pid: HashMap<String, u32>,
    pending_exit: HashMap<String, Vec<(Option<i32>, u32)>>,
}

impl RouterState {
    fn clear(&mut self) {
        // Dropping the senders closes every watcher's channel.
        *self = RouterState::default();
    }
}

fn push_bounded(map: &mut HashMap<String, VecDeque<String>>, session_id: &str, item: String) {
    let queued = map.entry(session_id.to_string()).or_default();
    queued.push_back(item);
    while queued.len() > MAX_BUFFERED {
        queued.pop_front();
    }
}

/// The bridge: routes harness events to the session that owns them. Lines
/// for an owned session with no watcher wait in a buffer of
/// [`MAX_BUFFERED`]. Lines for any other session are dropped.
#[derive(Default)]
pub struct ChildRouter {
    state: Mutex<RouterState>,
}

impl ChildRouter {
    #[cfg(test)]
    pub(crate) fn watched_children(&self) -> usize {
        self.state.lock().watchers.len()
    }

    #[cfg(test)]
    pub(crate) fn watched_streams(&self) -> usize {
        self.state.lock().sse_watchers.len()
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// `harness-stdout`.
    pub fn on_stdout(&self, session_id: &str, line: String) {
        let mut state = self.state.lock();
        if let Some(watcher) = state.watchers.get(session_id) {
            let _ = watcher.try_send(ChildEvent::Stdout(line));
            return;
        }
        if state.owned_children.contains(session_id) {
            push_bounded(&mut state.line_buffer, session_id, line);
        }
    }

    /// `harness-stderr`. Dropped when nobody watches the session.
    pub fn on_stderr(&self, session_id: &str, line: String) {
        if let Some(watcher) = self.state.lock().watchers.get(session_id) {
            let _ = watcher.try_send(ChildEvent::Stderr(line));
        }
    }

    /// `harness-exit`.
    pub fn on_exit(&self, session_id: &str, code: Option<i32>, pid: u32) {
        let mut state = self.state.lock();
        if pid == 0 || !state.watchers.contains_key(session_id) {
            return;
        }
        let current = state.live_pid.get(session_id).copied();
        if is_current_child_exit(current.map(i64::from), Some(i64::from(pid))) {
            state.live_pid.remove(session_id);
            if let Some(watcher) = state.watchers.get(session_id) {
                let _ = watcher.try_send(ChildEvent::Exit(code));
            }
            return;
        }
        if current.is_some() {
            return;
        }
        let exits = state
            .pending_exit
            .entry(session_id.to_string())
            .or_default();
        exits.push((code, pid));
        if exits.len() > 8 {
            let excess = exits.len() - 8;
            exits.drain(..excess);
        }
    }

    /// `harness-sse`.
    pub fn on_sse(&self, session_id: &str, data: String) {
        let mut state = self.state.lock();
        if let Some(watcher) = state.sse_watchers.get(session_id) {
            let _ = watcher.try_send(SseEvent::Data(data));
            return;
        }
        if state.owned_sse.contains(session_id) {
            push_bounded(&mut state.sse_buffer, session_id, data);
        }
    }

    /// `harness-sse-end`.
    pub fn on_sse_end(&self, session_id: &str, error: Option<String>) {
        if let Some(watcher) = self.state.lock().sse_watchers.get(session_id) {
            let _ = watcher.try_send(SseEvent::End(error));
        }
    }

    fn watch_child(&self, session_id: &str) -> ChildEvents {
        let (tx, rx) = async_channel::unbounded();
        let mut state = self.state.lock();
        if let Some(queued) = state.line_buffer.remove(session_id) {
            for line in queued {
                let _ = tx.try_send(ChildEvent::Stdout(line));
            }
        }
        state.watchers.insert(session_id.to_string(), tx);
        rx
    }

    fn unwatch_child(&self, session_id: &str) {
        let mut state = self.state.lock();
        state.watchers.remove(session_id);
        state.line_buffer.remove(session_id);
        state.owned_children.remove(session_id);
        state.pending_exit.remove(session_id);
    }

    fn watch_sse(&self, session_id: &str) -> SseEvents {
        let (tx, rx) = async_channel::unbounded();
        let mut state = self.state.lock();
        if let Some(queued) = state.sse_buffer.remove(session_id) {
            for data in queued {
                let _ = tx.try_send(SseEvent::Data(data));
            }
        }
        state.sse_watchers.insert(session_id.to_string(), tx);
        rx
    }

    fn unwatch_sse(&self, session_id: &str) {
        let mut state = self.state.lock();
        state.sse_watchers.remove(session_id);
        state.sse_buffer.remove(session_id);
        state.owned_sse.remove(session_id);
    }

    /// `ownedChildren.add`: a child this router is about to spawn.
    fn own_child(&self, session_id: &str) {
        self.state
            .lock()
            .owned_children
            .insert(session_id.to_string());
    }

    /// `ownedSse.add`: a stream this router is about to open.
    fn own_sse(&self, session_id: &str) {
        self.state.lock().owned_sse.insert(session_id.to_string());
    }

    fn clear_pid(&self, session_id: &str) {
        let mut state = self.state.lock();
        state.live_pid.remove(session_id);
        state.pending_exit.remove(session_id);
    }

    /// Record the spawned pid and deliver an exit that beat it here.
    fn spawned(&self, session_id: &str, pid: u32) {
        let mut state = self.state.lock();
        state.live_pid.insert(session_id.to_string(), pid);
        let exits = state.pending_exit.remove(session_id);
        let Some((code, _)) = exits.and_then(|exits| exits.into_iter().find(|(_, p)| *p == pid))
        else {
            return;
        };
        state.live_pid.remove(session_id);
        if let Some(watcher) = state.watchers.get(session_id) {
            let _ = watcher.try_send(ChildEvent::Exit(code));
        }
    }

    fn clear(&self) {
        self.state.lock().clear();
    }
}

impl HarnessEvents for ChildRouter {
    fn stdout(&self, session_id: &str, line: String) {
        self.on_stdout(session_id, line);
    }

    fn stderr(&self, session_id: &str, line: String) {
        self.on_stderr(session_id, line);
    }

    fn exit(&self, session_id: &str, code: Option<i32>, pid: u32) {
        self.on_exit(session_id, code, pid);
    }

    fn sse(&self, session_id: &str, data: String) {
        self.on_sse(session_id, data);
    }

    fn sse_end(&self, session_id: &str, error: Option<String>) {
        self.on_sse_end(session_id, error);
    }
}

/// Callback form of a child watch, for code ported from `watchChild(id,
/// onLine, onExit, onStderr)`.
pub struct ChildHandlers {
    pub on_line: Box<dyn FnMut(String) + Send>,
    pub on_exit: Box<dyn FnMut(Option<i32>) + Send>,
    pub on_stderr: Option<Box<dyn FnMut(String) + Send>>,
}

struct ChildrenInner {
    backend: Arc<dyn ChildBackend>,
    router: Arc<ChildRouter>,
    spawner: SharedSpawner,
    bridge_users: Mutex<usize>,
}

/// The child process handle providers use. Clones share one backend and
/// one router. Each function keeps its `child.ts` name in snake case.
#[derive(Clone)]
pub struct Children {
    inner: Arc<ChildrenInner>,
}

/// `startHarnessBridge`'s release function, as a guard.
pub struct BridgeLease {
    children: Children,
}

impl Drop for BridgeLease {
    fn drop(&mut self) {
        self.children.release_bridge();
    }
}

fn err(error: String) -> anyhow::Error {
    anyhow!(error)
}

impl Children {
    pub fn new(
        backend: Arc<dyn ChildBackend>,
        router: Arc<ChildRouter>,
        spawner: SharedSpawner,
    ) -> Self {
        Self {
            inner: Arc::new(ChildrenInner {
                backend,
                router,
                spawner,
                bridge_users: Mutex::new(0),
            }),
        }
    }

    /// Children over a local [`HarnessHost`]. Returns the host too, for the
    /// commands that take it directly (MCP, provider accounts, kill on quit).
    pub fn for_host(options: HostChildOptions, spawner: SharedSpawner) -> (Self, HarnessHost) {
        let router = Arc::new(ChildRouter::new());
        let harness_host = HarnessHost::new(router.clone());
        let backend = HostChildBackend::new(harness_host.clone(), options);
        (Self::new(Arc::new(backend), router, spawner), harness_host)
    }

    pub fn backend(&self) -> &Arc<dyn ChildBackend> {
        &self.inner.backend
    }

    pub fn router(&self) -> &Arc<ChildRouter> {
        &self.inner.router
    }

    pub fn spawner(&self) -> &SharedSpawner {
        &self.inner.spawner
    }

    /// `hasHeadlessChildBackend`.
    pub fn has_headless_child_backend(&self) -> bool {
        self.inner.backend.is_headless()
    }

    /// `readHarnessTextFile`.
    pub async fn read_harness_text_file(&self, path: &str) -> Result<String> {
        self.inner
            .backend
            .read_text_file(path.to_string())
            .await
            .map_err(err)
    }

    /// `startHarnessBridge`. The router always receives events; the lease
    /// count only decides when to clear routing state. When the last lease
    /// drops, the state clears on the next tick unless a new lease arrives,
    /// as the TypeScript `setTimeout(0)` did.
    pub fn start_harness_bridge(&self) -> BridgeLease {
        *self.inner.bridge_users.lock() += 1;
        BridgeLease {
            children: self.clone(),
        }
    }

    /// `acquireHarnessBridge`. Installing the bridge cannot fail here.
    pub async fn acquire_harness_bridge(&self) -> Result<BridgeLease> {
        Ok(self.start_harness_bridge())
    }

    fn release_bridge(&self) {
        {
            let mut users = self.inner.bridge_users.lock();
            *users = users.saturating_sub(1);
            if *users > 0 {
                return;
            }
        }
        let children = self.clone();
        self.inner.spawner.spawn(
            async move {
                smol::future::yield_now().await;
                if *children.inner.bridge_users.lock() == 0 {
                    children.inner.router.clear();
                }
            }
            .boxed(),
        );
    }

    /// `watchChild`: route this session's events to the returned channel.
    /// Lines buffered before the watch arrive first. A later watch of the
    /// same session closes this one.
    pub fn watch_child(&self, session_id: &str) -> ChildEvents {
        self.inner.router.watch_child(session_id)
    }

    /// `watchChild` with callbacks. A task on the spawner calls them in order
    /// until the watch closes.
    pub fn watch_child_with(&self, session_id: &str, handlers: ChildHandlers) {
        let events = self.watch_child(session_id);
        let ChildHandlers {
            mut on_line,
            mut on_exit,
            mut on_stderr,
        } = handlers;
        self.inner.spawner.spawn(
            async move {
                while let Ok(event) = events.recv().await {
                    match event {
                        ChildEvent::Stdout(line) => on_line(line),
                        ChildEvent::Stderr(line) => {
                            if let Some(on_stderr) = on_stderr.as_mut() {
                                on_stderr(line);
                            }
                        }
                        ChildEvent::Exit(code) => on_exit(code),
                    }
                }
            }
            .boxed(),
        );
    }

    /// `unwatchChild`.
    pub fn unwatch_child(&self, session_id: &str) {
        self.inner.router.unwatch_child(session_id);
    }

    /// `watchSse`.
    pub fn watch_sse(&self, session_id: &str) -> SseEvents {
        self.inner.router.watch_sse(session_id)
    }

    /// `unwatchSse`.
    pub fn unwatch_sse(&self, session_id: &str) {
        self.inner.router.unwatch_sse(session_id);
    }

    fn binary_path_for(&self, provider: HarnessId, choice: &BinaryPathChoice) -> Option<String> {
        match choice {
            BinaryPathChoice::Runtime => self.inner.backend.runtime_binary_path(provider),
            BinaryPathChoice::Given(path) => path.clone(),
        }
    }

    /// `spawnChild`. `binary_provider` validates the command against that
    /// provider's resolved binary.
    pub async fn spawn_child(
        &self,
        session_id: &str,
        command: &str,
        args: Vec<String>,
        cwd: &str,
        account: Option<ChildAccount>,
        binary_provider: Option<HarnessId>,
    ) -> Result<()> {
        self.spawn_request(SpawnRequest {
            session_id: session_id.to_string(),
            command: command.to_string(),
            args,
            cwd: cwd.to_string(),
            account,
            binary_provider,
            ..Default::default()
        })
        .await
    }

    pub async fn spawn_request(&self, mut request: SpawnRequest) -> Result<()> {
        self.inner.router.clear_pid(&request.session_id);
        self.inner.router.own_child(&request.session_id);
        if request.binary_path.is_none() {
            request.binary_path = request
                .binary_provider
                .and_then(|provider| self.binary_path_for(provider, &BinaryPathChoice::Runtime));
        }
        let session_id = request.session_id.clone();
        let pid = self.inner.backend.spawn(request).await.map_err(err)?;
        if pid == 0 {
            return Ok(());
        }
        self.inner.router.spawned(&session_id, pid);
        Ok(())
    }

    /// `writeChild`.
    pub async fn write_child(&self, session_id: &str, line: &str) -> Result<()> {
        self.inner
            .backend
            .write(session_id.to_string(), line.to_string())
            .await
            .map_err(err)
    }

    /// `killChild`. Stops routing the session at once, then kills it.
    pub async fn kill_child(&self, session_id: &str) -> Result<()> {
        self.inner.router.clear_pid(session_id);
        self.unwatch_child(session_id);
        self.inner
            .backend
            .kill(session_id.to_string())
            .await
            .map_err(err)
    }

    /// `killAllChildren`.
    pub async fn kill_all_children(&self) -> Result<()> {
        self.inner.router.clear();
        self.inner.backend.kill_all().await.map_err(err)
    }

    /// `resolveHarnessBinary`.
    pub async fn resolve_harness_binary(
        &self,
        provider: HarnessId,
        binary_path: BinaryPathChoice,
    ) -> Result<ResolvedHarnessBinary> {
        let configured = match &binary_path {
            BinaryPathChoice::Runtime => self.inner.backend.runtime_binary_path(provider),
            BinaryPathChoice::Given(path) => path
                .as_deref()
                .map(|path| path.trim().to_string())
                .filter(|path| !path.is_empty()),
        };
        let resolved = match configured {
            Some(path) => self.inner.backend.resolve_configured(provider, path).await,
            None => self.inner.backend.resolve_default(provider).await,
        };
        resolved.map_err(err)
    }

    /// The runtime binary for `provider`.
    pub async fn resolve_binary(&self, provider: HarnessId) -> Result<ResolvedHarnessBinary> {
        self.resolve_harness_binary(provider, BinaryPathChoice::Runtime)
            .await
    }

    /// `resolveCursorBinary`.
    pub async fn resolve_cursor_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Cursor).await
    }

    /// `resolveCodexBinary`.
    pub async fn resolve_codex_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Codex).await
    }

    /// `resolveOpenCodeBinary`.
    pub async fn resolve_open_code_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Opencode).await
    }

    /// `resolveClaudeBinary`.
    pub async fn resolve_claude_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Claude).await
    }

    /// `resolvePiBinary`.
    pub async fn resolve_pi_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Pi).await
    }

    /// `resolveOmpBinary`.
    pub async fn resolve_omp_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Omp).await
    }

    /// `resolveFxBinary`.
    pub async fn resolve_fx_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Fx).await
    }

    /// `resolveGrokBinary`.
    pub async fn resolve_grok_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Grok).await
    }

    /// `resolveHermesBinary`.
    pub async fn resolve_hermes_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Hermes).await
    }

    /// `resolveDroidBinary`.
    pub async fn resolve_droid_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Droid).await
    }

    /// `resolveAntigravityBinary`. The result carries launch `args`.
    pub async fn resolve_antigravity_binary(&self) -> Result<ResolvedHarnessBinary> {
        self.resolve_binary(HarnessId::Antigravity).await
    }

    /// `freeHarnessPort`.
    pub async fn free_harness_port(&self) -> Result<u16> {
        self.inner.backend.free_port().await.map_err(err)
    }

    /// `harnessHttp`.
    pub async fn harness_http(&self, request: HttpRequest) -> Result<HttpResponse> {
        self.inner.backend.http(request).await.map_err(err)
    }

    /// `openHarnessSse`.
    pub async fn open_harness_sse(
        &self,
        session_id: &str,
        url: &str,
        headers: Option<HashMap<String, String>>,
    ) -> Result<()> {
        self.inner.router.own_sse(session_id);
        self.inner
            .backend
            .sse_open(session_id.to_string(), url.to_string(), headers)
            .await
            .map_err(err)
    }

    /// `closeHarnessSse`.
    pub async fn close_harness_sse(&self, session_id: &str) -> Result<()> {
        self.unwatch_sse(session_id);
        self.inner
            .backend
            .sse_close(session_id.to_string())
            .await
            .map_err(err)
    }

    /// `inspectHarnessBinary`.
    pub async fn inspect_harness_binary(
        &self,
        provider: HarnessId,
        binary_path: BinaryPathChoice,
    ) -> Result<HarnessBinaryInspection> {
        let resolved = self
            .resolve_harness_binary(provider, binary_path.clone())
            .await?;
        if provider == HarnessId::Antigravity {
            return Ok(HarnessBinaryInspection {
                path: resolved.path,
                version: Some("ACP server".into()),
                error: None,
            });
        }
        let output = self
            .exec_child(
                &resolved.path,
                vec!["--version".into()],
                None,
                Some(provider),
                binary_path,
            )
            .await;
        Ok(match output {
            Ok(output) => {
                let version = output.trim().to_string();
                if VERSION.is_match(&version) {
                    HarnessBinaryInspection {
                        path: resolved.path,
                        version: Some(version),
                        error: None,
                    }
                } else {
                    HarnessBinaryInspection {
                        path: resolved.path,
                        version: None,
                        error: Some("CLI returned no valid version.".into()),
                    }
                }
            }
            Err(error) => HarnessBinaryInspection {
                path: resolved.path,
                version: None,
                error: Some(error.to_string()),
            },
        })
    }

    /// `updateHarnessCli`: run the CLI's own self-update against the binary
    /// MonoCode uses.
    pub async fn update_harness_cli(&self, provider: HarnessId) -> Result<()> {
        let resolved = self.resolve_binary(provider).await?;
        let binary_path = self.inner.backend.runtime_binary_path(provider);
        self.inner
            .backend
            .update_cli(resolved.path, provider, binary_path)
            .await
            .map_err(err)
    }

    /// `execChild`: one allowed argument list, stdout captured.
    pub async fn exec_child(
        &self,
        command: &str,
        args: Vec<String>,
        cwd: Option<&str>,
        binary_provider: Option<HarnessId>,
        binary_path: BinaryPathChoice,
    ) -> Result<String> {
        let binary_path = match (&binary_path, binary_provider) {
            (BinaryPathChoice::Runtime, Some(provider)) => {
                self.inner.backend.runtime_binary_path(provider)
            }
            (BinaryPathChoice::Runtime, None) => None,
            (BinaryPathChoice::Given(path), _) => path.clone(),
        };
        self.inner
            .backend
            .exec(ExecRequest {
                command: command.to_string(),
                args,
                cwd: cwd.map(str::to_string),
                binary_provider,
                binary_path,
            })
            .await
            .map_err(err)
    }

    /// `homeDir()` on the machine running the children.
    pub async fn home_dir(&self) -> Result<String> {
        self.inner.backend.home_dir().await.map_err(err)
    }
}

static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\.\d+\.\d+").unwrap());

/// Runs a harness CLI's self-update (`monocode_integrations::harness_updates::harness_update`).
pub type CliUpdater =
    Arc<dyn Fn(String, String, Option<String>) -> Result<(), String> + Send + Sync>;

/// Settings for [`HostChildBackend`].
#[derive(Clone, Default)]
pub struct HostChildOptions {
    /// The app data directory, which holds provider account profiles.
    pub data_dir: PathBuf,
    /// The loopback control service, for children that drive the app CLI.
    pub control: Option<Arc<ControlHost>>,
    /// Self-update. Without one, `update_cli` fails.
    pub updater: Option<CliUpdater>,
}

/// [`ChildBackend`] over the local process supervisor. Every call runs on
/// smol's blocking pool, because `harness_write` can block on a wedged child.
#[derive(Clone)]
pub struct HostChildBackend {
    host: HarnessHost,
    options: HostChildOptions,
}

impl HostChildBackend {
    pub fn new(host: HarnessHost, options: HostChildOptions) -> Self {
        Self { host, options }
    }

    pub fn host(&self) -> &HarnessHost {
        &self.host
    }
}

fn blocking<T: Send + 'static>(
    run: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> ChildFuture<T> {
    smol::unblock(run).boxed()
}

fn host_account(account: &ChildAccount) -> Result<host::HarnessAccount, String> {
    serde_json::from_value(serde_json::json!({
        "provider": account.provider.as_str(),
        "id": account.id,
    }))
    .map_err(|error| error.to_string())
}

fn home_dir_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir_path().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = home_dir_path()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

const MAX_TEXT_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// The desktop `read_text_file` command, without the git crate.
fn read_text_file(path: &str) -> Result<String, String> {
    let path = expand_home(path);
    let meta = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err("Not a file".into());
    }
    if meta.len() > MAX_TEXT_FILE_BYTES {
        return Err(format!(
            "File is too large to edit (maximum {} MB).",
            MAX_TEXT_FILE_BYTES / 1024 / 1024
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.contains(&0) {
        return Err("Binary files cannot be edited.".into());
    }
    String::from_utf8(bytes).map_err(|_| "File is not valid UTF-8.".into())
}

fn resolve_default_blocking(provider: HarnessId) -> Result<ResolvedHarnessBinary, String> {
    let plain = |resolved: Result<host::CursorBinary, String>| {
        resolved.map(|binary| ResolvedHarnessBinary {
            path: binary.path,
            args: None,
        })
    };
    match provider {
        HarnessId::Claude => plain(host::harness_resolve_claude()),
        HarnessId::Codex => plain(host::harness_resolve_codex()),
        HarnessId::Cursor => plain(host::harness_resolve_cursor()),
        HarnessId::Grok => plain(host::harness_resolve_grok()),
        HarnessId::Opencode => plain(host::harness_resolve_opencode()),
        HarnessId::Pi => plain(host::harness_resolve_pi()),
        HarnessId::Omp => plain(host::harness_resolve_omp()),
        HarnessId::Fx => plain(host::harness_resolve_fx()),
        HarnessId::Hermes => plain(host::harness_resolve_hermes()),
        HarnessId::Droid => plain(host::harness_resolve_droid()),
        HarnessId::Antigravity => {
            host::harness_resolve_antigravity().map(|binary| ResolvedHarnessBinary {
                path: binary.path,
                args: Some(binary.args),
            })
        }
    }
}

impl ChildBackend for HostChildBackend {
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
        let host = self.host.clone();
        let options = self.options.clone();
        blocking(move || {
            let account = request.account.as_ref().map(host_account).transpose()?;
            host::harness_spawn_with_env(
                &host,
                &options.data_dir,
                options.control.as_deref(),
                request.session_id,
                request.command,
                request.args,
                request.cwd,
                account,
                request.binary_provider.map(|id| id.as_str().to_string()),
                request.binary_path,
                request.environment,
            )
        })
    }

    fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
        let host = self.host.clone();
        blocking(move || host::harness_write(&host, session_id, line))
    }

    fn kill(&self, session_id: String) -> ChildFuture<()> {
        let host = self.host.clone();
        blocking(move || host::harness_kill(&host, session_id))
    }

    fn kill_all(&self) -> ChildFuture<()> {
        let host = self.host.clone();
        blocking(move || host::harness_kill_all(&host))
    }

    fn runtime_binary_path(&self, provider: HarnessId) -> Option<String> {
        self.host.runtime_binary_path(provider.as_str())
    }

    fn resolve_default(&self, provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        blocking(move || resolve_default_blocking(provider))
    }

    fn resolve_configured(
        &self,
        provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        blocking(move || {
            host::harness_resolve_configured(provider.as_str().to_string(), binary_path).map(
                |binary| ResolvedHarnessBinary {
                    path: binary.path,
                    args: binary.args,
                },
            )
        })
    }

    fn exec(&self, request: ExecRequest) -> ChildFuture<String> {
        blocking(move || {
            host::harness_exec(
                request.command,
                request.args,
                request.cwd,
                request.binary_provider.map(|id| id.as_str().to_string()),
                request.binary_path,
            )
        })
    }

    fn free_port(&self) -> ChildFuture<u16> {
        blocking(host::harness_free_port)
    }

    fn http(&self, request: HttpRequest) -> ChildFuture<HttpResponse> {
        blocking(move || {
            host::harness_http(
                request.url,
                request.method,
                request.headers,
                request.body,
                request.timeout_ms.map(|ms| ms.max(0) as u64),
            )
            .map(|response| HttpResponse {
                status: response.status,
                body: response.body,
            })
        })
    }

    fn sse_open(
        &self,
        session_id: String,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        let host = self.host.clone();
        blocking(move || host::harness_sse_open(&host, session_id, url, headers))
    }

    fn sse_close(&self, session_id: String) -> ChildFuture<()> {
        let host = self.host.clone();
        blocking(move || host::harness_sse_close(&host, session_id))
    }

    fn read_text_file(&self, path: String) -> ChildFuture<String> {
        blocking(move || read_text_file(&path))
    }

    fn update_cli(
        &self,
        command: String,
        provider: HarnessId,
        binary_path: Option<String>,
    ) -> ChildFuture<()> {
        let updater = self.options.updater.clone();
        blocking(move || match updater {
            Some(update) => update(command, provider.as_str().to_string(), binary_path),
            None => Err("CLI updates are not available here".into()),
        })
    }

    fn home_dir(&self) -> ChildFuture<String> {
        blocking(|| {
            home_dir_path()
                .map(|path| path.to_string_lossy().into_owned())
                .ok_or_else(|| "Could not find the home directory".to_string())
        })
    }
}

/// True when `path` names an existing directory. Small helper for adapters
/// that check a cwd before spawning.
pub fn is_dir(path: &str) -> bool {
    Path::new(&expand_home(path)).is_dir()
}

#[cfg(test)]
mod tests;
