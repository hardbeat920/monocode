//! A fake [`ChildBackend`] for the OpenCode tests. It plays the part of the
//! `vi.mock("../../core/child")` blocks in the TypeScript tests: spawning
//! prints the server's listening line, HTTP calls are recorded and answered
//! by a handler (or by queued one-shot replies, like `mockResolvedValueOnce`),
//! and tests push SSE frames through the router.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use futures::FutureExt;
use futures::channel::oneshot;
use parking_lot::Mutex;
use serde_json::Value;

use monocode_core::harness::HarnessId;

use crate::core::child::{
    ChildBackend, ChildFuture, ChildRouter, Children, ExecRequest, HttpRequest, HttpResponse,
    ResolvedHarnessBinary, SpawnRequest,
};
use crate::core::task::{SharedSpawner, SmolSpawner};

type Handler = Arc<dyn Fn(&HttpRequest) -> (u16, String) + Send + Sync>;

/// A reply a test sends later: status and body.
type LaterReply = oneshot::Receiver<(u16, String)>;

enum Once {
    Reply(u16, String),
    Pending(LaterReply),
}

struct State {
    http_calls: Vec<HttpRequest>,
    handler: Handler,
    once: VecDeque<Once>,
    /// Method, path, and the reply that call waits for.
    deferred: Vec<(String, String, LaterReply)>,
    spawns: Vec<SpawnRequest>,
    kills: Vec<String>,
    sse_opens: Vec<(String, String)>,
    exec_output: String,
}

struct Inner {
    router: Arc<ChildRouter>,
    state: Mutex<State>,
    next_pid: AtomicU32,
    server_line: String,
}

/// The fake host. Clones share one state.
#[derive(Clone)]
pub struct FakeHost {
    inner: Arc<Inner>,
    children: Children,
}

