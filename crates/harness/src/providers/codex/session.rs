//! Port of src/integrations/harness/providers/codex/codex.ts: live Codex
//! sessions over `codex app-server` JSON-RPC.
//!
//! The TypeScript kept its sessions in module globals and relied on the
//! single-threaded event loop for ordering. Here [`CodexSessions`] owns the
//! session maps, each live session keeps its mutable state behind one lock,
//! and one reader task feeds the child's stdout to the JSON-RPC client, whose
//! handlers run in arrival order as they did in TypeScript. Work the
//! TypeScript chained on promises (image saves, server requests that wait on
//! the user, question deadlines) runs on the caller's spawner.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::Shared;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::block::{ApprovalDecided, TurnIntent};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, GeneratedImage, HarnessEvent, HarnessSessionInput,
    QuestionDecision, RewindLastTurnInput, RewindLastTurnResult, SendTurnInput, SteerTurnInput,
};
use monocode_core::js;
use monocode_core::reducer::snapshot_remainder;
use monocode_core::user_question::{UserQuestionReply, question_prompt_title};

use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildAccount, ChildEvent, Children};
use crate::core::json_rpc::{
    JsonRpcClient, JsonRpcClientOptions, JsonRpcHandlers, JsonRpcId, RpcErrorBody,
};
use crate::core::registry::{AcceptedHook, EventSink};
use crate::core::task::{BoxFuture, SharedSpawner, sleep};

use super::elicitation::codex_mcp_confirmation;
use super::json::{Record, as_record, string_field};
use super::protocol::{
    CodexApprovalKind, ThreadStartInput, TurnStartInput, TurnSteerInput, build_thread_start_params,
    build_turn_start_params, build_turn_steer_params, codex_subagent_states,
    codex_subagent_thread_ids, is_recoverable_thread_resume_error, map_approval_request,
    map_codex_notification, map_codex_subagent_steps, to_codex_approval_decision,
};
use super::questions::{codex_question_response, codex_questions};
use super::rate_limits::{exhausted_window_reset_at, parse_codex_rate_limits};
use crate::core::provider_accounts::same_provider_account_id;

/// `QUESTION_AUTO_RESOLVE_MS`. Match Codex's non-blocking question policy: a
/// minute of grace, then a minute of countdown. Interaction keeps the
/// question open for the user.
pub const QUESTION_AUTO_RESOLVE_MS: i64 = 120_000;

/// `MAX_PENDING_SUBAGENT`: how many notifications a child thread nobody has
/// claimed yet may bank. Codex can stream a subagent's first calls before the
/// spawn item reports which thread it created, and those calls are the most
/// interesting ones, but an unknown thread must not grow this without bound.
const MAX_PENDING_SUBAGENT: usize = 64;

/// What `saveGeneratedImage` returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedImageAsset {
    pub path: String,
    pub mime_type: String,
    pub size: i64,
}

/// `saveGeneratedImage` and `deleteGeneratedImages` from
/// src/platform/tauri/fs.ts. The app implements this over monocode-git's
/// `save_generated_image` and `delete_generated_images`.
pub trait GeneratedImages: Send + Sync {
    fn save(
        &self,
        data: String,
        name: String,
    ) -> BoxFuture<'static, Result<GeneratedImageAsset, String>>;
    fn delete(&self, paths: Vec<String>) -> BoxFuture<'static, Result<(), String>>;
}

/// `Date.now()`, in epoch milliseconds.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The system clock.
pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0)
    })
}

/// Settings the TypeScript took from module imports and constants.
#[derive(Clone)]
pub struct SessionOptions {
    /// Where generated images are saved. Without it an image turns into a
    /// session error.
    pub images: Option<Arc<dyn GeneratedImages>>,
    pub clock: Clock,
    /// Defaults to [`QUESTION_AUTO_RESOLVE_MS`]. Tests shorten it.
    pub question_auto_resolve: Duration,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            images: None,
            clock: system_clock(),
            question_auto_resolve: Duration::from_millis(QUESTION_AUTO_RESOLVE_MS as u64),
        }
    }
}

/// What a live session needs from its owner.
struct Env {
    spawner: SharedSpawner,
    options: SessionOptions,
}

impl Env {
    fn now(&self) -> i64 {
        (self.options.clock)()
    }

    fn spawn(&self, future: impl std::future::Future<Output = ()> + Send + 'static) {
        self.spawner.spawn(future.boxed());
    }
}

struct PendingApproval {
    rpc_id: JsonRpcId,
    thread_id: String,
    // Kept for parity with the TypeScript record; nothing reads it yet.
    _kind: CodexApprovalKind,
    resolve: oneshot::Sender<ApprovalDecided>,
}

struct QuestionTimer {
    token: u64,
    // Dropping the sender closes the channel, which ends the timer task.
    _cancel: async_channel::Sender<()>,
}

struct PendingQuestion {
    rpc_id: JsonRpcId,
    thread_id: String,
    /// Always `HarnessEvent::QuestionAsked`.
    event: HarnessEvent,
    is_blocking: bool,
    timer: Option<QuestionTimer>,
    /// `None` means cancelled.
    resolve: oneshot::Sender<Option<UserQuestionReply>>,
}

/// `turnDone` and `turnFailed` together. The token stands in for the
/// function identity the TypeScript compared.
struct TurnSlot {
    token: u64,
    done: oneshot::Sender<Result<(), String>>,
}

type Queue = Shared<BoxFuture<'static, ()>>;

struct LiveState {
    runtime_mode: RuntimeMode,
    planning: bool,
    on_event: EventSink,
    approvals: BTreeMap<i64, PendingApproval>,
    // Ui ids only grow, so key order is the insertion order a JS Map kept.
    questions: BTreeMap<i64, PendingQuestion>,
    visible_question_id: Option<i64>,
    next_approval_ui_id: i64,
    cancelled: bool,
    mute_updates: bool,
    active_turn_id: Option<String>,
    turn: Option<TurnSlot>,
    next_token: u64,
    /// turn/completed arrived before `run_turn` registered its slot.
    turn_end_pending: bool,
    /// Completed snapshots describe one item, not all text in the turn.
    emitted_assistant_by_item: HashMap<String, String>,
    emitted_reasoning_by_item: HashMap<String, String>,
    emitted_generated_images: HashSet<String>,
    turn_generation: u64,
    notification_queue: Option<(u64, Queue)>,
    /// Child thread id to the agent tool row that spawned it.
    subagent_threads: HashMap<String, String>,
    /// Child notifications that arrived before their row was known.
    pending_subagent: HashMap<String, Vec<(String, Value)>>,
    /// Agent rows still running, by call id, with the name to settle them
    /// under. A Vec keeps the insertion order a JS Map kept.
    open_agent_rows: Vec<(String, String)>,
    /// Latest rate-limit windows by limit id, merged from sparse updates.
    rate_limits: HashMap<String, Record>,
    /// The active turn failed on a spent usage limit.
    usage_limited: bool,
}

impl LiveState {
    fn token(&mut self) -> u64 {
        self.next_token += 1;
        self.next_token
    }

    fn next_ui_id(&mut self) -> i64 {
        let id = self.next_approval_ui_id;
        self.next_approval_ui_id += 1;
        id
    }

    fn turn_token(&self) -> u64 {
        self.turn.as_ref().map_or(0, |turn| turn.token)
    }

    fn clear_emitted(&mut self) {
        self.emitted_assistant_by_item.clear();
        self.emitted_reasoning_by_item.clear();
        self.emitted_generated_images.clear();
    }
}

struct Live {
    rpc: JsonRpcClient,
    /// The client's own handle, cleared on stop to break the handler cycle.
    rpc_slot: RpcSlot,
    thread_id: String,
    cwd: String,
    provider_account_id: Option<String>,
    /// Thread-level network policy used when this app-server opened the thread.
    controls_agents: bool,
    /// `live.turns`: turns and compactions on this session run one at a time.
    turns: futures::lock::Mutex<()>,
    env: Arc<Env>,
    state: Mutex<LiveState>,
}

impl Live {
    fn emit(&self, event: HarnessEvent) {
        let sink = self.state.lock().on_event.clone();
        sink(event);
    }

    fn emit_all(&self, events: Vec<HarnessEvent>) {
        if events.is_empty() {
            return;
        }
        let sink = self.state.lock().on_event.clone();
        for event in events {
            sink(event);
        }
    }

