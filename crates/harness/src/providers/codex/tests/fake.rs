//! A scripted `codex app-server` for adapter tests: the stand-in for the
//! `vi.mock("../../core/child")` modules in the TypeScript tests. It records
//! every line the adapter writes, and the test answers by pushing lines back
//! through the child router.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use futures::FutureExt;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{HarnessEvent, HarnessSessionInput, SendTurnInput};

use crate::core::catalog::SharedCatalog;
use crate::core::child::*;
use crate::core::register::HarnessContext;
use crate::core::registry::{AcceptedHook, EventSink, HarnessRegistry, RegistryOptions};
use crate::core::task::{BoxFuture, SmolSpawner, sleep};

use super::super::adapter::CodexAdapter;
use super::super::git::GitContexts;
use super::super::session::{GeneratedImageAsset, GeneratedImages, SessionOptions};

type WriteHook = Box<dyn Fn(&Wire, &str, &Value) + Send + Sync>;

/// The parts of the harness a write hook may use.
pub struct Wire {
    router: Arc<ChildRouter>,
}

impl Wire {
    /// Push one stdout line to a session.
    pub fn push(&self, session_id: &str, value: Value) {
        self.router.on_stdout(session_id, value.to_string());
    }
}

#[derive(Default)]
pub struct FakeChild {
    pub sent: Mutex<Vec<(String, Value)>>,
    pub spawns: Mutex<Vec<SpawnRequest>>,
    pub kills: Mutex<Vec<String>>,
    /// The next write fails with this message (`writeChild.mockRejectedValueOnce`).
    pub fail_next_write: Mutex<Option<String>>,
    pub on_write: Mutex<Option<WriteHook>>,
    pub router: Mutex<Option<Arc<ChildRouter>>>,
}

impl ChildBackend for FakeChild {
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
        self.spawns.lock().push(request);
        async { Ok(7) }.boxed()
    }

    fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
        if let Some(message) = self.fail_next_write.lock().take() {
            return async move { Err(message) }.boxed();
        }
        let value: Value = serde_json::from_str(&line).expect("adapter writes JSON");
        self.sent.lock().push((session_id.clone(), value.clone()));
        let router = self.router.lock().clone();
        if let (Some(hook), Some(router)) = (self.on_write.lock().as_ref(), router) {
            hook(&Wire { router }, &session_id, &value);
        }
        async { Ok(()) }.boxed()
    }

    fn kill(&self, session_id: String) -> ChildFuture<()> {
        self.kills.lock().push(session_id);
        async { Ok(()) }.boxed()
    }

    fn kill_all(&self) -> ChildFuture<()> {
        async { Ok(()) }.boxed()
    }

    fn runtime_binary_path(&self, _provider: HarnessId) -> Option<String> {
        None
    }

    fn resolve_default(&self, _provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        async {
            Ok(ResolvedHarnessBinary {
                path: "/fake/codex".into(),
                args: None,
            })
        }
        .boxed()
    }

    fn resolve_configured(
        &self,
        _provider: HarnessId,
        path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        async move { Ok(ResolvedHarnessBinary { path, args: None }) }.boxed()
    }

    fn exec(&self, _request: ExecRequest) -> ChildFuture<String> {
        async { Err("unsupported".into()) }.boxed()
    }

    fn free_port(&self) -> ChildFuture<u16> {
        async { Err("unsupported".into()) }.boxed()
    }

    fn http(&self, _request: HttpRequest) -> ChildFuture<HttpResponse> {
        async { Err("unsupported".into()) }.boxed()
    }

    fn sse_open(
        &self,
        _session_id: String,
        _url: String,
        _headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        async { Err("unsupported".into()) }.boxed()
    }

    fn sse_close(&self, _session_id: String) -> ChildFuture<()> {
        async { Ok(()) }.boxed()
    }

    fn read_text_file(&self, _path: String) -> ChildFuture<String> {
        async { Err("unsupported".into()) }.boxed()
    }

    fn update_cli(
        &self,
        _command: String,
        _provider: HarnessId,
        _path: Option<String>,
    ) -> ChildFuture<()> {
        async { Err("unsupported".into()) }.boxed()
    }

    fn home_dir(&self) -> ChildFuture<String> {
        async { Ok("/home/test".into()) }.boxed()
    }
}

