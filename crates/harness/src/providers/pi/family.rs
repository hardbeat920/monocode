//! Port of src/integrations/harness/providers/pi/piFamily.ts: the live
//! session core that Pi and omp share.
//!
//! The TypeScript mutated one `Live` object from callbacks on a single
//! thread. Here each session's state sits behind a lock. A handler changes
//! state under the lock and collects what it must do afterwards (events to
//! emit, a turn to resolve, a task to start) as ordered [`Effect`]s, which
//! run once the lock is released. A sink that calls back into the adapter
//! cannot deadlock, and a turn never resolves before the events that close
//! it.
//!
//! Work the TypeScript started with `void promise` runs on the spawner. Its
//! synchronous prefix (state flags, request registration) still runs inline,
//! so the next frame sees the same state it did in TypeScript.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Either};
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::block::{ApprovalDecided, ModelSettings, TurnIntent};
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, HarnessEvent, HarnessSessionInput, QuestionDecision,
    RewindLastTurnInput, RewindLastTurnResult, SendTurnInput, SteerTurnInput,
};
use monocode_core::js;
use monocode_core::paths::slash;
use monocode_core::task_list::task_list_from_tool_input;
use monocode_core::user_question::{UserQuestion, UserQuestionOption, UserQuestionReply};

use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildEvent, Children};
use crate::core::native_commands::{
    CommandContext, CommandsListener, NativeCommand, NativeCommandProvider, Unsubscribe,
};
use crate::core::registry::{AcceptedHook, EventSink};

use super::client::{DEFAULT_REQUEST_TIMEOUT_MS, PiRpc};
use super::deps::Rec;
use super::flavor::PiFlavor;
use super::protocol::{
    PiDeltaKind, PiExtensionUiRequest, PiSpawnOptions, PiUiMethod, agent_end_will_retry, as_record,
    assistant_delta_from_event, build_pi_prompt, build_pi_spawn_args, build_pi_steer,
    context_from_session_stats, context_from_usage, extension_ui_response, extension_ui_title,
    fork_messages_from_rpc_data, is_agent_settled, is_pi_thinking_level, merge_tool_input,
    needs_extension_ui_reply, parse_extension_ui_request, parse_pi_model_ref, pi_native_id,
    preview_from_tool, provider_session_id_from_state, session_from_state, status_from_pi_event,
    string_field, summarize_tool_request, tool_call_delta_from_event, tool_call_end_from_event,
    tool_call_start_from_event, tool_execution_end_from_event, tool_execution_start_from_event,
    tool_execution_update_from_event, tool_kind_from_name, tool_title, try_parse_json_record,
    turn_error_from_event, turn_metrics_from_usage,
};
use super::skills::{discover_omp_commands, omp_commands_from_rpc_data};
use super::subagents::pi_subagent_events;

const INIT_TIMEOUT_MS: i64 = 45_000;
const STATS_TIMEOUT_MS: i64 = 4_000;
const COMPACT_TIMEOUT_MS: i64 = 30 * 60_000;

type TurnResult = std::result::Result<(), String>;

/// `turnDone` and `turnFailed`. TypeScript cleared both together, and a
/// settled promise ignored later calls. The outer `Option` on the state
/// field is "the resolvers are set"; the sender inside is taken once.
struct TurnHandle(Option<oneshot::Sender<TurnResult>>);

struct InFlightTool {
    id: String,
    name: String,
    input: Rec,
    partial_json: String,
    title: String,
    /// Set on `tool_execution_end`; later progress updates are stale.
    finished: bool,
}

struct PendingApproval {
    reply: Option<oneshot::Sender<ApprovalDecision>>,
}

struct PendingQuestion {
    id: String,
    reply: Option<oneshot::Sender<UserQuestionReply>>,
}

struct LiveState {
    cwd: String,
    provider_session_id: String,
    context_window: Option<i64>,
    native_model: String,
    thinking: String,
    fast_mode_enabled: Option<bool>,
    fast_mode_requested: Option<bool>,
    planning: bool,
    on_event: EventSink,
    approvals: HashMap<i64, PendingApproval>,
    questions: HashMap<i64, PendingQuestion>,
    available_commands: Option<Vec<NativeCommand>>,
    prompt_id: Option<String>,
    next_approval_ui_id: i64,
    /// `toolsByIndex`, holding tool ids. Both maps point at one tool.
    tools_by_index: HashMap<i64, String>,
    tools_by_id: HashMap<String, InFlightTool>,
    cancelled: bool,
    mute_updates: bool,
    compacting: bool,
    retrying: bool,
    settling: bool,
    settle_token: u64,
    turn_done: Option<TurnHandle>,
    turn_end_pending: bool,
    active_turn: bool,
    emitted_assistant: String,
    emitted_reasoning: String,
    /// Why the turn failed, held until it is clear that no retry follows.
    turn_error: Option<String>,
}

/// One live `--mode rpc` child for a MonoCode session.
pub(crate) struct Live {
    pub(crate) rpc: PiRpc,
    session_id: String,
    state: Mutex<LiveState>,
    /// `live.turns`: turns and compactions run one at a time.
    turns: smol::lock::Mutex<()>,
}

/// Something a handler does after it releases the lock.
enum Effect {
    Event(EventSink, Box<HarnessEvent>),
    Finish(oneshot::Sender<TurnResult>, TurnResult),
    Spawn(BoxFuture<'static, ()>),
    Resume(String, Resume),
    Commands(Vec<CommandsListener>, Vec<NativeCommand>),
}

#[derive(Default)]
struct Effects(Vec<Effect>);

impl Effects {
    fn emit(&mut self, state: &LiveState, event: HarnessEvent) {
        self.0
            .push(Effect::Event(state.on_event.clone(), Box::new(event)));
    }