    fn set_on_event(&self, on_event: EventSink) {
        self.state.lock().on_event = on_event;
    }

    fn quiet(&self) -> bool {
        let state = self.state.lock();
        state.mute_updates || state.cancelled
    }

    fn cancelled(&self) -> bool {
        self.state.lock().cancelled
    }

    /// `turnFailed(error)`.
    fn fail_turn(&self, error: &str) {
        let turn = self.state.lock().turn.take();
        if let Some(turn) = turn {
            let _ = turn.done.send(Err(error.to_string()));
        }
    }
}

type RpcSlot = Arc<Mutex<Option<JsonRpcClient>>>;
type LiveRef = Arc<OnceLock<Weak<Live>>>;

fn live_from(live_ref: &LiveRef) -> Option<Arc<Live>> {
    live_ref.get().and_then(Weak::upgrade)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Resume {
    thread_id: String,
    cwd: String,
    provider_account_id: Option<String>,
}

struct SessionsShared {
    children: Children,
    catalog: SharedCatalog,
    env: Arc<Env>,
    live_by_thread: Mutex<HashMap<String, Arc<Live>>>,
    resume_by_thread: Mutex<HashMap<String, Resume>>,
    cancelled_threads: Mutex<HashSet<String>>,
}

/// The live Codex sessions of one app. Clones share the same sessions.
#[derive(Clone)]
pub struct CodexSessions {
    shared: Arc<SessionsShared>,
}

/// `void promise.catch(() => undefined)`.
fn ignore<T>(_: Result<T>) {}

fn respond(rpc: &JsonRpcClient, id: JsonRpcId, result: Value) -> BoxFuture<'static, Result<()>> {
    let rpc = rpc.clone();
    async move { rpc.respond(id, result).await }.boxed()
}

/// `respond(...).catch(() => undefined)`.
fn respond_quietly(
    rpc: &JsonRpcClient,
    id: JsonRpcId,
    result: Value,
) -> BoxFuture<'static, Result<()>> {
    let rpc = rpc.clone();
    async move {
        ignore(rpc.respond(id, result).await);
        Ok(())
    }
    .boxed()
}

/// `pending.rpcId === rec.requestId`.
fn same_rpc_id(id: &JsonRpcId, value: Option<&Value>) -> bool {
    match (id, value) {
        (JsonRpcId::Number(id), Some(Value::Number(other))) => other.as_i64() == Some(*id),
        (JsonRpcId::String(id), Some(Value::String(other))) => id == other,
        _ => false,
    }
}

fn error_text(error: &anyhow::Error) -> String {
    error.to_string()
}

impl CodexSessions {
    pub fn new(
        children: Children,
        spawner: SharedSpawner,
        catalog: SharedCatalog,
        options: SessionOptions,
    ) -> Self {
        Self {
            shared: Arc::new(SessionsShared {
                children,
                catalog,
                env: Arc::new(Env { spawner, options }),
                live_by_thread: Mutex::new(HashMap::new()),
                resume_by_thread: Mutex::new(HashMap::new()),
                cancelled_threads: Mutex::new(HashSet::new()),
            }),
        }
    }

    fn live(&self, session_id: &str) -> Option<Arc<Live>> {
        self.shared.live_by_thread.lock().get(session_id).cloned()
    }

    fn take_cancelled(&self, session_id: &str) -> bool {
        self.shared.cancelled_threads.lock().remove(session_id)
    }

    /// `sendCodexTurn`.
    pub async fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> Result<()> {
        let session_id = input.session.session_id.clone();
        let live = match self.ensure_live(&input.session, on_event.clone()).await {
            Ok(live) => live,
            Err(error) => {
                self.take_cancelled(&session_id);
                return Err(error);
            }
        };
        if self.take_cancelled(&session_id) {
            return Ok(());
        }

        live.set_on_event(on_event);
        let _turn = live.turns.lock().await;
        {
            let mut state = live.state.lock();
            state.runtime_mode = input.session.runtime_mode;
            state.planning = input.session.intent == Some(TurnIntent::Plan);
            state.cancelled = false;
            state.mute_updates = false;
        }
        match self.run_turn(&live, &input, on_accepted).await {
            Err(_) if live.cancelled() => Ok(()),
            result => result,
        }
    }

    /// `compactCodexContext`.
    pub async fn compact_context(
        &self,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> Result<()> {
        let live = match self.ensure_live(&input, on_event.clone()).await {
            Ok(live) => live,
            Err(error) => {
                self.take_cancelled(&input.session_id);
                return Err(error);
            }
        };
        if self.take_cancelled(&input.session_id) {
            return Ok(());
        }

        live.set_on_event(on_event);
        let _turn = live.turns.lock().await;
        {
            let mut state = live.state.lock();
            state.cancelled = false;
            state.mute_updates = false;
        }
        match run_compaction(&live).await {
            Err(_) if live.cancelled() => Ok(()),
            result => result,
        }
    }

    /// `rewindCodexLastTurn`.
    pub async fn rewind_last_turn(
        &self,
        input: RewindLastTurnInput,
        on_event: EventSink,
    ) -> Result<RewindLastTurnResult> {
        let session_id = input.session.session_id.clone();
        let live = match self.ensure_live(&input.session, on_event.clone()).await {
            Ok(live) => live,
            Err(error) => {
                self.take_cancelled(&session_id);
                return Err(error);
            }
        };
        if self.take_cancelled(&session_id) {
            return Ok(RewindLastTurnResult { submitted: false });
        }

        live.set_on_event(on_event);
        drop(live.turns.lock().await);
        if live.state.lock().active_turn_id.is_some() {
            bail!("Stop the current turn before editing the last message");
        }

        let before_turn_id = last_user_turn_id(&live, input.provider_turn_id.as_deref()).await?;
        live.rpc
            .request_value(
                "thread/revert",
                Some(json!({ "threadId": live.thread_id, "beforeTurnId": before_turn_id })),
                0,
            )
            .await?;
        Ok(RewindLastTurnResult { submitted: false })
    }

    /// `steerCodexTurn`.
    pub async fn steer_turn(&self, input: SteerTurnInput) -> Result<()> {
        let Some(live) = self.live(&input.session_id) else {
            bail!("No active Codex session");
        };
        let Some(turn_id) = live.state.lock().active_turn_id.clone() else {
            bail!("No active turn to steer");
        };

        let text = js::trim(&input.text);
        let params = build_turn_steer_params(&TurnSteerInput {
            thread_id: &live.thread_id,
            expected_turn_id: &turn_id,
            prompt: (!text.is_empty()).then_some(text),
            attachments: input.attachments.as_deref().unwrap_or(&[]),
        })
        .map_err(|error| anyhow!(error))?;
        if params
            .get("input")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        {
            return Ok(());
        }

        live.rpc
            .request_value("turn/steer", Some(Value::Object(params)), 0)
            .await?;
        live.emit(HarnessEvent::TurnStarted {
            provider_turn_id: turn_id,
        });
        Ok(())
    }

    /// `respondCodexApproval`.
    pub fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        let Some(live) = self.live(session_id) else {
            return;
        };
        let decided = match decision {
            ApprovalDecision::Allow => ApprovalDecided::Allow,
            ApprovalDecision::Deny => ApprovalDecided::Deny,
        };
        resolve_approval(&live, request_id, decided);
    }