/// `saveGeneratedImage` and `deleteGeneratedImages` mocks. A gate holds the
/// next save open until the test releases it.
#[derive(Default)]
pub struct FakeImages {
    pub saved: Mutex<Vec<(String, String)>>,
    pub deleted: Mutex<Vec<Vec<String>>>,
    pub gate: Mutex<Option<async_channel::Receiver<()>>>,
}

pub const IMAGE_PATH: &str = "/app-data/generated-images/image.png";

impl FakeImages {
    /// Hold the next save until the returned sender is dropped or sends.
    pub fn hold_next(&self) -> async_channel::Sender<()> {
        let (release, gate) = async_channel::bounded(1);
        *self.gate.lock() = Some(gate);
        release
    }
}

impl GeneratedImages for FakeImages {
    fn save(
        &self,
        data: String,
        name: String,
    ) -> BoxFuture<'static, Result<GeneratedImageAsset, String>> {
        self.saved.lock().push((data, name));
        let gate = self.gate.lock().take();
        async move {
            if let Some(gate) = gate {
                let _ = gate.recv().await;
            }
            Ok(GeneratedImageAsset {
                path: IMAGE_PATH.into(),
                mime_type: "image/png".into(),
                size: 8,
            })
        }
        .boxed()
    }

    fn delete(&self, paths: Vec<String>) -> BoxFuture<'static, Result<(), String>> {
        self.deleted.lock().push(paths);
        async { Ok(()) }.boxed()
    }
}

/// Events an adapter reported, in order.
#[derive(Clone, Default)]
pub struct Events(pub Arc<Mutex<Vec<HarnessEvent>>>);

impl Events {
    pub fn sink(&self) -> EventSink {
        let events = self.0.clone();
        Arc::new(move |event| events.lock().push(event))
    }

    pub fn all(&self) -> Vec<HarnessEvent> {
        self.0.lock().clone()
    }

    pub fn json(&self) -> Vec<Value> {
        self.all()
            .iter()
            .map(|event| serde_json::to_value(event).unwrap())
            .collect()
    }

    pub fn of_type(&self, kind: &str) -> Vec<Value> {
        self.json()
            .into_iter()
            .filter(|event| event["type"] == kind)
            .collect()
    }

    pub fn has(&self, kind: &str) -> bool {
        !self.of_type(kind).is_empty()
    }

    /// Events other than `turn.started` (`withoutTurnIdentity`).
    pub fn without_turn_identity(&self) -> Vec<Value> {
        self.json()
            .into_iter()
            .filter(|event| event["type"] != "turn.started")
            .collect()
    }

    pub fn message_text(&self) -> Vec<String> {
        self.texts("message.delta")
    }

    pub fn texts(&self, kind: &str) -> Vec<String> {
        self.of_type(kind)
            .iter()
            .map(|event| event["text"].as_str().unwrap_or("").to_string())
            .collect()
    }

    /// `events.reduce(applyHarnessEvent, newSession("codex", "/repo"))`.
    pub fn reduce(&self) -> monocode_core::session::Session {
        self.reduce_onto(monocode_core::session::Session::blank(
            "codex-test",
            HarnessId::Codex,
            "codex:gpt-5.4",
            "/repo",
        ))
    }

    /// Apply every event to `session`, one at a time.
    pub fn reduce_onto(
        &self,
        mut session: monocode_core::session::Session,
    ) -> monocode_core::session::Session {
        for event in self.all() {
            session = monocode_core::reducer::apply_harness_event(&session, &event);
        }
        session
    }

    /// The transcript blocks as JSON.
    pub fn blocks(&self) -> Value {
        serde_json::to_value(&self.reduce().blocks).unwrap()
    }
}

