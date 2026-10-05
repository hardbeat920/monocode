//! Port of src/integrations/harness/providers/antigravity/antigravity.ts:
//! live Antigravity sessions over ACP. It spawns `agy_acp_server.par`, not
//! `agy acp`.
//!
//! Antigravity answers `session/prompt` only after its post-turn work
//! (trajectory idle and external hooks) finishes, so a wedged server never
//! resolves it. Silence is ambiguous, since a long quiet tool call is
//! legitimate, so past [`AntigravityOptions::stall_notify_ms`] the adapter
//! shows a status note instead of killing the turn.
//!
//! The TypeScript kept its maps and epochs in module globals. Here they live
//! in [`AntigravitySessions`]. Promise chains become tails: each queued step
//! waits on the previous step's tail and settles its own when it ends.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use anyhow::{Result, anyhow, bail};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use monocode_core::block::{ApprovalDecided, TurnIntent};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent, SendTurnInput, SteerTurnInput};
use parking_lot::Mutex;
use serde_json::{Map, Value, json};

use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::acp_subagents::AcpSubagents;
use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildHandlers, Children};
use crate::core::json_rpc::{JsonRpcClientOptions, RpcErrorBody};
use crate::core::registry::EventSink;
use crate::core::task::{self, SharedSpawner};

use super::protocol::{
    ConfigValue, SessionConfigOption, antigravity_mode_id, antigravity_prompt_blocks,
    antigravity_spawn_cwd, auto_permission_option, events_from_acp_update, extract_model_config_id,
    permission_option_id, permission_request_from_acp, read_config_options,
    resolve_setting_config_id, session_id_from_result,
};

/// `AUTH_HELP`.
pub const AUTH_HELP: &str = "Run `agy` once in Terminal to sign in.";

/// The status shown after [`AntigravityOptions::stall_notify_ms`] of silence.
pub const STALL_STATUS: &str = "Antigravity has been quiet for two minutes — its post-turn work may be stuck. Stop and resend to recover.";

/// The status shown when a saved conversation could not be restored.
pub const RESTORE_FAILED_STATUS: &str =
    "Antigravity could not restore the previous conversation — starting a new session.";

/// Timeouts that were constants in the TypeScript. Tests shorten them.
#[derive(Debug, Clone)]
pub struct AntigravityOptions {
    /// `INIT_TIMEOUT_MS`.
    pub init_timeout_ms: i64,
    /// `SESSION_TIMEOUT_MS`.
    pub session_timeout_ms: i64,
    /// `CONTROL_TIMEOUT_MS`.
    pub control_timeout_ms: i64,
    /// `PROMPT_TIMEOUT_MS`. A prompt may legitimately run much longer than a
    /// control request.
    pub prompt_timeout_ms: i64,
    /// `STALL_NOTIFY_MS`.
    pub stall_notify_ms: i64,
    /// The JSON-RPC client options, including the write timeout.
    pub rpc: JsonRpcClientOptions,
}

impl Default for AntigravityOptions {
    fn default() -> Self {
        Self {
            init_timeout_ms: 12_000,
            session_timeout_ms: 45_000,
            control_timeout_ms: 15_000,
            prompt_timeout_ms: 30 * 60_000,
            stall_notify_ms: 120_000,
            rpc: JsonRpcClientOptions::default(),
        }
    }
}

/// `antigravityError`: add sign-in help to anything that looks like an
/// authentication failure.
pub fn antigravity_error(error: &anyhow::Error) -> anyhow::Error {
    let detail = error.to_string();
    if detail.contains(AUTH_HELP) {
        return anyhow!(detail);
    }
    let lower = detail.to_lowercase();
    let auth_like = lower.contains("auth")
        || lower.contains("login")
        || has_wildcard(&lower, "sign", "in")
        || lower.contains("credential")
        || has_wildcard(&lower, "api", "key");
    if auth_like {
        anyhow!("{}\n\n{AUTH_HELP}", monocode_core::js::trim(&detail))
    } else {
        anyhow!(detail)
    }
}

/// `/a.b/`: `a`, any one character other than a line terminator, then `b`.
fn has_wildcard(text: &str, before: &str, after: &str) -> bool {
    text.match_indices(before).any(|(index, _)| {
        let rest = &text[index + before.len()..];
        let mut chars = rest.chars();
        matches!(chars.next(), Some(c) if !monocode_core::js::is_line_terminator(c))
            && chars.as_str().starts_with(after)
    })
}

/// `isTimeout`.
fn is_timeout(error: &anyhow::Error) -> bool {
    error.to_string().ends_with("timed out")
}

/// `CLIENT_CAPABILITIES`. Config-option support is advertised because a
/// compliant agent may leave `configOptions` out for clients that never claim
/// it, which would silently drop model selection.
pub fn client_capabilities() -> Value {
    json!({
        "fs": { "readTextFile": false, "writeTextFile": false },
        "terminal": false,
        "session": { "configOptions": { "boolean": {} } },
    })
}

/// A cancellable `setTimeout`. Dropping it is `clearTimeout`.
struct Timer {
    token: u64,
    _cancel: async_channel::Sender<()>,
}

static TIMER_TOKENS: AtomicU64 = AtomicU64::new(1);

fn start_timer(
    spawner: &SharedSpawner,
    delay_ms: i64,
    fire: impl FnOnce(u64) + Send + 'static,
) -> Timer {
    let token = TIMER_TOKENS.fetch_add(1, Ordering::SeqCst);
    let (cancel, cancelled) = async_channel::bounded::<()>(1);
    spawner.spawn(Box::pin(async move {
        let fired = smol::future::or(
            async {
                task::sleep(task::ms(delay_ms)).await;
                true
            },
            async {
                let _ = cancelled.recv().await;
                false
            },
        )
        .await;
        if fired {
            fire(token);
        }
    }));
    Timer {
        token,
        _cancel: cancel,
    }
}