    /// `respondCodexQuestion`.
    pub fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        if let Some(live) = self.live(session_id) {
            resolve_question(&live, request_id, Some(reply));
        }
    }

    /// `keepCodexQuestionOpen`.
    pub fn keep_question_open(&self, session_id: &str, request_id: i64) {
        let Some(live) = self.live(session_id) else {
            return;
        };
        {
            let mut state = live.state.lock();
            let Some(pending) = state.questions.get_mut(&request_id) else {
                return;
            };
            if pending.timer.take().is_none() {
                return;
            }
        }
        live.emit(HarnessEvent::QuestionUpdated {
            request_id,
            auto_resolve_at: None,
        });
    }

    /// `cancelCodexTurn`.
    pub async fn cancel_turn(&self, session_id: &str) -> Result<()> {
        let Some(live) = self.live(session_id) else {
            self.shared
                .cancelled_threads
                .lock()
                .insert(session_id.to_string());
            return Ok(());
        };
        let turn_id = {
            let mut state = live.state.lock();
            state.cancelled = true;
            state.mute_updates = true;
            state.active_turn_id.clone()
        };
        clear_server_requests(&live);
        if let Some(turn_id) = turn_id {
            ignore(
                live.rpc
                    .request_value(
                        "turn/interrupt",
                        Some(json!({ "threadId": live.thread_id, "turnId": turn_id })),
                        0,
                    )
                    .await,
            );
        }
        finish_active_turn(
            &live,
            vec![
                HarnessEvent::MessageCompleted,
                HarnessEvent::ReasoningCompleted,
            ],
        );
        Ok(())
    }

    /// `stopCodexSession`.
    pub async fn stop_session(&self, session_id: &str) -> Result<()> {
        self.take_cancelled(session_id);
        let live = self.shared.live_by_thread.lock().remove(session_id);
        if let Some(live) = live {
            {
                let mut state = live.state.lock();
                state.mute_updates = true;
                state.turn_generation += 1;
            }
            clear_server_requests(&live);
            let turn = live.state.lock().turn.take();
            if let Some(turn) = turn {
                let _ = turn.done.send(Ok(()));
            }
            live.rpc.close(None);
            live.rpc_slot.lock().take();
        }
        self.shared.children.unwatch_child(session_id);
        ignore(self.shared.children.kill_child(session_id).await);
        Ok(())
    }

    /// `forgetCodexSession`.
    pub async fn forget_session(&self, session_id: &str) -> Result<()> {
        self.shared.resume_by_thread.lock().remove(session_id);
        self.stop_session(session_id).await
    }

    /// `bindCodexSession`.
    pub fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        provider_account_id: Option<&str>,
    ) {
        let provider_thread_id = js::trim(provider_session_id);
        if thread_id.is_empty() || provider_thread_id.is_empty() || js::trim(cwd).is_empty() {
            return;
        }
        self.shared.resume_by_thread.lock().insert(
            thread_id.to_string(),
            Resume {
                thread_id: provider_thread_id.to_string(),
                cwd: cwd.to_string(),
                provider_account_id: provider_account_id.map(str::to_string),
            },
        );
    }

    /// The provider thread a session resumes. Test seam (`__codexTestResumeMap`).
    pub fn resume_thread_id(&self, session_id: &str) -> Option<String> {
        self.shared
            .resume_by_thread
            .lock()
            .get(session_id)
            .map(|resume| resume.thread_id.clone())
    }

    async fn ensure_live(
        &self,
        input: &HarnessSessionInput,
        on_event: EventSink,
    ) -> Result<Arc<Live>> {
        let session_id = input.session_id.clone();
        let account = input.provider_account_id.as_deref();
        let controls_agents = input.controls_agents == Some(true);
        if let Some(existing) = self.live(&session_id) {
            let same_account =
                same_provider_account_id(existing.provider_account_id.as_deref(), account);
            if existing.cwd == input.cwd
                && same_account
                && existing.controls_agents == controls_agents
            {
                existing.set_on_event(on_event);
                return Ok(existing);
            }
            // Codex may keep the thread's sandbox network policy across turns.
            // Switch it when this session gains app access or loses agent
            // control, so its local CLI socket matches the current policy.
            if existing.cwd != input.cwd || !same_account {
                self.shared.resume_by_thread.lock().remove(&session_id);
            }
            self.stop_session(&session_id).await?;
        }

        let resume = self
            .shared
            .resume_by_thread
            .lock()
            .get(&session_id)
            .cloned();
        let can_resume = resume.as_ref().is_some_and(|resume| {
            resume.cwd == input.cwd
                && same_provider_account_id(resume.provider_account_id.as_deref(), account)
        });
        if resume.is_some() && !can_resume {
            self.shared.resume_by_thread.lock().remove(&session_id);
        }

        let binary = self.shared.children.resolve_codex_binary().await?;
        let live_ref: LiveRef = Arc::new(OnceLock::new());
        let rpc_slot: RpcSlot = Arc::new(Mutex::new(None));
        let env = self.shared.env.clone();

        let handlers = {
            let notify_ref = live_ref.clone();
            let request_ref = live_ref.clone();
            let slot = rpc_slot.clone();
            let env = env.clone();
            let fallback = on_event.clone();
            JsonRpcHandlers::default()
                .on_notification(move |method, params| {
                    if let Some(live) = live_from(&notify_ref) {
                        on_notification(&live, method, params);
                    }
                })
                .on_request(move |id, method, params| {
                    on_request(&env, &request_ref, &slot, &fallback, id, method, params);
                })
        };
        let rpc = JsonRpcClient::new(
            &session_id,
            Arc::new(self.shared.children.clone()),
            handlers,
            JsonRpcClientOptions {
                include_jsonrpc: false,
                label: "codex".into(),
                ..Default::default()
            },
        );
        *rpc_slot.lock() = Some(rpc.clone());

        self.watch(&session_id, &rpc, &live_ref, &rpc_slot, on_event.clone());

        self.shared
            .children
            .spawn_child(
                &session_id,
                &binary.path,
                vec!["app-server".into()],
                &input.cwd,
                Some(ChildAccount {
                    provider: HarnessId::Codex,
                    id: input
                        .provider_account_id
                        .clone()
                        .unwrap_or_else(|| "default".into()),
                }),
                Some(HarnessId::Codex),
            )
            .await?;

        let opened = self
            .open_live(
                input,
                &rpc,
                &rpc_slot,
                &live_ref,
                resume.filter(|_| can_resume),
                on_event,
            )
            .await;
        match opened {
            Ok(live) => Ok(live),
            Err(error) => {
                rpc.close(Some(&error_text(&error)));
                rpc_slot.lock().take();
                self.stop_session(&session_id).await?;
                Err(error)
            }
        }
    }

    /// `watchChild` with the exit handler from `ensureLive`.
    fn watch(
        &self,
        session_id: &str,
        rpc: &JsonRpcClient,
        live_ref: &LiveRef,
        rpc_slot: &RpcSlot,
        fallback: EventSink,
    ) {
        let events = self.shared.children.watch_child(session_id);
        let rpc = rpc.clone();
        let live_ref = live_ref.clone();
        let rpc_slot = rpc_slot.clone();
        let shared = Arc::downgrade(&self.shared);
        let session_id = session_id.to_string();
        self.shared.env.spawn(async move {
            while let Ok(event) = events.recv().await {
                match event {
                    ChildEvent::Stdout(line) => rpc.push_line(&line),
                    ChildEvent::Stderr(_) => {}
                    ChildEvent::Exit(code) => {
                        rpc.close(Some("Codex app-server exited"));
                        rpc_slot.lock().take();
                        let live = live_from(&live_ref);
                        if let Some(shared) = shared.upgrade() {
                            shared.live_by_thread.lock().remove(&session_id);
                        }
                        let muted = live
                            .as_ref()
                            .is_some_and(|live| live.state.lock().mute_updates);
                        if !muted {
                            let ended = HarnessEvent::SessionEnded {
                                code: code.map(i64::from),
                            };
                            match &live {
                                Some(live) => live.emit(ended),
                                None => fallback(ended),
                            }
                        }
                        if let Some(live) = live {
                            live.fail_turn("Codex app-server exited");
                            clear_server_requests(&live);
                            live.state.lock().turn = None;
                        }
                    }
                }
            }
        });
    }

    /// The `try` block of `ensureLive`: initialize, then resume or start the
    /// thread, then bind the live session.
    async fn open_live(
        &self,
        input: &HarnessSessionInput,
        rpc: &JsonRpcClient,
        rpc_slot: &RpcSlot,
        live_ref: &LiveRef,
        resume: Option<Resume>,
        on_event: EventSink,
    ) -> Result<Arc<Live>> {
        rpc.request_value(
            "initialize",
            Some(json!({
                "clientInfo": { "name": "monocode", "title": "MonoCode", "version": "0.1.0" },
                // Required by collaborationMode (including Plan). currentTime/read
                // is handled even while the thread is starting or resuming.
                "capabilities": { "experimentalApi": true },
            })),
            0,
        )
        .await?;
        rpc.notify("initialized", None).await?;

        let model = self.shared.catalog.read().native_model_id_for(&input.model);
        let service_tier = input
            .model_settings
            .as_ref()
            .and_then(|settings| settings.get("serviceTier"))
            .cloned();
        let thread_params = || {
            build_thread_start_params(&ThreadStartInput {
                cwd: &input.cwd,
                runtime_mode: input.runtime_mode,
                controls_agents: input.controls_agents == Some(true),
                model: Some(&model),
                service_tier: service_tier.as_deref(),
            })
        };

        let mut thread_id: Option<String> = None;
        let mut did_resume = false;
        if let Some(resume) = &resume {
            let mut params = Record::new();
            params.insert("threadId".into(), json!(resume.thread_id));
            params.extend(thread_params());
            match rpc
                .request_value("thread/resume", Some(Value::Object(params)), 0)
                .await
            {
                Ok(opened) => {
                    let id = opened
                        .get("thread")
                        .and_then(|thread| thread.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    thread_id = Some(id.unwrap_or_else(|| resume.thread_id.clone()));
                    did_resume = true;
                }
                Err(error) => {
                    if !is_recoverable_thread_resume_error(&error_text(&error)) {
                        return Err(error);
                    }
                }
            }
        }

        if thread_id.as_deref().is_none_or(str::is_empty) {
            let opened = rpc
                .request_value("thread/start", Some(Value::Object(thread_params())), 0)
                .await?;
            thread_id = opened
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str)
                .map(|id| js::trim(id).to_string());
        }

        let Some(thread_id) = thread_id.filter(|id| !id.is_empty()) else {
            bail!("Codex did not return a thread id");
        };

        let live = Arc::new(Live {
            rpc: rpc.clone(),
            rpc_slot: rpc_slot.clone(),
            thread_id: thread_id.clone(),
            cwd: input.cwd.clone(),
            provider_account_id: input.provider_account_id.clone(),
            controls_agents: input.controls_agents == Some(true),
            turns: futures::lock::Mutex::new(()),
            env: self.shared.env.clone(),
            state: Mutex::new(LiveState {
                runtime_mode: input.runtime_mode,
                planning: input.intent == Some(TurnIntent::Plan),
                on_event,
                approvals: BTreeMap::new(),
                questions: BTreeMap::new(),
                visible_question_id: None,
                next_approval_ui_id: 1,
                cancelled: false,
                mute_updates: did_resume,
                active_turn_id: None,
                turn: None,
                next_token: 0,
                turn_end_pending: false,
                emitted_assistant_by_item: HashMap::new(),
                emitted_reasoning_by_item: HashMap::new(),
                emitted_generated_images: HashSet::new(),
                turn_generation: 0,
                notification_queue: None,
                subagent_threads: HashMap::new(),
                pending_subagent: HashMap::new(),
                open_agent_rows: Vec::new(),
                rate_limits: HashMap::new(),
                usage_limited: false,
            }),
        });
        let _ = live_ref.set(Arc::downgrade(&live));
        self.shared
            .live_by_thread
            .lock()
            .insert(input.session_id.clone(), live.clone());
        self.shared.resume_by_thread.lock().insert(
            input.session_id.clone(),
            Resume {
                thread_id: thread_id.clone(),
                cwd: input.cwd.clone(),
                provider_account_id: input.provider_account_id.clone(),
            },
        );
        live.emit(HarnessEvent::SessionProviderBound {
            provider_session_id: thread_id,
        });
        live.emit(HarnessEvent::SessionStarted);
        Ok(live)
    }

    /// `runTurn`.
    async fn run_turn(
        &self,
        live: &Arc<Live>,
        input: &SendTurnInput,
        on_accepted: Option<AcceptedHook>,
    ) -> Result<()> {
        let session = &input.session;
        let model = self
            .shared
            .catalog
            .read()
            .native_model_id_for(&session.model);
        let setting = |key: &str| {
            session
                .model_settings
                .as_ref()
                .and_then(|settings| settings.get(key))
                .cloned()
        };
        let effort = setting("reasoningEffort");
        let service_tier = setting("serviceTier");
        let text = js::trim(&input.text);

        let params = build_turn_start_params(&TurnStartInput {
            thread_id: &live.thread_id,
            runtime_mode: session.runtime_mode,
            controls_agents: session.controls_agents == Some(true),
            prompt: (!text.is_empty()).then_some(text),
            attachments: input.attachments.as_deref().unwrap_or(&[]),
            model: Some(&model),
            effort: effort.as_deref(),
            service_tier: service_tier.as_deref(),
            intent: session.intent,
        })
        .map_err(|error| anyhow!(error))?;

        if params
            .get("input")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            return Ok(());
        }

        let turn_done = {
            let mut state = live.state.lock();
            state.clear_emitted();
            state.turn_generation += 1;
            let (done, turn_done) = oneshot::channel();
            let token = state.token();
            state.turn = Some(TurnSlot { token, done });
            turn_done
        };
        settle_pending_turn(live);

        let result: Result<()> = async {
            let response = live
                .rpc
                .request_value("turn/start", Some(Value::Object(params)), 0)
                .await?;
            if let Some(on_accepted) = &on_accepted {
                on_accepted();
            }
            let started = {
                let mut state = live.state.lock();
                let turn_id = response
                    .get("turn")
                    .and_then(|turn| turn.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| state.active_turn_id.clone())
                    .filter(|id| !id.is_empty());
                // turn/completed can arrive before turn/start returns. Do not
                // resurrect a finished turn's id after `finish_active_turn`
                // cleared it.
                match turn_id {
                    Some(turn_id) if state.turn.is_some() => {
                        if state.active_turn_id.is_none() {
                            state.active_turn_id = Some(turn_id.clone());
                        }
                        Some(turn_id)
                    }
                    _ => None,
                }
            };
            if let Some(turn_id) = started {
                live.emit(HarnessEvent::TurnStarted {
                    provider_turn_id: turn_id,
                });
            }
            settle_pending_turn(live);
            match turn_done.await {
                Ok(Ok(())) | Err(_) => Ok(()),
                Ok(Err(error)) => Err(anyhow!(error)),
            }
        }
        .await;

        live.state.lock().turn = None;
        match result {
            Ok(()) => Ok(()),
            Err(_) if live.cancelled() => Ok(()),
            Err(error) => {
                live.emit(HarnessEvent::SessionError {
                    message: error_text(&error),
                });
                Err(error)
            }
        }
    }
}

