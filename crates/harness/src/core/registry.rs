//! Port of src/integrations/harness/core/registry.ts: the `HarnessAdapter`
//! lifecycle, the registry the app dispatches through, idle parking, and the
//! per-session operation queues.
//!
//! The TypeScript kept the registry in module globals. Here it is a
//! [`HarnessRegistry`] value the engine owns and hands to each provider's
//! `register` function. Promises become futures, and the `onEvent` and
//! `onAccepted` callbacks become [`EventSink`] and [`AcceptedHook`]
//! arguments. The `isTauri()` check around the control service becomes an
//! optional [`TurnControl`].

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::Shared;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use monocode_core::block::{Block, BlockRole, ModelSettings, TaskListMeta, TurnIntent};
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, HarnessEvent, RewindLastTurnInput, RewindLastTurnResult,
    SendTurnInput, SteerTurnInput,
};
use monocode_core::user_question::UserQuestionReply;

use super::native_commands::NativeCommandProvider;
use super::session_title::GeneratedSessionTitle;
use super::task::{AbortSignal, BoxFuture, SharedSpawner};

/// `onEvent`: where an adapter reports what its harness did.
pub type EventSink = Arc<dyn Fn(HarnessEvent) + Send + Sync>;

/// `onAccepted`: called once the provider has accepted the user turn.
pub type AcceptedHook = Arc<dyn Fn() + Send + Sync>;

/// `onThreadId` on a text prompt.
pub type ThreadIdHook = Arc<dyn Fn(String) + Send + Sync>;

/// Wrap a closure as an [`EventSink`].
pub fn event_sink(sink: impl Fn(HarnessEvent) + Send + Sync + 'static) -> EventSink {
    Arc::new(sink)
}

/// An [`EventSink`] that drops every event.
pub fn ignore_events() -> EventSink {
    Arc::new(|_| {})
}

/// `TitleInput`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleInput {
    pub session_id: String,
    pub cwd: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_account_id: Option<String>,
}

/// `TextPromptInput`: one-shot, isolated text generation shared by titles and
/// side questions.
#[derive(Clone, Default)]
pub struct TextPromptInput {
    pub cwd: String,
    pub provider_account_id: Option<String>,
    pub model: Option<String>,
    pub model_settings: Option<ModelSettings>,
    pub thread_id: Option<String>,
    pub on_thread_id: Option<ThreadIdHook>,
    pub intent: Option<TurnIntent>,
    pub prompt: String,
    pub timeout_ms: Option<i64>,
    pub signal: Option<AbortSignal>,
    pub on_event: Option<EventSink>,
}

/// `PrContent & { base: string; head: string }`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPrContent {
    pub title: String,
    pub body: String,
    pub base: String,
    pub head: String,
}

/// Which optional `HarnessAdapter` methods an adapter implements. The
/// TypeScript tested `adapter.compactContext != null`; a Rust trait always
/// has every method, so each adapter sets the flag for each optional method it
/// overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AdapterCapabilities {
    pub compact_context: bool,
    pub rewind_last_turn: bool,
    pub respond_question: bool,
    pub keep_question_open: bool,
    pub restore_task_lists: bool,
    pub refresh_catalog: bool,
    pub generate_title: bool,
    pub generate_commit_message: bool,
    pub generate_pr_content: bool,
    pub generate_branch_name: bool,
    pub warmup_text: bool,
    pub run_text_prompt: bool,
    pub stop_text_prompt: bool,
}

fn unsupported<T: Send + 'static>(harness: HarnessId, what: &str) -> BoxFuture<'static, Result<T>> {
    let message = format!("{harness} does not support {what}");
    async move { Err(anyhow!(message)) }.boxed()
}

fn ok<T: Send + 'static>(value: T) -> BoxFuture<'static, Result<T>> {
    async move { Ok(value) }.boxed()
}