/// Start `future` the way calling an `async` function does in JavaScript: run
/// it now up to its first await, then spawn the rest.
fn run_now(spawner: &SharedSpawner, future: BoxFuture<'static, ()>) {
    let mut future = future;
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    if future.as_mut().poll(&mut cx).is_pending() {
        spawner.spawn(future);
    }
}

type Tail = Shared<BoxFuture<'static, ()>>;

/// A tail that settles when the returned sender drops.
fn new_tail() -> (oneshot::Sender<()>, Tail) {
    let (done, settled) = oneshot::channel::<()>();
    (done, settled.map(|_| ()).boxed().shared())
}

/// The mutable half of `Live`.
struct LiveState {
    subagents: AcpSubagents,
    model_config_id: String,
    config_options: Vec<SessionConfigOption>,
    mute_updates: bool,
    cancelled: bool,
    /// Server state is unknowable after a mid-prompt cancel; recycle before reuse.
    stale: bool,
    runtime_mode: RuntimeMode,
    planning: bool,
    on_event: EventSink,
    approvals: HashMap<i64, oneshot::Sender<ApprovalDecision>>,
    prompt_in_flight: bool,
    /// The owning turn may have state-changing requests in flight, so a
    /// cancel here makes the transport indeterminate even mid-config.
    turn_active: bool,
    stall_notified: bool,
    watchdog: Option<Timer>,
}

/// `Live`: one generation of the ACP process for a thread.
struct Live {
    thread_id: String,
    /// Generation-scoped child id (`thread#n`): a recycled process can never
    /// deliver stdout to, or take writes from, a different generation.
    child_key: String,
    acp: AcpClient,
    acp_session_id: String,
    cwd: String,
    state: Mutex<LiveState>,
}