/// `lastUserTurnId`.
async fn last_user_turn_id(live: &Live, provider_turn_id: Option<&str>) -> Result<String> {
    let exact = provider_turn_id.map(js::trim).unwrap_or("");
    if !exact.is_empty() {
        return Ok(exact.to_string());
    }

    let page = live
        .rpc
        .request_value(
            "thread/turns/list",
            Some(json!({
                "threadId": live.thread_id,
                "limit": 100,
                "sortDirection": "desc",
                "itemsView": "summary",
            })),
            0,
        )
        .await?;
    let turns: &[Value] = page
        .get("data")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let user_turn = turns.iter().find(|candidate| {
        as_record(Some(candidate))
            .and_then(|turn| turn.get("items"))
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| string_field(as_record(Some(item)), "type") == Some("userMessage"))
            })
    });
    match string_field(as_record(user_turn), "id") {
        Some(id) => Ok(id.to_string()),
        None => bail!("Codex did not expose a user turn id to edit"),
    }
}

/// `runCompaction`.
async fn run_compaction(live: &Arc<Live>) -> Result<()> {
    let turn_done = {
        let mut state = live.state.lock();
        state.clear_emitted();
        let (done, turn_done) = oneshot::channel();
        let token = state.token();
        state.turn = Some(TurnSlot { token, done });
        turn_done
    };
    settle_pending_turn(live);

    let result: Result<()> = async {
        live.rpc
            .request_value(
                "thread/compact/start",
                Some(json!({ "threadId": live.thread_id })),
                0,
            )
            .await?;
        settle_pending_turn(live);
        match turn_done.await {
            Ok(Ok(())) | Err(_) => Ok(()),
            Ok(Err(error)) => Err(anyhow!(error)),
        }
    }
    .await;
    live.state.lock().turn = None;
    result
}

/// `clearServerRequests`.
fn clear_server_requests(live: &Live) {
    let (approvals, questions) = {
        let mut state = live.state.lock();
        state.visible_question_id = None;
        (
            std::mem::take(&mut state.approvals),
            std::mem::take(&mut state.questions),
        )
    };
    for pending in approvals.into_values() {
        let _ = pending.resolve.send(ApprovalDecided::Cancelled);
    }
    for pending in questions.into_values() {
        let _ = pending.resolve.send(None);
    }
}