/// `HarnessAdapter`: the lifecycle contract for a live harness adapter. The
/// app dispatches through the registry instead of harness-specific branches.
///
/// Async methods return boxed futures so the trait stays object safe. An
/// implementation usually writes `Box::pin(async move { ... })`. Optional
/// methods have defaults that match a missing method in TypeScript; override
/// them and set the matching [`AdapterCapabilities`] flag together.
pub trait HarnessAdapter: Send + Sync {
    fn id(&self) -> HarnessId;

    /// True when this adapter can run live turns.
    fn live(&self) -> bool {
        true
    }

    /// `canSteer`. False when the harness cannot accept a follow-up while a
    /// turn is running.
    fn can_steer(&self) -> bool {
        true
    }

    /// The optional methods this adapter implements.
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::default()
    }

    /// `commands`: the provider's own slash commands.
    fn commands(&self) -> Option<Arc<dyn NativeCommandProvider>> {
        None
    }

    fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>>;

    /// Trigger provider-owned compaction outside the normal user-turn path.
    fn compact_context(
        &self,
        _input: CompactContextInput,
        _on_event: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        unsupported(self.id(), "manual compaction")
    }

    /// Rewind provider state so the last user turn can be replaced.
    fn rewind_last_turn(
        &self,
        _input: RewindLastTurnInput,
        _on_event: EventSink,
    ) -> BoxFuture<'_, Result<RewindLastTurnResult>> {
        unsupported(self.id(), "editing the last message")
    }

    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>>;

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>>;

    fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision);

    fn respond_question(&self, _session_id: &str, _request_id: i64, _reply: UserQuestionReply) {}

    /// Keep a timed question open once the user starts answering it.
    fn keep_question_open(&self, _session_id: &str, _request_id: i64) {}

    /// Kill the child but keep resume state for a later rebind.
    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>>;

    /// Drop resume state and kill the child (delete, harness switch, idle detach).
    fn forget_session(&self, session_id: String) -> BoxFuture<'_, Result<()>>;

    /// Seed resume state from a restored MonoCode session.
    fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        provider_account_id: Option<&str>,
    );

    /// Seed provider task state from a restored session's persisted panels.
    fn restore_task_lists(&self, _thread_id: &str, _lists: Vec<TaskListMeta>) {}

    /// Refresh the model catalog overlay.
    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        ok(())
    }

    /// LLM tab title for the first turn.
    fn generate_title(
        &self,
        _input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        ok(None)
    }

    /// LLM commit message from staged changes.
    fn generate_commit_message(
        &self,
        _cwd: String,
        _signal: Option<AbortSignal>,
    ) -> BoxFuture<'_, Result<String>> {
        unsupported(self.id(), "commit message generation")
    }

    /// LLM pull request title and body from the branch diff.
    fn generate_pr_content(
        &self,
        _cwd: String,
    ) -> BoxFuture<'_, Result<Option<GeneratedPrContent>>> {
        ok(None)
    }

    /// LLM branch name from a user message.
    fn generate_branch_name(
        &self,
        _cwd: String,
        _message: String,
    ) -> BoxFuture<'_, Result<Option<String>>> {
        ok(None)
    }

    /// Warm up a text-generation backend.
    fn warmup_text(&self, _cwd: String) -> BoxFuture<'_, Result<()>> {
        ok(())
    }

    /// Run an isolated, read-only prompt without changing the main session.
    fn run_text_prompt(&self, _input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        unsupported(self.id(), "isolated text prompts")
    }

    /// Stop an isolated text-generation backend.
    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        ok(())
    }
}

/// The control service calls `sendHarnessTurn` made through Tauri
/// (`control_authorize_turn`, `control_turn_finished`). The registry skips
/// them when it has no `TurnControl`, as the TypeScript did outside Tauri.
pub trait TurnControl: Send + Sync {
    fn authorize_turn(&self, session_id: &str, cwd: &str, app_access: bool) -> Result<(), String>;
    fn turn_finished(&self, session_id: &str);
}