impl Live {
    fn sink(&self) -> EventSink {
        self.state.lock().on_event.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Resume {
    acp_session_id: String,
    cwd: String,
}

/// A startup that has spawned but not yet published its `Live`.
struct PendingSetup {
    acp: AcpClient,
    child_key: String,
    token: u64,
}

#[derive(Default)]
struct State {
    live_by_thread: HashMap<String, Arc<Live>>,
    resume_by_thread: HashMap<String, Resume>,
    /// Every user-intent cancel, stop, or forget bumps this: sends already
    /// pending at that moment are suppressed, while a later send captures
    /// the new epoch and proceeds.
    cancel_epoch: HashMap<String, u64>,
    /// Bumped by user intent only, never by internal recycles. Lifecycle work
    /// checks the epoch it captured so queued steps cannot resurrect a
    /// session the user already abandoned.
    session_epoch: HashMap<String, u64>,
    /// Startup, teardown, and replacement serialize through this per thread.
    lifecycle_by_thread: HashMap<String, Tail>,
    pending_setup_by_thread: HashMap<String, PendingSetup>,
    /// Turns serialize per thread, not per `Live`, so a recycled transport
    /// cannot let a queued send run beside its replacement.
    turns_by_thread: HashMap<String, Tail>,
    child_seq: u64,
    setup_tokens: u64,
}

/// The module state of antigravity.ts.
pub struct AntigravitySessions {
    children: Children,
    spawner: SharedSpawner,
    catalog: SharedCatalog,
    options: AntigravityOptions,
    state: Mutex<State>,
}

fn is_plan(input: &SendTurnInput) -> bool {
    input.session.intent == Some(TurnIntent::Plan)
}

impl AntigravitySessions {
    pub fn new(
        children: Children,
        spawner: SharedSpawner,
        catalog: SharedCatalog,
        options: AntigravityOptions,
    ) -> Arc<Self> {
        Arc::new(Self {
            children,
            spawner,
            catalog,
            options,
            state: Mutex::new(State::default()),
        })
    }

    fn cancel_epoch(&self, session_id: &str) -> u64 {
        self.state
            .lock()
            .cancel_epoch
            .get(session_id)
            .copied()
            .unwrap_or(0)
    }

    fn session_epoch(&self, session_id: &str) -> u64 {
        self.state
            .lock()
            .session_epoch
            .get(session_id)
            .copied()
            .unwrap_or(0)
    }

    fn live(&self, session_id: &str) -> Option<Arc<Live>> {
        self.state.lock().live_by_thread.get(session_id).cloned()
    }

    fn is_current(&self, live: &Arc<Live>) -> bool {
        self.live(&live.thread_id)
            .is_some_and(|current| Arc::ptr_eq(&current, live))
    }

    fn bump_epochs(&self, session_id: &str) {
        let mut state = self.state.lock();
        *state
            .cancel_epoch
            .entry(session_id.to_string())
            .or_default() += 1;
        *state
            .session_epoch
            .entry(session_id.to_string())
            .or_default() += 1;
    }

    /// `sendAntigravityTurn`. The eager startup is submitted when this is
    /// called, as the promise was, so epochs are captured at call time.
    pub fn send_antigravity_turn(
        self: &Arc<Self>,
        input: SendTurnInput,
        on_event: EventSink,
    ) -> BoxFuture<'static, Result<()>> {
        let session_id = input.session.session_id.clone();
        let epoch = self.cancel_epoch(&session_id);
        // Eager prewarm only starts a child when none exists. It never tears
        // down a parked transport, so a queued send cannot interrupt a turn.
        let eager = self.ensure_live(&input, &on_event, false);
        let this = self.clone();
        async move {
            let cancelled = || this.cancel_epoch(&session_id) != epoch;
            if let Err(error) = eager.await {
                if cancelled() {
                    return Ok(());
                }
                return Err(error);
            }
            if cancelled() {
                return Ok(());
            }

            let (done, mine) = new_tail();
            let previous = this
                .state
                .lock()
                .turns_by_thread
                .insert(session_id.clone(), mine.clone());
            if let Some(previous) = previous {
                previous.await;
            }
            let result = this.run_turn(&input, on_event, epoch).await;
            drop(done);
            {
                let mut state = this.state.lock();
                if state
                    .turns_by_thread
                    .get(&session_id)
                    .is_some_and(|tail| tail.ptr_eq(&mine))
                {
                    state.turns_by_thread.remove(&session_id);
                }
            }
            result
        }
        .boxed()
    }

    /// The body of one queued turn.
    async fn run_turn(
        self: &Arc<Self>,
        input: &SendTurnInput,
        on_event: EventSink,
        epoch: u64,
    ) -> Result<()> {
        let session_id = &input.session.session_id;
        let cancelled = || self.cancel_epoch(session_id) != epoch;
        if cancelled() {
            return Ok(());
        }
        // This send owns the turn: it may recycle a stale or wrong-cwd
        // transport. A superseded step is a quiet no-op, not an error.
        let live = match self.ensure_live(input, &on_event, true).await {
            Ok(live) => live,
            Err(error) => {
                if cancelled() {
                    return Ok(());
                }
                return Err(error);
            }
        };
        if cancelled() {
            return Ok(());
        }
        // Bind the listener and policy only now, so a queued send cannot
        // redirect or repolicy the turn still running.
        {
            let mut state = live.state.lock();
            state.on_event = on_event;
            state.runtime_mode = input.session.runtime_mode;
            state.planning = is_plan(input);
            state.cancelled = false;
            state.mute_updates = false;
            // From here until the prompt settles, a cancel can interrupt a
            // state-changing request whose server-side effect is unknowable.
            state.turn_active = true;
        }
        let result = async {
            self.apply_model_selection(&live, input).await?;
            if live.state.lock().cancelled || cancelled() {
                return Ok(());
            }
            self.apply_runtime_mode(&live, input.session.runtime_mode, is_plan(input))
                .await?;
            if live.state.lock().cancelled || cancelled() {
                return Ok(());
            }
            self.prompt(&live, input).await
        }
        .await;
        let outcome = match result {
            Ok(()) => Ok(()),
            Err(error) => {
                if live.state.lock().cancelled {
                    Ok(())
                } else {
                    // A timed-out or failed turn leaves the process state
                    // unknowable. Keep the provider session id, but recycle
                    // this generation's child so the next turn resumes on a
                    // fresh transport, unless a newer live already replaced it.
                    if self.is_current(&live) {
                        self.teardown_live(&live).await;
                    }
                    Err(error)
                }
            }
        };
        live.state.lock().turn_active = false;
        outcome
    }

    /// `steerAntigravityTurn`.
    pub async fn steer_antigravity_turn(&self, _input: SteerTurnInput) -> Result<()> {
        bail!("Antigravity does not support steering an in-flight turn")
    }

    /// `respondAntigravityApproval`.
    pub fn respond_antigravity_approval(
        &self,
        session_id: &str,
        request_id: i64,
        decision: ApprovalDecision,
    ) {
        if let Some(live) = self.live(session_id)
            && let Some(resolve) = live.state.lock().approvals.remove(&request_id)
        {
            let _ = resolve.send(decision);
        }
    }

    /// `cancelAntigravityTurn`. Everything happens before the first await,
    /// so this is synchronous; the wire notify is fire and forget.
    pub fn cancel_antigravity_turn_now(&self, session_id: &str) {
        self.bump_epochs(session_id);
        // Also retire queued and in-flight lifecycle work: a cancel before
        // the child exists must not leave a process spawning.
        self.abort_pending_setup(session_id);
        let Some(live) = self.live(session_id) else {
            return;
        };
        {
            let mut state = live.state.lock();
            state.cancelled = true;
            state.mute_updates = true;
            // A prompt cancelled on the wire may still run server-side, and a
            // control request in flight may already have taken effect, so
            // the next send must not reuse this transport.
            if state.turn_active {
                state.stale = true;
            }
            for (_, resolve) in state.approvals.drain() {
                let _ = resolve.send(ApprovalDecision::Deny);
            }
        }
        // Unwind the local turn first: a blocked stdin must not keep the
        // send (or this cancel) wedged.
        live.acp.reject_pending(Some("cancelled"));
        let acp = live.acp.clone();
        let acp_session_id = live.acp_session_id.clone();
        self.spawner.spawn(Box::pin(async move {
            let _ = acp
                .notify(
                    "session/cancel",
                    Some(json!({ "sessionId": acp_session_id })),
                )
                .await;
        }));
    }

    /// `cancelAntigravityTurn`.
    pub async fn cancel_antigravity_turn(&self, session_id: &str) -> Result<()> {
        self.cancel_antigravity_turn_now(session_id);
        Ok(())
    }

    /// `teardownLive`: tear down exactly this generation. Internal recycles
    /// go through here and must not bump the user-intent epochs.
    async fn teardown_live(&self, live: &Arc<Live>) {
        {
            let mut state = self.state.lock();
            if state
                .live_by_thread
                .get(&live.thread_id)
                .is_some_and(|current| Arc::ptr_eq(current, live))
            {
                state.live_by_thread.remove(&live.thread_id);
            }
        }
        settle_live(live);
        live.acp.close(None);
        self.children.unwatch_child(&live.child_key);
        let _ = self.children.kill_child(&live.child_key).await;
    }

    /// `stopAntigravitySession`. Stopping invalidates work already queued for
    /// the session, not just the registered live.
    pub async fn stop_antigravity_session(&self, session_id: &str) -> Result<()> {
        self.bump_epochs(session_id);
        self.abort_pending_setup(session_id);
        if let Some(live) = self.live(session_id) {
            self.teardown_live(&live).await;
        }
        Ok(())
    }

    /// `abortPendingSetup`: a setup still inside initialize or the session
    /// requests owns no `Live` yet, so reject its pending calls and kill its
    /// child now instead of after the session-request timeout.
    fn abort_pending_setup(&self, session_id: &str) {
        let pending = self
            .state
            .lock()
            .pending_setup_by_thread
            .get(session_id)
            .map(|pending| (pending.acp.clone(), pending.child_key.clone()));
        let Some((acp, child_key)) = pending else {
            return;
        };
        // close() rejects in-flight requests and fails any later request at
        // once, so a cancelled setup cannot fall into another 45s wait.
        acp.close(Some("cancelled"));
        let children = self.children.clone();
        self.spawner.spawn(Box::pin(async move {
            let _ = children.kill_child(&child_key).await;
        }));
    }

    /// `forgetAntigravitySession`.
    pub async fn forget_antigravity_session(&self, session_id: &str) -> Result<()> {
        self.state.lock().resume_by_thread.remove(session_id);
        self.stop_antigravity_session(session_id).await
    }

    /// `bindAntigravitySession`.
    pub fn bind_antigravity_session(&self, thread_id: &str, acp_session_id: &str, cwd: &str) {
        let session_id = monocode_core::js::trim(acp_session_id);
        if thread_id.is_empty() || session_id.is_empty() || monocode_core::js::trim(cwd).is_empty()
        {
            return;
        }
        self.state.lock().resume_by_thread.insert(
            thread_id.to_string(),
            Resume {
                acp_session_id: session_id.to_string(),
                cwd: cwd.to_string(),
            },
        );
    }

    /// `noteActivity`: re-arm the stall clock. Any inbound traffic and the
    /// end of an approval wait call this.
    fn note_activity(self: &Arc<Self>, live: &Arc<Live>) {
        let mut state = live.state.lock();
        state.stall_notified = false;
        if !state.prompt_in_flight {
            return;
        }
        let weak_live = Arc::downgrade(live);
        let weak_self = Arc::downgrade(self);
        state.watchdog = Some(start_timer(
            &self.spawner,
            self.options.stall_notify_ms,
            move |token| {
                let (Some(live), Some(this)) = (weak_live.upgrade(), weak_self.upgrade()) else {
                    return;
                };
                let sink = {
                    let mut state = live.state.lock();
                    if state.watchdog.as_ref().map(|timer| timer.token) != Some(token) {
                        return;
                    }
                    state.watchdog = None;
                    drop(state);
                    if !this.is_current(&live) {
                        return;
                    }
                    let mut state = live.state.lock();
                    if !state.prompt_in_flight
                        || !state.approvals.is_empty()
                        || state.stall_notified
                    {
                        return;
                    }
                    state.stall_notified = true;
                    state.on_event.clone()
                };
                sink(HarnessEvent::Status {
                    text: STALL_STATUS.into(),
                });
            },
        ));
    }

    fn ensure_live(
        self: &Arc<Self>,
        input: &SendTurnInput,
        on_event: &EventSink,
        recycle: bool,
    ) -> BoxFuture<'static, Result<Arc<Live>>> {
        if let Some(existing) = self.live(&input.session.session_id) {
            let reusable =
                !recycle || (!existing.state.lock().stale && existing.cwd == input.session.cwd);
            if reusable {
                return futures::future::ready(Ok(existing)).boxed();
            }
        }
        self.queue_lifecycle(input, on_event, recycle)
    }