/// Resolve one approval. The TypeScript resolved the promise and removed the
/// entry in its `finally`; here both happen at once.
fn resolve_approval(live: &Live, ui_id: i64, decision: ApprovalDecided) {
    let pending = live.state.lock().approvals.remove(&ui_id);
    if let Some(pending) = pending {
        let _ = pending.resolve.send(decision);
    }
}

/// Resolve one question. `None` means cancelled. Removing the entry drops its
/// timer, as `clearTimeout` did.
fn resolve_question(live: &Live, ui_id: i64, reply: Option<UserQuestionReply>) {
    let pending = live.state.lock().questions.remove(&ui_id);
    if let Some(pending) = pending {
        let _ = pending.resolve.send(reply);
    }
}

/// `showNextQuestion`.
fn show_next_question(live: &Arc<Live>) {
    let event = {
        let mut state = live.state.lock();
        if let Some(visible) = state.visible_question_id
            && state.questions.contains_key(&visible)
        {
            return;
        }
        let next = state.questions.keys().next().copied();
        state.visible_question_id = next;
        let Some(next) = next else {
            return;
        };
        let token = state.token();
        let deadline = live.env.now() + live.env.options.question_auto_resolve.as_millis() as i64;
        let pending = state.questions.get_mut(&next).expect("question is queued");
        if !pending.is_blocking {
            if let HarnessEvent::QuestionAsked {
                auto_resolve_at, ..
            } = &mut pending.event
            {
                *auto_resolve_at = Some(deadline);
            }
            let (cancel, cancelled) = async_channel::bounded::<()>(1);
            pending.timer = Some(QuestionTimer {
                token,
                _cancel: cancel,
            });
            let weak = Arc::downgrade(live);
            let delay = live.env.options.question_auto_resolve;
            live.env.spawn(async move {
                let fired = smol::future::or(
                    async {
                        sleep(delay).await;
                        true
                    },
                    async {
                        let _ = cancelled.recv().await;
                        false
                    },
                )
                .await;
                let Some(live) = weak.upgrade().filter(|_| fired) else {
                    return;
                };
                let current = live
                    .state
                    .lock()
                    .questions
                    .get(&next)
                    .and_then(|pending| pending.timer.as_ref())
                    .is_some_and(|timer| timer.token == token);
                if current {
                    resolve_question(&live, next, Some(UserQuestionReply::Skipped));
                }
            });
        }
        pending.event.clone()
    };
    live.emit(event);
}

/// `waitApproval`: register the request before it is shown.
fn wait_approval(
    live: &Live,
    ui_id: i64,
    rpc_id: JsonRpcId,
    kind: CodexApprovalKind,
    thread_id: String,
) -> oneshot::Receiver<ApprovalDecided> {
    let (resolve, decided) = oneshot::channel();
    live.state.lock().approvals.insert(
        ui_id,
        PendingApproval {
            rpc_id,
            thread_id,
            _kind: kind,
            resolve,
        },
    );
    decided
}

/// `autoApproval`.
fn auto_approval(runtime_mode: RuntimeMode, kind: CodexApprovalKind) -> Option<ApprovalDecision> {
    match runtime_mode {
        RuntimeMode::Supervised => None,
        RuntimeMode::FullAccess => Some(ApprovalDecision::Allow),
        // auto_review is set on the server. Still prompt if Codex asks.
        RuntimeMode::Auto => None,
        // auto-accept-edits: auto file changes, ask for commands.
        RuntimeMode::AutoAcceptEdits => {
            (kind == CodexApprovalKind::FileChange).then_some(ApprovalDecision::Allow)
        }
    }
}

/// The `onRequest` handler from `ensureLive`.
fn on_request(
    env: &Arc<Env>,
    live_ref: &LiveRef,
    rpc_slot: &RpcSlot,
    fallback: &EventSink,
    id: JsonRpcId,
    method: &str,
    params: Value,
) {
    let live = live_from(live_ref);
    let turn = live.as_ref().map(|live| live.state.lock().turn_token());
    // The external clock can be read before thread/start or resume returns,
    // so it must not depend on the live session being bound.
    let response = if method == "currentTime/read" {
        let Some(rpc) = rpc_slot.lock().clone() else {
            return;
        };
        Some(respond(
            &rpc,
            id,
            json!({ "currentTimeAt": env.now().div_euclid(1000) }),
        ))
    } else {
        live.as_ref()
            .map(|live| handle_server_request(live, id, method, params))
    };
    let Some(response) = response else {
        return;
    };
    let fallback = fallback.clone();
    env.spawn(async move {
        let Err(error) = response.await else {
            return;
        };
        if let Some(live) = &live {
            let (muted, current, has_turn) = {
                let state = live.state.lock();
                (state.mute_updates, state.turn_token(), state.turn.is_some())
            };
            if muted || Some(current) != turn {
                return;
            }
            if has_turn {
                live.fail_turn(&error_text(&error));
                return;
            }
        }
        let event = HarnessEvent::SessionError {
            message: error_text(&error),
        };
        match &live {
            Some(live) => live.emit(event),
            None => fallback(event),
        }
    });
}