    fn spawn(&mut self, future: impl std::future::Future<Output = ()> + Send + 'static) {
        self.0.push(Effect::Spawn(future.boxed()));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Resume {
    session_id: String,
    cwd: String,
}

#[derive(Default)]
struct FlavorState {
    live_by_thread: HashMap<String, Arc<Live>>,
    resume_by_thread: HashMap<String, Resume>,
    cancelled_threads: HashSet<String>,
    command_listeners: HashMap<String, Vec<(u64, CommandsListener)>>,
    next_listener: u64,
}

/// The state behind a [`PiFamily`]. Its fields stay private.
pub struct FamilyInner {
    flavor: PiFlavor,
    children: Children,
    catalog: SharedCatalog,
    state: Mutex<FlavorState>,
}

/// The shared Pi and omp core for one flavor. In TypeScript `stateByFlavor`
/// kept one of these per flavor in a module global; here each adapter owns
/// its own. Clones share one state.
#[derive(Clone)]
pub struct PiFamily(Arc<FamilyInner>);

impl std::ops::Deref for PiFamily {
    type Target = FamilyInner;

    fn deref(&self) -> &FamilyInner {
        &self.0
    }
}

fn completed_events() -> Vec<HarnessEvent> {
    vec![
        HarnessEvent::MessageCompleted,
        HarnessEvent::ReasoningCompleted,
    ]
}

fn status(text: impl Into<String>) -> HarnessEvent {
    HarnessEvent::Status { text: text.into() }
}

/// `normalizeProjectPath`.
fn normalize_project_path(path: &str) -> String {
    let slashed = slash(path);
    let trimmed = slashed.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `commandContextKey`.
fn command_context_key(context: &CommandContext) -> String {
    format!(
        "{}\0{}",
        context.session_id.as_deref().unwrap_or(""),
        normalize_project_path(&context.cwd)
    )
}

fn rec(value: Value) -> Rec {
    match value {
        Value::Object(rec) => rec,
        _ => Rec::new(),
    }
}

fn is_false(value: Option<&Value>) -> bool {
    value == Some(&Value::Bool(false))
}

impl FamilyInner {
    fn live(&self, session_id: &str) -> Option<Arc<Live>> {
        self.state.lock().live_by_thread.get(session_id).cloned()
    }

    fn run_effects(&self, effects: Effects) {
        for effect in effects.0 {
            match effect {
                Effect::Event(sink, event) => sink(*event),
                Effect::Finish(sender, result) => {
                    let _ = sender.send(result);
                }
                Effect::Spawn(future) => self.children.spawner().spawn(future),
                Effect::Resume(thread_id, resume) => {
                    self.state.lock().resume_by_thread.insert(thread_id, resume);
                }
                Effect::Commands(listeners, commands) => {
                    for listener in listeners {
                        listener(commands.clone());
                    }
                }
            }
        }
    }

    /// Change `live` under its lock, then run what the change asked for.
    fn update<R>(&self, live: &Live, change: impl FnOnce(&mut LiveState, &mut Effects) -> R) -> R {
        let mut effects = Effects::default();
        let result = {
            let mut state = live.state.lock();
            change(&mut state, &mut effects)
        };
        self.run_effects(effects);
        result
    }
}

impl PiFamily {
    pub fn new(flavor: PiFlavor, children: Children, catalog: SharedCatalog) -> Self {
        Self(Arc::new(FamilyInner {
            flavor,
            children,
            catalog,
            state: Mutex::new(FlavorState::default()),
        }))
    }

    pub fn flavor(&self) -> &PiFlavor {
        &self.flavor
    }

    /// `sendTurn`.
    pub async fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> Result<()> {
        let session_id = input.session.session_id.clone();
        let live = match self.ensure_live(&input.session, &on_event).await {
            Ok(live) => live,
            Err(error) => {
                self.state.lock().cancelled_threads.remove(&session_id);
                return Err(error);
            }
        };
        if self.state.lock().cancelled_threads.remove(&session_id) {
            return Ok(());
        }
        live.state.lock().on_event = on_event;
        let _turn = live.turns.lock().await;
        {
            let mut state = live.state.lock();
            state.cancelled = false;
            state.mute_updates = false;
        }
        match self.run_turn(&live, &input, on_accepted).await {
            Err(_) if live.state.lock().cancelled => Ok(()),
            result => result,
        }
    }

    /// `compactContext`.
    pub async fn compact_context(
        &self,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> Result<()> {
        let live = self.live_for_command(&input, &on_event).await?;
        if self
            .state
            .lock()
            .cancelled_threads
            .remove(&input.session_id)
        {
            return Ok(());
        }
        live.state.lock().on_event = on_event;
        let _turn = live.turns.lock().await;
        {
            let mut state = live.state.lock();
            state.cancelled = false;
            state.mute_updates = false;
        }
        let response = live
            .rpc
            .request(rec(json!({ "type": "compact" })), COMPACT_TIMEOUT_MS)
            .await?;
        let used = as_record(response.get("data"))
            .and_then(|data| data.get("estimatedTokensAfter"))
            .and_then(Value::as_f64);
        if let Some(used) = used.filter(|used| *used > 0.0) {
            self.update(&live, |state, fx| {
                fx.emit(
                    state,
                    HarnessEvent::Context {
                        used: Some(used as i64),
                        window: state.context_window,
                    },
                )
            });
        }
        Ok(())
    }

    /// The shared start of `compactContext` and `rewindLastTurn`.
    async fn live_for_command(
        &self,
        input: &HarnessSessionInput,
        on_event: &EventSink,
    ) -> Result<Arc<Live>> {
        match self.live(&input.session_id) {
            Some(live) if live.state.lock().cwd == input.cwd => {
                live.state.lock().on_event = on_event.clone();
                self.apply_model(&live, input).await?;
                Ok(live)
            }
            _ => self.ensure_live(input, on_event).await,
        }
    }

    /// `rewindLastTurn`.
    pub async fn rewind_last_turn(
        &self,
        input: RewindLastTurnInput,
        on_event: EventSink,
    ) -> Result<RewindLastTurnResult> {
        let session = &input.session;
        let live = self.live_for_command(session, &on_event).await?;
        if live.state.lock().active_turn {
            return Err(anyhow!(
                "Stop the current turn before editing the last message"
            ));
        }
        let fork_messages = live
            .rpc
            .request(
                rec(json!({ "type": "get_fork_messages" })),
                DEFAULT_REQUEST_TIMEOUT_MS,
            )
            .await?;
        let messages = fork_messages_from_rpc_data(fork_messages.get("data"));
        let Some(last) = messages.last() else {
            return Err(anyhow!("No user message to edit"));
        };
        let fork = live
            .rpc
            .request(
                rec(json!({ "type": "fork", "entryId": last.entry_id })),
                DEFAULT_REQUEST_TIMEOUT_MS,
            )
            .await?;
        if as_record(fork.get("data")).and_then(|data| data.get("cancelled"))
            == Some(&Value::Bool(true))
        {
            return Err(anyhow!("Edit cancelled"));
        }
        let fork_state = live
            .rpc
            .request(
                rec(json!({ "type": "get_state" })),
                DEFAULT_REQUEST_TIMEOUT_MS,
            )
            .await?;
        let Some(provider_session_id) = provider_session_id_from_state(fork_state.get("data"))
        else {
            return Err(anyhow!("Pi did not expose the forked session"));
        };
        self.update(&live, |state, fx| {
            self.bind_state(&live.session_id, state, fx, fork_state.get("data"));
            fx.emit(
                state,
                HarnessEvent::SessionProviderBound {
                    provider_session_id,
                },
            );
        });
        Ok(RewindLastTurnResult { submitted: false })
    }

    /// `steerTurn`.
    pub async fn steer_turn(&self, input: SteerTurnInput) -> Result<()> {
        let live = self
            .live(&input.session_id)
            .filter(|live| live.state.lock().active_turn)
            .ok_or_else(|| anyhow!("No active turn to steer"))?;
        let message = js::trim(&input.text);
        let attachments = input.attachments.as_deref().unwrap_or(&[]);
        let command = if self.flavor.is_omp() && message.starts_with('/') {
            build_pi_prompt(message, attachments, true)
        } else {
            build_pi_steer(message, attachments)
        }
        .map_err(|error| anyhow!(error))?;
        let has_message = command
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty());
        if !has_message && !matches!(command.get("images"), Some(Value::Array(_))) {
            return Ok(());
        }
        live.rpc
            .request(command, DEFAULT_REQUEST_TIMEOUT_MS)
            .await?;
        Ok(())
    }

    /// `respondApproval`.
    pub fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        let Some(live) = self.live(session_id) else {
            return;
        };
        let reply = live
            .state
            .lock()
            .approvals
            .get_mut(&request_id)
            .and_then(|pending| pending.reply.take());
        if let Some(reply) = reply {
            let _ = reply.send(decision);
        }
    }

    /// `respondQuestion`.
    pub fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        let Some(live) = self.live(session_id) else {
            return;
        };
        let sender = live
            .state
            .lock()
            .questions
            .get_mut(&request_id)
            .and_then(|question| question.reply.take());
        if let Some(sender) = sender {
            let _ = sender.send(reply);
        }
    }

    fn release_prompts(state: &mut LiveState) {
        for (_, pending) in state.approvals.drain() {
            if let Some(reply) = pending.reply {
                let _ = reply.send(ApprovalDecision::Deny);
            }
        }
        for (_, question) in state.questions.drain() {
            if let Some(reply) = question.reply {
                let _ = reply.send(UserQuestionReply::Skipped);
            }
        }
    }

    /// `cancelTurn`.
    pub async fn cancel_turn(&self, session_id: &str) -> Result<()> {
        let Some(live) = self.live(session_id) else {
            self.state
                .lock()
                .cancelled_threads
                .insert(session_id.to_string());
            return Ok(());
        };
        {
            let mut state = live.state.lock();
            state.cancelled = true;
            state.mute_updates = true;
            state.settle_token += 1;
            Self::release_prompts(&mut state);
        }
        let _ = live
            .rpc
            .request(rec(json!({ "type": "abort" })), 5_000)
            .await;
        self.update(&live, |state, fx| {
            finish_active_turn(state, fx, completed_events())
        });
        Ok(())
    }

    /// `stopSession`.
    pub async fn stop_session(&self, session_id: &str) -> Result<()> {
        let live = {
            let mut state = self.state.lock();
            state.cancelled_threads.remove(session_id);
            state.live_by_thread.remove(session_id)
        };
        if let Some(live) = live {
            self.update(&live, |state, fx| {
                state.mute_updates = true;
                state.settle_token += 1;
                Self::release_prompts(state);
                state.active_turn = false;
                if let Some(mut handle) = state.turn_done.take()
                    && let Some(sender) = handle.0.take()
                {
                    fx.0.push(Effect::Finish(sender, Ok(())));
                }
            });
            live.rpc.close(None);
        }
        self.children.unwatch_child(session_id);
        let _ = self.children.kill_child(session_id).await;
        Ok(())
    }

    /// `forgetSession`.
    pub async fn forget_session(&self, session_id: &str) -> Result<()> {
        self.state.lock().resume_by_thread.remove(session_id);
        self.stop_session(session_id).await
    }

    /// `bindSession`.
    pub fn bind_session(&self, thread_id: &str, provider_session_id: &str, cwd: &str) {
        let session_id = js::trim(provider_session_id);
        if thread_id.is_empty() || session_id.is_empty() || js::trim(cwd).is_empty() {
            return;
        }
        self.state.lock().resume_by_thread.insert(
            thread_id.to_string(),
            Resume {
                session_id: session_id.to_string(),
                cwd: cwd.to_string(),
            },
        );
    }

    async fn ensure_live(
        &self,
        input: &HarnessSessionInput,
        on_event: &EventSink,
    ) -> Result<Arc<Live>> {
        let existing = self.live(&input.session_id);
        let want_planning = input.intent == Some(TurnIntent::Plan);
        if let Some(existing) = existing.as_ref() {
            let reuse = {
                let state = existing.state.lock();
                state.cwd == input.cwd && state.planning == want_planning
            };
            if reuse {
                existing.state.lock().on_event = on_event.clone();
                self.apply_model(existing, input).await?;
                return Ok(existing.clone());
            }
        }
        if let Some(existing) = existing {
            if existing.state.lock().cwd != input.cwd {
                self.state.lock().resume_by_thread.remove(&input.session_id);
            }
            self.stop_session(&input.session_id).await?;
        }

        let resume = self
            .state
            .lock()
            .resume_by_thread
            .get(&input.session_id)
            .cloned();
        let can_resume = resume
            .as_ref()
            .is_some_and(|resume| resume.cwd == input.cwd);
        if resume.is_some() && !can_resume {
            self.state.lock().resume_by_thread.remove(&input.session_id);
        }

        let resume_id = resume
            .filter(|_| can_resume)
            .map(|resume| resume.session_id);
        match self.start_live(input, on_event, resume_id).await {
            Ok(live) => Ok(live),
            Err(error) => {
                if !can_resume {
                    return Err(error);
                }
                self.state.lock().resume_by_thread.remove(&input.session_id);
                self.stop_session(&input.session_id).await?;
                self.start_live(input, on_event, None).await
            }
        }
    }

    async fn start_live(
        &self,
        input: &HarnessSessionInput,
        on_event: &EventSink,
        resume: Option<String>,
    ) -> Result<Arc<Live>> {
        let session_id = input.session_id.clone();
        let path = self.children.resolve_binary(self.flavor.id).await?.path;
        let native = self.catalog.read().native_model_id_for(&input.model);
        let model_ref = parse_pi_model_ref(Some(&native));
        let rpc = PiRpc::new(&self.children, &session_id, self.flavor.label);
        let live = Arc::new(Live {
            rpc,
            session_id: session_id.clone(),
            state: Mutex::new(LiveState {
                cwd: input.cwd.clone(),
                provider_session_id: resume.clone().unwrap_or_default(),
                context_window: None,
                native_model: native.clone(),
                thinking: input
                    .model_settings
                    .as_ref()
                    .and_then(|settings| settings.get("thinking"))
                    .cloned()
                    .unwrap_or_default(),
                fast_mode_enabled: None,
                fast_mode_requested: None,
                planning: input.intent == Some(TurnIntent::Plan),
                on_event: on_event.clone(),
                approvals: HashMap::new(),
                questions: HashMap::new(),
                available_commands: None,
                prompt_id: None,
                next_approval_ui_id: 1,
                tools_by_index: HashMap::new(),
                tools_by_id: HashMap::new(),
                cancelled: false,
                mute_updates: false,
                compacting: false,
                retrying: false,
                settling: false,
                settle_token: 0,
                turn_done: None,
                turn_end_pending: false,
                active_turn: false,
                emitted_assistant: String::new(),
                emitted_reasoning: String::new(),
                turn_error: None,
            }),
            turns: smol::lock::Mutex::new(()),
        });

        let events = self.children.watch_child(&session_id);
        let family = self.clone();
        let pump = live.clone();
        self.children.spawner().spawn(
            async move {
                while let Ok(event) = events.recv().await {
                    match event {
                        ChildEvent::Stdout(line) => {
                            if let Some(rec) = pump.rpc.push_line(&line) {
                                family.handle_frame(&pump, rec);
                            }
                        }
                        ChildEvent::Stderr(line) => log::debug!("[{}] {line}", family.flavor.id),
                        ChildEvent::Exit(code) => family.on_child_exit(&pump, code),
                    }
                }
            }
            .boxed(),
        );

        let args = build_pi_spawn_args(
            &self.flavor,
            &PiSpawnOptions {
                resume,
                model: model_ref.is_some().then(|| native.clone()),
                plan: input.intent == Some(TurnIntent::Plan),
                ..PiSpawnOptions::default()
            },
        );
        self.children
            .spawn_child(
                &session_id,
                &path,
                args,
                &input.cwd,
                None,
                Some(self.flavor.id),
            )
            .await?;

        self.state
            .lock()
            .live_by_thread
            .insert(session_id.clone(), live.clone());

        let started: Result<()> = async {
            let state_frame = live
                .rpc
                .request(rec(json!({ "type": "get_state" })), INIT_TIMEOUT_MS)
                .await?;
            self.update(&live, |state, fx| {
                self.bind_state(&session_id, state, fx, state_frame.get("data"))
            });
            self.apply_model(&live, input).await?;
            self.update(&live, |state, fx| {
                if !state.provider_session_id.is_empty() {
                    let provider_session_id = state.provider_session_id.clone();
                    fx.emit(
                        state,
                        HarnessEvent::SessionProviderBound {
                            provider_session_id,
                        },
                    );
                }
                fx.emit(state, HarnessEvent::SessionStarted);
            });
            Ok(())
        }
        .await;
        match started {
            Ok(()) => Ok(live),
            Err(error) => {
                self.stop_session(&session_id).await?;
                Err(error)
            }
        }
    }

    /// The `watchChild` exit handler of `startLive`.
    fn on_child_exit(&self, live: &Arc<Live>, code: Option<i32>) {
        let label = self.flavor.label;
        live.rpc.close(Some(format!("{label} exited")));
        {
            // An exit queued before a newer child took the session over must
            // not remove that child.
            let mut state = self.state.lock();
            if state
                .live_by_thread
                .get(&live.session_id)
                .is_some_and(|current| Arc::ptr_eq(current, live))
            {
                state.live_by_thread.remove(&live.session_id);
            }
        }
        self.update(live, |state, fx| {
            if !state.mute_updates {
                fx.emit(
                    state,
                    HarnessEvent::SessionEnded {
                        code: code.map(i64::from),
                    },
                );
            }
            for question in state.questions.values_mut() {
                if let Some(reply) = question.reply.take() {
                    let _ = reply.send(UserQuestionReply::Skipped);
                }
            }
            for approval in state.approvals.values_mut() {
                if let Some(reply) = approval.reply.take() {
                    let _ = reply.send(ApprovalDecision::Deny);
                }
            }
            turn_failed(state, fx, format!("{label} exited"));
            state.turn_done = None;
        });
    }

    async fn run_turn(
        &self,
        live: &Arc<Live>,
        input: &SendTurnInput,
        on_accepted: Option<AcceptedHook>,
    ) -> Result<()> {
        self.apply_model(live, &input.session).await?;
        let prompt_id = format!("mc_turn_{}", uuid::Uuid::new_v4());
        let (sender, receiver) = oneshot::channel::<TurnResult>();
        let turn = receiver
            .map(|result| result.unwrap_or(Ok(())))
            .boxed()
            .shared();
        self.update(live, |state, fx| {
            state.emitted_assistant.clear();
            state.emitted_reasoning.clear();
            state.turn_error = None;
            state.tools_by_index.clear();
            state.tools_by_id.clear();
            state.compacting = false;
            state.retrying = false;
            state.settle_token += 1;
            state.settling = false;
            state.prompt_id = Some(prompt_id.clone());
            state.turn_done = Some(TurnHandle(Some(sender)));
            state.active_turn = true;
            settle_pending_turn(state, fx);
        });

        let result: Result<()> = async {
            let attachments = input.attachments.as_deref().unwrap_or(&[]);
            let mut command =
                build_pi_prompt(&input.text, attachments, false).map_err(|error| anyhow!(error))?;
            command.insert("id".into(), Value::String(prompt_id.clone()));
            let timeout_ms = if self.flavor.is_omp() {
                COMPACT_TIMEOUT_MS
            } else {
                DEFAULT_REQUEST_TIMEOUT_MS
            };
            let request = live.rpc.request(command, timeout_ms);
            let response = match futures::future::select(request, turn.clone()).await {
                Either::Left((response, _)) => Some(response?),
                Either::Right((finished, _)) => {
                    finished.map_err(|error| anyhow!(error))?;
                    None
                }
            };
            if let Some(on_accepted) = on_accepted.as_ref() {
                on_accepted();
            }
            let agent_invoked = response
                .as_ref()
                .and_then(|response| as_record(response.get("data")))
                .and_then(|data| data.get("agentInvoked"));
            let quick_finish = self.flavor.is_omp() && is_false(agent_invoked);
            self.update(live, |state, fx| {
                if quick_finish {
                    finish_active_turn(state, fx, completed_events());
                }
                settle_pending_turn(state, fx);
            });
            turn.await.map_err(|error| anyhow!(error))
        }
        .await;

        let outcome = match result {
            Err(_) if live.state.lock().cancelled => Ok(()),
            Err(error) => {
                let message = error.to_string();
                self.update(live, |state, fx| {
                    fx.emit(state, HarnessEvent::SessionError { message })
                });
                Err(error)
            }
            Ok(()) => Ok(()),
        };
        {
            let mut state = live.state.lock();
            state.active_turn = false;
            state.prompt_id = None;
            state.turn_done = None;
        }
        live.rpc.cancel_request(&prompt_id);
        outcome
    }

    /// `handleFrame`.
    fn handle_frame(&self, live: &Arc<Live>, rec: Rec) {
        let omp = self.flavor.is_omp();
        let raw_type = rec.get("type").and_then(Value::as_str);
        if omp && raw_type == Some("available_commands_update") {
            // A malformed update keeps the last valid inventory.
            if let Ok(commands) = omp_commands_from_rpc_data(Some(&Value::Object(rec))) {
                let cwd = {
                    let mut state = live.state.lock();
                    state.available_commands = Some(commands.clone());
                    state.cwd.clone()
                };
                let key = command_context_key(&CommandContext {
                    cwd,
                    session_id: Some(live.session_id.clone()),
                });
                let listeners: Vec<CommandsListener> = self
                    .state
                    .lock()
                    .command_listeners
                    .get(&key)
                    .map(|listeners| {
                        listeners
                            .iter()
                            .map(|(_, listener)| listener.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                self.run_effects(Effects(vec![Effect::Commands(listeners, commands)]));
            }
            return;
        }
        if omp
            && raw_type == Some("extension_ui_request")
            && rec.get("method").and_then(Value::as_str) == Some("cancel")
        {
            let id = rec.get("id");
            let replies: Vec<_> = {
                let mut state = live.state.lock();
                state
                    .questions
                    .values_mut()
                    .filter(|question| id.and_then(Value::as_str) == Some(question.id.as_str()))
                    .filter_map(|question| question.reply.take())
                    .collect()
            };
            for reply in replies {
                let _ = reply.send(UserQuestionReply::Skipped);
            }
            return;
        }
        if let Some(ui) = parse_extension_ui_request(&rec) {
            self.handle_extension_ui(live, ui);
            return;
        }
        self.update(live, |state, fx| self.frame_events(live, state, fx, &rec));
    }

    /// The part of `handleFrame` after extension UI, under the session lock.
    fn frame_events(&self, live: &Arc<Live>, state: &mut LiveState, fx: &mut Effects, rec: &Rec) {
        if state.mute_updates {
            return;
        }
        let frame = Some(rec);
        let kind = string_field(frame, "type");
        if self.flavor.is_omp() {
            // omp emits persisted custom_message entries on the live RPC
            // stream as a message_start and message_end pair. Render the
            // start once, and consume the end and hidden custom messages so
            // none can leak through a generic path.
            if matches!(kind, Some("message_start" | "message_end")) {
                let message = as_record(rec.get("message"));
                if let Some(message) = message
                    && message.get("role").and_then(Value::as_str) == Some("custom")
                {
                    if kind == Some("message_start")
                        && let Some(interjection) = interjection_from_custom_message(message)
                    {
                        fx.emit(state, interjection);
                    }
                    return;
                }
            }
            match kind {
                Some("advisor_yielded") => {
                    fx.emit(state, status("Advisor reviewed this turn"));
                    return;
                }
                Some("session_info_update") => {
                    if let Some(provider_session_id) = string_field(frame, "sessionId") {
                        let data = json!({ "sessionId": provider_session_id });
                        self.bind_state(&live.session_id, state, fx, Some(&data));
                        fx.emit(
                            state,
                            HarnessEvent::SessionProviderBound {
                                provider_session_id: provider_session_id.to_string(),
                            },
                        );
                    }
                    return;
                }
                Some("config_update") => {
                    self.config_update(state, fx, rec);
                    return;
                }
                Some("command_output") => {
                    if let Some(text) = string_field(frame, "text") {
                        let text =
                            extension_ui_title(&PiExtensionUiRequest::notify("output", text));
                        fx.emit(state, status(text));
                    }
                    return;
                }
                Some("prompt_result") => {
                    if state.active_turn
                        && rec.get("id").and_then(Value::as_str) == state.prompt_id.as_deref()
                        && state.prompt_id.is_some()
                        && is_false(rec.get("agentInvoked"))
                    {
                        finish_active_turn(state, fx, completed_events());
                    }
                    return;
                }
                // omp can acknowledge a prompt and later report an asynchronous error.
                Some("response")
                    if rec.get("command").and_then(Value::as_str) == Some("prompt")
                        && state.prompt_id.is_some()
                        && rec.get("id").and_then(Value::as_str) == state.prompt_id.as_deref()
                        && is_false(rec.get("success")) =>
                {
                    let error = string_field(frame, "error")
                        .unwrap_or("OMP command failed")
                        .to_string();
                    turn_failed(state, fx, error);
                    return;
                }
                Some("agent_end") if is_false(rec.get("isTerminal")) => return,
                _ => {}
            }
        }
        match kind {
            Some("compaction_start") => state.compacting = true,
            Some("compaction_end") => state.compacting = false,
            Some("auto_retry_start") => state.retrying = true,
            Some("auto_retry_end") => state.retrying = false,
            _ => {}
        }

        if let Some(text) = status_from_pi_event(rec) {
            fx.emit(state, status(text));
        }
        if let Some(error) = turn_error_from_event(rec) {
            state.turn_error = Some(error);
        }
        if let Some(context) = context_from_usage(rec, state.context_window) {
            fx.emit(
                state,
                HarnessEvent::Context {
                    used: context.used,
                    window: context.window,
                },
            );
        }
        if let Some(metrics) = turn_metrics_from_usage(rec) {
            fx.emit(state, HarnessEvent::TurnMetrics(metrics));
        }

        if let Some(delta) = assistant_delta_from_event(rec) {
            match delta.kind {
                PiDeltaKind::Text => {
                    state.emitted_assistant.push_str(&delta.text);
                    fx.emit(state, HarnessEvent::MessageDelta { text: delta.text });
                }
                PiDeltaKind::Thinking => {
                    state.emitted_reasoning.push_str(&delta.text);
                    fx.emit(state, HarnessEvent::ReasoningDelta { text: delta.text });
                }
            }
        }

        if let Some(started) = tool_call_start_from_event(rec) {
            upsert_tool(
                state,
                fx,
                &started.id,
                &started.name,
                Rec::new(),
                Some(started.index),
            );
        }
        if let Some(delta) = tool_call_delta_from_event(rec)
            && let Some(id) = state.tools_by_index.get(&delta.index).cloned()
        {
            let parsed = state.tools_by_id.get_mut(&id).and_then(|tool| {
                tool.partial_json.push_str(&delta.delta);
                try_parse_json_record(&tool.partial_json)
            });
            if let Some(parsed) = parsed {
                update_tool(state, fx, &id, parsed);
            }
        }
        if let Some(ended) = tool_call_end_from_event(rec) {
            upsert_tool(state, fx, &ended.id, &ended.name, ended.input, None);
        }
        if let Some(started) = tool_execution_start_from_event(rec) {
            upsert_tool(state, fx, &started.id, &started.name, started.input, None);
            if let Some(tool) = state.tools_by_id.get(&started.id) {
                let event = HarnessEvent::ToolUpdated {
                    agent_model: None,
                    call_id: tool.id.clone(),
                    title: Some(tool.title.clone()),
                    kind: Some(tool_kind_from_name(&tool.name)),
                    status: Some("running".into()),
                    detail: None,
                    preview: preview_from_tool(&tool.name, &tool.input, None),
                    paths: None,
                };
                fx.emit(state, event);
            }
        }
        if let Some(update) = tool_execution_update_from_event(rec) {
            // omp can deliver an update after the tool's end (omp#12875,
            // steer during bash). Replaying it would flip the finished card
            // back to running.
            let mut events = Vec::new();
            if let Some(tool) = state
                .tools_by_id
                .get_mut(&update.id)
                .filter(|tool| !tool.finished)
            {
                if !update.input.is_empty() {
                    tool.input = merge_tool_input(&tool.input, &update.input);
                    tool.title = tool_title(&tool.name, &tool.input);
                }
                let kind = tool_kind_from_name(&tool.name);
                events.push(HarnessEvent::ToolUpdated {
                    agent_model: None,
                    call_id: tool.id.clone(),
                    title: Some(tool.title.clone()),
                    kind: Some(kind.clone()),
                    status: Some("running".into()),
                    detail: update.detail.clone(),
                    preview: preview_from_tool(&tool.name, &tool.input, update.detail.as_deref()),
                    paths: None,
                });
                if kind == "agent" {
                    events.extend(pi_subagent_events(
                        &tool.id,
                        &tool.input,
                        rec.get("partialResult"),
                        false,
                        false,
                    ));
                }
            }
            for event in events {
                fx.emit(state, event);
            }
        }
        if let Some(end) = tool_execution_end_from_event(rec) {
            let mut events = Vec::new();
            if let Some(tool) = state.tools_by_id.get_mut(&end.id) {
                tool.finished = true;
                let kind = tool_kind_from_name(&tool.name);
                events.push(HarnessEvent::ToolUpdated {
                    agent_model: None,
                    call_id: tool.id.clone(),
                    title: Some(tool.title.clone()),
                    kind: Some(kind.clone()),
                    status: Some(if end.is_error { "failed" } else { "completed" }.into()),
                    detail: end.detail.clone(),
                    preview: preview_from_tool(&tool.name, &tool.input, end.detail.as_deref()),
                    paths: None,
                });
                if kind == "agent" {
                    events.extend(pi_subagent_events(
                        &tool.id,
                        &tool.input,
                        rec.get("result"),
                        true,
                        end.is_error,
                    ));
                }
            }
            for event in events {
                fx.emit(state, event);
            }
        }

        if is_agent_settled(rec) {
            self.flush_turn_error(state, fx);
            self.settle_turn(live, state, fx);
            return;
        }
        let will_retry = agent_end_will_retry(rec);
        // The retry carries the real answer, so the attempt it replaces stays quiet.
        if will_retry == Some(true) {
            state.turn_error = None;
        }
        if will_retry == Some(false) && !state.compacting && !state.retrying {
            self.flush_turn_error(state, fx);
            self.settle_turn(live, state, fx);
        }
    }

    /// The `config_update` branch of `handleFrame`.
    fn config_update(&self, state: &mut LiveState, fx: &mut Effects, rec: &Rec) {
        let frame = Some(rec);
        let model = as_record(rec.get("model"));
        let provider = string_field(model, "provider");
        let model_id = string_field(model, "id");
        let thinking =
            string_field(frame, "thinkingLevel").filter(|level| is_pi_thinking_level(Some(level)));
        let fast_mode_enabled = rec.get("fastModeEnabled").and_then(Value::as_bool);
        let native = match (provider, model_id) {
            (Some(provider), Some(model_id)) => Some(pi_native_id(provider, model_id)),
            _ => None,
        };
        if let Some(native) = native.as_ref() {
            state.native_model = native.clone();
        }
        if let Some(thinking) = thinking {
            state.thinking = thinking.to_string();
        }
        if let Some(enabled) = fast_mode_enabled {
            state.fast_mode_enabled = Some(enabled);
            state.fast_mode_requested = Some(enabled);
        }
        let model_settings = (thinking.is_some() || fast_mode_enabled.is_some()).then(|| {
            let mut settings = ModelSettings::new();
            if let Some(thinking) = thinking {
                settings.insert("thinking".into(), thinking.to_string());
            }
            if let Some(enabled) = fast_mode_enabled {
                settings.insert("fast".into(), enabled.to_string());
            }
            settings
        });
        fx.emit(
            state,
            HarnessEvent::SessionConfigChanged {
                model: native.map(|native| format!("{}:{native}", self.flavor.id)),
                model_settings,
            },
        );
    }

    /// `flushTurnError`.
    fn flush_turn_error(&self, state: &mut LiveState, fx: &mut Effects) {
        let Some(message) = state.turn_error.take() else {
            return;
        };
        let message = if message.is_empty() {
            format!("{} turn failed", self.flavor.label)
        } else {
            message
        };
        fx.emit(state, HarnessEvent::SessionError { message });
    }

    /// `settleTurn`. The checks and the stats request run now; the reply is
    /// awaited on the spawner.
    fn settle_turn(&self, live: &Arc<Live>, state: &mut LiveState, fx: &mut Effects) {
        if state.settling || state.cancelled || state.mute_updates {
            return;
        }
        if !state.active_turn && state.turn_done.is_none() {
            return;
        }
        state.settling = true;
        let token = state.settle_token;
        let stats = live.rpc.request(
            rec(json!({ "type": "get_session_stats" })),
            STATS_TIMEOUT_MS,
        );
        let family = self.clone();
        let live = live.clone();
        fx.spawn(async move {
            // On failure the meter stays on the last streamed usage.
            let stats = stats.await.ok();
            family.update(&live, |state, fx| {
                if state.settle_token == token
                    && !state.cancelled
                    && let Some(context) = stats
                        .as_ref()
                        .and_then(|stats| context_from_session_stats(stats.get("data")))
                {
                    fx.emit(
                        state,
                        HarnessEvent::Context {
                            used: context.used,
                            window: context.window,
                        },
                    );
                }
                if state.settle_token == token && !state.cancelled {
                    finish_active_turn(state, fx, completed_events());
                }
                if state.settle_token == token {
                    state.settling = false;
                }
            });
        });
    }

    /// `handleExtensionUi`. Everything before the TypeScript's first await
    /// runs now; waiting for the reply runs on the spawner.
    fn handle_extension_ui(&self, live: &Arc<Live>, request: PiExtensionUiRequest) {
        if !needs_extension_ui_reply(&request) {
            let text = if request.title.is_some() {
                extension_ui_title(&request)
            } else {
                String::new()
            };
            if !js::trim(&text).is_empty() {
                self.update(live, |state, fx| fx.emit(state, status(text)));
            }
            return;
        }
        let omp_question = self.flavor.is_omp()
            && matches!(
                request.method,
                PiUiMethod::Select | PiUiMethod::Input | PiUiMethod::Editor
            );
        let family = self.clone();
        let task_live = live.clone();
        self.update(live, move |state, fx| {
            if state.cancelled || state.mute_updates {
                let deny = extension_ui_response(&request, ApprovalDecision::Deny).to_string();
                let write = task_live.rpc.write_line(deny);
                fx.spawn(async move {
                    let _ = write.await;
                });
                return;
            }
            let ui_id = state.next_approval_ui_id;
            state.next_approval_ui_id += 1;
            let title = extension_ui_title(&request);
            if omp_question {
                let (reply, replied) = oneshot::channel();
                state
                    .questions
                    .insert(ui_id, PendingQuestion { id: request.id.clone(), reply: Some(reply) });
                let options = if request.method == PiUiMethod::Select {
                    request
                        .options
                        .iter()
                        .enumerate()
                        .map(|(index, label)| UserQuestionOption {
                            id: index.to_string(),
                            label: extension_ui_title(&PiExtensionUiRequest::notify(&request.id, label)),
                            description: None,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                fx.emit(
                    state,
                    HarnessEvent::QuestionAsked {
                        request_id: ui_id,
                        title: Some(title.clone()),
                        questions: vec![UserQuestion {
                            id: request.id.clone(),
                            header: None,
                            prompt: title,
                            multi_select: false,
                            allow_custom: request.method != PiUiMethod::Select,
                            options,
                        }],
                        call_id: None,
                        auto_resolve_at: None,
                    },
                );
                fx.spawn(async move {
                    let Ok(reply) = replied.await else {
                        return;
                    };
                    let value = question_value(&request, &reply);
                    let response = match value.as_ref() {
                        Some(value) => {
                            json!({ "type": "extension_ui_response", "id": request.id, "value": value })
                        }
                        None => json!({ "type": "extension_ui_response", "id": request.id, "cancelled": true }),
                    };
                    let write = family.update(&task_live, |state, fx| {
                        state.questions.remove(&ui_id);
                        let decision =
                            if value.is_none() { QuestionDecision::Skipped } else { QuestionDecision::Answered };
                        fx.emit(state, HarnessEvent::QuestionResolved { request_id: ui_id, decision });
                        task_live.rpc.write_line(response.to_string())
                    });
                    let _ = write.await;
                });
                return;
            }
            fx.emit(
                state,
                HarnessEvent::ApprovalRequested {
                    request_id: ui_id,
                    title,
                    kind: Some("other".into()),
                    call_id: None,
                    preview: None,
                },
            );
            let (reply, replied) = oneshot::channel();
            state.approvals.insert(ui_id, PendingApproval { reply: Some(reply) });
            fx.spawn(async move {
                let Ok(decision) = replied.await else {
                    return;
                };
                let write = family.update(&task_live, |state, fx| {
                    state.approvals.remove(&ui_id);
                    let decided = match decision {
                        ApprovalDecision::Allow => ApprovalDecided::Allow,
                        ApprovalDecision::Deny => ApprovalDecided::Deny,
                    };
                    fx.emit(state, HarnessEvent::ApprovalResolved { request_id: ui_id, decision: decided });
                    task_live.rpc.write_line(extension_ui_response(&request, decision).to_string())
                });
                let _ = write.await;
            });
        });
    }

    /// `applyModel`.
    async fn apply_model(&self, live: &Arc<Live>, input: &HarnessSessionInput) -> Result<()> {
        let native = self.catalog.read().native_model_id_for(&input.model);
        let reference = parse_pi_model_ref(Some(&native));
        let current = live.state.lock().native_model.clone();
        match reference {
            Some(reference) if native != current => {
                let result = live
                    .rpc
                    .request(
                        rec(json!({
                            "type": "set_model",
                            "provider": reference.provider,
                            "modelId": reference.model_id,
                        })),
                        DEFAULT_REQUEST_TIMEOUT_MS,
                    )
                    .await?;
                let window = as_record(result.get("data"))
                    .and_then(|model| model.get("contextWindow"))
                    .and_then(Value::as_f64);
                let mut state = live.state.lock();
                state.native_model = native;
                if let Some(window) = window.filter(|window| *window > 0.0) {
                    state.context_window = Some(window as i64);
                }
            }
            Some(_) => live.state.lock().native_model = native,
            None => {}
        }

        let settings = input.model_settings.as_ref();
        let thinking = settings.and_then(|settings| settings.get("thinking"));
        if let Some(thinking) = thinking.filter(|level| is_pi_thinking_level(Some(level)))
            && *thinking != live.state.lock().thinking
        {
            let _ = live
                .rpc
                .request(
                    rec(json!({ "type": "set_thinking_level", "level": thinking })),
                    DEFAULT_REQUEST_TIMEOUT_MS,
                )
                .await;
            live.state.lock().thinking = thinking.clone();
        }

        let fast = settings
            .and_then(|settings| settings.get("fast"))
            .map(String::as_str);
        if self.flavor.is_omp()
            && let Some(fast) = fast.filter(|fast| *fast == "true" || *fast == "false")
            && Some(fast == "true") != live.state.lock().fast_mode_requested
        {
            let enabled = fast == "true";
            live.state.lock().fast_mode_requested = Some(enabled);
            let response = live
                .rpc
                .request(
                    rec(json!({ "type": "set_fast_mode", "enabled": enabled })),
                    DEFAULT_REQUEST_TIMEOUT_MS,
                )
                .await;
            match response {
                Ok(response) => {
                    let confirmed = as_record(response.get("data"))
                        .and_then(|data| data.get("enabled"))
                        .and_then(Value::as_bool)
                        .unwrap_or(enabled);
                    live.state.lock().fast_mode_enabled = Some(confirmed);
                }
                Err(error) if enabled => {
                    self.update(live, |state, fx| {
                        state.fast_mode_enabled = Some(false);
                        let settings =
                            ModelSettings::from([("fast".to_string(), "false".to_string())]);
                        fx.emit(
                            state,
                            HarnessEvent::SessionConfigChanged {
                                model: None,
                                model_settings: Some(settings),
                            },
                        );
                        fx.emit(state, status(error.to_string()));
                    });
                }
                Err(_) => {}
            }
        }

        if self.flavor.is_pi() {
            let current = self
                .live(&input.session_id)
                .is_some_and(|registered| Arc::ptr_eq(&registered, live));
            self.update(live, |state, fx| {
                let model = format!("pi:{}", state.native_model);
                if parse_pi_model_ref(Some(&state.native_model)).is_some()
                    && input.model != model
                    && current
                    && !state.mute_updates
                {
                    fx.emit(
                        state,
                        HarnessEvent::SessionConfigChanged {
                            model: Some(model),
                            model_settings: None,
                        },
                    );
                }
            });
        }
        Ok(())
    }

    /// `bindState`. The resume entry is written once the session lock drops.
    fn bind_state(
        &self,
        thread_id: &str,
        state: &mut LiveState,
        fx: &mut Effects,
        data: Option<&Value>,
    ) {
        let session = session_from_state(data);
        let provider_session_id = provider_session_id_from_state(data);
        if let Some(window) = session.context_window {
            state.context_window = Some(window);
        }
        if let Some(provider_session_id) = provider_session_id {
            state.provider_session_id = provider_session_id.clone();
            fx.0.push(Effect::Resume(
                thread_id.to_string(),
                Resume {
                    session_id: provider_session_id,
                    cwd: state.cwd.clone(),
                },
            ));
        }
        let model = as_record(as_record(data).and_then(|data| data.get("model")));
        if let (Some(provider), Some(model_id)) =
            (string_field(model, "provider"), string_field(model, "id"))
            && (self.flavor.is_pi() || state.native_model.is_empty())
        {
            state.native_model = pi_native_id(provider, model_id);
        }
        let fast_mode_enabled = as_record(data)
            .and_then(|data| data.get("fastModeEnabled"))
            .and_then(Value::as_bool);
        if self.flavor.is_omp()
            && let Some(enabled) = fast_mode_enabled
        {
            state.fast_mode_enabled = Some(enabled);
            state.fast_mode_requested = Some(enabled);
        }
    }

    /// The live session's provider id, for tests and diagnostics.
    pub fn provider_session_id(&self, session_id: &str) -> Option<String> {
        let live = self.live(session_id)?;
        let id = live.state.lock().provider_session_id.clone();
        (!id.is_empty()).then_some(id)
    }

    /// `ompCommandProvider`, bound to this family.
    pub fn command_provider(&self) -> Arc<dyn NativeCommandProvider> {
        Arc::new(OmpCommandProvider {
            family: self.clone(),
        })
    }
}

/// The selected value an omp question reply carries.
fn question_value(request: &PiExtensionUiRequest, reply: &UserQuestionReply) -> Option<String> {
    let UserQuestionReply::Answered { answers, custom } = reply else {
        return None;
    };
    if request.method == PiUiMethod::Select {
        let selected = answers.get(&request.id)?.first()?;
        if selected.is_empty() || !selected.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let index: usize = selected.parse().ok()?;
        request.options.get(index).cloned()
    } else {
        custom.as_ref()?.get(&request.id).cloned()
    }
}

/// `customMessageText`.
fn custom_message_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                let record = part.as_object()?;
                if record.get("type").and_then(Value::as_str) != Some("text") {
                    return None;
                }
                record.get("text").and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `interjectionFromCustomMessage`. omp's advisor and extensions interject
/// mid-turn. Frames marked `display: true` exist so clients can show them:
/// structured advisor notes replace the raw advisory envelope when present,
/// and even a blank body stays a labeled boundary so the segments around it
/// never merge.
fn interjection_from_custom_message(message: &Rec) -> Option<HarnessEvent> {
    use monocode_core::block::InterjectionSeverity;
    if message.get("display") != Some(&Value::Bool(true)) {
        return None;
    }
    let custom_type = string_field(Some(message), "customType")
        .unwrap_or("custom")
        .to_string();
    if custom_type == "advisor" {
        let notes = as_record(message.get("details")).and_then(|details| details.get("notes"));
        let mut bodies: Vec<&str> = Vec::new();
        let mut severity: Option<InterjectionSeverity> = None;
        if let Some(Value::Array(notes)) = notes {
            for raw in notes {
                let note = raw.as_object();
                let Some(body) = string_field(note, "note") else {
                    continue;
                };
                bodies.push(body);
                // Keep the highest severity the retained notes carry.
                let level = note
                    .and_then(|note| note.get("severity"))
                    .and_then(Value::as_str);
                match level {
                    Some("blocker") => severity = Some(InterjectionSeverity::Blocker),
                    Some("concern") if severity != Some(InterjectionSeverity::Blocker) => {
                        severity = Some(InterjectionSeverity::Concern)
                    }
                    Some("nit") if severity.is_none() => severity = Some(InterjectionSeverity::Nit),
                    _ => {}
                }
            }
        }
        if !bodies.is_empty() {
            return Some(HarnessEvent::Interjection {
                text: bodies.join("\n\n"),
                custom_type,
                severity,
            });
        }
    }
    Some(HarnessEvent::Interjection {
        text: custom_message_text(message.get("content")),
        custom_type,
        severity: None,
    })
}

/// `upsertTool`.
fn upsert_tool(
    state: &mut LiveState,
    fx: &mut Effects,
    id: &str,
    name: &str,
    input: Rec,
    index: Option<i64>,
) {
    if !state.tools_by_id.contains_key(id) {
        let title = tool_title(name, &input);
        let event = HarnessEvent::ToolStarted {
            agent_model: None,
            call_id: id.to_string(),
            title: title.clone(),
            kind: Some(tool_kind_from_name(name)),
            status: Some("pending".into()),
            background: None,
            preview: preview_from_tool(name, &input, None),
            paths: None,
        };
        state.tools_by_id.insert(
            id.to_string(),
            InFlightTool {
                id: id.to_string(),
                name: name.to_string(),
                input,
                partial_json: String::new(),
                title,
                finished: false,
            },
        );
        fx.emit(state, event);
        emit_task_list_if_needed(state, fx, id);
    } else if !input.is_empty() {
        update_tool(state, fx, id, input);
    }
    if let Some(index) = index.filter(|index| *index >= 0) {
        state.tools_by_index.insert(index, id.to_string());
    }
}

/// `updateTool`.
fn update_tool(state: &mut LiveState, fx: &mut Effects, id: &str, input: Rec) {
    let Some(tool) = state.tools_by_id.get_mut(id) else {
        return;
    };
    tool.input = merge_tool_input(&tool.input, &input);
    tool.title = tool_title(&tool.name, &tool.input);
    let event = HarnessEvent::ToolUpdated {
        agent_model: None,
        call_id: tool.id.clone(),
        title: Some(tool.title.clone()),
        kind: Some(tool_kind_from_name(&tool.name)),
        status: Some("pending".into()),
        detail: Some(summarize_tool_request(&tool.name, &tool.input)),
        preview: preview_from_tool(&tool.name, &tool.input, None),
        paths: None,
    };
    fx.emit(state, event);
    emit_task_list_if_needed(state, fx, id);
}

/// `emitTaskListIfNeeded`.
fn emit_task_list_if_needed(state: &LiveState, fx: &mut Effects, id: &str) {
    let Some(tool) = state.tools_by_id.get(id) else {
        return;
    };
    if let Some(items) = task_list_from_tool_input(&tool.name, &Value::Object(tool.input.clone())) {
        fx.emit(
            state,
            HarnessEvent::TasksUpdated {
                key: None,
                explanation: None,
                merge: None,
                authoritative: None,
                provider_session_id: None,
                items,
            },
        );
    }
}

/// `live.turnFailed?.(error)`.
fn turn_failed(state: &mut LiveState, fx: &mut Effects, error: String) {
    if let Some(handle) = state.turn_done.as_mut()
        && let Some(sender) = handle.0.take()
    {
        fx.0.push(Effect::Finish(sender, Err(error)));
    }
}

/// `finishActiveTurn`.
fn finish_active_turn(state: &mut LiveState, fx: &mut Effects, extra_events: Vec<HarnessEvent>) {
    if !state.active_turn && state.turn_done.is_none() {
        return;
    }
    state.turn_end_pending = false;
    state.active_turn = false;
    for event in extra_events {
        fx.emit(state, event);
    }
    match state.turn_done.take() {
        Some(mut handle) => {
            if let Some(sender) = handle.0.take() {
                fx.0.push(Effect::Finish(sender, Ok(())));
            }
        }
        None => state.turn_end_pending = true,
    }
}

/// `settlePendingTurn`.
fn settle_pending_turn(state: &mut LiveState, fx: &mut Effects) {
    if !state.turn_end_pending || state.turn_done.is_none() {
        return;
    }
    finish_active_turn(state, fx, Vec::new());
}

/// `ompCommandProvider`.
struct OmpCommandProvider {
    family: PiFamily,
}

impl NativeCommandProvider for OmpCommandProvider {
    fn discover(&self, context: CommandContext) -> BoxFuture<'_, Result<Vec<NativeCommand>>> {
        Box::pin(async move {
            let family = &self.family;
            let live = context
                .session_id
                .as_deref()
                .and_then(|session_id| family.live(session_id));
            let Some(live) = live.filter(|live| {
                normalize_project_path(&live.state.lock().cwd)
                    == normalize_project_path(&context.cwd)
            }) else {
                return discover_omp_commands(&family.children, &context.cwd).await;
            };
            if let Some(commands) = live.state.lock().available_commands.clone() {
                return Ok(commands);
            }
            let response = live
                .rpc
                .request(
                    rec(json!({ "type": "get_available_commands" })),
                    INIT_TIMEOUT_MS,
                )
                .await?;
            if let Some(commands) = live.state.lock().available_commands.clone() {
                return Ok(commands);
            }
            omp_commands_from_rpc_data(response.get("data")).map_err(|error| anyhow!(error))
        })
    }

    fn subscribe(
        &self,
        context: CommandContext,
        on_commands: CommandsListener,
    ) -> Option<Unsubscribe> {
        let family = self.family.clone();
        let key = command_context_key(&context);
        let id = {
            let mut state = family.state.lock();
            state.next_listener += 1;
            let id = state.next_listener;
            state
                .command_listeners
                .entry(key.clone())
                .or_default()
                .push((id, on_commands.clone()));
            id
        };
        let live = context
            .session_id
            .as_deref()
            .and_then(|session_id| family.live(session_id));
        if let Some(live) = live {
            let commands = {
                let state = live.state.lock();
                (normalize_project_path(&state.cwd) == normalize_project_path(&context.cwd))
                    .then(|| state.available_commands.clone())
                    .flatten()
            };
            if let Some(commands) = commands {
                on_commands(commands);
            }
        }
        Some(Box::new(move || {
            let mut state = family.state.lock();
            if let Some(listeners) = state.command_listeners.get_mut(&key) {
                listeners.retain(|(entry, _)| *entry != id);
                if listeners.is_empty() {
                    state.command_listeners.remove(&key);
                }
            }
        }))
    }

    fn raw_slash_commands(&self) -> bool {
        true
    }
}