/// [`TurnControl`] over the process crate's control service, for one owner
/// (the window label in the Tauri app).
pub struct ControlTurns {
    pub host: Arc<monocode_process::control::ControlHost>,
    pub owner: String,
}

impl TurnControl for ControlTurns {
    fn authorize_turn(&self, session_id: &str, cwd: &str, app_access: bool) -> Result<(), String> {
        monocode_process::control::control_authorize_turn(
            &self.host,
            &self.owner,
            session_id.to_string(),
            cwd.to_string(),
            app_access,
        )
    }

    fn turn_finished(&self, session_id: &str) {
        monocode_process::control::control_turn_finished(&self.host, session_id.to_string());
    }
}

/// `HARNESS_IDLE_PARK_MS`. After a turn settles, keep the child warm for
/// follow-ups, then park it. Resume state stays, so the next prompt respawns
/// instead of starting over.
pub const HARNESS_IDLE_PARK_MS: i64 = 5 * 60_000;

/// Registry settings that were module constants or globals in TypeScript.
#[derive(Clone)]
pub struct RegistryOptions {
    pub turn_control: Option<Arc<dyn TurnControl>>,
    /// Defaults to [`HARNESS_IDLE_PARK_MS`]. Tests shorten it.
    pub idle_park: Duration,
}

impl Default for RegistryOptions {
    fn default() -> Self {
        Self {
            turn_control: None,
            idle_park: Duration::from_millis(HARNESS_IDLE_PARK_MS as u64),
        }
    }
}

type Tail = Shared<BoxFuture<'static, ()>>;

struct IdleTimer {
    token: u64,
    // Dropping the sender closes the channel, which wakes and ends the timer task.
    _cancel: async_channel::Sender<()>,
}

#[derive(Default)]
struct State {
    operation_tails: HashMap<String, Tail>,
    steer_tails: HashMap<String, Tail>,
    active_turns: HashSet<String>,
    idle_park_timers: HashMap<String, IdleTimer>,
    text_prompt_owners: HashMap<HarnessId, HashSet<u64>>,
    next_token: u64,
}

impl State {
    fn token(&mut self) -> u64 {
        self.next_token += 1;
        self.next_token
    }
}

struct Inner {
    spawner: SharedSpawner,
    options: RegistryOptions,
    // A Vec keeps registration order, like the TypeScript Map.
    adapters: Mutex<Vec<Arc<dyn HarnessAdapter>>>,
    state: Mutex<State>,
}

/// The harness registry. Clones share one registry.
#[derive(Clone)]
pub struct HarnessRegistry {
    inner: Arc<Inner>,
}

/// Which tail map a queued operation belongs to.
#[derive(Clone, Copy)]
enum Queue {
    Operation,
    Steer,
}

/// Settles a queue tail when the operation ends, however it ends.
struct SettleGuard {
    registry: Weak<Inner>,
    queue: Queue,
    session_id: String,
    settled: Tail,
    done: Option<oneshot::Sender<()>>,
}

impl Drop for SettleGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.registry.upgrade() {
            let mut state = inner.state.lock();
            let tails = match self.queue {
                Queue::Operation => &mut state.operation_tails,
                Queue::Steer => &mut state.steer_tails,
            };
            if tails
                .get(&self.session_id)
                .is_some_and(|tail| tail.ptr_eq(&self.settled))
            {
                tails.remove(&self.session_id);
            }
        }
        if let Some(done) = self.done.take() {
            let _ = done.send(());
        }
    }
}

fn new_tail() -> (oneshot::Sender<()>, Tail) {
    let (done, settled) = oneshot::channel::<()>();
    (done, settled.map(|_| ()).boxed().shared())
}

fn cancelled_text_prompt() -> anyhow::Error {
    anyhow!("By-the-way request cancelled")
}

fn not_connected(harness: HarnessId) -> anyhow::Error {
    anyhow!("{harness} is not connected yet")
}

