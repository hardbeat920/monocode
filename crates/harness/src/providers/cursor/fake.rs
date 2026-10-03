//! A scripted child backend for the provider tests, the Rust form of the
//! `vi.mock("../../core/child", ...)` blocks in cursorLive.test.ts,
//! cursorText.test.ts, and antigravityLive.test.ts.
//!
//! Writes are recorded as parsed JSON. A responder decides which lines to
//! send back for each write; the backend delivers them on a later tick
//! through the real [`ChildRouter`], as `queueMicrotask` did. Antigravity's
//! tests include this file with `#[path]`.
#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use futures::FutureExt;
use monocode_core::harness::HarnessId;
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::core::child::{
    ChildBackend, ChildFuture, ChildRouter, Children, ExecRequest, HttpRequest, HttpResponse,
    ResolvedHarnessBinary, SpawnRequest,
};
use crate::core::task::SmolSpawner;

/// One recorded write.
#[derive(Debug, Clone)]
pub struct Sent {
    pub session: String,
    pub message: Value,
}

/// Decides the lines to send back for one write: `(session, message) -> lines`.
pub type Responder = Arc<dyn Fn(&Wire, &str, &Value) -> Vec<Value> + Send + Sync>;

/// What a held spawn does once the test releases it.
pub type SpawnGate = async_channel::Receiver<Result<(), String>>;

/// The fake's recorded state and controls.
pub struct Wire {
    pub router: Arc<ChildRouter>,
    pub sent: Mutex<Vec<Sent>>,
    pub spawns: Mutex<Vec<SpawnRequest>>,
    pub kills: Mutex<Vec<String>>,
    pids: Mutex<HashMap<String, u32>>,
    next_pid: AtomicU32,
    pub responder: Mutex<Option<Responder>>,
    /// Every write hangs, like a child that stopped draining stdin.
    pub block_writes: AtomicBool,
    /// Spawns wait on these, in order, when any are queued.
    pub spawn_gates: Mutex<VecDeque<SpawnGate>>,
    /// Binary resolution waits on this when set.
    pub resolve_gate: Mutex<Option<async_channel::Receiver<()>>>,
    pub resolve_waiting: AtomicU32,
    pub resolved: Mutex<ResolvedHarnessBinary>,
    pub home: String,
    pub exec_output: Mutex<String>,
}

impl Wire {
    /// Send one stdout line from a child.
    pub fn push(&self, session: &str, message: Value) {
        self.router.on_stdout(session, message.to_string());
    }

    /// Answer request `id` with `result`.
    pub fn reply(&self, session: &str, id: &Value, result: Value) {
        self.push(
            session,
            json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        );
    }

    /// A notification from the child.
    pub fn notify(&self, session: &str, method: &str, params: Value) {
        self.push(
            session,
            json!({ "jsonrpc": "2.0", "method": method, "params": params }),
        );
    }

    /// A request from the child.
    pub fn request(&self, session: &str, id: i64, method: &str, params: Value) {
        self.push(
            session,
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        );
    }

    /// The child for `session` exits with `code`.
    pub fn exit(&self, session: &str, code: i32) {
        let pid = self.pids.lock().get(session).copied().unwrap_or(0);
        self.router.on_exit(session, Some(code), pid);
    }

    pub fn messages(&self) -> Vec<Value> {
        self.sent
            .lock()
            .iter()
            .map(|sent| sent.message.clone())
            .collect()
    }