/// `handleServerRequest`. The synchronous part (registering and showing the
/// request) runs now, in arrival order. The returned future waits for the
/// user and sends the reply.
fn handle_server_request(
    live: &Arc<Live>,
    id: JsonRpcId,
    method: &str,
    params: Value,
) -> BoxFuture<'static, Result<()>> {
    let rec = as_record(Some(&params));
    let thread_id = string_field(rec, "threadId")
        .unwrap_or(&live.thread_id)
        .to_string();
    let rpc = live.rpc.clone();

    if method == "item/tool/requestUserInput" {
        if live.quiet() {
            return respond(&rpc, id, json!({ "answers": {} }));
        }
        let questions = match codex_questions(&params) {
            Ok(questions) => questions,
            Err(message) => {
                live.emit(HarnessEvent::Status { text: message });
                return respond(&rpc, id, json!({ "answers": {} }));
            }
        };
        let (ui_id, outcome) = {
            let mut state = live.state.lock();
            let ui_id = state.next_ui_id();
            let event = HarnessEvent::QuestionAsked {
                request_id: ui_id,
                title: Some(question_prompt_title(&questions)),
                questions: questions.clone(),
                call_id: string_field(rec, "itemId").map(str::to_string),
                auto_resolve_at: None,
            };
            let (resolve, outcome) = oneshot::channel();
            state.questions.insert(
                ui_id,
                PendingQuestion {
                    rpc_id: id.clone(),
                    thread_id,
                    event,
                    // Older servers omit this field and keep their blocking behavior.
                    is_blocking: rec.and_then(|rec| rec.get("isBlocking"))
                        != Some(&Value::Bool(false)),
                    timer: None,
                    resolve,
                },
            );
            (ui_id, outcome)
        };
        show_next_question(live);
        let live = live.clone();
        return async move {
            let reply = outcome.await.unwrap_or(None);
            live.emit(HarnessEvent::QuestionResolved {
                request_id: ui_id,
                decision: match &reply {
                    None => QuestionDecision::Cancelled,
                    Some(UserQuestionReply::Answered { .. }) => QuestionDecision::Answered,
                    Some(UserQuestionReply::Skipped) => QuestionDecision::Skipped,
                },
            });
            show_next_question(&live);
            if let Some(reply) = reply {
                rpc.respond(id, codex_question_response(&questions, &reply))
                    .await?;
            }
            Ok(())
        }
        .boxed();
    }

    if method == "mcpServer/elicitation/request" {
        let confirmation = codex_mcp_confirmation(&params);
        let quiet = live.quiet();
        let Some(confirmation) = confirmation.filter(|_| !quiet) else {
            if !quiet {
                live.emit(HarnessEvent::Status {
                    text: "This MCP server requested a form or browser sign-in that MonoCode does not support yet. Complete it in the server's own interface.".into(),
                });
            }
            return respond(
                &rpc,
                id,
                json!({ "action": "cancel", "content": null, "_meta": null }),
            );
        };
        let (planning, runtime_mode) = {
            let state = live.state.lock();
            (state.planning, state.runtime_mode)
        };
        if !planning && runtime_mode == RuntimeMode::FullAccess {
            return respond(
                &rpc,
                id,
                json!({ "action": "accept", "content": confirmation.content, "_meta": null }),
            );
        }
        let ui_id = live.state.lock().next_ui_id();
        let decided = wait_approval(
            live,
            ui_id,
            id.clone(),
            CodexApprovalKind::Permissions,
            thread_id,
        );
        // MCP consent needs an explicit decision outside non-plan Full Access turns.
        live.emit(HarnessEvent::ApprovalRequested {
            request_id: ui_id,
            title: confirmation.title,
            kind: Some("other".into()),
            call_id: None,
            preview: None,
        });
        let live = live.clone();
        let content = confirmation.content;
        return async move {
            let decision = decided.await.unwrap_or(ApprovalDecided::Cancelled);
            live.emit(HarnessEvent::ApprovalResolved {
                request_id: ui_id,
                decision,
            });
            if decision != ApprovalDecided::Cancelled {
                let allow = decision == ApprovalDecided::Allow;
                rpc.respond(
                    id,
                    json!({
                        "action": if allow { "accept" } else { "decline" },
                        "content": if allow { Value::Object(content) } else { Value::Null },
                        "_meta": null,
                    }),
                )
                .await?;
            }
            Ok(())
        }
        .boxed();
    }

    let ui_id = live.state.lock().next_ui_id();
    let Some(mapped) = map_approval_request(method, &params, ui_id) else {
        // An empty success or an invented denial hides a protocol mismatch.
        live.emit(HarnessEvent::Status {
            text: format!("Unsupported Codex request: {method}"),
        });
        let message = format!("Unsupported method: {method}");
        return async move {
            rpc.respond_error(
                id,
                RpcErrorBody {
                    code: -32601,
                    message,
                    data: None,
                },
            )
            .await
        }
        .boxed();
    };

    let (planning, cancelled, muted, runtime_mode) = {
        let state = live.state.lock();
        (
            state.planning,
            state.cancelled,
            state.mute_updates,
            state.runtime_mode,
        )
    };
    let permissions_request = method == "item/permissions/requestApproval";
    if planning || cancelled || muted {
        // Plan turns run in a read-only sandbox that does not escalate. If an
        // older app-server still asks for broader access, deny it silently
        // instead of leaking a Supervised prompt into the selected mode.
        if permissions_request {
            return respond_quietly(&rpc, id, json!({ "permissions": {} }));
        }
        return respond_quietly(
            &rpc,
            id,
            json!({ "decision": to_codex_approval_decision(ApprovalDecision::Deny, mapped.kind) }),
        );
    }

    let requested_permissions = rec
        .and_then(|rec| rec.get("permissions"))
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or_else(|| json!({}));

    if permissions_request {
        // Grant extra permissions for the session in full access. Ask in
        // supervised. Grant them for the turn in the auto modes.
        if runtime_mode == RuntimeMode::FullAccess {
            return respond(
                &rpc,
                id,
                json!({ "scope": "session", "permissions": requested_permissions }),
            );
        }
        if runtime_mode == RuntimeMode::Supervised {
            let decided = wait_approval(live, ui_id, id.clone(), mapped.kind, thread_id);
            live.emit(mapped.event);
            let live = live.clone();
            return async move {
                let decision = decided.await.unwrap_or(ApprovalDecided::Cancelled);
                live.emit(HarnessEvent::ApprovalResolved {
                    request_id: ui_id,
                    decision,
                });
                match decision {
                    ApprovalDecided::Cancelled => Ok(()),
                    ApprovalDecided::Allow => {
                        rpc.respond(
                            id,
                            json!({ "scope": "turn", "permissions": requested_permissions }),
                        )
                        .await
                    }
                    ApprovalDecided::Deny => rpc.respond(id, json!({ "permissions": {} })).await,
                }
            }
            .boxed();
        }
        return respond(
            &rpc,
            id,
            json!({ "scope": "turn", "permissions": requested_permissions }),
        );
    }

    if let Some(auto) = auto_approval(runtime_mode, mapped.kind) {
        return respond(
            &rpc,
            id,
            json!({ "decision": to_codex_approval_decision(auto, mapped.kind) }),
        );
    }

    let kind = mapped.kind;
    let decided = wait_approval(live, ui_id, id.clone(), kind, thread_id);
    live.emit(mapped.event);
    let live = live.clone();
    async move {
        let decision = decided.await.unwrap_or(ApprovalDecided::Cancelled);
        live.emit(HarnessEvent::ApprovalResolved {
            request_id: ui_id,
            decision,
        });
        let wire = match decision {
            ApprovalDecided::Cancelled => return Ok(()),
            ApprovalDecided::Allow => ApprovalDecision::Allow,
            ApprovalDecided::Deny => ApprovalDecision::Deny,
        };
        rpc.respond(
            id,
            json!({ "decision": to_codex_approval_decision(wire, kind) }),
        )
        .await
    }
    .boxed()
}

/// The `onNotification` handler from `ensureLive`, with the notification
/// queue that keeps later notifications behind an image still saving.
fn on_notification(live: &Arc<Live>, method: &str, params: Value) {
    let (queue, generation) = {
        let state = live.state.lock();
        if state.mute_updates {
            return;
        }
        (
            state
                .notification_queue
                .as_ref()
                .map(|(_, queue)| queue.clone()),
            state.turn_generation,
        )
    };
    if let Some(queue) = queue {
        let queued_live = live.clone();
        let method = method.to_string();
        let queued = async move {
            queue.await;
            {
                let state = queued_live.state.lock();
                if state.mute_updates || state.cancelled || generation != state.turn_generation {
                    return;
                }
            }
            if let Some(pending) = handle_notification(&queued_live, &method, &params) {
                pending.await;
            }
        };
        track_notification_queue(live, queued.boxed());
        return;
    }
    if let Some(pending) = handle_notification(live, method, &params) {
        track_notification_queue(live, pending);
    }
}

/// `trackNotificationQueue`. The spawned task drives the queued work, as a
/// promise would run on its own, and clears the queue when it is the last.
fn track_notification_queue(live: &Arc<Live>, queued: BoxFuture<'static, ()>) {
    let queued = queued.shared();
    let id = {
        let mut state = live.state.lock();
        let id = state.token();
        state.notification_queue = Some((id, queued.clone()));
        id
    };
    let live = live.clone();
    live.env.clone().spawn(async move {
        queued.await;
        let mut state = live.state.lock();
        if state
            .notification_queue
            .as_ref()
            .is_some_and(|(current, _)| *current == id)
        {
            state.notification_queue = None;
        }
    });
}

/// What `handleNotification` does once a notification's images are saved.
struct Finish {
    item: Option<Record>,
    rate_limits: Option<Record>,
    usage_limited: bool,
    active_turn_id: Option<Option<String>>,
    turn_completed: bool,
}