impl HarnessRegistry {
    pub fn new(spawner: SharedSpawner, options: RegistryOptions) -> Self {
        Self {
            inner: Arc::new(Inner {
                spawner,
                options,
                adapters: Mutex::new(Vec::new()),
                state: Mutex::new(State::default()),
            }),
        }
    }

    pub fn spawner(&self) -> &SharedSpawner {
        &self.inner.spawner
    }

    /// `registerHarness`. A second adapter with the same id replaces the
    /// first in place.
    pub fn register_harness(&self, adapter: Arc<dyn HarnessAdapter>) {
        let mut adapters = self.inner.adapters.lock();
        let id = adapter.id();
        match adapters.iter_mut().find(|entry| entry.id() == id) {
            Some(entry) => *entry = adapter,
            None => adapters.push(adapter),
        }
    }

    /// True when an adapter with this id is registered.
    pub fn is_registered(&self, id: HarnessId) -> bool {
        self.get_harness(id).is_some()
    }

    /// `getHarness`.
    pub fn get_harness(&self, id: HarnessId) -> Option<Arc<dyn HarnessAdapter>> {
        self.inner
            .adapters
            .lock()
            .iter()
            .find(|adapter| adapter.id() == id)
            .cloned()
    }

    /// `requireHarness`.
    pub fn require_harness(&self, id: HarnessId) -> Result<Arc<dyn HarnessAdapter>> {
        self.get_harness(id)
            .ok_or_else(|| anyhow!("No harness adapter registered for \"{id}\""))
    }

    /// `isLiveHarness`.
    pub fn is_live_harness(&self, id: HarnessId) -> bool {
        self.get_harness(id).is_some_and(|adapter| adapter.live())
    }

    /// `listHarnesses`, in registration order.
    pub fn list_harnesses(&self) -> Vec<Arc<dyn HarnessAdapter>> {
        self.inner.adapters.lock().clone()
    }

