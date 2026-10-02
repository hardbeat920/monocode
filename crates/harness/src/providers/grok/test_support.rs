#![allow(dead_code)]

//! A scripted ACP peer over `core::testing::Fake`, for the ports of the
//! `*Live.test.ts` and `grokText.test.ts` suites. Those suites mock
//! `core/child` and record every line the adapter writes; this does the same
//! through the real `Children` and router.

use std::collections::HashMap;
use std::sync::Arc;

use futures::FutureExt;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::block::ModelSettings;
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{HarnessEvent, HarnessSessionInput, SendTurnInput};

use crate::core::catalog::SharedCatalog;
use crate::core::child::{
    ChildBackend, ChildFuture, ChildRouter, Children, ExecRequest, HttpRequest, HttpResponse,
    ResolvedHarnessBinary, SpawnRequest,
};
use crate::core::register::HarnessContext;
use crate::core::registry::{EventSink, HarnessRegistry, RegistryOptions};
use crate::core::task::{SharedSpawner, SmolSpawner};
use crate::core::testing::{Call, Fake, children};

/// The pid `Fake::spawn` hands out.
const FAKE_PID: u32 = 7;

pub struct Peer {
    pub ctx: HarnessContext,
    pub fake: Arc<Fake>,
    /// Calls before this index are hidden, like `sent.length = 0`.
    mark: Mutex<usize>,
}

impl Peer {
    pub fn new() -> Self {
        Self::with_fake(Fake::default())
    }

    pub fn with_fake(fake: Fake) -> Self {
        let registry = HarnessRegistry::new(Arc::new(SmolSpawner), RegistryOptions::default());
        let (children, fake) = children(fake);
        let ctx = HarnessContext::new(registry, children, SharedCatalog::new());
        Self {
            ctx,
            fake,
            mark: Mutex::new(0),
        }
    }

    /// A peer whose `read_harness_text_file` reads from the returned map.
    pub fn with_files() -> (Self, TextFiles) {
        let fake = Arc::new(Fake::default());
        let files = TextFiles::default();
        let backend = FileBackend {
            fake: fake.clone(),
            files: files.clone(),
        };
        let spawner: SharedSpawner = Arc::new(SmolSpawner);
        let children = Children::new(
            Arc::new(backend),
            Arc::new(ChildRouter::new()),
            spawner.clone(),
        );
        let registry = HarnessRegistry::new(spawner, RegistryOptions::default());
        let ctx = HarnessContext::new(registry, children, SharedCatalog::new());
        (
            Self {
                ctx,
                fake,
                mark: Mutex::new(0),
            },
            files,
        )
    }

    /// Every call since the last [`Peer::clear`].
    pub fn calls(&self) -> Vec<Call> {
        let calls = self.fake.calls();
        let mark = (*self.mark.lock()).min(calls.len());
        calls[mark..].to_vec()
    }

    /// `sent.length = 0`.
    pub fn clear(&self) {
        *self.mark.lock() = self.fake.calls().len();
    }