pub struct Harness {
    pub adapter: Arc<CodexAdapter>,
    pub fake: Arc<FakeChild>,
    pub images: Arc<FakeImages>,
    pub catalog: SharedCatalog,
    pub now: Arc<AtomicI64>,
}

/// Options for [`Harness::with`].
pub struct HarnessOptions {
    pub question_auto_resolve: Duration,
    pub git: Option<Arc<dyn GitContexts>>,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        Self {
            question_auto_resolve: Duration::from_millis(120_000),
            git: None,
        }
    }
}

impl Harness {
    pub fn new() -> Self {
        Self::with(HarnessOptions::default())
    }

    pub fn with(options: HarnessOptions) -> Self {
        let fake = Arc::new(FakeChild::default());
        let router = Arc::new(ChildRouter::new());
        *fake.router.lock() = Some(router.clone());
        let spawner: crate::core::task::SharedSpawner = Arc::new(SmolSpawner);
        let children = Children::new(fake.clone(), router, spawner.clone());
        let registry = HarnessRegistry::new(spawner, RegistryOptions::default());
        let catalog = SharedCatalog::new();
        let ctx = HarnessContext::new(registry, children, catalog.clone());
        let images = Arc::new(FakeImages::default());
        let now = Arc::new(AtomicI64::new(1_700_000_000_000));
        let clock_now = now.clone();
        let adapter = Arc::new(CodexAdapter::with_options(
            &ctx,
            SessionOptions {
                images: Some(images.clone()),
                clock: Arc::new(move || clock_now.load(Ordering::SeqCst)),
                question_auto_resolve: options.question_auto_resolve,
            },
            options.git,
        ));
        Self {
            adapter,
            fake,
            images,
            catalog,
            now,
        }
    }

    pub fn set_now(&self, millis: i64) {
        self.now.store(millis, Ordering::SeqCst);
    }

    pub fn now(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }

    /// Every message the adapter wrote, in order (`parse()`).
    pub fn sent(&self) -> Vec<Value> {
        self.fake
            .sent
            .lock()
            .iter()
            .map(|(_, value)| value.clone())
            .collect()
    }

    pub fn clear_sent(&self) {
        self.fake.sent.lock().clear();
    }

    pub fn find_method(&self, method: &str) -> Option<Value> {
        self.sent()
            .into_iter()
            .find(|message| message["method"] == method)
    }

    pub fn methods(&self, method: &str) -> Vec<Value> {
        self.sent()
            .into_iter()
            .filter(|message| message["method"] == method)
            .collect()
    }

    /// The reply the adapter sent to a server request.
    pub fn reply_to(&self, id: Value) -> Option<Value> {
        self.sent()
            .into_iter()
            .find(|message| message["id"] == id && message.get("method").is_none())
    }

    pub fn replies_to(&self, id: Value) -> usize {
        self.sent()
            .into_iter()
            .filter(|message| message["id"] == id && message.get("method").is_none())
            .count()
    }

    fn router(&self) -> Arc<ChildRouter> {
        self.fake.router.lock().clone().unwrap()
    }

    /// `onLine(JSON.stringify(value))` for a session.
    pub fn push(&self, session_id: &str, value: Value) {
        self.router().on_stdout(session_id, value.to_string());
    }

    pub fn reply(&self, session_id: &str, id: &Value, result: Value) {
        self.push(session_id, json!({ "id": id, "result": result }));
    }

    pub fn notify(&self, session_id: &str, method: &str, params: Value) {
        self.push(session_id, json!({ "method": method, "params": params }));
    }

    pub fn request(&self, session_id: &str, id: Value, method: &str, params: Value) {
        self.push(
            session_id,
            json!({ "id": id, "method": method, "params": params }),
        );
    }
}