/// The handler `opencodeLive.test.ts` mocks `harnessHttp` with.
pub fn live_test_handler(session_messages: Value) -> Handler {
    Arc::new(move |request: &HttpRequest| {
        let path = path_of(&request.url);
        match (request.method.as_str(), path.as_str()) {
            ("POST", "/session") => (200, r#"{"id":"session_1"}"#.into()),
            ("GET", "/session/session_1") => {
                (200, r#"{"id":"session_1","directory":"/repo"}"#.into())
            }
            ("GET", "/session/session_1/message") => (200, session_messages.to_string()),
            _ => (204, String::new()),
        }
    })
}

/// The URL path, without the query.
pub fn path_of(url: &str) -> String {
    url::Url::parse(url)
        .map(|url| url.path().to_string())
        .unwrap_or_default()
}

impl FakeHost {
    pub fn new() -> Self {
        let router = Arc::new(ChildRouter::new());
        let inner = Arc::new(Inner {
            router: router.clone(),
            state: Mutex::new(State {
                http_calls: Vec::new(),
                handler: live_test_handler(Value::Array(Vec::new())),
                once: VecDeque::new(),
                deferred: Vec::new(),
                spawns: Vec::new(),
                kills: Vec::new(),
                sse_opens: Vec::new(),
                exec_output: "opencode 1.14.19".into(),
            }),
            next_pid: AtomicU32::new(4000),
            server_line: "opencode server listening on http://127.0.0.1:4096".into(),
        });
        let spawner: SharedSpawner = Arc::new(SmolSpawner);
        let backend = Arc::new(FakeBackend {
            inner: inner.clone(),
        });
        let children = Children::new(backend, router, spawner);
        Self { inner, children }
    }

    pub fn children(&self) -> Children {
        self.children.clone()
    }

    pub fn spawner(&self) -> SharedSpawner {
        self.children.spawner().clone()
    }

    /// Answer every HTTP call with `handler`.
    pub fn respond_with(
        &self,
        handler: impl Fn(&HttpRequest) -> (u16, String) + Send + Sync + 'static,
    ) {
        self.inner.state.lock().handler = Arc::new(handler);
    }

    /// `mockResolvedValueOnce`: the next call gets this reply.
    pub fn respond_once(&self, status: u16, body: &str) {
        self.inner
            .state
            .lock()
            .once
            .push_back(Once::Reply(status, body.into()));
    }

    /// `mockImplementationOnce(() => new Promise(...))`: the next call waits
    /// until the returned sender answers.
    pub fn respond_once_later(&self) -> oneshot::Sender<(u16, String)> {
        let (tx, rx) = oneshot::channel();
        self.inner.state.lock().once.push_back(Once::Pending(rx));
        tx
    }

    /// The next `method` call to `path` waits until the returned sender
    /// answers, like a mock that returns an unresolved promise for one route.
    pub fn defer(&self, method: &str, path: &str) -> oneshot::Sender<(u16, String)> {
        let (tx, rx) = oneshot::channel();
        self.inner
            .state
            .lock()
            .deferred
            .push((method.into(), path.into(), rx));
        tx
    }

    pub fn set_exec_output(&self, output: &str) {
        self.inner.state.lock().exec_output = output.into();
    }

    pub fn http_calls(&self) -> Vec<HttpRequest> {
        self.inner.state.lock().http_calls.clone()
    }

    /// Calls whose URL contains `needle`.
    pub fn calls_to(&self, needle: &str) -> Vec<HttpRequest> {
        self.http_calls()
            .into_iter()
            .filter(|call| call.url.contains(needle))
            .collect()
    }

    pub fn spawns(&self) -> Vec<SpawnRequest> {
        self.inner.state.lock().spawns.clone()
    }

    pub fn kills(&self) -> Vec<String> {
        self.inner.state.lock().kills.clone()
    }

    pub fn sse_opens(&self) -> Vec<(String, String)> {
        self.inner.state.lock().sse_opens.clone()
    }

    /// One SSE frame on `stream_id`.
    pub fn sse(&self, stream_id: &str, event: Value) {
        self.inner.router.on_sse(stream_id, event.to_string());
    }

    /// The end of the SSE stream on `stream_id`.
    pub fn sse_end(&self, stream_id: &str, error: Option<&str>) {
        self.inner
            .router
            .on_sse_end(stream_id, error.map(str::to_string));
    }

    /// The server process for `session_id` exited.
    pub fn exit(&self, session_id: &str, code: Option<i32>) {
        let pid = self.inner.next_pid.load(Ordering::SeqCst) - 1;
        self.inner.router.on_exit(session_id, code, pid);
    }
}

/// `waitFor`: poll `predicate` every 5 ms for up to 2 s.
pub async fn wait_for(label: &str, mut predicate: impl FnMut() -> bool) {
    for _ in 0..400 {
        if predicate() {
            return;
        }
        smol::Timer::after(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for {label}");
}

struct FakeBackend {
    inner: Arc<Inner>,
}

fn ready<T: Send + 'static>(value: Result<T, String>) -> ChildFuture<T> {
    async move { value }.boxed()
}

impl ChildBackend for FakeBackend {
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
        let session_id = request.session_id.clone();
        self.inner.state.lock().spawns.push(request);
        let pid = self.inner.next_pid.fetch_add(1, Ordering::SeqCst);
        self.inner
            .router
            .on_stdout(&session_id, self.inner.server_line.clone());
        ready(Ok(pid))
    }

    fn write(&self, _session_id: String, _line: String) -> ChildFuture<()> {
        ready(Ok(()))
    }

    fn kill(&self, session_id: String) -> ChildFuture<()> {
        self.inner.state.lock().kills.push(session_id);
        ready(Ok(()))
    }

    fn kill_all(&self) -> ChildFuture<()> {
        ready(Ok(()))
    }

    fn runtime_binary_path(&self, _provider: HarnessId) -> Option<String> {
        None
    }

    fn resolve_default(&self, _provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        ready(Ok(ResolvedHarnessBinary {
            path: "/fake/opencode".into(),
            args: None,
        }))
    }

    fn resolve_configured(
        &self,
        _provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        ready(Ok(ResolvedHarnessBinary {
            path: binary_path,
            args: None,
        }))
    }

    fn exec(&self, _request: ExecRequest) -> ChildFuture<String> {
        ready(Ok(self.inner.state.lock().exec_output.clone()))
    }

    fn free_port(&self) -> ChildFuture<u16> {
        ready(Ok(4096))
    }

    fn http(&self, request: HttpRequest) -> ChildFuture<HttpResponse> {
        let (once, deferred, handler) = {
            let mut state = self.inner.state.lock();
            state.http_calls.push(request.clone());
            let path = path_of(&request.url);
            let deferred = state
                .deferred
                .iter()
                .position(|(method, route, _)| *method == request.method && *route == path)
                .map(|index| state.deferred.remove(index).2);
            let once = match deferred {
                Some(_) => None,
                None => state.once.pop_front(),
            };
            (once, deferred, state.handler.clone())
        };
        async move {
            let (status, body) = match (deferred, once) {
                (Some(reply), _) => reply.await.map_err(|_| "dropped".to_string())?,
                (None, Some(Once::Reply(status, body))) => (status, body),
                (None, Some(Once::Pending(reply))) => {
                    reply.await.map_err(|_| "dropped".to_string())?
                }
                (None, None) => handler(&request),
            };
            Ok(HttpResponse { status, body })
        }
        .boxed()
    }

    fn sse_open(
        &self,
        session_id: String,
        url: String,
        _headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        self.inner.state.lock().sse_opens.push((session_id, url));
        ready(Ok(()))
    }

    fn sse_close(&self, _session_id: String) -> ChildFuture<()> {
        ready(Ok(()))
    }

    fn read_text_file(&self, _path: String) -> ChildFuture<String> {
        ready(Err("not in tests".into()))
    }

    fn update_cli(
        &self,
        _command: String,
        _provider: HarnessId,
        _binary_path: Option<String>,
    ) -> ChildFuture<()> {
        ready(Ok(()))
    }

    fn home_dir(&self) -> ChildFuture<String> {
        ready(Ok("/home/test".into()))
    }
}