    /// Every JSON line written to any child since the last clear.
    pub fn sent(&self) -> Vec<Value> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Write(_, line) => serde_json::from_str(&line).ok(),
                _ => None,
            })
            .collect()
    }

    /// The `(path, args)` of each spawn since the last clear.
    pub fn spawned(&self) -> Vec<(String, Vec<String>)> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Spawn(request) => Some((request.command, request.args)),
                _ => None,
            })
            .collect()
    }

    pub fn find(&self, method: &str) -> Option<Value> {
        self.sent()
            .into_iter()
            .find(|message| message["method"] == method)
    }

    pub fn count(&self, method: &str) -> usize {
        self.sent()
            .iter()
            .filter(|message| message["method"] == method)
            .count()
    }

    /// One stdout line from the child of `session_id`.
    pub fn line(&self, session_id: &str, message: Value) {
        self.ctx
            .children
            .router()
            .on_stdout(session_id, message.to_string());
    }

    pub fn stderr(&self, session_id: &str, line: &str) {
        self.ctx
            .children
            .router()
            .on_stderr(session_id, line.to_string());
    }

    pub fn exit(&self, session_id: &str, code: i32) {
        self.ctx
            .children
            .router()
            .on_exit(session_id, Some(code), FAKE_PID);
    }

    pub fn reply(&self, session_id: &str, id: &Value, result: Value) {
        self.line(
            session_id,
            json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        );
    }

    pub fn fail(&self, session_id: &str, id: &Value, error: Value) {
        self.line(
            session_id,
            json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        );
    }

    pub fn notify(&self, session_id: &str, method: &str, params: Value) {
        self.line(
            session_id,
            json!({ "jsonrpc": "2.0", "method": method, "params": params }),
        );
    }

    /// `waitFor`: poll until `predicate` holds, or fail with what was sent.
    pub async fn wait_for(&self, label: &str, predicate: impl Fn(&Peer) -> bool) {
        for _ in 0..600 {
            if predicate(self) {
                return;
            }
            smol::Timer::after(std::time::Duration::from_millis(5)).await;
        }
        let sent: Vec<String> = self
            .sent()
            .iter()
            .map(|message| match message["method"].as_str() {
                Some(method) => method.to_string(),
                None => format!("reply:{}", message["id"]),
            })
            .collect();
        panic!("timed out waiting for {label}; sent={sent:?}");
    }

    /// The first request for `method` that matches, once it is sent.
    pub async fn next(&self, method: &str, matches: impl Fn(&Value) -> bool) -> Value {
        let found = |peer: &Peer| {
            peer.sent()
                .into_iter()
                .find(|message| message["method"] == method && matches(message))
        };
        self.wait_for(method, |peer| found(peer).is_some()).await;
        found(self).unwrap()
    }

    /// Wait for `method`, then answer it.
    pub async fn answer(&self, session_id: &str, method: &str, result: Value) -> Value {
        let request = self.next(method, |_| true).await;
        self.reply(session_id, &request["id"], result);
        request
    }
}

/// Events collected from an [`EventSink`].
#[derive(Clone, Default)]
pub struct Events(Arc<Mutex<Vec<HarnessEvent>>>);

impl Events {
    pub fn sink(&self) -> EventSink {
        let events = self.0.clone();
        Arc::new(move |event| events.lock().push(event))
    }

    pub fn all(&self) -> Vec<HarnessEvent> {
        self.0.lock().clone()
    }

    pub fn any(&self, check: impl Fn(&HarnessEvent) -> bool) -> bool {
        self.0.lock().iter().any(check)
    }
}

/// A send-turn input with the fields the live suites set.
pub fn turn(
    session_id: &str,
    model: &str,
    runtime_mode: RuntimeMode,
    text: &str,
    model_settings: Option<ModelSettings>,
) -> SendTurnInput {
    SendTurnInput {
        session: session(session_id, model, runtime_mode, model_settings),
        text: text.into(),
        attachments: Some(Vec::new()),
    }
}

pub fn session(
    session_id: &str,
    model: &str,
    runtime_mode: RuntimeMode,
    model_settings: Option<ModelSettings>,
) -> HarnessSessionInput {
    HarnessSessionInput {
        session_id: session_id.into(),
        cwd: "/repo".into(),
        model: model.into(),
        model_settings,
        provider_account_id: None,
        runtime_mode,
        intent: None,
        controls_agents: None,
        app_access: None,
    }
}

