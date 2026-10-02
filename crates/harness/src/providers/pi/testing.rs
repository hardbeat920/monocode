//! A fake child backend for the Pi and omp tests. It records spawns, writes,
//! and kills, and lets a test answer each written command by pushing stdout
//! lines through the real router, the way the TypeScript tests mocked
//! `core/child`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::FutureExt;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::HarnessId;

use crate::core::catalog::SharedCatalog;
use crate::core::child::{
    ChildBackend, ChildFuture, ChildRouter, Children, ExecRequest, HttpRequest, HttpResponse,
    ResolvedHarnessBinary, SpawnRequest,
};
use crate::core::register::HarnessContext;
use crate::core::registry::{HarnessRegistry, RegistryOptions};
use crate::core::task::{SharedSpawner, SmolSpawner};

use super::deps::Rec;

/// How the fake answers one write.
pub enum WriteReply {
    Ok,
    Fail(String),
    /// Never finish the write.
    Stall,
}

type WriteHandler = Arc<dyn Fn(&Responder, &str, &str) -> WriteReply + Send + Sync>;

/// Pushes stdout lines and exits into the router, as the CLI would.
#[derive(Clone)]
pub struct Responder {
    router: Arc<ChildRouter>,
    pids: Arc<Mutex<HashMap<String, u32>>>,
}

impl Responder {
    /// `frame(sessionId, value)`.
    pub fn frame(&self, session_id: &str, value: Value) {
        self.router.on_stdout(session_id, value.to_string());
    }

    /// `response(sessionId, command, data)`: a successful reply to `command`.
    pub fn respond(&self, session_id: &str, command: &Rec, data: Option<Value>) {
        let mut reply = json!({
            "type": "response",
            "id": command.get("id").cloned().unwrap_or(Value::Null),
            "command": command.get("type").cloned().unwrap_or(Value::Null),
            "success": true,
        });
        if let Some(data) = data {
            reply["data"] = data;
        }
        self.frame(session_id, reply);
    }

    /// A failed reply to `command`.
    pub fn fail(&self, session_id: &str, command: &Rec, error: &str) {
        self.frame(
            session_id,
            json!({
                "type": "response",
                "id": command.get("id").cloned().unwrap_or(Value::Null),
                "command": command.get("type").cloned().unwrap_or(Value::Null),
                "success": false,
                "error": error,
            }),
        );
    }

    /// The child for `session_id` exits.
    pub fn exit(&self, session_id: &str, code: Option<i32>) {
        let pid = self.pids.lock().get(session_id).copied().unwrap_or(0);
        self.router.on_exit(session_id, code, pid);
    }
}

#[derive(Default)]
struct Recorded {
    spawns: Vec<SpawnRequest>,
    writes: Vec<(String, String)>,
    kills: Vec<String>,
}

struct FakeBackend {
    responder: Responder,
    recorded: Arc<Mutex<Recorded>>,
    handler: Arc<Mutex<Option<WriteHandler>>>,
    next_pid: Mutex<u32>,
    home: String,
}

impl ChildBackend for FakeBackend {
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
        let pid = {
            let mut next = self.next_pid.lock();
            *next += 1;
            *next
        };
        self.responder
            .pids
            .lock()
            .insert(request.session_id.clone(), pid);
        self.recorded.lock().spawns.push(request);
        async move { Ok(pid) }.boxed()
    }

    fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
        self.recorded
            .lock()
            .writes
            .push((session_id.clone(), line.clone()));
        let handler = self.handler.lock().clone();
        let reply = match handler {
            Some(handler) => handler(&self.responder, &session_id, &line),
            None => WriteReply::Ok,
        };
        match reply {
            WriteReply::Ok => async { Ok(()) }.boxed(),
            WriteReply::Fail(error) => async move { Err(error) }.boxed(),
            WriteReply::Stall => futures::future::pending().boxed(),
        }
    }

    fn kill(&self, session_id: String) -> ChildFuture<()> {
        self.recorded.lock().kills.push(session_id);
        async { Ok(()) }.boxed()
    }

    fn kill_all(&self) -> ChildFuture<()> {
        async { Ok(()) }.boxed()
    }

    fn runtime_binary_path(&self, _provider: HarnessId) -> Option<String> {
        None
    }

    fn resolve_default(&self, provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        let path = format!("/fake/{provider}");
        async move { Ok(ResolvedHarnessBinary { path, args: None }) }.boxed()
    }

    fn resolve_configured(
        &self,
        _provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        async move {
            Ok(ResolvedHarnessBinary {
                path: binary_path,
                args: None,
            })
        }
        .boxed()
    }

    fn exec(&self, _request: ExecRequest) -> ChildFuture<String> {
        async { Ok(String::new()) }.boxed()
    }

    fn free_port(&self) -> ChildFuture<u16> {
        async { Ok(0) }.boxed()
    }

    fn http(&self, _request: HttpRequest) -> ChildFuture<HttpResponse> {
        async { Err("no http in tests".to_string()) }.boxed()
    }

    fn sse_open(
        &self,
        _session_id: String,
        _url: String,
        _headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        async { Ok(()) }.boxed()
    }

    fn sse_close(&self, _session_id: String) -> ChildFuture<()> {
        async { Ok(()) }.boxed()
    }

    fn read_text_file(&self, _path: String) -> ChildFuture<String> {
        async { Err("no files in tests".to_string()) }.boxed()
    }

    fn update_cli(
        &self,
        _command: String,
        _provider: HarnessId,
        _binary_path: Option<String>,
    ) -> ChildFuture<()> {
        async { Ok(()) }.boxed()
    }

    fn home_dir(&self) -> ChildFuture<String> {
        let home = self.home.clone();
        async move { Ok(home) }.boxed()
    }
}