/// Poll until `pred` holds, or panic after about five seconds.
pub async fn wait_for(label: &str, pred: impl Fn() -> bool) {
    for _ in 0..1000 {
        if pred() {
            return;
        }
        sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting for {label}");
}

/// Let spawned tasks run: the stand-in for `await Promise.resolve()`.
pub async fn settle() {
    sleep(Duration::from_millis(30)).await;
}

/// A step to run before the thread request is answered.
pub type BeforeReply = Box<dyn Fn(&Harness) + Send + Sync>;

/// `startTurn` options.
#[derive(Default)]
pub struct StartTurn {
    pub runtime_mode: Option<RuntimeMode>,
    pub intent: Option<monocode_core::block::TurnIntent>,
    pub resume: bool,
    pub provider_account_id: Option<String>,
    pub resume_provider_account_id: Option<String>,
    pub expect_resume: Option<bool>,
    pub controls_agents: Option<bool>,
    pub on_accepted: Option<AcceptedHook>,
    /// Runs after the thread request is written, before it is answered.
    pub before_thread_reply: Option<BeforeReply>,
}

pub struct Started {
    pub events: Events,
    pub turn: smol::Task<anyhow::Result<()>>,
}

pub fn session_input(session_id: &str, runtime_mode: RuntimeMode) -> HarnessSessionInput {
    HarnessSessionInput {
        session_id: session_id.into(),
        cwd: "/repo".into(),
        model: "codex:gpt-5.4".into(),
        model_settings: Some(Default::default()),
        provider_account_id: None,
        runtime_mode,
        intent: None,
        controls_agents: None,
        app_access: None,
    }
}

/// Send a turn without scripting the app-server.
pub fn send(
    h: &Harness,
    input: SendTurnInput,
    events: &Events,
    on_accepted: Option<AcceptedHook>,
) -> smol::Task<anyhow::Result<()>> {
    let adapter = h.adapter.clone();
    let sink = events.sink();
    smol::spawn(async move { adapter.sessions().send_turn(input, sink, on_accepted).await })
}

/// `startTurn`: send "summarize the changelog" and answer initialize, the
/// thread request, and turn/start, then report turn/started.
pub async fn start_turn(h: &Harness, session_id: &str, options: StartTurn) -> Started {
    let events = Events::default();
    if options.resume {
        h.adapter.sessions().bind_session(
            session_id,
            "thr_1",
            "/repo",
            options.resume_provider_account_id.as_deref(),
        );
    }
    let mut session = session_input(
        session_id,
        options.runtime_mode.unwrap_or(RuntimeMode::Supervised),
    );
    session.provider_account_id = options.provider_account_id.clone();
    session.controls_agents = options.controls_agents;
    session.intent = options.intent;
    let input = SendTurnInput {
        session,
        text: "summarize the changelog".into(),
        attachments: Some(Vec::new()),
    };
    let initialize_count = h.methods("initialize").len();
    let turn = send(h, input, &events, options.on_accepted.clone());

    wait_for("initialize", || {
        h.methods("initialize").len() > initialize_count
    })
    .await;
    let initialize = h.methods("initialize").last().cloned().unwrap();
    h.reply(session_id, &initialize["id"], json!({}));

    let thread_method = if options.expect_resume.unwrap_or(options.resume) {
        "thread/resume"
    } else {
        "thread/start"
    };
    wait_for(thread_method, || h.find_method(thread_method).is_some()).await;
    if let Some(before) = &options.before_thread_reply {
        before(h);
        settle().await;
    }
    let thread = h.find_method(thread_method).unwrap();
    h.reply(
        session_id,
        &thread["id"],
        json!({ "thread": { "id": "thr_1" } }),
    );

    wait_for("turn/start", || h.find_method("turn/start").is_some()).await;
    let turn_start = h.find_method("turn/start").unwrap();
    h.reply(
        session_id,
        &turn_start["id"],
        json!({ "turn": { "id": "turn_1", "status": "inProgress" } }),
    );
    h.notify(
        session_id,
        "turn/started",
        json!({ "turn": { "id": "turn_1", "status": "inProgress" } }),
    );
    wait_for("turn.started", || events.has("turn.started")).await;
    Started { events, turn }
}

pub fn complete_turn(h: &Harness, session_id: &str, turn_id: &str) {
    h.notify(
        session_id,
        "turn/completed",
        json!({ "turn": { "id": turn_id, "status": "completed" } }),
    );
}