pub fn settings(pairs: &[(&str, &str)]) -> ModelSettings {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Fakes resolve every binary to `/resolved` unless told otherwise.
pub fn fake_resolving(provider: HarnessId, path: &str) -> Fake {
    let mut fake = Fake::default();
    fake.resolved.insert(provider, path.into());
    fake
}

/// Text files a [`FileBackend`] serves.
#[derive(Clone, Default)]
pub struct TextFiles(Arc<Mutex<HashMap<String, String>>>);

impl TextFiles {
    pub fn set(&self, path: &str, text: &str) {
        self.0.lock().insert(path.into(), text.into());
    }
}

/// [`Fake`] with a scripted `read_text_file`.
struct FileBackend {
    fake: Arc<Fake>,
    files: TextFiles,
}

impl ChildBackend for FileBackend {
    fn spawn(&self, request: SpawnRequest) -> ChildFuture<u32> {
        self.fake.spawn(request)
    }
    fn write(&self, session_id: String, line: String) -> ChildFuture<()> {
        self.fake.write(session_id, line)
    }
    fn kill(&self, session_id: String) -> ChildFuture<()> {
        self.fake.kill(session_id)
    }
    fn kill_all(&self) -> ChildFuture<()> {
        self.fake.kill_all()
    }
    fn runtime_binary_path(&self, provider: HarnessId) -> Option<String> {
        self.fake.runtime_binary_path(provider)
    }
    fn resolve_default(&self, provider: HarnessId) -> ChildFuture<ResolvedHarnessBinary> {
        self.fake.resolve_default(provider)
    }
    fn resolve_configured(
        &self,
        provider: HarnessId,
        binary_path: String,
    ) -> ChildFuture<ResolvedHarnessBinary> {
        self.fake.resolve_configured(provider, binary_path)
    }
    fn exec(&self, request: ExecRequest) -> ChildFuture<String> {
        self.fake.exec(request)
    }
    fn free_port(&self) -> ChildFuture<u16> {
        self.fake.free_port()
    }
    fn http(&self, request: HttpRequest) -> ChildFuture<HttpResponse> {
        self.fake.http(request)
    }
    fn sse_open(
        &self,
        session_id: String,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> ChildFuture<()> {
        self.fake.sse_open(session_id, url, headers)
    }
    fn sse_close(&self, session_id: String) -> ChildFuture<()> {
        self.fake.sse_close(session_id)
    }
    fn read_text_file(&self, path: String) -> ChildFuture<String> {
        let found = self.files.0.lock().get(&path).cloned();
        async move { found.ok_or_else(|| format!("missing {path}")) }.boxed()
    }
    fn update_cli(
        &self,
        command: String,
        provider: HarnessId,
        binary_path: Option<String>,
    ) -> ChildFuture<()> {
        self.fake.update_cli(command, provider, binary_path)
    }
    fn home_dir(&self) -> ChildFuture<String> {
        self.fake.home_dir()
    }
}

/// The real framework over the local process supervisor, for the ignored
/// tests that spawn an installed CLI. Each rig works in its own temp folder.
pub struct LiveRig {
    dir: std::path::PathBuf,
    pub ctx: HarnessContext,
    _lease: crate::core::child::BridgeLease,
}

impl LiveRig {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("monocode-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let spawner: SharedSpawner = Arc::new(SmolSpawner);
        let (children, _host) = Children::for_host(
            crate::core::child::HostChildOptions {
                data_dir: dir.join("data"),
                control: None,
                updater: None,
            },
            spawner.clone(),
        );
        let lease = children.start_harness_bridge();
        let registry = HarnessRegistry::new(spawner, RegistryOptions::default());
        let ctx = HarnessContext::new(registry, children, SharedCatalog::new());
        Self {
            dir,
            ctx,
            _lease: lease,
        }
    }

    pub fn cwd(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    /// One turn through the registry, with a three-minute limit. Returns the
    /// turn's result and every event it emitted.
    pub async fn turn(
        &self,
        harness: HarnessId,
        model: &str,
        text: &str,
    ) -> (anyhow::Result<()>, Vec<HarnessEvent>) {
        let events = Events::default();
        let mut input = turn("live-turn", model, RuntimeMode::Supervised, text, None);
        input.session.cwd = self.cwd();
        let run = self
            .ctx
            .registry
            .send_harness_turn(harness, input, events.sink(), None);
        let result =
            match crate::core::task::timeout(std::time::Duration::from_secs(180), run).await {
                Some(result) => result,
                None => Err(anyhow::anyhow!("live turn timed out")),
            };
        let _ = self
            .ctx
            .registry
            .forget_harness_session(harness, "live-turn")
            .await;
        (result, events.all())
    }
}

impl Drop for LiveRig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The assistant text a turn streamed.
pub fn reply_text(events: &[HarnessEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            HarnessEvent::MessageDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