/// `handleNotification`. Returns the part that has to wait (an image save or
/// a banked child step), or `None` when everything ran now.
fn handle_notification(
    live: &Arc<Live>,
    method: &str,
    params: &Value,
) -> Option<BoxFuture<'static, ()>> {
    if live.quiet() {
        return None;
    }
    let rec = as_record(Some(params));
    if method == "serverRequest/resolved" {
        let request_id = rec.and_then(|rec| rec.get("requestId"));
        let thread_id = rec
            .and_then(|rec| rec.get("threadId"))
            .and_then(Value::as_str);
        let (approvals, questions): (Vec<i64>, Vec<i64>) = {
            let state = live.state.lock();
            let matches = |rpc_id: &JsonRpcId, owner: &str| {
                same_rpc_id(rpc_id, request_id) && thread_id == Some(owner)
            };
            (
                state
                    .approvals
                    .iter()
                    .filter(|(_, pending)| matches(&pending.rpc_id, &pending.thread_id))
                    .map(|(id, _)| *id)
                    .collect(),
                state
                    .questions
                    .iter()
                    .filter(|(_, pending)| matches(&pending.rpc_id, &pending.thread_id))
                    .map(|(id, _)| *id)
                    .collect(),
            )
        };
        for ui_id in approvals {
            resolve_approval(live, ui_id, ApprovalDecided::Cancelled);
        }
        for ui_id in questions {
            resolve_question(live, ui_id, None);
        }
        return None;
    }

    // Child threads share this connection. Their lifecycle must not touch the
    // parent's turn or clear its approvals, but what they do is the inside of
    // a subagent, so mirror it onto the row that spawned them.
    let thread_id = string_field(rec, "threadId").or_else(|| {
        (method == "thread/started")
            .then(|| string_field(as_record(rec.and_then(|rec| rec.get("thread"))), "id"))
            .flatten()
    });
    if let Some(thread_id) = thread_id
        && thread_id != live.thread_id
    {
        return handle_subagent_notification(live, thread_id, method, params);
    }

    // A Codex turn is a sequence of items. Completing an agentMessage does not
    // end the turn: more tools and messages can still arrive. Only
    // turn/completed (and turn/aborted) settle `send_turn`, which is what the
    // UI uses for busy, stop, and "Working for".
    let mapped = map_codex_notification(method, params);
    if let Some(diagnostic) = &mapped.diagnostic {
        log::debug!("[monocode] codex {} {method} {diagnostic}", live.thread_id);
    }
    // Codex describes one spawned agent through more than one item type. The
    // first row to name a child thread owns it. A later item for the same
    // thread would otherwise stand up a second agent that never does anything.
    let duplicate = bind_subagent_threads(live, method, rec);
    let snapshot = method == "item/completed";
    let item = as_record(rec.and_then(|rec| rec.get("item")));
    let item_id = if snapshot {
        string_field(item, "id")
    } else {
        string_field(rec, "itemId")
    };

    let mut images: Vec<GeneratedImage> = Vec::new();
    for event in mapped.events {
        if duplicate && duplicate_agent_row(&event) {
            continue;
        }
        if let HarnessEvent::ImageGenerated(image) = event {
            if !mark_image(live, image.item_id()) {
                continue;
            }
            match image {
                GeneratedImage::Inline { .. } => images.push(image),
                file => live.emit(HarnessEvent::ImageGenerated(file)),
            }
            continue;
        }
        track_agent_row(live, &event);
        match event {
            HarnessEvent::MessageDelta { text } => {
                publish_codex_text(live, TextRole::Assistant, &text, snapshot, item_id)
            }
            HarnessEvent::ReasoningDelta { text } => {
                publish_codex_text(live, TextRole::Reasoning, &text, snapshot, item_id)
            }
            other => live.emit(other),
        }
    }

    // Metadata and steps can arrive before the spawn. Create its row first.
    let mut replay: Option<BoxFuture<'static, ()>> = None;
    for child_id in item.map(codex_subagent_thread_ids).unwrap_or_default() {
        let (owner, backlog) = {
            let mut state = live.state.lock();
            let Some(owner) = state.subagent_threads.get(&child_id).cloned() else {
                continue;
            };
            (owner, state.pending_subagent.remove(&child_id))
        };
        for (step_method, step_params) in backlog.unwrap_or_default() {
            match replay.take() {
                Some(previous) => {
                    let live = live.clone();
                    let owner = owner.clone();
                    replay = Some(
                        async move {
                            previous.await;
                            if let Some(pending) =
                                emit_subagent_steps(&live, &owner, &step_method, &step_params)
                            {
                                pending.await;
                            }
                        }
                        .boxed(),
                    );
                }
                None => replay = emit_subagent_steps(live, &owner, &step_method, &step_params),
            }
        }
    }
    // The replay runs now, as the promise chain did, while images save.
    let replay = replay.map(|replay| {
        let replay = replay.shared();
        live.env.spawn(replay.clone());
        replay
    });

    let finish = Finish {
        item: item.cloned(),
        rate_limits: mapped.rate_limits,
        usage_limited: mapped.usage_limited,
        active_turn_id: mapped.active_turn_id,
        turn_completed: mapped.turn_completed.is_some(),
    };
    if images.is_empty() {
        return after_images(live, replay, finish);
    }
    let live = live.clone();
    Some(
        async move {
            for image in images {
                materialize_generated_image(&live, image).await;
            }
            if let Some(pending) = after_images(&live, replay, finish) {
                pending.await;
            }
        }
        .boxed(),
    )
}

/// `afterImages`.
fn after_images(
    live: &Arc<Live>,
    replay: Option<Shared<BoxFuture<'static, ()>>>,
    finish: Finish,
) -> Option<BoxFuture<'static, ()>> {
    if live.quiet() {
        return None;
    }
    match replay {
        Some(replay) => {
            let live = live.clone();
            Some(
                async move {
                    replay.await;
                    finish_notification(&live, finish);
                }
                .boxed(),
            )
        }
        None => {
            finish_notification(live, finish);
            None
        }
    }
}

/// `finish` in `handleNotification`.
fn finish_notification(live: &Arc<Live>, finish: Finish) {
    if let Some(item) = &finish.item {
        settle_subagent_rows(live, item);
    }
    if let Some(update) = &finish.rate_limits {
        note_rate_limits(live, update);
    }
    let usage_event = {
        let mut state = live.state.lock();
        if finish.usage_limited {
            state.usage_limited = true;
        }
        if let Some(active) = finish.active_turn_id {
            state.active_turn_id = active;
        }
        if !finish.turn_completed {
            return;
        }
        // Report the limit before the turn settles, so queued follow-ups hold.
        let event = (state.usage_limited && !state.cancelled).then(|| HarnessEvent::UsageLimited {
            resets_at: usage_limit_reset_at(&state),
        });
        state.usage_limited = false;
        event
    };
    if let Some(event) = usage_event {
        live.emit(event);
    }
    finish_active_turn(live, Vec::new());
}

/// Record that an image item was handled. False when it already was.
fn mark_image(live: &Live, item_id: &str) -> bool {
    live.state
        .lock()
        .emitted_generated_images
        .insert(item_id.to_string())
}

/// `materializeGeneratedImage`: save inline image data, then report the file.
async fn materialize_generated_image(live: &Arc<Live>, image: GeneratedImage) {
    let GeneratedImage::Inline {
        item_id,
        data,
        name,
        alt,
    } = image
    else {
        live.emit(HarnessEvent::ImageGenerated(image));
        return;
    };
    let generation = live.state.lock().turn_generation;
    let stale = || live.quiet() || generation != live.state.lock().turn_generation;
    let saved = match live.env.options.images.clone() {
        Some(images) => images.save(data, name.clone()).await,
        None => Err("no image store is configured".into()),
    };
    match saved {
        Ok(asset) => {
            if stale() {
                if let Some(images) = live.env.options.images.clone() {
                    live.env.spawn(async move {
                        let _ = images.delete(vec![asset.path]).await;
                    });
                }
                return;
            }
            live.emit(HarnessEvent::ImageGenerated(GeneratedImage::File {
                item_id,
                path: asset.path,
                name,
                mime_type: asset.mime_type,
                size: asset.size,
                alt,
            }));
        }
        Err(cause) => {
            if stale() {
                return;
            }
            live.emit(HarnessEvent::SessionError {
                message: format!("Could not save generated image: {cause}"),
            });
        }
    }
}

/// `noteRateLimits`. Updates are sparse: a missing window keeps its last
/// reading.
fn note_rate_limits(live: &Live, update: &Record) {
    let id = string_field(Some(update), "limitId")
        .unwrap_or("")
        .to_string();
    let mut state = live.state.lock();
    let current = state.rate_limits.get(&id).cloned().unwrap_or_default();
    let mut next = Record::new();
    for key in ["primary", "secondary"] {
        let value = update
            .get(key)
            .filter(|value| !value.is_null())
            .or_else(|| current.get(key));
        if let Some(value) = value {
            next.insert(key.into(), value.clone());
        }
    }
    state.rate_limits.insert(id, next);
}

/// `usageLimitResetAt`.
fn usage_limit_reset_at(state: &LiveState) -> Option<i64> {
    let mut latest: Option<i64> = None;
    for windows in state.rate_limits.values() {
        let limits = parse_codex_rate_limits(&Value::Object(windows.clone()));
        if let Some(resets_at) = exhausted_window_reset_at(&limits) {
            latest = Some(latest.unwrap_or(0).max(resets_at));
        }
    }
    latest
}