    /// `queueLifecycle`: one lifecycle step at a time per thread.
    fn queue_lifecycle(
        self: &Arc<Self>,
        input: &SendTurnInput,
        on_event: &EventSink,
        recycle: bool,
    ) -> BoxFuture<'static, Result<Arc<Live>>> {
        let session_id = input.session.session_id.clone();
        let (life, previous, done) = {
            let mut state = self.state.lock();
            let life = state.session_epoch.get(&session_id).copied().unwrap_or(0);
            let (done, mine) = new_tail();
            let previous = state.lifecycle_by_thread.insert(session_id.clone(), mine);
            (life, previous, done)
        };
        let this = self.clone();
        let input = input.clone();
        let on_event = on_event.clone();
        async move {
            let _done = done;
            if let Some(previous) = previous {
                previous.await;
            }
            if this.session_epoch(&session_id) != life {
                bail!("Antigravity session superseded");
            }
            if let Some(live) = this.live(&session_id) {
                if !recycle {
                    return Ok(live);
                }
                if !live.state.lock().stale && live.cwd == input.session.cwd {
                    return Ok(live);
                }
                if live.cwd != input.session.cwd {
                    this.state.lock().resume_by_thread.remove(&session_id);
                }
                this.teardown_live(&live).await;
            }
            this.start_live(&input, on_event, life).await
        }
        .boxed()
    }

    async fn start_live(
        self: &Arc<Self>,
        input: &SendTurnInput,
        on_event: EventSink,
        life: u64,
    ) -> Result<Arc<Live>> {
        let session_id = input.session.session_id.clone();
        let cwd = input.session.cwd.clone();
        let resume = {
            let mut state = self.state.lock();
            let resume = state.resume_by_thread.get(&session_id).cloned();
            if resume.as_ref().is_some_and(|resume| resume.cwd != cwd) {
                state.resume_by_thread.remove(&session_id);
            }
            resume
        };
        let can_load = resume.as_ref().is_some_and(|resume| resume.cwd == cwd);
        let retired = || self.session_epoch(&session_id) != life;
        let stopped = || anyhow!("Antigravity session stopped during startup");
        if retired() {
            return Err(stopped());
        }
        let child_key = {
            let mut state = self.state.lock();
            let key = format!("{session_id}#{}", state.child_seq);
            state.child_seq += 1;
            key
        };

        let resolved = self.children.resolve_antigravity_binary().await?;
        let path = resolved.path;
        let args = resolved.args.unwrap_or_default();
        let live_ref: Arc<Mutex<Weak<Live>>> = Arc::new(Mutex::new(Weak::new()));
        let weak_self = Arc::downgrade(self);
        let handlers = AcpHandlers::default()
            .on_notification({
                let live_ref = live_ref.clone();
                let weak_self = weak_self.clone();
                move |method, params| {
                    let (Some(live), Some(this)) = (live_ref.lock().upgrade(), weak_self.upgrade()) else {
                        return;
                    };
                    this.note_activity(&live);
                    handle_notification(&live, method, &params);
                }
            })
            .on_request({
                let live_ref = live_ref.clone();
                let weak_self = weak_self.clone();
                let spawner = self.spawner.clone();
                let children = self.children.clone();
                let child_key = child_key.clone();
                move |id, method, params| {
                    let live = live_ref.lock().upgrade();
                    let Some(this) = weak_self.upgrade() else {
                        return;
                    };
                    if let Some(live) = &live {
                        this.note_activity(live);
                    }
                    let Some(live) = live else {
                        let line = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32601, "message": format!("Method not found: {method}") },
                        })
                        .to_string();
                        let children = children.clone();
                        let child_key = child_key.clone();
                        spawner.spawn(Box::pin(async move {
                            let _ = children.write_child(&child_key, &line).await;
                        }));
                        return;
                    };
                    let method = method.to_string();
                    run_now(
                        &spawner,
                        Box::pin(async move {
                            if let Err(error) = this.handle_request(&live, id, &method, &params).await {
                                // A reply that could not be written is a
                                // transport failure: the provider waits for
                                // an answer that will never arrive. Fail the
                                // generation so the turn unwinds.
                                if !this.is_current(&live) || live.state.lock().cancelled {
                                    return;
                                }
                                live.state.lock().stale = true;
                                live.acp.close(Some(&error.to_string()));
                            }
                        }),
                    );
                }
            });
        let acp = AcpClient::with_options(
            &child_key,
            Arc::new(self.children.clone()),
            handlers,
            self.options.rpc.clone(),
        );
        let token = {
            let mut state = self.state.lock();
            state.setup_tokens += 1;
            let token = state.setup_tokens;
            state.pending_setup_by_thread.insert(
                session_id.clone(),
                PendingSetup {
                    acp: acp.clone(),
                    child_key: child_key.clone(),
                    token,
                },
            );
            token
        };
        let clear_pending = || {
            let mut state = self.state.lock();
            if state
                .pending_setup_by_thread
                .get(&session_id)
                .is_some_and(|pending| pending.token == token)
            {
                state.pending_setup_by_thread.remove(&session_id);
            }
        };

        // These handlers outlive the turn that created them. Routing through
        // the live record keeps them on the current turn's listener.
        let emit = {
            let live_ref = live_ref.clone();
            let fallback = on_event.clone();
            move |event: HarnessEvent| {
                let sink = live_ref
                    .lock()
                    .upgrade()
                    .map(|live| live.sink())
                    .unwrap_or_else(|| fallback.clone());
                sink(event);
            }
        };
        let emit = Arc::new(emit);

        self.children.watch_child_with(
            &child_key,
            ChildHandlers {
                on_line: Box::new({
                    let acp = acp.clone();
                    move |line| acp.push_line(&line)
                }),
                on_exit: Box::new({
                    let acp = acp.clone();
                    let live_ref = live_ref.clone();
                    let weak_self = weak_self.clone();
                    let child_key = child_key.clone();
                    let session_id = session_id.clone();
                    let emit = emit.clone();
                    move |code| {
                        let Some(this) = weak_self.upgrade() else {
                            return;
                        };
                        this.children.unwatch_child(&child_key);
                        let live = live_ref.lock().upgrade();
                        {
                            let mut state = this.state.lock();
                            let current = state.live_by_thread.get(&session_id).cloned();
                            let same = match (&current, &live) {
                                (Some(current), Some(live)) => Arc::ptr_eq(current, live),
                                (None, None) => true,
                                _ => false,
                            };
                            if same {
                                state.live_by_thread.remove(&session_id);
                            }
                        }
                        // Settle before closing, so in-flight requests unwind as
                        // cancelled rather than as a transport error.
                        if let Some(live) = &live {
                            settle_live(live);
                        }
                        acp.close(Some("Antigravity exited"));
                        // A generation retired by user intent settles quietly.
                        if live.is_some() || this.session_epoch(&session_id) == life {
                            emit(HarnessEvent::SessionEnded {
                                code: code.map(i64::from),
                            });
                        }
                    }
                }),
                on_stderr: Some(Box::new(|line| {
                    log::debug!("[monocode] antigravity stderr {line}")
                })),
            },
        );

        let setup = async {
            // Binary resolution may have raced a stop or forget: re-check
            // before the child is ever spawned.
            if retired() {
                return Err(stopped());
            }
            self.children
                .spawn_child(
                    &child_key,
                    &path,
                    args.clone(),
                    &antigravity_spawn_cwd(&path, &cwd),
                    None,
                    Some(HarnessId::Antigravity),
                )
                .await?;
            if retired() {
                return Err(stopped());
            }
            acp.request_value(
                "initialize",
                Some(json!({
                    "protocolVersion": 1,
                    "clientCapabilities": client_capabilities(),
                    "clientInfo": { "name": "monocode", "version": "0.1.0" },
                })),
                self.options.init_timeout_ms,
            )
            .await
            .map_err(|error| antigravity_error(&error))?;

            let mut setup: Option<Value> = None;
            let mut acp_session_id: Option<String> = None;
            let mut did_load = false;

            if can_load && let Some(resume) = &resume {
                let params =
                    json!({ "sessionId": resume.acp_session_id, "cwd": cwd, "mcpServers": [] });
                match acp
                    .request_value(
                        "session/resume",
                        Some(params.clone()),
                        self.options.session_timeout_ms,
                    )
                    .await
                {
                    Ok(result) => {
                        acp_session_id = Some(
                            session_id_from_result(&result)
                                .unwrap_or_else(|| resume.acp_session_id.clone()),
                        );
                        setup = Some(result);
                        did_load = true;
                    }
                    Err(error) => {
                        // User intent wins over the fallback ladder.
                        if retired() {
                            return Err(stopped());
                        }
                        // A resume that timed out may still run server-side:
                        // never stack session/load or a new session on it.
                        if is_timeout(&error) {
                            return Err(error);
                        }
                        match acp
                            .request_value(
                                "session/load",
                                Some(params),
                                self.options.session_timeout_ms,
                            )
                            .await
                        {
                            Ok(result) => {
                                acp_session_id = Some(
                                    session_id_from_result(&result)
                                        .unwrap_or_else(|| resume.acp_session_id.clone()),
                                );
                                setup = Some(result);
                                did_load = true;
                            }
                            Err(load_error) => {
                                if retired() {
                                    return Err(stopped());
                                }
                                if is_timeout(&load_error) {
                                    return Err(load_error);
                                }
                            }
                        }
                    }
                }
            }

            if acp_session_id.is_none() {
                if retired() {
                    return Err(stopped());
                }
                let dropped_binding = can_load && resume.is_some();
                let created = acp
                    .request_value(
                        "session/new",
                        Some(json!({ "cwd": cwd, "mcpServers": [] })),
                        self.options.session_timeout_ms,
                    )
                    .await?;
                acp_session_id = session_id_from_result(&created);
                setup = Some(created);
                if acp_session_id.is_some() && dropped_binding {
                    emit(HarnessEvent::Status {
                        text: RESTORE_FAILED_STATUS.into(),
                    });
                }
            }
            let Some(acp_session_id) = acp_session_id else {
                bail!("Antigravity did not return a session id");
            };
            if retired() {
                return Err(stopped());
            }

            let config_options =
                read_config_options(setup.as_ref().and_then(|setup| setup.get("configOptions")));
            let live = Arc::new(Live {
                thread_id: session_id.clone(),
                child_key: child_key.clone(),
                acp: acp.clone(),
                acp_session_id: acp_session_id.clone(),
                cwd: cwd.clone(),
                state: Mutex::new(LiveState {
                    subagents: AcpSubagents::new(),
                    model_config_id: extract_model_config_id(&config_options),
                    config_options,
                    mute_updates: did_load,
                    cancelled: false,
                    stale: false,
                    runtime_mode: input.session.runtime_mode,
                    planning: is_plan(input),
                    on_event: on_event.clone(),
                    approvals: HashMap::new(),
                    prompt_in_flight: false,
                    turn_active: false,
                    stall_notified: false,
                    watchdog: None,
                }),
            });
            *live_ref.lock() = Arc::downgrade(&live);
            {
                let mut state = self.state.lock();
                state
                    .live_by_thread
                    .insert(session_id.clone(), live.clone());
                state.resume_by_thread.insert(
                    session_id.clone(),
                    Resume {
                        acp_session_id: acp_session_id.clone(),
                        cwd: cwd.clone(),
                    },
                );
            }
            // Registration ends before the first listener callback: a cancel
            // reentered from on_event must take the live path, never
            // abort_pending_setup, which would orphan this transport.
            clear_pending();
            let sink = live.sink();
            call_listener(
                &sink,
                HarnessEvent::SessionProviderBound {
                    provider_session_id: acp_session_id,
                },
            )?;
            call_listener(&sink, HarnessEvent::SessionStarted)?;
            Ok(live)
        };

        let result = setup.await;
        clear_pending();
        match result {
            Ok(live) => Ok(live),
            Err(error) => {
                // Clean up only this generation's child: a successor may
                // already own the thread slot. A listener that threw after
                // publish is the exception: drop the dead transport this
                // generation registered.
                acp.close(Some(&error.to_string()));
                self.children.unwatch_child(&child_key);
                let _ = self.children.kill_child(&child_key).await;
                if let Some(live) = live_ref.lock().upgrade() {
                    let mut state = self.state.lock();
                    if state
                        .live_by_thread
                        .get(&session_id)
                        .is_some_and(|current| Arc::ptr_eq(current, &live))
                    {
                        state.live_by_thread.remove(&session_id);
                    }
                }
                Err(antigravity_error(&error))
            }
        }
    }

    /// `applyModelSelection`.
    async fn apply_model_selection(&self, live: &Arc<Live>, input: &SendTurnInput) -> Result<()> {
        let base = self
            .catalog
            .read()
            .native_model_id_for(&input.session.model);
        let model_config_id = {
            let state = live.state.lock();
            if state.model_config_id == "provider" {
                "model".to_string()
            } else {
                state.model_config_id.clone()
            }
        };
        self.set_config_option(live, &model_config_id, &base)
            .await?;
        for (setting_id, value) in input.session.model_settings.iter().flatten() {
            let config_id =
                resolve_setting_config_id(&live.state.lock().config_options, setting_id);
            let Some(config_id) = config_id.filter(|id| id != "provider") else {
                continue;
            };
            self.set_config_option(live, &config_id, value).await?;
        }
        Ok(())
    }

    /// `applyRuntimeMode`. Fails closed: a rejected downgrade must not leave
    /// a prior yolo mode active.
    async fn apply_runtime_mode(
        &self,
        live: &Arc<Live>,
        runtime_mode: RuntimeMode,
        planning: bool,
    ) -> Result<()> {
        live.acp
            .request_value(
                "session/set_mode",
                Some(json!({
                    "sessionId": live.acp_session_id,
                    "modeId": antigravity_mode_id(runtime_mode, planning).as_str(),
                })),
                self.options.control_timeout_ms,
            )
            .await?;
        Ok(())
    }

    /// `setConfigOption`. Only options the session advertised are valid.
    async fn set_config_option(
        &self,
        live: &Arc<Live>,
        config_id: &str,
        value: &str,
    ) -> Result<()> {
        let (is_bool, bool_value) = {
            let state = live.state.lock();
            let Some(current) = state
                .config_options
                .iter()
                .find(|option| option.id == config_id)
            else {
                return Ok(());
            };
            // Boolean options take the typed variant; everything else is a
            // value-id string.
            let is_bool = current.kind.as_deref() == Some("boolean");
            if is_bool && value != "true" && value != "false" {
                // A stray string must not be coerced into `false`.
                return Ok(());
            }
            let bool_value = value == "true";
            let already = if is_bool {
                current.current_value == Some(ConfigValue::Bool(bool_value))
            } else {
                current
                    .current_value
                    .as_ref()
                    .map(ConfigValue::as_js_string)
                    .unwrap_or_default()
                    == value
            };
            if already {
                return Ok(());
            }
            (is_bool, bool_value)
        };
        let mut params = Map::new();
        params.insert("sessionId".into(), Value::from(live.acp_session_id.clone()));
        params.insert("configId".into(), Value::from(config_id));
        params.insert(
            "value".into(),
            if is_bool {
                Value::Bool(bool_value)
            } else {
                Value::from(value)
            },
        );
        if is_bool {
            params.insert("type".into(), Value::from("boolean"));
        }
        let result = live
            .acp
            .request_value(
                "session/set_config_option",
                Some(Value::Object(params)),
                self.options.control_timeout_ms,
            )
            .await?;
        if let Some(options @ Value::Array(_)) = result.get("configOptions") {
            let mut state = live.state.lock();
            state.config_options = read_config_options(Some(options));
            state.model_config_id = extract_model_config_id(&state.config_options);
        }
        Ok(())
    }

    /// `prompt`.
    async fn prompt(self: &Arc<Self>, live: &Arc<Live>, input: &SendTurnInput) -> Result<()> {
        let result = async {
            let blocks =
                antigravity_prompt_blocks(&input.text, input.attachments.as_deref().unwrap_or(&[]))
                    .map_err(|error| anyhow!(error))?;
            if blocks.is_empty() {
                return Ok(());
            }
            live.state.lock().prompt_in_flight = true;
            self.note_activity(live);
            let result = live
                .acp
                .request_value(
                    "session/prompt",
                    Some(json!({ "sessionId": live.acp_session_id, "prompt": blocks })),
                    self.options.prompt_timeout_ms,
                )
                .await?;
            let stop_reason = result.get("stopReason").filter(|value| !value.is_null());
            if live.state.lock().cancelled || stop_reason == Some(&Value::from("cancelled")) {
                return Ok(());
            }
            // A fulfilled request is not always a normal end: refusals,
            // truncations, and provider-specific reasons surface as errors.
            if let Some(reason) = stop_reason
                && reason != "end_turn"
            {
                live.sink()(HarnessEvent::SessionError {
                    message: format!(
                        "Antigravity ended the turn ({}).",
                        crate::core::json_text::js_string(reason)
                    ),
                });
                return Ok(());
            }
            let sink = live.sink();
            sink(HarnessEvent::MessageCompleted);
            sink(HarnessEvent::ReasoningCompleted);
            Ok(())
        }
        .await;
        {
            let mut state = live.state.lock();
            state.prompt_in_flight = false;
            state.watchdog = None;
        }
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                if live.state.lock().cancelled {
                    return Ok(());
                }
                let failure = antigravity_error(&error);
                live.sink()(HarnessEvent::SessionError {
                    message: failure.to_string(),
                });
                Err(failure)
            }
        }
    }

    /// `handleRequest`. A failed reply write propagates to the caller.
    async fn handle_request(
        self: &Arc<Self>,
        live: &Arc<Live>,
        id: i64,
        method: &str,
        params: &Value,
    ) -> Result<()> {
        if method == "session/request_permission" {
            return self.handle_permission(live, id, params).await;
        }
        live.acp
            .respond_error(
                id,
                RpcErrorBody {
                    code: -32601,
                    message: format!("Method not found: {method}"),
                    data: None,
                },
            )
            .await
    }

    /// `handlePermission`. Protocol replies always flow; UI events only while
    /// the turn is live, so a cancelled run cannot leave approval cards.
    async fn handle_permission(
        self: &Arc<Self>,
        live: &Arc<Live>,
        id: i64,
        params: &Value,
    ) -> Result<()> {
        let request = permission_request_from_acp(params);
        let (in_flight, planning, runtime_mode, sink) = {
            let state = live.state.lock();
            (
                state.prompt_in_flight && !state.cancelled && !state.mute_updates,
                state.planning,
                state.runtime_mode,
                state.on_event.clone(),
            )
        };
        if let Some(call_id) = &request.call_id
            && in_flight
        {
            sink(HarnessEvent::ToolUpdated {
                agent_model: None,
                call_id: call_id.clone(),
                title: Some(request.title.clone()),
                kind: request.kind.clone(),
                status: None,
                detail: None,
                preview: request.preview.clone(),
                paths: None,
            });
        }
        let kind = request.kind.as_deref();
        let option_id = if !in_flight || request.option_ids.is_empty() {
            None
        } else if planning {
            let decision = if kind == Some("read") || kind == Some("search") {
                ApprovalDecision::Allow
            } else {
                ApprovalDecision::Deny
            };
            permission_option_id(decision, &request.option_ids, &request.option_kinds)
        } else {
            match auto_permission_option(
                runtime_mode,
                kind,
                &request.option_ids,
                &request.option_kinds,
            ) {
                Some(option) => Some(option),
                None => {
                    let (resolve, pending) = oneshot::channel();
                    live.state.lock().approvals.insert(id, resolve);
                    live.sink()(HarnessEvent::ApprovalRequested {
                        request_id: id,
                        title: request.title.clone(),
                        kind: request.kind.clone(),
                        call_id: request.call_id.clone(),
                        preview: request.preview.clone(),
                    });
                    let decision = pending.await.unwrap_or(ApprovalDecision::Deny);
                    live.state.lock().approvals.remove(&id);
                    // The approval wait itself was the silence.
                    self.note_activity(live);
                    // Always close the card, even for a cancelled or exited turn.
                    live.sink()(HarnessEvent::ApprovalResolved {
                        request_id: id,
                        decision: match decision {
                            ApprovalDecision::Allow => ApprovalDecided::Allow,
                            ApprovalDecision::Deny => ApprovalDecided::Deny,
                        },
                    });
                    if live.state.lock().cancelled {
                        None
                    } else {
                        permission_option_id(decision, &request.option_ids, &request.option_kinds)
                    }
                }
            }
        };
        let outcome = match option_id {
            Some(option_id) => json!({ "outcome": "selected", "optionId": option_id }),
            None => json!({ "outcome": "cancelled" }),
        };
        live.acp.respond(id, json!({ "outcome": outcome })).await
    }
}