/// The fake transport and the `Children` handle over it.
pub struct Fake {
    pub children: Children,
    pub responder: Responder,
    recorded: Arc<Mutex<Recorded>>,
    handler: Arc<Mutex<Option<WriteHandler>>>,
}

impl Fake {
    pub fn new() -> Self {
        let router = Arc::new(ChildRouter::new());
        let responder = Responder {
            router: router.clone(),
            pids: Arc::default(),
        };
        let recorded = Arc::new(Mutex::new(Recorded::default()));
        let handler: Arc<Mutex<Option<WriteHandler>>> = Arc::default();
        let backend = FakeBackend {
            responder: responder.clone(),
            recorded: recorded.clone(),
            handler: handler.clone(),
            next_pid: Mutex::new(100),
            home: "/home/test".into(),
        };
        let spawner: SharedSpawner = Arc::new(SmolSpawner);
        let children = Children::new(Arc::new(backend), router, spawner);
        Self {
            children,
            responder,
            recorded,
            handler,
        }
    }

    /// A registration context over this transport.
    pub fn context(&self) -> HarnessContext {
        let spawner: SharedSpawner = Arc::new(SmolSpawner);
        let registry = HarnessRegistry::new(spawner, RegistryOptions::default());
        HarnessContext::new(registry, self.children.clone(), SharedCatalog::new())
    }

    /// Answer each write with `handler(responder, session_id, line)`.
    pub fn on_write(
        &self,
        handler: impl Fn(&Responder, &str, &str) -> WriteReply + Send + Sync + 'static,
    ) {
        *self.handler.lock() = Some(Arc::new(handler));
    }

    /// Every JSON command written so far, with its session id.
    pub fn requests(&self) -> Vec<(String, Rec)> {
        self.recorded
            .lock()
            .writes
            .iter()
            .filter_map(|(session_id, line)| {
                let rec = serde_json::from_str::<Value>(line)
                    .ok()?
                    .as_object()?
                    .clone();
                Some((session_id.clone(), rec))
            })
            .collect()
    }

    /// Commands written to `session_id`.
    pub fn commands(&self, session_id: &str) -> Vec<Rec> {
        self.requests()
            .into_iter()
            .filter(|(id, _)| id == session_id)
            .map(|(_, rec)| rec)
            .collect()
    }

    /// The last command of `kind` written to `session_id`.
    pub fn last_command(&self, session_id: &str, kind: &str) -> Option<Rec> {
        self.commands(session_id)
            .into_iter()
            .rev()
            .find(|rec| rec.get("type").and_then(Value::as_str) == Some(kind))
    }

    pub fn clear_requests(&self) {
        self.recorded.lock().writes.clear();
    }

    pub fn spawns(&self) -> Vec<SpawnRequest> {
        self.recorded.lock().spawns.clone()
    }

    pub fn kills(&self) -> Vec<String> {
        self.recorded.lock().kills.clone()
    }

    /// `frame(sessionId, value)`.
    pub fn frame(&self, session_id: &str, value: Value) {
        self.responder.frame(session_id, value);
    }

    /// `vi.waitFor`: poll until `predicate` holds, or panic after two seconds.
    pub async fn wait_for(&self, predicate: impl Fn() -> bool) {
        wait_for(predicate).await;
    }
}

/// `vi.waitFor`: poll until `predicate` holds, or panic after two seconds.
pub async fn wait_for(predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a condition"
        );
        smol::Timer::after(Duration::from_millis(2)).await;
    }
}

/// Let queued tasks run, standing in for `await Promise.resolve()`.
pub async fn settle() {
    smol::Timer::after(Duration::from_millis(30)).await;
}