/// `bindSubagentThreads`: learn which agent row a child thread belongs to.
/// True when every thread this item names already belongs to another row,
/// which makes the item a second description of an agent already shown.
fn bind_subagent_threads(live: &Live, method: &str, rec: Option<&Record>) -> bool {
    if method != "item/started" && method != "item/completed" {
        return false;
    }
    let Some(item) = as_record(rec.and_then(|rec| rec.get("item"))) else {
        return false;
    };
    let item_type = string_field(Some(item), "type").unwrap_or("");
    if item_type != "subAgentActivity" && item_type != "collabAgentToolCall" {
        return false;
    }
    let Some(call_id) = string_field(Some(item), "id") else {
        return false;
    };
    let children: Vec<String> = codex_subagent_thread_ids(item)
        .into_iter()
        .filter(|child| *child != live.thread_id)
        .collect();
    let mut claimed = 0;
    let mut events = Vec::new();
    {
        let mut state = live.state.lock();
        for child in &children {
            if let Some(owner) = state.subagent_threads.get(child).cloned() {
                if let Some(model) = string_field(Some(item), "model")
                    && item.get("tool") == Some(&json!("spawnAgent"))
                {
                    events.push(HarnessEvent::ToolUpdated {
                        agent_model: Some(model.to_string()),
                        call_id: owner.clone(),
                        title: None,
                        kind: Some("agent".into()),
                        status: None,
                        detail: None,
                        preview: None,
                        paths: None,
                    });
                }
                if owner != call_id {
                    claimed += 1;
                }
                continue;
            }
            state
                .subagent_threads
                .insert(child.clone(), call_id.to_string());
        }
    }
    live.emit_all(events);
    !children.is_empty() && claimed == children.len()
}

/// `duplicateAgentRow`: an agent row for a child thread another row already
/// owns. A failure still gets its row, since the reason a run died is worth a
/// line of its own, but a duplicate "running" or "done" is noise.
fn duplicate_agent_row(event: &HarnessEvent) -> bool {
    match event {
        HarnessEvent::ToolStarted { kind, status, .. }
        | HarnessEvent::ToolUpdated { kind, status, .. } => {
            kind.as_deref() == Some("agent") && status.as_deref() != Some("failed")
        }
        _ => false,
    }
}

/// `handleSubagentNotification`. Until the spawn item says which row the
/// thread belongs to, keep its notifications: dropping them loses the opening
/// moves of the run.
fn handle_subagent_notification(
    live: &Arc<Live>,
    thread_id: &str,
    method: &str,
    params: &Value,
) -> Option<BoxFuture<'static, ()>> {
    let call_id = {
        let mut state = live.state.lock();
        match state.subagent_threads.get(thread_id).cloned() {
            Some(call_id) => call_id,
            None => {
                state.active_turn_id.as_ref()?;
                if !matches!(method, "item/started" | "item/completed" | "thread/started") {
                    return None;
                }
                let backlog = state
                    .pending_subagent
                    .entry(thread_id.to_string())
                    .or_default();
                if backlog.len() < MAX_PENDING_SUBAGENT {
                    backlog.push((method.to_string(), params.clone()));
                }
                return None;
            }
        }
    };
    emit_subagent_steps(live, &call_id, method, params)
}

/// `emitSubagentSteps`.
fn emit_subagent_steps(
    live: &Arc<Live>,
    call_id: &str,
    method: &str,
    params: &Value,
) -> Option<BoxFuture<'static, ()>> {
    let image = map_codex_notification(method, params)
        .events
        .into_iter()
        .find_map(|event| match event {
            HarnessEvent::ImageGenerated(image) => Some(image),
            _ => None,
        });
    if let Some(image) = image {
        if !mark_image(live, image.item_id()) {
            return None;
        }
        if matches!(image, GeneratedImage::Inline { .. }) {
            let live = live.clone();
            return Some(async move { materialize_generated_image(&live, image).await }.boxed());
        }
        live.emit(HarnessEvent::ImageGenerated(image));
        return None;
    }
    live.emit_all(map_codex_subagent_steps(call_id, method, params));
    None
}

/// `trackAgentRow`: remember an agent row while it runs, so the turn can
/// close it out.
fn track_agent_row(live: &Live, event: &HarnessEvent) {
    let (call_id, title, kind, status) = match event {
        HarnessEvent::ToolStarted {
            call_id,
            title,
            kind,
            status,
            ..
        } => (call_id, Some(title.as_str()), kind, status),
        HarnessEvent::ToolUpdated {
            call_id,
            title,
            kind,
            status,
            ..
        } => (call_id, title.as_deref(), kind, status),
        _ => return,
    };
    if kind.as_deref() != Some("agent") {
        return;
    }
    let mut state = live.state.lock();
    let rows = &mut state.open_agent_rows;
    if matches!(status.as_deref(), Some("in_progress" | "pending")) {
        let title = title.unwrap_or("Subagent").to_string();
        match rows.iter_mut().find(|(id, _)| id == call_id) {
            Some(row) => row.1 = title,
            None => rows.push((call_id.clone(), title)),
        }
        return;
    }
    rows.retain(|(id, _)| id != call_id);
}

/// `settleSubagentRows`: settle spawned agents from the per-agent state a
/// collab item reports. The spawn call returns at once; this is the first
/// word on whether the agent it started finished.
fn settle_subagent_rows(live: &Live, item: &Record) {
    let mut events = Vec::new();
    {
        let mut state = live.state.lock();
        for agent in codex_subagent_states(item) {
            let Some(call_id) = state.subagent_threads.get(&agent.thread_id).cloned() else {
                continue;
            };
            let Some(index) = state
                .open_agent_rows
                .iter()
                .position(|(id, title)| *id == call_id && !title.is_empty())
            else {
                continue;
            };
            let (_, title) = state.open_agent_rows.remove(index);
            events.push(HarnessEvent::ToolUpdated {
                agent_model: None,
                call_id,
                title: Some(title),
                kind: Some("agent".into()),
                status: Some(agent.status.into()),
                detail: agent.message,
                preview: None,
                paths: None,
            });
        }
    }
    live.emit_all(events);
}

/// `closeOpenAgentRows`. A turn cannot end with an agent still working. Codex
/// does not always report a closing state for every child, and a row left
/// running would spin forever.
fn close_open_agent_rows(live: &Live) {
    let rows = std::mem::take(&mut live.state.lock().open_agent_rows);
    live.emit_all(
        rows.into_iter()
            .map(|(call_id, title)| HarnessEvent::ToolUpdated {
                agent_model: None,
                call_id,
                title: Some(title),
                kind: Some("agent".into()),
                status: Some("completed".into()),
                detail: None,
                preview: None,
                paths: None,
            })
            .collect(),
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextRole {
    Assistant,
    Reasoning,
}

/// `publishCodexText`.
fn publish_codex_text(
    live: &Live,
    role: TextRole,
    text: &str,
    snapshot: bool,
    item_id: Option<&str>,
) {
    let emit = {
        let mut state = live.state.lock();
        let emitted = match role {
            TextRole::Assistant => &mut state.emitted_assistant_by_item,
            TextRole::Reasoning => &mut state.emitted_reasoning_by_item,
        };
        // Keep id-less notifications compatible without mixing them into known items.
        let key = item_id.unwrap_or("").to_string();
        let already = emitted.get(&key).cloned().unwrap_or_default();
        let emit = if snapshot {
            snapshot_remainder(&already, text)
        } else {
            text
        };
        if emit.is_empty() {
            return;
        }
        // These are deltas (or a snapshot's missing suffix), so repeated
        // tokens count. Completed items stay until the turn ends, so repeated
        // completions are ignored.
        emitted.insert(key, already + emit);
        emit.to_string()
    };
    live.emit(match role {
        TextRole::Assistant => HarnessEvent::MessageDelta { text: emit },
        TextRole::Reasoning => HarnessEvent::ReasoningDelta { text: emit },
    });
}

/// `finishActiveTurn`.
fn finish_active_turn(live: &Live, extra_events: Vec<HarnessEvent>) {
    clear_server_requests(live);
    close_open_agent_rows(live);
    {
        let mut state = live.state.lock();
        state.turn_end_pending = false;
        state.turn_generation += 1;
        state.active_turn_id = None;
        state.clear_emitted();
        state.subagent_threads.clear();
        state.pending_subagent.clear();
    }
    live.emit_all(extra_events);
    let turn = {
        let mut state = live.state.lock();
        let turn = state.turn.take();
        if turn.is_none() {
            state.turn_end_pending = true;
        }
        turn
    };
    if let Some(turn) = turn {
        let _ = turn.done.send(Ok(()));
    }
}

/// `settlePendingTurn`.
fn settle_pending_turn(live: &Live) {
    let pending = {
        let state = live.state.lock();
        state.turn_end_pending && state.turn.is_some()
    };
    if pending {
        finish_active_turn(live, Vec::new());
    }
}