/// Call a listener the way the TypeScript did, where a throwing listener
/// failed the startup. A Rust listener that panics fails it the same way.
fn call_listener(sink: &EventSink, event: HarnessEvent) -> Result<()> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(event))).map_err(|panic| {
        let message = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
            .unwrap_or_else(|| "listener failed".into());
        anyhow!(message)
    })
}

/// `settleLive`: the terminal settle for a retired transport.
fn settle_live(live: &Arc<Live>) {
    let mut state = live.state.lock();
    state.cancelled = true;
    state.mute_updates = true;
    state.prompt_in_flight = false;
    state.watchdog = None;
    for (_, resolve) in state.approvals.drain() {
        let _ = resolve.send(ApprovalDecision::Deny);
    }
}

/// `handleNotification`.
fn handle_notification(live: &Arc<Live>, method: &str, params: &Value) {
    if method != "session/update" {
        return;
    }
    let rec = params.as_object();
    let update = rec
        .and_then(|rec| rec.get("update"))
        .and_then(Value::as_object)
        .or(rec);
    let events = {
        let mut state = live.state.lock();
        // Config state stays fresh even while the transcript is muted.
        if let Some(update) = update
            && update.get("sessionUpdate").and_then(Value::as_str) == Some("config_option_update")
            && let Some(options @ Value::Array(_)) = update.get("configOptions")
        {
            state.config_options = read_config_options(Some(options));
            state.model_config_id = extract_model_config_id(&state.config_options);
        }
        if state.mute_updates {
            return;
        }
        state
            .subagents
            .route(params, events_from_acp_update(params))
    };
    let sink = live.sink();
    for event in events {
        sink(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_sign_in_help_to_auth_failures_once() {
        let wrapped = antigravity_error(&anyhow!("Authentication required"));
        assert_eq!(
            wrapped.to_string(),
            format!("Authentication required\n\n{AUTH_HELP}")
        );
        assert_eq!(antigravity_error(&wrapped).to_string(), wrapped.to_string());
        assert!(
            antigravity_error(&anyhow!("Please sign-in first"))
                .to_string()
                .contains(AUTH_HELP)
        );
        assert!(
            antigravity_error(&anyhow!("missing API key"))
                .to_string()
                .contains(AUTH_HELP)
        );
        assert_eq!(
            antigravity_error(&anyhow!("unsupported")).to_string(),
            "unsupported"
        );
        assert!(is_timeout(&anyhow!("session/resume timed out")));
        assert!(!is_timeout(&anyhow!("timed out early")));
    }
}