    pub fn methods(&self) -> Vec<String> {
        self.messages()
            .iter()
            .filter_map(|message| {
                message
                    .get("method")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }

    pub fn count(&self, method: &str) -> usize {
        self.methods().iter().filter(|m| *m == method).count()
    }

    /// The first outbound message with this method.
    pub fn outbound(&self, method: &str) -> Option<Value> {
        self.messages()
            .into_iter()
            .find(|message| message.get("method").and_then(Value::as_str) == Some(method))
    }

    /// The last outbound message with this method.
    pub fn last_outbound(&self, method: &str) -> Option<Sent> {
        self.sent
            .lock()
            .iter()
            .rev()
            .find(|sent| sent.message.get("method").and_then(Value::as_str) == Some(method))
            .cloned()
    }

    /// The last response we sent for an inbound request id.
    pub fn response(&self, id: i64) -> Option<Value> {
        self.messages()
            .into_iter()
            .rev()
            .find(|message| {
                message.get("id") == Some(&json!(id)) && message.get("result").is_some()
            })
            .and_then(|message| message.get("result").cloned())
    }

    pub fn clear_sent(&self) {
        self.sent.lock().clear();
    }

    pub fn spawn_count(&self) -> usize {
        self.spawns.lock().len()
    }

    pub fn spawned_ids(&self) -> Vec<String> {
        self.spawns
            .lock()
            .iter()
            .map(|spawn| spawn.session_id.clone())
            .collect()
    }

    pub fn killed(&self, session: &str) -> bool {
        self.kills.lock().iter().any(|kill| kill == session)
    }

    pub fn set_responder(
        &self,
        responder: impl Fn(&Wire, &str, &Value) -> Vec<Value> + Send + Sync + 'static,
    ) {
        *self.responder.lock() = Some(Arc::new(responder));
    }

    /// Hold the next spawn until the returned sender sends.
    pub fn hold_next_spawn(&self) -> async_channel::Sender<Result<(), String>> {
        let (tx, rx) = async_channel::bounded(1);
        self.spawn_gates.lock().push_back(rx);
        tx
    }
}

struct FakeBackend {
    wire: Arc<Wire>,
}

fn done<T: Send + 'static>(value: T) -> ChildFuture<T> {
    async move { Ok(value) }.boxed()
}

impl ChildBackend for FakeBackend {
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
        let wire = self.wire.clone();
        let gate = wire.spawn_gates.lock().pop_front();
        async move {
            wire.spawns.lock().push(request.clone());
            if let Some(gate) = gate {
                match gate.recv().await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => return Err(error),
                    Err(_) => return Err("spawn gate dropped".into()),
                }
            }
            let pid = wire.next_pid.fetch_add(1, Ordering::SeqCst);
            wire.pids.lock().insert(request.session_id.clone(), pid);
            Ok(pid)
        }
        .boxed()
    }

    fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
        let wire = self.wire.clone();
        let message: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        wire.sent.lock().push(Sent {
            session: session_id.clone(),
            message: message.clone(),
        });
        if wire.block_writes.load(Ordering::SeqCst) {
            return futures::future::pending().boxed();
        }
        let responder = wire.responder.lock().clone();
        if let Some(responder) = responder {
            let lines = responder(&wire, &session_id, &message);
            if !lines.is_empty() {
                let wire = wire.clone();
                smol::spawn(async move {
                    smol::future::yield_now().await;
                    for line in lines {
                        wire.push(&session_id, line);
                    }
                })
                .detach();
            }
        }
        done(())
    }

    fn kill(&self, session_id: String) -> ChildFuture<()> {
        self.wire.kills.lock().push(session_id);
        done(())
    }

    fn kill_all(&self) -> ChildFuture<()> {
        done(())
    }

    fn runtime_binary_path(&self, _provider: HarnessId) -> Option<String> {
        None
    }

    fn resolve_default(&self, _provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        let wire = self.wire.clone();
        async move {
            let gate = wire.resolve_gate.lock().clone();
            if let Some(gate) = gate {
                wire.resolve_waiting.fetch_add(1, Ordering::SeqCst);
                let _ = gate.recv().await;
            }
            Ok(wire.resolved.lock().clone())
        }
        .boxed()
    }

    fn resolve_configured(
        &self,
        _provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        done(ResolvedHarnessBinary {
            path: binary_path,
            args: None,
        })
    }

    fn exec(&self, _request: ExecRequest) -> ChildFuture<String> {
        done(self.wire.exec_output.lock().clone())
    }

    fn free_port(&self) -> ChildFuture<u16> {
        done(4100)
    }

    fn http(&self, _request: HttpRequest) -> ChildFuture<HttpResponse> {
        done(HttpResponse::default())
    }

    fn sse_open(
        &self,
        _: String,
        _: String,
        _: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        done(())
    }

    fn sse_close(&self, _: String) -> ChildFuture<()> {
        done(())
    }

    fn read_text_file(&self, path: String) -> ChildFuture<String> {
        async move { Err(format!("no file {path}")) }.boxed()
    }

    fn update_cli(&self, _: String, _: HarnessId, _: Option<String>) -> ChildFuture<()> {
        done(())
    }

    fn home_dir(&self) -> ChildFuture<String> {
        done(self.wire.home.clone())
    }
}

/// Children over a fresh fake, resolving every binary to `path` with `args`.
pub fn fake_children(path: &str, args: Option<Vec<String>>) -> (Children, Arc<Wire>) {
    let router = Arc::new(ChildRouter::new());
    let wire = Arc::new(Wire {
        router: router.clone(),
        sent: Mutex::new(Vec::new()),
        spawns: Mutex::new(Vec::new()),
        kills: Mutex::new(Vec::new()),
        pids: Mutex::new(HashMap::new()),
        next_pid: AtomicU32::new(100),
        responder: Mutex::new(None),
        block_writes: AtomicBool::new(false),
        spawn_gates: Mutex::new(VecDeque::new()),
        resolve_gate: Mutex::new(None),
        resolve_waiting: AtomicU32::new(0),
        resolved: Mutex::new(ResolvedHarnessBinary {
            path: path.to_string(),
            args,
        }),
        home: "/home/test".into(),
        exec_output: Mutex::new(String::new()),
    });
    let children = Children::new(
        Arc::new(FakeBackend { wire: wire.clone() }),
        router,
        Arc::new(SmolSpawner),
    );
    (children, wire)
}

/// `waitFor`: poll until `predicate` holds, or panic with `label`.
pub async fn wait_for(mut predicate: impl FnMut() -> bool, label: &str) {
    for _ in 0..400 {
        if predicate() {
            return;
        }
        smol::Timer::after(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for {label}");
}

/// Let queued deliveries and spawned handlers run.
pub async fn flush() {
    smol::Timer::after(Duration::from_millis(30)).await;
}