    /// Run `operation` on the spawner, so it runs whether or not the caller
    /// polls the returned future, as a promise would.
    fn run_detached<T: Send + 'static>(
        &self,
        operation: impl Future<Output = Result<T>> + Send + 'static,
    ) -> BoxFuture<'static, Result<T>> {
        let (result_tx, result_rx) = oneshot::channel();
        self.inner.spawner.spawn(
            async move {
                let _ = result_tx.send(operation.await);
            }
            .boxed(),
        );
        async move {
            result_rx
                .await
                .unwrap_or_else(|_| Err(anyhow!("Harness operation ended without a result")))
        }
        .boxed()
    }

    /// `queueSessionOperation`: provider-state operations for one session run
    /// one at a time, in call order. A failed operation does not block the next.
    fn queue_session_operation<T: Send + 'static>(
        &self,
        session_id: &str,
        operation: impl Future<Output = Result<T>> + Send + 'static,
    ) -> BoxFuture<'static, Result<T>> {
        let (done, settled) = new_tail();
        let previous = self
            .inner
            .state
            .lock()
            .operation_tails
            .insert(session_id.to_string(), settled.clone());
        let guard = SettleGuard {
            registry: Arc::downgrade(&self.inner),
            queue: Queue::Operation,
            session_id: session_id.to_string(),
            settled,
            done: Some(done),
        };
        self.run_detached(async move {
            let _guard = guard;
            if let Some(previous) = previous {
                previous.await;
            }
            operation.await
        })
    }

    /// `queueSteerOperation`. A live turn must stay steerable while its long
    /// send is pending. Other provider-state operations still form a barrier
    /// for the steer.
    fn queue_steer_operation<T: Send + 'static>(
        &self,
        session_id: &str,
        operation: impl Future<Output = Result<T>> + Send + 'static,
    ) -> BoxFuture<'static, Result<T>> {
        let (done, settled) = new_tail();
        let (previous_steer, barrier) = {
            let mut state = self.inner.state.lock();
            let barrier = if state.active_turns.contains(session_id) {
                None
            } else {
                state.operation_tails.get(session_id).cloned()
            };
            let previous = state
                .steer_tails
                .insert(session_id.to_string(), settled.clone());
            (previous, barrier)
        };
        let guard = SettleGuard {
            registry: Arc::downgrade(&self.inner),
            queue: Queue::Steer,
            session_id: session_id.to_string(),
            settled,
            done: Some(done),
        };
        self.run_detached(async move {
            let _guard = guard;
            let previous = async {
                if let Some(previous) = previous_steer {
                    previous.await;
                }
            };
            let barrier = async {
                if let Some(barrier) = barrier {
                    barrier.await;
                }
            };
            futures::join!(previous, barrier);
            operation.await
        })
    }

    fn cancel_idle_park(&self, session_id: &str) {
        self.inner.state.lock().idle_park_timers.remove(session_id);
    }

    fn schedule_idle_park(&self, harness: HarnessId, session_id: &str) {
        let (cancel, cancelled) = async_channel::bounded::<()>(1);
        let token = {
            let mut state = self.inner.state.lock();
            let token = state.token();
            state.idle_park_timers.insert(
                session_id.to_string(),
                IdleTimer {
                    token,
                    _cancel: cancel,
                },
            );
            token
        };
        let registry = Arc::downgrade(&self.inner);
        let delay = self.inner.options.idle_park;
        let session_id = session_id.to_string();
        self.inner.spawner.spawn(
            async move {
                let fired = smol::future::or(
                    async {
                        smol::Timer::after(delay).await;
                        true
                    },
                    async {
                        let _ = cancelled.recv().await;
                        false
                    },
                )
                .await;
                if !fired {
                    return;
                }
                let Some(inner) = registry.upgrade() else {
                    return;
                };
                {
                    let mut state = inner.state.lock();
                    if state
                        .idle_park_timers
                        .get(&session_id)
                        .is_none_or(|timer| timer.token != token)
                    {
                        return;
                    }
                    state.idle_park_timers.remove(&session_id);
                }
                let registry = HarnessRegistry { inner };
                let _ = registry.stop_harness_session(harness, &session_id).await;
            }
            .boxed(),
        );
    }

    /// `resetHarnessIdlePark`. Test seam.
    pub fn reset_harness_idle_park(&self) {
        self.inner.state.lock().idle_park_timers.clear();
    }

    /// `sendHarnessTurn`.
    pub fn send_harness_turn(
        &self,
        harness: HarnessId,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'static, Result<()>> {
        let registry = self.clone();
        let session_id = input.session.session_id.clone();
        self.queue_session_operation(&session_id.clone(), async move {
            let adapter = registry.require_harness(harness)?;
            if !adapter.live() {
                return Err(not_connected(harness));
            }
            registry.cancel_idle_park(&session_id);
            let control = registry.inner.options.turn_control.clone();
            if let Some(control) = &control {
                control
                    .authorize_turn(
                        &session_id,
                        &input.session.cwd,
                        input.session.app_access == Some(true),
                    )
                    .map_err(|error| anyhow!(error))?;
            }
            registry
                .inner
                .state
                .lock()
                .active_turns
                .insert(session_id.clone());
            let result = adapter.send_turn(input, on_event, on_accepted).await;
            registry.inner.state.lock().active_turns.remove(&session_id);
            if let Some(control) = &control {
                control.turn_finished(&session_id);
            }
            registry.schedule_idle_park(harness, &session_id);
            result
        })
    }

    /// `canCompactHarnessContext`.
    pub fn can_compact_harness_context(&self, id: HarnessId) -> bool {
        self.get_harness(id)
            .is_some_and(|adapter| adapter.live() && adapter.capabilities().compact_context)
    }

    /// `compactHarnessContext`.
    pub fn compact_harness_context(
        &self,
        harness: HarnessId,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> BoxFuture<'static, Result<()>> {
        let registry = self.clone();
        let session_id = input.session_id.clone();
        self.queue_session_operation(&session_id.clone(), async move {
            let adapter = registry.require_harness(harness)?;
            if !adapter.live() {
                return Err(not_connected(harness));
            }
            if !adapter.capabilities().compact_context {
                bail!("{harness} does not support manual compaction");
            }
            registry.cancel_idle_park(&session_id);
            let result = adapter.compact_context(input, on_event).await;
            registry.schedule_idle_park(harness, &session_id);
            result
        })
    }

    /// `canSteerHarness`.
    pub fn can_steer_harness(&self, id: HarnessId) -> bool {
        self.get_harness(id)
            .is_some_and(|adapter| adapter.live() && adapter.can_steer())
    }

    /// `canRewindHarnessLastTurn`.
    pub fn can_rewind_harness_last_turn(&self, id: HarnessId) -> bool {
        self.get_harness(id)
            .is_some_and(|adapter| adapter.live() && adapter.capabilities().rewind_last_turn)
    }

    /// `rewindHarnessLastTurn`.
    pub fn rewind_harness_last_turn(
        &self,
        harness: HarnessId,
        input: RewindLastTurnInput,
        on_event: EventSink,
    ) -> BoxFuture<'static, Result<RewindLastTurnResult>> {
        let registry = self.clone();
        let session_id = input.session.session_id.clone();
        self.queue_session_operation(&session_id.clone(), async move {
            let adapter = registry.require_harness(harness)?;
            if !adapter.capabilities().rewind_last_turn {
                bail!("{harness} does not support editing the last message");
            }
            registry.cancel_idle_park(&session_id);
            let result = adapter.rewind_last_turn(input, on_event).await;
            registry.schedule_idle_park(harness, &session_id);
            result
        })
    }

    /// `steerHarnessTurn`.
    pub fn steer_harness_turn(
        &self,
        harness: HarnessId,
        input: SteerTurnInput,
    ) -> BoxFuture<'static, Result<()>> {
        let registry = self.clone();
        let session_id = input.session_id.clone();
        self.queue_steer_operation(&session_id.clone(), async move {
            let adapter = registry.require_harness(harness)?;
            if !adapter.live() {
                return Err(not_connected(harness));
            }
            registry.cancel_idle_park(&session_id);
            adapter.steer_turn(input).await
        })
    }

    /// `cancelHarnessTurn`.
    pub async fn cancel_harness_turn(&self, harness: HarnessId, session_id: &str) -> Result<()> {
        let Some(adapter) = self.get_harness(harness).filter(|adapter| adapter.live()) else {
            return Ok(());
        };
        self.cancel_idle_park(session_id);
        adapter.cancel_turn(session_id.to_string()).await?;
        self.schedule_idle_park(harness, session_id);
        Ok(())
    }

    /// `respondHarnessApproval`.
    pub fn respond_harness_approval(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
        decision: ApprovalDecision,
    ) {
        if let Some(adapter) = self.get_harness(harness) {
            adapter.respond_approval(session_id, request_id, decision);
        }
    }

    /// `respondHarnessQuestion`.
    pub fn respond_harness_question(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
        reply: UserQuestionReply,
    ) {
        if let Some(adapter) = self.get_harness(harness) {
            adapter.respond_question(session_id, request_id, reply);
        }
    }

    /// `keepHarnessQuestionOpen`.
    pub fn keep_harness_question_open(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
    ) {
        if let Some(adapter) = self.get_harness(harness) {
            adapter.keep_question_open(session_id, request_id);
        }
    }

    /// `stopHarnessSession`.
    pub async fn stop_harness_session(&self, harness: HarnessId, session_id: &str) -> Result<()> {
        self.cancel_idle_park(session_id);
        let Some(adapter) = self.get_harness(harness).filter(|adapter| adapter.live()) else {
            return Ok(());
        };
        adapter.stop_session(session_id.to_string()).await
    }

    /// `forgetHarnessSession`.
    pub async fn forget_harness_session(&self, harness: HarnessId, session_id: &str) -> Result<()> {
        self.cancel_idle_park(session_id);
        let Some(adapter) = self.get_harness(harness) else {
            return Ok(());
        };
        adapter.forget_session(session_id.to_string()).await
    }

    /// `bindHarnessSession`. `blocks` is the restored transcript, so the
    /// adapter can reseed its task state.
    pub fn bind_harness_session(
        &self,
        harness: HarnessId,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        provider_account_id: Option<&str>,
        blocks: Option<&[Block]>,
    ) {
        let adapter = self.get_harness(harness);
        if let Some(adapter) = &adapter {
            adapter.bind_session(thread_id, provider_session_id, cwd, provider_account_id);
        }
        let (Some(blocks), Some(adapter)) = (blocks, adapter) else {
            return;
        };
        if !adapter.capabilities().restore_task_lists {
            return;
        }
        let lists: Vec<TaskListMeta> = blocks
            .iter()
            .filter(|block| block.role == BlockRole::Tasks)
            .filter_map(|block| block.task_list.clone())
            .collect();
        if !lists.is_empty() {
            adapter.restore_task_lists(thread_id, lists);
        }
    }

    /// `refreshHarnessCatalogs`. Probe model lists only for the harnesses the
    /// caller needs. Boot used to refresh every adapter, which spawned unused
    /// CLIs (Pi with extensions can sit at about 1 GB) even when the workspace
    /// never touched them. `force` re-reads a catalog that already loaded, for
    /// example after a CLI update. `has_live_catalog` is
    /// `ModelCatalog::has_live_catalog` on the app's catalog.
    pub async fn refresh_harness_catalogs(
        &self,
        ids: impl IntoIterator<Item = HarnessId>,
        force: bool,
        has_live_catalog: impl Fn(HarnessId) -> bool,
    ) {
        let wanted: HashSet<HarnessId> = ids.into_iter().collect();
        if wanted.is_empty() {
            return;
        }
        let refreshes = self
            .list_harnesses()
            .into_iter()
            .filter(|adapter| wanted.contains(&adapter.id()))
            .filter(|adapter| adapter.capabilities().refresh_catalog)
            .filter(|adapter| force || !has_live_catalog(adapter.id()))
            .map(|adapter| async move {
                if let Err(error) = adapter.refresh_catalog().await {
                    log::debug!("[monocode] {} catalog {error:#}", adapter.id());
                }
            });
        futures::future::join_all(refreshes).await;
    }

    /// `generateHarnessTitle`.
    pub async fn generate_harness_title(
        &self,
        harness: HarnessId,
        input: TitleInput,
    ) -> Result<Option<GeneratedSessionTitle>> {
        match self.get_harness(harness) {
            Some(adapter) if adapter.capabilities().generate_title => {
                adapter.generate_title(input).await
            }
            _ => Ok(None),
        }
    }

    /// `generateHarnessCommitMessage`.
    pub async fn generate_harness_commit_message(
        &self,
        harness: HarnessId,
        cwd: &str,
        signal: Option<AbortSignal>,
    ) -> Result<String> {
        let adapter = self.require_harness(harness)?;
        if !adapter.capabilities().generate_commit_message {
            bail!("{harness} does not support commit message generation");
        }
        if let Some(signal) = &signal {
            signal.throw_if_aborted()?;
        }
        adapter
            .generate_commit_message(cwd.to_string(), signal)
            .await
    }

    /// `generateHarnessPrContent`.
    pub async fn generate_harness_pr_content(
        &self,
        harness: HarnessId,
        cwd: &str,
    ) -> Result<Option<GeneratedPrContent>> {
        match self.get_harness(harness) {
            Some(adapter) if adapter.capabilities().generate_pr_content => {
                adapter.generate_pr_content(cwd.to_string()).await
            }
            _ => Ok(None),
        }
    }

    /// `generateHarnessBranchName`.
    pub async fn generate_harness_branch_name(
        &self,
        harness: HarnessId,
        cwd: &str,
        message: &str,
    ) -> Result<Option<String>> {
        match self.get_harness(harness) {
            Some(adapter) if adapter.capabilities().generate_branch_name => {
                adapter
                    .generate_branch_name(cwd.to_string(), message.to_string())
                    .await
            }
            _ => Ok(None),
        }
    }

    /// `warmupHarnessText`.
    pub async fn warmup_harness_text(&self, harness: HarnessId, cwd: &str) -> Result<()> {
        match self.get_harness(harness) {
            Some(adapter) if adapter.capabilities().warmup_text => {
                adapter.warmup_text(cwd.to_string()).await
            }
            _ => Ok(()),
        }
    }

    /// `canRunHarnessTextPrompt`.
    pub fn can_run_harness_text_prompt(&self, harness: HarnessId) -> bool {
        self.get_harness(harness)
            .is_some_and(|adapter| adapter.live() && adapter.capabilities().run_text_prompt)
    }

    fn begin_text_prompt(&self, harness: HarnessId) -> u64 {
        let mut state = self.inner.state.lock();
        let owner = state.token();
        state
            .text_prompt_owners
            .entry(harness)
            .or_default()
            .insert(owner);
        owner
    }

    fn finish_text_prompt(
        &self,
        adapter: &Arc<dyn HarnessAdapter>,
        owner: u64,
        stop_if_last: bool,
    ) {
        let harness = adapter.id();
        {
            let mut state = self.inner.state.lock();
            let Some(owners) = state.text_prompt_owners.get_mut(&harness) else {
                return;
            };
            if !owners.remove(&owner) {
                return;
            }
            if !owners.is_empty() {
                return;
            }
            state.text_prompt_owners.remove(&harness);
        }
        if stop_if_last {
            let adapter = adapter.clone();
            self.inner.spawner.spawn(
                async move {
                    let _ = adapter.stop_text_prompt().await;
                }
                .boxed(),
            );
        }
    }

    /// `runHarnessTextPrompt`. Aborting `input.signal` rejects at once with
    /// "By-the-way request cancelled". The checks and the adapter call happen
    /// when this is called, before the first await, as in TypeScript. The
    /// adapter's run then keeps going on the spawner even after an abort, as
    /// the promise did, so its own abort handling still runs.
    pub fn run_harness_text_prompt(
        &self,
        harness: HarnessId,
        input: TextPromptInput,
    ) -> BoxFuture<'static, Result<String>> {
        let adapter = match self.require_harness(harness) {
            Ok(adapter) => adapter,
            Err(error) => return async move { Err(error) }.boxed(),
        };
        if !adapter.live() {
            return async move { Err(not_connected(harness)) }.boxed();
        }
        if !adapter.capabilities().run_text_prompt {
            return async move { Err(anyhow!("{harness} does not support isolated text prompts")) }
                .boxed();
        }
        let signal = input.signal.clone();
        if signal.as_ref().is_some_and(AbortSignal::is_aborted) {
            return async { Err(cancelled_text_prompt()) }.boxed();
        }

        let owner = self.begin_text_prompt(harness);
        let run = {
            let adapter = adapter.clone();
            self.run_detached(async move { adapter.run_text_prompt(input).await })
        };
        let registry = self.clone();
        let Some(signal) = signal else {
            return async move {
                let result = run.await;
                registry.finish_text_prompt(&adapter, owner, false);
                result
            }
            .boxed();
        };
        async move {
            let aborted = async {
                signal.aborted().await;
                registry.finish_text_prompt(&adapter, owner, false);
                Err(cancelled_text_prompt())
            };
            let result = smol::future::or(run, aborted).await;
            registry.finish_text_prompt(&adapter, owner, false);
            result
        }
        .boxed()
    }

    /// `stopHarnessTextPrompts`.
    pub async fn stop_harness_text_prompts(&self) {
        let stops = self.list_harnesses().into_iter().map(|adapter| async move {
            let _ = adapter.stop_text_prompt().await;
        });
        futures::future::join_all(stops).await;
    }
}

#[cfg(test)]
mod tests;
