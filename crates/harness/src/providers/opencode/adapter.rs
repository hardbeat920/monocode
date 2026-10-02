//! Port of src/integrations/harness/providers/opencode/opencode.ts and
//! opencodeAdapter.ts: live OpenCode sessions over `opencode serve`.
//!
//! Each MonoCode thread gets its own server on a free loopback port. A turn
//! starts with `prompt_async`, and the SSE stream is its only completion
//! channel: `session.status` idle ends it.
//!
//! The TypeScript kept `liveByThread`, `resumeByThread`, and
//! `cancelledThreads` in module globals. Here they are fields of
//! [`OpenCodeAdapter`]. A live session's mutable state sits behind one lock;
//! events queue while it is held and reach the sink, in order, after it is
//! released. `handleEvent` ran synchronously up to its first `await` and was
//! not awaited, so a pending approval never held up later events. Here the
//! synchronous part runs on the SSE task and the rest is spawned.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use futures::FutureExt;
use futures::channel::oneshot;
use parking_lot::{Mutex, ReentrantMutex};
use regex::Regex;
use serde_json::{Value, json};

use monocode_core::block::{
    AgentStepKind, ApprovalDecided, ModelSettings, TurnIntent, TurnMetrics,
};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, HarnessEvent, HarnessSessionInput, QuestionDecision,
    RewindLastTurnInput, RewindLastTurnResult, SendTurnInput, SteerTurnInput,
};
use monocode_core::js;
use monocode_core::task_list::task_list_from_tool_input;
use monocode_core::user_question::{
    UserQuestion, UserQuestionReply, question_prompt_title, questions_from_unknown,
    selected_answer_labels,
};

use super::catalog::CatalogRefresher;
use super::client::{
    OpenCodeClient, OpenCodeSession, PermissionUpdate, PromptInput, is_not_found_error, parse_event,
};
use super::deps::{
    ComposeToolTitle, compose_tool_title, extract_shell_command, extract_skill_name,
    stream_text_delta,
};
use super::git::{
    SharedGitSource, generate_open_code_branch_name, generate_open_code_commit_message,
    generate_open_code_pr_content,
};
use super::protocol::{
    MINIMUM_OPENCODE_VERSION, OpenCodePart, ParsedOpenCodeModelSlug, PartStore, PartTime, Record,
    append_open_code_assistant_text_delta, build_open_code_permission_rules, compare_semver,
    context_used_from_message_info, detail_from_tool_part, event_session_id, field,
    is_known_hidden_agent, is_truthy, merge_open_code_assistant_text, open_code_child_session_id,
    parse_open_code_model_slug, parse_open_code_version, parse_server_url_from_output,
    permission_title, preview_from_tool_part, record_field, session_error_message, string_field,
    text_delta_event, to_open_code_permission_reply, to_open_code_prompt_parts,
    tool_kind_from_name, turn_metrics_from_message_info,
};
use super::text::OpenCodeText;
use super::title::generate_open_code_session_title;
use crate::core::catalog::SharedCatalog;
use crate::core::child::{BinaryPathChoice, ChildEvent, Children, SseEvent, SseEvents};
use crate::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, GeneratedPrContent, HarnessAdapter,
    TextPromptInput, TitleInput,
};
use crate::core::session_title::GeneratedSessionTitle;
use crate::core::task::{AbortSignal, BoxFuture, SharedSpawner, sleep};

const SERVER_TIMEOUT_MS: u64 = 30_000;
/// How many parts an unidentified child may bank before its row is known.
const MAX_PENDING_SUBAGENT: usize = 64;
/// How many unidentified children may bank parts at once.
const MAX_PENDING_SUBAGENT_SESSIONS: usize = 32;
const MODEL_ID_ERROR: &str =
    "OpenCode models use provider/model ids. Wait for the catalog to load, then pick a model.";

static FUNCTIONALITY_NOT_SUPPORTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)functionality not supported").unwrap());
static FILE_PART_MEDIA_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)file part media type\s+([^\s'"`]+)"#).unwrap());

/// The role a message plays in the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
    Hidden,
}

struct PendingApproval {
    id: String,
    resolve: Option<oneshot::Sender<ApprovalDecision>>,
}

impl PendingApproval {
    /// A promise resolves once; later calls do nothing.
    fn resolve(&mut self, decision: ApprovalDecision) {
        if let Some(resolve) = self.resolve.take() {
            let _ = resolve.send(decision);
        }
    }
}

struct PendingQuestion {
    id: String,
    resolve: Option<oneshot::Sender<UserQuestionReply>>,
}

impl PendingQuestion {
    fn resolve(&mut self, reply: UserQuestionReply) {
        if let Some(resolve) = self.resolve.take() {
            let _ = resolve.send(reply);
        }
    }
}

/// `turnDone` and `turnFailed`: the latch of the turn in flight. `token`
/// stands in for the function identity the TypeScript compared.
struct TurnLatch {
    token: u64,
    done: oneshot::Sender<Result<(), String>>,
}

/// The mutable half of the TypeScript `Live`.
struct LiveState {
    runtime_mode: RuntimeMode,
    planning: bool,
    on_event: EventSink,
    /// Keyed by UI request id. Ids only grow, so key order is insertion order.
    approvals: BTreeMap<i64, PendingApproval>,
    questions: BTreeMap<i64, (PendingQuestion, Vec<UserQuestion>)>,
    visible_question_id: Option<i64>,
    next_approval_ui_id: i64,
    session_parent_by_id: HashMap<String, Option<String>>,
    /// Child session id to the agent tool row that spawned it.
    subagent_sessions: HashMap<String, String>,
    subagent_models: HashMap<String, String>,
    /// Child parts that arrived before their row was known, oldest first.
    pending_subagent: Vec<(String, Vec<OpenCodePart>)>,
    part_by_id: PartStore,
    emitted_text_by_part_id: HashMap<String, String>,
    message_role_by_id: HashMap<String, Role>,
    turn_metrics_by_message_id: HashMap<String, TurnMetrics>,
    cancelled: bool,
    mute_updates: bool,
    turn: Option<TurnLatch>,
    turn_end_pending: bool,
    active_turn: bool,
    /// The rejection of the last queued operation, which `await live.turns`
    /// rethrew.
    turns_error: Option<String>,
    outbox: Vec<HarnessEvent>,
}

impl LiveState {
    fn new(runtime_mode: RuntimeMode, planning: bool, on_event: EventSink) -> Self {
        Self {
            runtime_mode,
            planning,
            on_event,
            approvals: BTreeMap::new(),
            questions: BTreeMap::new(),
            visible_question_id: None,
            next_approval_ui_id: 1,
            session_parent_by_id: HashMap::new(),
            subagent_sessions: HashMap::new(),
            subagent_models: HashMap::new(),
            pending_subagent: Vec::new(),
            part_by_id: PartStore::default(),
            emitted_text_by_part_id: HashMap::new(),
            message_role_by_id: HashMap::new(),
            turn_metrics_by_message_id: HashMap::new(),
            cancelled: false,
            mute_updates: false,
            turn: None,
            turn_end_pending: false,
            active_turn: false,
            turns_error: None,
            outbox: Vec::new(),
        }
    }

    fn emit(&mut self, event: HarnessEvent) {
        self.outbox.push(event);
    }

    fn turn_token(&self) -> Option<u64> {
        self.turn.as_ref().map(|turn| turn.token)
    }

    /// Deny every pending approval and skip every pending question.
    fn resolve_pending(&mut self) {
        for pending in self.approvals.values_mut() {
            pending.resolve(ApprovalDecision::Deny);
        }
        self.approvals.clear();
        for (pending, _) in self.questions.values_mut() {
            pending.resolve(UserQuestionReply::Skipped);
        }
        self.questions.clear();
    }
}

/// `Live`: one thread's server, session, and stream state.
struct Live {
    client: OpenCodeClient,
    open_code_session_id: String,
    cwd: String,
    catalog: SharedCatalog,
    spawner: SharedSpawner,
    /// Held while events reach the sink, so they arrive in the order the
    /// state changed. Reentrant, so a sink may call back into the adapter.
    order: ReentrantMutex<()>,
    state: Mutex<LiveState>,
    /// `live.turns`: queued operations run one at a time.
    turns: smol::lock::Mutex<()>,
}

impl Live {
    /// Change the state, then deliver what the change emitted.
    fn with<R>(&self, change: impl FnOnce(&mut LiveState) -> R) -> R {
        let _order = self.order.lock();
        let (result, events, sink) = {
            let mut state = self.state.lock();
            let result = change(&mut state);
            let events = std::mem::take(&mut state.outbox);
            (result, events, state.on_event.clone())
        };
        for event in events {
            sink(event);
        }
        result
    }
}

#[derive(Debug, Clone)]
struct Resume {
    session_id: String,
    cwd: String,
}

#[derive(Default)]
struct Threads {
    live_by_thread: HashMap<String, Arc<Live>>,
    resume_by_thread: HashMap<String, Resume>,
    cancelled_threads: HashSet<String>,
}

/// What the server watch saw before the client connected.
#[derive(Default)]
struct ServerStart {
    url: String,
    /// `Some(code)` once the server exited.
    exited: Option<Option<i32>>,
}

type Continuation = BoxFuture<'static, Result<()>>;

struct Inner {
    children: Children,
    catalog: SharedCatalog,
    spawner: SharedSpawner,
    git: Option<SharedGitSource>,
    threads: Mutex<Threads>,
    refresher: CatalogRefresher,
    text: OpenCodeText,
    next_turn_token: AtomicU64,
    processed_events: Arc<AtomicUsize>,
}

/// The OpenCode [`HarnessAdapter`]. Clones share one adapter.
#[derive(Clone)]
pub struct OpenCodeAdapter {
    inner: Arc<Inner>,
}

impl OpenCodeAdapter {
    /// `git` supplies the repository reads for commit and pull request text.
    pub fn new(
        children: Children,
        catalog: SharedCatalog,
        spawner: SharedSpawner,
        git: Option<SharedGitSource>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                refresher: CatalogRefresher::new(
                    children.clone(),
                    catalog.clone(),
                    spawner.clone(),
                ),
                text: OpenCodeText::new(children.clone(), catalog.clone(), spawner.clone()),
                children,
                catalog,
                spawner,
                git,
                threads: Mutex::new(Threads::default()),
                next_turn_token: AtomicU64::new(0),
                processed_events: Arc::new(AtomicUsize::new(0)),
            }),
        }
    }

    /// The isolated text backend.
    pub fn text(&self) -> &OpenCodeText {
        &self.inner.text
    }

    /// SSE frames handled so far across every session. Tests wait on it.
    #[cfg(test)]
    pub(crate) fn processed_events(&self) -> usize {
        self.inner.processed_events.load(Ordering::SeqCst)
    }

    fn live(&self, session_id: &str) -> Option<Arc<Live>> {
        self.inner
            .threads
            .lock()
            .live_by_thread
            .get(session_id)
            .cloned()
    }

    fn take_cancelled(&self, session_id: &str) -> bool {
        self.inner
            .threads
            .lock()
            .cancelled_threads
            .remove(session_id)
    }

    /// `parseOpenCodeModelSlug(nativeModelId(model))`.
    fn parsed_model(&self, model: &str) -> Result<ParsedOpenCodeModelSlug> {
        let native = self.inner.catalog.read().native_model_id_for(model);
        parse_open_code_model_slug(Some(&native)).ok_or_else(|| anyhow!(MODEL_ID_ERROR))
    }

    /// `ensureLive`, clearing a pending cancel when it fails.
    async fn ensure_live_or_forget_cancel(
        &self,
        input: &HarnessSessionInput,
        on_event: &EventSink,
    ) -> Result<Arc<Live>> {
        match self.ensure_live(input, on_event).await {
            Ok(live) => Ok(live),
            Err(error) => {
                self.take_cancelled(&input.session_id);
                Err(error)
            }
        }
    }

    /// `sendOpenCodeTurn`.
    async fn send_open_code_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> Result<()> {
        let session_id = input.session.session_id.clone();
        let live = self
            .ensure_live_or_forget_cancel(&input.session, &on_event)
            .await?;
        if self.take_cancelled(&session_id) {
            return Ok(());
        }
        live.with(|s| {
            s.on_event = on_event;
            s.runtime_mode = input.session.runtime_mode;
            s.planning = input.session.intent == Some(TurnIntent::Plan);
        });
        let turn = async {
            live.with(|s| {
                s.cancelled = false;
                s.mute_updates = false;
            });
            match self.run_turn(&live, &input, on_accepted).await {
                Err(_) if live.with(|s| s.cancelled) => Ok(()),
                result => result,
            }
        };
        queue_turn(&live, turn).await
    }

    /// `compactOpenCodeContext`.
    async fn compact_open_code_context(
        &self,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> Result<()> {
        let live = self.ensure_live_or_forget_cancel(&input, &on_event).await?;
        if self.take_cancelled(&input.session_id) {
            return Ok(());
        }
        let model = self.parsed_model(&input.model)?;
        live.with(|s| s.on_event = on_event);
        let compaction = async {
            live.with(|s| {
                s.cancelled = false;
                s.mute_updates = false;
            });
            // Unlike prompt_async, summarize responds only after the
            // compaction pass. Keep this outside the normal turn latch: its
            // eventual session.status=idle must not become a pending
            // completion for the next user turn.
            match live
                .client
                .summarize_session(&live.open_code_session_id, &model)
                .await
            {
                Err(_) if live.with(|s| s.cancelled) => Ok(()),
                result => result,
            }
        };
        queue_turn(&live, compaction).await
    }

    /// `rewindOpenCodeLastTurn`.
    async fn rewind_open_code_last_turn(
        &self,
        input: RewindLastTurnInput,
        on_event: EventSink,
    ) -> Result<RewindLastTurnResult> {
        let live = self
            .ensure_live_or_forget_cancel(&input.session, &on_event)
            .await?;
        if self.take_cancelled(&input.session.session_id) {
            return Ok(RewindLastTurnResult { submitted: false });
        }
        live.with(|s| s.on_event = on_event);
        {
            let _turns = live.turns.lock().await;
            // TODO(port): `await live.turns` rethrew the last queued turn's
            // rejection, so a failed prompt_async makes the next edit fail
            // with that old error.
            if let Some(error) = live.with(|s| s.turns_error.clone()) {
                bail!(error);
            }
        }
        if live.with(|s| s.active_turn) {
            bail!("Stop the current turn before editing the last message");
        }
        let message_id = latest_open_code_user_message_id(&live).await?;
        live.client
            .revert_session(&live.open_code_session_id, &message_id)
            .await?;
        Ok(RewindLastTurnResult { submitted: false })
    }

    /// `steerOpenCodeTurn`.
    async fn steer_open_code_turn(&self, input: SteerTurnInput) -> Result<()> {
        let live = self
            .live(&input.session_id)
            .filter(|live| live.with(|s| s.active_turn))
            .ok_or_else(|| anyhow!("No active turn to steer"))?;
        let model = self.parsed_model(&input.model)?;
        let parts = to_open_code_prompt_parts(
            &input.text,
            input.attachments.as_deref().unwrap_or_default(),
        )
        .map_err(|error| anyhow!(error))?;
        if parts.is_empty() {
            return Ok(());
        }
        let setting = |key: &str| {
            input
                .model_settings
                .as_ref()
                .and_then(|settings| settings.get(key).cloned())
        };
        live.client
            .prompt_async(&PromptInput {
                session_id: live.open_code_session_id.clone(),
                model,
                agent: setting("agent"),
                variant: setting("variant"),
                parts,
            })
            .await
    }

    /// `respondOpenCodeApproval`.
    fn respond_open_code_approval(
        &self,
        session_id: &str,
        request_id: i64,
        decision: ApprovalDecision,
    ) {
        if let Some(live) = self.live(session_id) {
            live.with(|s| {
                if let Some(pending) = s.approvals.get_mut(&request_id) {
                    pending.resolve(decision);
                }
            });
        }
    }

    /// `respondOpenCodeQuestion`.
    fn respond_open_code_question(
        &self,
        session_id: &str,
        request_id: i64,
        reply: UserQuestionReply,
    ) {
        if let Some(live) = self.live(session_id) {
            live.with(|s| {
                if let Some((pending, _)) = s.questions.get_mut(&request_id) {
                    pending.resolve(reply);
                }
            });
        }
    }

    /// `cancelOpenCodeTurn`.
    async fn cancel_open_code_turn(&self, session_id: &str) -> Result<()> {
        let Some(live) = self.live(session_id) else {
            self.inner
                .threads
                .lock()
                .cancelled_threads
                .insert(session_id.to_string());
            return Ok(());
        };
        live.with(|s| {
            s.cancelled = true;
            s.mute_updates = true;
            s.resolve_pending();
        });
        live.client.abort_session(&live.open_code_session_id).await;
        live.with(|s| {
            finish_active_turn(
                s,
                vec![
                    HarnessEvent::MessageCompleted,
                    HarnessEvent::ReasoningCompleted,
                ],
            )
        });
        Ok(())
    }

    /// `stopOpenCodeSession`: kill the server but keep resume state.
    async fn stop_open_code_session(&self, session_id: &str) {
        let live = {
            let mut threads = self.inner.threads.lock();
            threads.cancelled_threads.remove(session_id);
            threads.live_by_thread.remove(session_id)
        };
        if let Some(live) = live {
            live.with(|s| {
                s.mute_updates = true;
                s.resolve_pending();
                s.active_turn = false;
                if let Some(turn) = s.turn.take() {
                    let _ = turn.done.send(Ok(()));
                }
            });
            live.client.abort_session(&live.open_code_session_id).await;
            live.client.close_events(session_id).await;
        }
        self.inner.children.unwatch_child(session_id);
        let _ = self.inner.children.kill_child(session_id).await;
    }

    /// `forgetOpenCodeSession`.
    async fn forget_open_code_session(&self, session_id: &str) {
        self.inner
            .threads
            .lock()
            .resume_by_thread
            .remove(session_id);
        self.stop_open_code_session(session_id).await;
    }

    /// `bindOpenCodeSession`.
    fn bind_open_code_session(&self, thread_id: &str, provider_session_id: &str, cwd: &str) {
        let session_id = js::trim(provider_session_id);
        if thread_id.is_empty() || session_id.is_empty() || js::trim(cwd).is_empty() {
            return;
        }
        self.inner.threads.lock().resume_by_thread.insert(
            thread_id.to_string(),
            Resume {
                session_id: session_id.to_string(),
                cwd: cwd.to_string(),
            },
        );
    }

    /// `ensureLive`.
    async fn ensure_live(
        &self,
        input: &HarnessSessionInput,
        on_event: &EventSink,
    ) -> Result<Arc<Live>> {
        let session_id = &input.session_id;
        let existing = self.live(session_id);
        if let Some(existing) = existing.as_ref().filter(|live| live.cwd == input.cwd) {
            let mode_changed = existing.with(|s| {
                s.on_event = on_event.clone();
                s.runtime_mode != input.runtime_mode
            });
            if mode_changed {
                let permission = build_open_code_permission_rules(input.runtime_mode);
                existing
                    .client
                    .update_session(
                        &existing.open_code_session_id,
                        &PermissionUpdate {
                            permission: &permission,
                        },
                    )
                    .await?;
            }
            existing.with(|s| s.runtime_mode = input.runtime_mode);
            return Ok(existing.clone());
        }
        if existing.is_some() {
            self.inner
                .threads
                .lock()
                .resume_by_thread
                .remove(session_id);
            self.stop_open_code_session(session_id).await;
        }

        let resume = {
            let mut threads = self.inner.threads.lock();
            let resume = threads.resume_by_thread.get(session_id).cloned();
            if resume
                .as_ref()
                .is_some_and(|resume| resume.cwd != input.cwd)
            {
                threads.resume_by_thread.remove(session_id);
            }
            resume.filter(|resume| resume.cwd == input.cwd)
        };

        let children = &self.inner.children;
        let binary = children.resolve_open_code_binary().await?;
        self.assert_open_code_version(&binary.path, &input.cwd)
            .await?;

        let start = Arc::new(Mutex::new(ServerStart::default()));
        let live_ref: Arc<Mutex<Option<Arc<Live>>>> = Arc::default();
        self.watch_server(session_id, &start, &live_ref, on_event);

        let port = children.free_harness_port().await?;
        children
            .spawn_child(
                session_id,
                &binary.path,
                vec![
                    "serve".into(),
                    "--hostname=127.0.0.1".into(),
                    format!("--port={port}"),
                ],
                &input.cwd,
                None,
                Some(HarnessId::Opencode),
            )
            .await?;

        match self
            .connect(input, on_event, &start, &live_ref, resume)
            .await
        {
            Ok(live) => Ok(live),
            Err(error) => {
                self.stop_open_code_session(session_id).await;
                Err(error)
            }
        }
    }

    /// The `watchChild` handlers: the listening URL from stdout or stderr,
    /// and the server's exit.
    fn watch_server(
        &self,
        session_id: &str,
        start: &Arc<Mutex<ServerStart>>,
        live_ref: &Arc<Mutex<Option<Arc<Live>>>>,
        on_event: &EventSink,
    ) {
        let events = self.inner.children.watch_child(session_id);
        let inner = self.inner.clone();
        let session_id = session_id.to_string();
        let start = start.clone();
        let live_ref = live_ref.clone();
        let fallback = on_event.clone();
        self.inner.spawner.spawn(
            async move {
                while let Ok(event) = events.recv().await {
                    let code = match event {
                        ChildEvent::Stdout(line) | ChildEvent::Stderr(line) => {
                            if let Some(url) = parse_server_url_from_output(&line) {
                                start.lock().url = url;
                            }
                            continue;
                        }
                        ChildEvent::Exit(code) => code,
                    };
                    start.lock().exited = Some(code);
                    inner.threads.lock().live_by_thread.remove(&session_id);
                    let ended = HarnessEvent::SessionEnded {
                        code: code.map(i64::from),
                    };
                    let live = live_ref.lock().clone();
                    match live {
                        Some(live) => live.with(|s| {
                            if !s.mute_updates {
                                s.emit(ended);
                            }
                            s.mute_updates = true;
                            if let Some(turn) = s.turn.take() {
                                let _ = turn.done.send(Err("OpenCode server exited".into()));
                            }
                        }),
                        None => fallback(ended),
                    }
                }
            }
            .boxed(),
        );
    }

    /// The `try` half of `ensureLive`: connect, pick the session, subscribe.
    async fn connect(
        &self,
        input: &HarnessSessionInput,
        on_event: &EventSink,
        start: &Arc<Mutex<ServerStart>>,
        live_ref: &Arc<Mutex<Option<Arc<Live>>>>,
        resume: Option<Resume>,
    ) -> Result<Arc<Live>> {
        let url = wait_for_server_url(start, SERVER_TIMEOUT_MS).await?;
        let client = OpenCodeClient::new(&url, &input.cwd, self.inner.children.clone());
        let can_resume = resume.is_some();
        let session =
            resolve_session(&client, resume.as_ref(), input.runtime_mode, &input.cwd).await?;
        if can_resume && let Err(error) = repair_unsupported_file_turn(&client, &session.id).await {
            log::debug!("[monocode] opencode attachment recovery {error:#}");
        }

        let live = Arc::new(Live {
            client,
            open_code_session_id: session.id.clone(),
            cwd: input.cwd.clone(),
            catalog: self.inner.catalog.clone(),
            spawner: self.inner.spawner.clone(),
            order: ReentrantMutex::new(()),
            state: Mutex::new(LiveState::new(
                input.runtime_mode,
                input.intent == Some(TurnIntent::Plan),
                on_event.clone(),
            )),
            turns: smol::lock::Mutex::new(()),
        });
        *live_ref.lock() = Some(live.clone());
        {
            let mut threads = self.inner.threads.lock();
            threads
                .live_by_thread
                .insert(input.session_id.clone(), live.clone());
            threads.resume_by_thread.insert(
                input.session_id.clone(),
                Resume {
                    session_id: session.id.clone(),
                    cwd: input.cwd.clone(),
                },
            );
        }

        let events = live.client.subscribe_events(&input.session_id).await?;
        self.pump_events(&input.session_id, &live, events);

        live.with(|s| {
            s.emit(HarnessEvent::SessionProviderBound {
                provider_session_id: session.id.clone(),
            });
            s.emit(HarnessEvent::SessionStarted);
        });
        Ok(live)
    }

    /// The `subscribeEvents` callbacks, as a task reading the stream.
    fn pump_events(&self, session_id: &str, live: &Arc<Live>, events: SseEvents) {
        let adapter = self.clone();
        let live = live.clone();
        let session_id = session_id.to_string();
        let processed = self.inner.processed_events.clone();
        self.inner.spawner.spawn(
            async move {
                while let Ok(event) = events.recv().await {
                    match event {
                        SseEvent::Data(data) => {
                            if let Some(event) = parse_event(&data) {
                                on_sse_event(&live, event);
                            }
                        }
                        SseEvent::End(error) => adapter.on_sse_end(&session_id, &live, error).await,
                    }
                    processed.fetch_add(1, Ordering::SeqCst);
                }
            }
            .boxed(),
        );
    }

    /// The `subscribeEvents` end handler.
    async fn on_sse_end(&self, session_id: &str, live: &Arc<Live>, error: Option<String>) {
        if live.with(|s| s.mute_updates || s.cancelled) {
            return;
        }
        let message = error
            .map(|error| js::trim(&error).to_string())
            .filter(|error| !error.is_empty())
            .unwrap_or_else(|| "OpenCode event stream ended unexpectedly.".into());
        // prompt_async has no response body to await; the SSE stream is its
        // only completion channel. Reusing a Live after this point accepts the
        // next prompt but can never observe it, which looks like a dead thread.
        self.inner.threads.lock().live_by_thread.remove(session_id);
        let failed = live.with(|s| {
            let failed = s.turn.take();
            s.mute_updates = true;
            s.resolve_pending();
            failed
        });
        self.inner.children.unwatch_child(session_id);
        let _ = self.inner.children.kill_child(session_id).await;
        match failed {
            Some(turn) => {
                let _ = turn.done.send(Err(message));
            }
            None => live.with(|s| s.emit(HarnessEvent::SessionError { message })),
        }
    }

    /// `runTurn`.
    async fn run_turn(
        &self,
        live: &Arc<Live>,
        input: &SendTurnInput,
        on_accepted: Option<AcceptedHook>,
    ) -> Result<()> {
        let model = self.parsed_model(&input.session.model)?;
        let parts = to_open_code_prompt_parts(
            &input.text,
            input.attachments.as_deref().unwrap_or_default(),
        )
        .map_err(|error| anyhow!(error))?;
        if parts.is_empty() {
            return Ok(());
        }

        let (done, finished) = oneshot::channel();
        let token = self.inner.next_turn_token.fetch_add(1, Ordering::SeqCst) + 1;
        live.with(|s| {
            s.turn = Some(TurnLatch { token, done });
            s.active_turn = true;
            s.turn_metrics_by_message_id.clear();
            settle_pending_turn(s);
        });

        let settings = input.session.model_settings.as_ref();
        let prompt = PromptInput {
            session_id: live.open_code_session_id.clone(),
            model,
            agent: Some(open_code_agent_for_turn(input.session.intent, settings)),
            variant: settings.and_then(|settings| settings.get("variant").cloned()),
            parts,
        };
        let result = async {
            live.client.prompt_async(&prompt).await?;
            if let Some(on_accepted) = &on_accepted {
                on_accepted();
            }
            live.with(settle_pending_turn);
            match finished.await {
                Ok(Err(message)) => Err(anyhow!(message)),
                Ok(Ok(())) | Err(_) => Ok(()),
            }
        }
        .await;
        let result = match result {
            Ok(()) => Ok(()),
            Err(_) if live.with(|s| s.cancelled) => Ok(()),
            Err(error) => {
                live.with(|s| {
                    s.emit(HarnessEvent::SessionError {
                        message: error.to_string(),
                    })
                });
                Err(error)
            }
        };
        live.with(|s| s.turn = None);
        result
    }

    /// `assertOpenCodeVersion`.
    async fn assert_open_code_version(&self, path: &str, cwd: &str) -> Result<()> {
        let output = self
            .inner
            .children
            .exec_child(
                path,
                vec!["--version".into()],
                Some(cwd),
                Some(HarnessId::Opencode),
                BinaryPathChoice::Runtime,
            )
            .await
            .unwrap_or_default();
        let Some(version) = parse_open_code_version(&output) else {
            bail!(
                "Unable to determine OpenCode version. MonoCode requires v{MINIMUM_OPENCODE_VERSION} or newer."
            );
        };
        if compare_semver(&version, MINIMUM_OPENCODE_VERSION) < 0 {
            bail!(
                "OpenCode v{version} is too old. Upgrade to v{MINIMUM_OPENCODE_VERSION} or newer."
            );
        }
        // TODO(port): there is no upper bound. OpenCode 2 passes this gate but
        // serves its API under `/api` behind basic auth, answers
        // `POST /session` with 405, and serves its web app at `/event`, so a
        // turn fails with "OpenCode HTTP 405". `opencode_major_version` in
        // monocode-process tells the two apart.
        Ok(())
    }
}

/// `live.turns = live.turns.catch(() => undefined).then(run); await live.turns`.
async fn queue_turn(live: &Arc<Live>, run: impl Future<Output = Result<()>>) -> Result<()> {
    let _turns = live.turns.lock().await;
    let result = run.await;
    live.with(|s| s.turns_error = result.as_ref().err().map(ToString::to_string));
    result
}

/// The `subscribeEvents` event handler.
fn on_sse_event(live: &Arc<Live>, event: Record) {
    let (muted, turn) = live.with(|s| (s.mute_updates, s.turn_token()));
    if muted {
        return;
    }
    let Some(continuation) = handle_event(live, &event) else {
        return;
    };
    let task_live = live.clone();
    live.spawner.spawn(
        async move {
            if let Err(error) = continuation.await {
                route_error(&task_live, turn, &error);
            }
        }
        .boxed(),
    );
}

/// The `handleEvent(...).catch(...)` handler.
fn route_error(live: &Live, turn: Option<u64>, error: &anyhow::Error) {
    live.with(|s| {
        if s.mute_updates || s.turn_token() != turn {
            return;
        }
        // Failed ancestry lookups or replies must end the turn visibly;
        // otherwise a child can remain blocked on an unanswered request.
        s.emit(HarnessEvent::SessionError {
            message: format!("Could not route OpenCode event: {error}"),
        });
        finish_active_turn(s, Vec::new());
    });
}

/// `handleEvent`. Runs the synchronous part now and returns the rest, if
/// any, for the caller to spawn.
fn handle_event(live: &Arc<Live>, event: &Record) -> Option<Continuation> {
    let event_type = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let properties = record_field(Some(event), "properties")
        .cloned()
        .unwrap_or_default();
    // Session lifecycle events establish ancestry, including nested
    // subagents. Record them before applying the parent transcript's filter.
    if event_type == "session.created" || event_type == "session.updated" {
        let info = record_field(Some(&properties), "info");
        if let Some(id) = string_field(info, "id") {
            let parent_id = string_field(info, "parentID").map(str::to_string);
            live.with(|s| s.session_parent_by_id.insert(id.to_string(), parent_id));
        }
        return None;
    }

    if let Some(payload_session_id) =
        event_session_id(event).filter(|session_id| *session_id != live.open_code_session_id)
    {
        if matches!(
            event_type.as_str(),
            "message.updated" | "message.part.updated" | "message.part.delta"
        ) {
            live.with(|s| {
                handle_subagent_event(
                    s,
                    &live.open_code_session_id,
                    &payload_session_id,
                    &event_type,
                    &properties,
                )
            });
            return None;
        }
        // Only blocking interactions are forwarded otherwise. In particular, a
        // child's idle/error event must never finish the parent's active turn.
        if event_type != "permission.asked" && event_type != "question.asked" {
            return None;
        }
        let turn = live.with(|s| s.turn_token());
        let known =
            live.with(|s| descendant_known(s, &live.open_code_session_id, &payload_session_id));
        match known {
            Some(false) => return None,
            Some(true) => {
                if live.with(|s| s.mute_updates || s.turn_token() != turn) {
                    return None;
                }
            }
            None => {
                // Resumed children may predate the SSE subscription. Resolve
                // their ancestry from the server.
                let live = live.clone();
                return Some(
                    async move {
                        if !is_descendant_session(&live, &payload_session_id).await? {
                            return Ok(());
                        }
                        if live.with(|s| s.mute_updates || s.turn_token() != turn) {
                            return Ok(());
                        }
                        match handle_request(&live, &event_type, &properties) {
                            Some(next) => next.await,
                            None => Ok(()),
                        }
                    }
                    .boxed(),
                );
            }
        }
    }

    match event_type.as_str() {
        "permission.asked" | "question.asked" => handle_request(live, &event_type, &properties),
        _ => {
            live.with(|s| handle_transcript_event(s, live, &event_type, &properties));
            None
        }
    }
}

fn handle_request(live: &Arc<Live>, event_type: &str, properties: &Record) -> Option<Continuation> {
    if event_type == "permission.asked" {
        handle_permission(live, properties)
    } else {
        handle_question(live, properties)
    }
}

enum PermissionNext {
    Reply(super::protocol::PermissionReply),
    Wait(i64, oneshot::Receiver<ApprovalDecision>),
}

/// The `permission.asked` branch of `handleEvent`.
fn handle_permission(live: &Arc<Live>, properties: &Record) -> Option<Continuation> {
    let props = Some(properties);
    let id = string_field(props, "id")
        .or_else(|| string_field(props, "requestID"))?
        .to_string();
    let next = live.with(|s| {
        if s.approvals.values().any(|pending| pending.id == id) {
            return None;
        }
        let permission = string_field(props, "permission").unwrap_or("tool");
        let patterns: Vec<String> = field(props, "patterns")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let empty = Record::new();
        let metadata = record_field(props, "metadata").unwrap_or(&empty);
        let metadata_rec = Some(metadata);
        let call_id = string_field(record_field(props, "tool"), "callID")
            .or_else(|| string_field(props, "callID"))
            .or_else(|| string_field(props, "toolCallId"))
            .or_else(|| string_field(metadata_rec, "callID"))
            .or_else(|| string_field(metadata_rec, "toolCallId"))
            .map(str::to_string);
        let kind = tool_kind_from_name(permission);
        let first_pattern = patterns.first();
        let tool_part = |state: Record| OpenCodePart {
            id: id.clone(),
            part_type: "tool".into(),
            tool: Some(permission.to_string()),
            state: Some(state),
            ..Default::default()
        };
        let mut state = metadata.clone();
        let input = metadata
            .get("input")
            .filter(|input| !input.is_null())
            .cloned()
            .or_else(|| first_pattern.map(|pattern| json!({ "path": pattern })));
        match input {
            Some(input) => state.insert("input".into(), input),
            None => state.remove("input"),
        };
        let preview = preview_from_tool_part(&tool_part(state)).or_else(|| {
            first_pattern.and_then(|pattern| {
                let state = json!({ "input": { "path": pattern, "pattern": pattern } });
                preview_from_tool_part(&tool_part(state.as_object().cloned().unwrap_or_default()))
            })
        });
        let fallback = permission_title(permission, &patterns);
        let command = extract_shell_command(metadata.get("input")).or_else(|| {
            (permission == "bash")
                .then(|| first_pattern.cloned())
                .flatten()
        });
        let skill = extract_skill_name(metadata.get("input"));
        let composed = compose_tool_title(&ComposeToolTitle {
            kind: Some(&kind),
            title: Some(&fallback),
            command: command.as_deref(),
            skill: skill.as_deref(),
            path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
            query: preview
                .as_ref()
                .and_then(|preview| preview.query.as_deref()),
            preview_kind: preview.as_ref().map(|preview| preview.kind),
        });
        let title = if composed.is_empty() {
            fallback
        } else {
            composed
        };

        if s.planning {
            let decision = if kind == "read" || kind == "search" {
                ApprovalDecision::Allow
            } else {
                ApprovalDecision::Deny
            };
            return Some(PermissionNext::Reply(to_open_code_permission_reply(
                decision,
            )));
        }
        if s.runtime_mode == RuntimeMode::FullAccess {
            return Some(PermissionNext::Reply(
                super::protocol::PermissionReply::Once,
            ));
        }
        let ui_id = s.next_approval_ui_id;
        s.next_approval_ui_id += 1;
        let (resolve, decision) = oneshot::channel();
        s.approvals.insert(
            ui_id,
            PendingApproval {
                id: id.clone(),
                resolve: Some(resolve),
            },
        );
        if let Some(call_id) = &call_id {
            s.emit(HarnessEvent::ToolUpdated {
                agent_model: None,
                call_id: call_id.clone(),
                title: Some(title.clone()),
                kind: Some(kind.clone()),
                status: None,
                detail: None,
                preview: preview.clone(),
                paths: None,
            });
        }
        s.emit(HarnessEvent::ApprovalRequested {
            request_id: ui_id,
            title,
            kind: Some(kind),
            call_id,
            preview,
        });
        Some(PermissionNext::Wait(ui_id, decision))
    })?;
    let live = live.clone();
    Some(match next {
        PermissionNext::Reply(reply) => {
            async move { live.client.reply_permission(&id, reply).await }.boxed()
        }
        PermissionNext::Wait(ui_id, decision) => wait_approval(live, ui_id, id, decision).boxed(),
    })
}

/// The `question.asked` branch of `handleEvent`.
fn handle_question(live: &Arc<Live>, properties: &Record) -> Option<Continuation> {
    let props = Some(properties);
    let id = string_field(props, "id")
        .or_else(|| string_field(props, "requestID"))?
        .to_string();
    let (ui_id, reply, questions) = live.with(|s| {
        if s.questions.values().any(|(pending, _)| pending.id == id) {
            return None;
        }
        let questions = questions_from_unknown(&Value::Object(properties.clone()));
        let ui_id = s.next_approval_ui_id;
        s.next_approval_ui_id += 1;
        let (resolve, reply) = oneshot::channel();
        s.questions.insert(
            ui_id,
            (
                PendingQuestion {
                    id: id.clone(),
                    resolve: Some(resolve),
                },
                questions.clone(),
            ),
        );
        show_next_question(s);
        Some((ui_id, reply, questions))
    })?;
    Some(wait_question(live.clone(), ui_id, id, questions, reply).boxed())
}

/// `waitApproval`, from the decision on.
async fn wait_approval(
    live: Arc<Live>,
    ui_id: i64,
    id: String,
    decision: oneshot::Receiver<ApprovalDecision>,
) -> Result<()> {
    // A dropped sender is a promise that never settles.
    let Ok(decision) = decision.await else {
        return Ok(());
    };
    live.with(|s| {
        s.approvals.remove(&ui_id);
        s.emit(HarnessEvent::ApprovalResolved {
            request_id: ui_id,
            decision: match decision {
                ApprovalDecision::Allow => ApprovalDecided::Allow,
                ApprovalDecision::Deny => ApprovalDecided::Deny,
            },
        });
    });
    live.client
        .reply_permission(&id, to_open_code_permission_reply(decision))
        .await
}

/// `waitQuestion`, from the reply on.
async fn wait_question(
    live: Arc<Live>,
    ui_id: i64,
    id: String,
    questions: Vec<UserQuestion>,
    reply: oneshot::Receiver<UserQuestionReply>,
) -> Result<()> {
    let Ok(reply) = reply.await else {
        return Ok(());
    };
    let decision = match &reply {
        UserQuestionReply::Answered { .. } => QuestionDecision::Answered,
        UserQuestionReply::Skipped => QuestionDecision::Skipped,
    };
    live.with(|s| {
        s.questions.remove(&ui_id);
        s.emit(HarnessEvent::QuestionResolved {
            request_id: ui_id,
            decision,
        });
        show_next_question(s);
    });
    match reply {
        UserQuestionReply::Answered { answers, custom } => {
            let labels: Vec<Vec<String>> = questions
                .iter()
                .map(|question| selected_answer_labels(question, &answers, custom.as_ref()))
                .collect();
            live.client.reply_question(&id, &labels).await
        }
        UserQuestionReply::Skipped => live.client.reject_question(&id).await,
    }
}

/// `showNextQuestion`: one question at a time, oldest first.
fn show_next_question(s: &mut LiveState) {
    if s.mute_updates || s.cancelled {
        return;
    }
    if s.visible_question_id
        .is_some_and(|visible| s.questions.contains_key(&visible))
    {
        return;
    }
    let next = s
        .questions
        .iter()
        .next()
        .map(|(request_id, (_, questions))| (*request_id, questions.clone()));
    s.visible_question_id = next.as_ref().map(|(request_id, _)| *request_id);
    let Some((request_id, questions)) = next else {
        return;
    };
    let title = question_prompt_title(&questions);
    s.emit(HarnessEvent::QuestionAsked {
        request_id,
        title: Some(if title.is_empty() {
            "OpenCode question".into()
        } else {
            title
        }),
        questions,
        call_id: None,
        auto_resolve_at: None,
    });
}

/// The transcript branches of `handleEvent`, for the thread's own session.
fn handle_transcript_event(s: &mut LiveState, live: &Live, event_type: &str, properties: &Record) {
    let props = Some(properties);
    match event_type {
        "message.updated" => {
            let info = record_field(props, "info");
            let id = string_field(info, "id");
            let role = string_field(info, "role");
            let hidden = string_field(info, "agent").is_some_and(is_known_hidden_agent);
            if let Some(id) = id {
                let role = match role {
                    Some("user") => Some(Role::User),
                    Some("assistant") => Some(Role::Assistant),
                    _ => None,
                };
                if let Some(role) = role {
                    let role = if hidden { Role::Hidden } else { role };
                    s.message_role_by_id.insert(id.to_string(), role);
                }
            }
            // A compaction assistant's usage describes the summarization call,
            // not the rebuilt context. Keep the previous meter value until a
            // real turn reports the post-compaction window level.
            if role == Some("assistant") && !hidden {
                emit_context(s, &live.catalog, info);
            }
        }
        "message.removed" => {
            if let Some(message_id) = string_field(props, "messageID") {
                s.message_role_by_id.remove(message_id);
            }
        }
        "message.part.delta" => {
            let part_id = string_field(props, "partID");
            let delta = stream_text_delta(field(props, "delta"));
            let (Some(part_id), false) = (part_id, delta.is_empty()) else {
                return;
            };
            let Some(existing) = s.part_by_id.get(part_id).cloned() else {
                return;
            };
            if role_for_part(s, &existing) != Some(Role::Assistant) {
                return;
            }
            let previous = s
                .emitted_text_by_part_id
                .get(part_id)
                .cloned()
                .or_else(|| existing.text.clone())
                .unwrap_or_default();
            let next = append_open_code_assistant_text_delta(&previous, &delta);
            s.emitted_text_by_part_id
                .insert(part_id.to_string(), next.next_text.clone());
            if existing.part_type == "text" || existing.part_type == "reasoning" {
                s.part_by_id.set(OpenCodePart {
                    text: Some(next.next_text.clone()),
                    ..existing.clone()
                });
            }
            if let Some(mapped) = text_delta_event(&existing, &next.delta_to_emit) {
                s.emit(mapped);
            }
        }
        "message.part.updated" => {
            let Some(part) = parse_part(field(props, "part")) else {
                return;
            };
            s.part_by_id.set(part.clone());
            if role_for_part(s, &part) == Some(Role::Assistant) {
                emit_assistant_text(s, &part);
            }
            if part.part_type == "tool" {
                emit_tool(s, &live.open_code_session_id, &part);
            }
        }
        "session.status" => {
            let status = record_field(props, "status");
            match string_field(status, "type") {
                Some("retry") => {
                    if let Some(message) = string_field(status, "message") {
                        s.emit(HarnessEvent::Status {
                            text: message.to_string(),
                        });
                    }
                }
                Some("idle") if s.active_turn => finish_active_turn(
                    s,
                    vec![
                        HarnessEvent::MessageCompleted,
                        HarnessEvent::ReasoningCompleted,
                    ],
                ),
                _ => {}
            }
        }
        "session.error" => {
            let message = session_error_message(field(props, "error"));
            s.emit(HarnessEvent::SessionError { message });
            finish_active_turn(s, Vec::new());
        }
        _ => {}
    }
}

/// The part of `isDescendantSession` that needs no lookup. `None` when an
/// ancestor's parent is unknown.
fn descendant_known(s: &LiveState, root: &str, session_id: &str) -> Option<bool> {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut current = Some(session_id);
    while let Some(id) = current.filter(|id| !visited.contains(id)) {
        if id == root {
            return Some(true);
        }
        visited.insert(id);
        current = s.session_parent_by_id.get(id)?.as_deref();
    }
    Some(false)
}

/// `isDescendantSession`.
async fn is_descendant_session(live: &Live, session_id: &str) -> Result<bool> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut current = Some(session_id.to_string());
    while let Some(id) = current.filter(|id| !visited.contains(id)) {
        if id == live.open_code_session_id {
            return Ok(true);
        }
        visited.insert(id.clone());
        if !live.with(|s| s.session_parent_by_id.contains_key(&id)) {
            // Resumed children may predate the SSE subscription. Resolve their
            // ancestry from the server instead of relying on session.created.
            let session = live.client.get_session(&id).await?;
            live.with(|s| s.session_parent_by_id.insert(id.clone(), session.parent_id));
        }
        current = live.with(|s| s.session_parent_by_id.get(&id).cloned().flatten());
    }
    Ok(false)
}

/// `openCodeAgentForTurn`.
pub fn open_code_agent_for_turn(
    intent: Option<TurnIntent>,
    settings: Option<&ModelSettings>,
) -> String {
    match intent {
        Some(TurnIntent::Plan) => return "plan".into(),
        Some(TurnIntent::Build) => return "build".into(),
        _ => {}
    }
    let configured = settings
        .and_then(|settings| settings.get("agent"))
        .map(|agent| js::trim(agent))
        .unwrap_or_default();
    if !configured.is_empty() && configured != "plan" {
        configured.to_string()
    } else {
        "build".into()
    }
}

/// `emitContext`. OpenCode reports tokens per assistant message but not the
/// window, so the window comes from the catalog entry for the model that
/// produced it.
fn emit_context(s: &mut LiveState, catalog: &SharedCatalog, info: Option<&Record>) {
    let used = context_used_from_message_info(info);
    let metrics = turn_metrics_from_message_info(info);
    let message_id = string_field(info, "id");
    if let Some(metrics) = &metrics {
        match message_id {
            Some(message_id) => {
                s.turn_metrics_by_message_id
                    .insert(message_id.to_string(), metrics.clone());
            }
            None => s.emit(HarnessEvent::TurnMetrics(metrics.clone())),
        }
    }
    if !s.turn_metrics_by_message_id.is_empty() {
        let mut aggregate = TurnMetrics {
            input_tokens: Some(0),
            output_tokens: Some(0),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            ..Default::default()
        };
        let add = |total: &mut Option<i64>, value: Option<i64>| {
            *total = Some(total.unwrap_or(0) + value.unwrap_or(0));
        };
        for current in s.turn_metrics_by_message_id.values() {
            add(&mut aggregate.input_tokens, current.input_tokens);
            add(&mut aggregate.output_tokens, current.output_tokens);
            add(&mut aggregate.cache_read_tokens, current.cache_read_tokens);
            add(
                &mut aggregate.cache_write_tokens,
                current.cache_write_tokens,
            );
        }
        let read = aggregate.cache_read_tokens.unwrap_or(0);
        let aggregate_input =
            aggregate.input_tokens.unwrap_or(0) + read + aggregate.cache_write_tokens.unwrap_or(0);
        let has_aggregate = [
            aggregate.input_tokens,
            aggregate.output_tokens,
            aggregate.cache_read_tokens,
            aggregate.cache_write_tokens,
        ]
        .into_iter()
        .any(|value| value.unwrap_or(0) > 0);
        if has_aggregate {
            aggregate.cache_hit_percent =
                (aggregate_input > 0).then(|| (read as f64 / aggregate_input as f64) * 100.0);
            s.emit(HarnessEvent::TurnMetrics(aggregate));
        }
    }
    let Some(used) = used else {
        return;
    };
    let window = match (
        string_field(info, "providerID"),
        string_field(info, "modelID"),
    ) {
        (Some(provider_id), Some(model_id)) => catalog
            .read()
            .model_context_window(&format!("opencode:{provider_id}/{model_id}")),
        _ => None,
    };
    s.emit(HarnessEvent::Context {
        used: Some(used),
        window,
    });
}

/// `emitAssistantText`.
fn emit_assistant_text(s: &mut LiveState, part: &OpenCodePart) {
    let Some(text) = part.text.as_deref() else {
        return;
    };
    let previous = s.emitted_text_by_part_id.get(&part.id).cloned();
    let next = merge_open_code_assistant_text(previous.as_deref(), text);
    s.emitted_text_by_part_id
        .insert(part.id.clone(), next.latest_text);
    if let Some(mapped) = text_delta_event(part, &next.delta_to_emit) {
        s.emit(mapped);
    }
}

/// The row title `emitTool` and `emitSubagentStep` share.
fn tool_title(part: &OpenCodePart) -> (String, String, Option<monocode_core::block::ToolPreview>) {
    let tool = part.tool.as_deref().unwrap_or("tool");
    let state = part.state.as_ref();
    let kind = tool_kind_from_name(tool);
    let preview = preview_from_tool_part(part);
    let state_title = field(state, "title")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty());
    let input = field(state, "input");
    let command = extract_shell_command(input);
    let skill = extract_skill_name(input);
    let composed = compose_tool_title(&ComposeToolTitle {
        kind: Some(&kind),
        title: Some(state_title.unwrap_or(tool)),
        command: command.as_deref(),
        skill: skill.as_deref(),
        path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
        query: preview
            .as_ref()
            .and_then(|preview| preview.query.as_deref()),
        preview_kind: preview.as_ref().map(|preview| preview.kind),
    });
    let title = if composed.is_empty() {
        state_title.unwrap_or(tool).to_string()
    } else {
        composed
    };
    (title, kind, preview)
}

fn tool_status(part: &OpenCodePart) -> &str {
    field(part.state.as_ref(), "status")
        .and_then(Value::as_str)
        .unwrap_or("pending")
}

/// `emitTool`.
fn emit_tool(s: &mut LiveState, root: &str, part: &OpenCodePart) {
    let call_id = part.call_id.clone().unwrap_or_else(|| part.id.clone());
    let tool = part.tool.as_deref().unwrap_or("tool");
    let status = tool_status(part).to_string();
    let (title, kind, preview) = tool_title(part);
    let detail = detail_from_tool_part(part);
    let input = field(part.state.as_ref(), "input").unwrap_or(&Value::Null);
    if let Some(items) = task_list_from_tool_input(tool, input) {
        s.emit(HarnessEvent::TasksUpdated {
            key: None,
            explanation: None,
            merge: None,
            authoritative: None,
            provider_session_id: None,
            items,
        });
    }
    if status == "pending" {
        s.emit(HarnessEvent::ToolStarted {
            agent_model: None,
            call_id: call_id.clone(),
            title,
            kind: Some(kind.clone()),
            status: Some("pending".into()),
            background: None,
            preview,
            paths: None,
        });
        if kind == "agent" {
            track_subagent_row(s, root, &call_id, part);
        }
        return;
    }
    let failed = status == "error";
    let detail = detail.or_else(|| {
        failed.then(|| {
            if kind == "agent" {
                "Subagent failed.".to_string()
            } else {
                "Tool failed.".to_string()
            }
        })
    });
    s.emit(HarnessEvent::ToolUpdated {
        agent_model: None,
        call_id: call_id.clone(),
        title: Some(title),
        kind: Some(kind.clone()),
        status: Some(match status.as_str() {
            "error" => "failed".into(),
            "completed" => "completed".into(),
            other => other.to_string(),
        }),
        detail,
        preview,
        paths: None,
    });
    // Bind after creating the parent block: replayed steps need an owner.
    if kind == "agent" {
        track_subagent_row(s, root, &call_id, part);
    }
}

/// `trackSubagentRow`. Task metadata names the child session. Arrival order
/// is not an identity: concurrent tasks can create their sessions in any order.
fn track_subagent_row(s: &mut LiveState, root: &str, call_id: &str, part: &OpenCodePart) {
    if let Some(named) = open_code_child_session_id(part).filter(|named| named != root) {
        bind_subagent_session(s, root, &named, call_id);
    }
}

/// `bindSubagentSession`.
fn bind_subagent_session(s: &mut LiveState, root: &str, session_id: &str, call_id: &str) {
    if s.subagent_sessions.get(session_id).map(String::as_str) == Some(call_id) {
        return;
    }
    s.subagent_sessions
        .insert(session_id.to_string(), call_id.to_string());
    if let Some(model) = s.subagent_models.get(session_id).cloned() {
        s.emit(agent_model_event(call_id, model));
    }
    let backlog = s
        .pending_subagent
        .iter()
        .position(|(id, _)| id == session_id)
        .map(|index| s.pending_subagent.remove(index).1)
        .unwrap_or_default();
    for part in backlog {
        emit_subagent_step(s, root, call_id, session_id, &part);
    }
}

fn agent_model_event(call_id: &str, model: String) -> HarnessEvent {
    HarnessEvent::ToolUpdated {
        agent_model: Some(model),
        call_id: call_id.to_string(),
        title: None,
        kind: Some("agent".into()),
        status: None,
        detail: None,
        preview: None,
        paths: None,
    }
}

/// `handleSubagentEvent`.
fn handle_subagent_event(
    s: &mut LiveState,
    root: &str,
    session_id: &str,
    event_type: &str,
    properties: &Record,
) {
    // The server broadcasts other sessions too. Only retain known descendants.
    let mut ancestor = Some(session_id.to_string());
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(id) = ancestor.clone().filter(|id| !visited.contains(id)) {
        if id == root || s.subagent_sessions.contains_key(&id) {
            break;
        }
        ancestor = s.session_parent_by_id.get(&id).cloned().flatten();
        visited.insert(id);
    }
    match &ancestor {
        Some(ancestor) if !visited.contains(ancestor) => {}
        _ => return,
    }

    let props = Some(properties);
    if event_type == "message.updated" {
        let info = record_field(props, "info");
        let id = string_field(info, "id");
        let role = string_field(info, "role");
        let hidden = string_field(info, "agent").is_some_and(is_known_hidden_agent);
        let model = string_field(info, "modelID");
        // Nested agents share the outer trail, but have their own model.
        if role == Some("assistant")
            && let Some(model) = model
            && !hidden
            && s.session_parent_by_id
                .get(session_id)
                .and_then(Option::as_deref)
                == Some(root)
        {
            s.subagent_models
                .insert(session_id.to_string(), model.to_string());
            if let Some(call_id) = s.subagent_sessions.get(session_id).cloned() {
                s.emit(agent_model_event(&call_id, model.to_string()));
            }
        }
        let role = match role {
            Some("user") => Some(Role::User),
            Some("assistant") => Some(Role::Assistant),
            _ => None,
        };
        if let (Some(id), Some(role)) = (id, role) {
            s.message_role_by_id
                .insert(id.to_string(), if hidden { Role::Hidden } else { role });
            // Message metadata may follow the first part on a resumed stream.
            for part in s.part_by_id.of_message(id) {
                mirror_subagent_part(s, root, session_id, part);
            }
        }
        return;
    }
    let mut part = if event_type == "message.part.updated" {
        parse_part(field(props, "part"))
    } else {
        None
    };
    if event_type == "message.part.delta" {
        let existing = string_field(props, "partID").and_then(|id| s.part_by_id.get(id));
        let delta = stream_text_delta(field(props, "delta"));
        if let Some(existing) = existing
            && !delta.is_empty()
            && (existing.part_type == "text" || existing.part_type == "reasoning")
        {
            part = Some(OpenCodePart {
                text: Some(format!(
                    "{}{delta}",
                    existing.text.as_deref().unwrap_or_default()
                )),
                ..existing.clone()
            });
        }
    }
    let Some(part) = part else {
        return;
    };
    s.part_by_id.set(part.clone());
    mirror_subagent_part(s, root, session_id, part);
}

/// `mirrorSubagentPart`. One thing a subagent did, mirrored onto its row.
/// Until the child's session is tied to a row the part is kept, because a
/// task's opening moves arrive before OpenCode reports the session it created
/// for them.
fn mirror_subagent_part(s: &mut LiveState, root: &str, session_id: &str, part: OpenCodePart) {
    if let Some(call_id) = s.subagent_sessions.get(session_id).cloned() {
        emit_subagent_step(s, root, &call_id, session_id, &part);
        return;
    }
    if !matches!(part.part_type.as_str(), "tool" | "text" | "reasoning") {
        return;
    }
    let known = s
        .pending_subagent
        .iter()
        .position(|(id, _)| id == session_id);
    let mut backlog = known
        .map(|index| std::mem::take(&mut s.pending_subagent[index].1))
        .unwrap_or_default();
    match backlog.iter().position(|entry| entry.id == part.id) {
        Some(index) => backlog[index] = part,
        None => backlog.push(part),
    }
    if backlog.len() > MAX_PENDING_SUBAGENT {
        backlog.remove(0);
    }
    match known {
        Some(index) => s.pending_subagent[index].1 = backlog,
        None => {
            if s.pending_subagent.len() >= MAX_PENDING_SUBAGENT_SESSIONS {
                s.pending_subagent.remove(0);
            }
            s.pending_subagent.push((session_id.to_string(), backlog));
        }
    }
}

/// `emitSubagentStep`.
fn emit_subagent_step(
    s: &mut LiveState,
    root: &str,
    call_id: &str,
    session_id: &str,
    part: &OpenCodePart,
) {
    if part
        .message_id
        .as_ref()
        .is_some_and(|id| !s.message_role_by_id.contains_key(id))
    {
        return;
    }
    if role_for_part(s, part) != Some(Role::Assistant) {
        return;
    }
    if part.part_type == "text" || part.part_type == "reasoning" {
        let text = js::trim(part.text.as_deref().unwrap_or_default());
        if text.is_empty() {
            return;
        }
        s.emit(HarnessEvent::AgentStep {
            call_id: call_id.to_string(),
            step_id: format!("{session_id}:{}", part.id),
            kind: if part.part_type == "reasoning" {
                AgentStepKind::Reasoning
            } else {
                AgentStepKind::Message
            },
            text: text.to_string(),
            tool_kind: None,
            status: None,
            detail: None,
            preview: None,
            agent_name: None,
            agent_type: None,
        });
        return;
    }
    if part.part_type != "tool" {
        return;
    }
    let status = tool_status(part);
    let (title, kind, preview) = tool_title(part);
    let failed = status == "error";
    s.emit(HarnessEvent::AgentStep {
        call_id: call_id.to_string(),
        step_id: format!(
            "{session_id}:{}",
            part.call_id.as_deref().unwrap_or(&part.id)
        ),
        kind: AgentStepKind::Tool,
        text: title,
        tool_kind: Some(kind.clone()),
        status: Some(
            if failed {
                "failed"
            } else if status == "completed" {
                "completed"
            } else {
                "in_progress"
            }
            .into(),
        ),
        // Only a failure earns detail; a preview's output is never shown here.
        detail: if failed {
            detail_from_tool_part(part)
        } else {
            None
        },
        preview,
        agent_name: None,
        agent_type: None,
    });
    if kind == "agent" {
        track_subagent_row(s, root, call_id, part);
    }
}

/// `finishActiveTurn`.
fn finish_active_turn(s: &mut LiveState, extra_events: Vec<HarnessEvent>) {
    s.turn_end_pending = false;
    s.active_turn = false;
    for event in extra_events {
        s.emit(event);
    }
    match s.turn.take() {
        Some(turn) => {
            let _ = turn.done.send(Ok(()));
        }
        // TODO(port): an idle or a cancel with no turn in flight leaves
        // `turnEndPending` set, and the next turn then ends as soon as it
        // starts, before OpenCode replies.
        None => s.turn_end_pending = true,
    }
}

/// `settlePendingTurn`.
fn settle_pending_turn(s: &mut LiveState) {
    if !s.turn_end_pending || s.turn.is_none() {
        return;
    }
    finish_active_turn(s, Vec::new());
}

/// `parsePart`.
fn parse_part(value: Option<&Value>) -> Option<OpenCodePart> {
    let record = value.and_then(Value::as_object)?;
    let rec = Some(record);
    let id = string_field(rec, "id")?;
    let part_type = string_field(rec, "type")?;
    let time = record_field(rec, "time").map(|time| PartTime {
        start: time.get("start").and_then(Value::as_f64),
        end: time.get("end").and_then(Value::as_f64),
    });
    Some(OpenCodePart {
        id: id.into(),
        part_type: part_type.into(),
        message_id: string_field(rec, "messageID").map(str::to_string),
        call_id: string_field(rec, "callID").map(str::to_string),
        tool: string_field(rec, "tool").map(str::to_string),
        text: field(rec, "text")
            .and_then(Value::as_str)
            .map(str::to_string),
        time,
        state: record_field(rec, "state").cloned(),
    })
}

/// `roleForPart`.
fn role_for_part(s: &LiveState, part: &OpenCodePart) -> Option<Role> {
    if let Some(known) = part
        .message_id
        .as_ref()
        .and_then(|id| s.message_role_by_id.get(id))
    {
        return Some(*known);
    }
    matches!(part.part_type.as_str(), "tool" | "text" | "reasoning").then_some(Role::Assistant)
}

/// `sameDirectory`.
fn same_directory(left: &str, right: &str) -> bool {
    let normalize = |value: &str| value.trim_end_matches('/').replace('\\', "/");
    normalize(left) == normalize(right)
}

/// `resolveSession`: adopt the bound session, fork it into a new directory,
/// or start a new one.
async fn resolve_session(
    client: &OpenCodeClient,
    resume: Option<&Resume>,
    runtime_mode: RuntimeMode,
    cwd: &str,
) -> Result<OpenCodeSession> {
    let permission = build_open_code_permission_rules(runtime_mode);
    let update = PermissionUpdate {
        permission: &permission,
    };
    if let Some(resume) = resume {
        let adopted = async {
            let adopted = client.get_session(&resume.session_id).await?;
            let same = adopted
                .directory
                .as_deref()
                .filter(|directory| !directory.is_empty())
                .is_none_or(|directory| same_directory(directory, cwd));
            if same {
                let _ = client.update_session(&adopted.id, &update).await;
                return Ok(adopted);
            }
            let forked = client.fork_session(&adopted.id, cwd).await?;
            let _ = client.update_session(&forked.id, &update).await;
            Ok(forked)
        }
        .await;
        match adopted {
            Ok(session) => return Ok(session),
            Err(error) if is_not_found_error(&error) => {}
            Err(error) => return Err(error),
        }
    }
    client.create_session(None, Some(&permission)).await
}

/// `latestOpenCodeUserMessageId`.
async fn latest_open_code_user_message_id(live: &Live) -> Result<String> {
    let messages = live
        .client
        .get_messages(&live.open_code_session_id)
        .await?
        .unwrap_or_default();
    let candidates: Vec<(String, Option<f64>)> = messages
        .iter()
        .filter_map(|message| {
            let info = message.info.as_ref();
            if string_field(info, "role") != Some("user") {
                return None;
            }
            let id = string_field(info, "id")?;
            let created = field(record_field(info, "time"), "created")
                .and_then(Value::as_f64)
                .filter(|created| created.is_finite());
            Some((id.to_string(), created))
        })
        .collect();
    let all_timestamped = candidates.iter().all(|(_, created)| created.is_some());
    let latest = if !candidates.is_empty() && all_timestamped {
        candidates.iter().reduce(|current, candidate| {
            if candidate.1 >= current.1 {
                candidate
            } else {
                current
            }
        })
    } else {
        candidates.last()
    };
    latest
        .map(|(id, _)| id.clone())
        .ok_or_else(|| anyhow!("OpenCode did not expose the last user message"))
}

/// `repairUnsupportedFileTurn`.
///
/// A rejected native file remains in OpenCode's durable history and can make
/// every later prompt fail while converting that history for the provider.
/// Revert the original attachment turn before resuming; OpenCode removes the
/// reverted tail when the next prompt starts.
async fn repair_unsupported_file_turn(client: &OpenCodeClient, session_id: &str) -> Result<()> {
    let Some(messages) = client.get_messages(session_id).await? else {
        return Ok(());
    };
    let by_id: HashMap<&str, &super::client::OpenCodeMessage> = messages
        .iter()
        .filter_map(|message| string_field(message.info.as_ref(), "id").map(|id| (id, message)))
        .collect();
    let created_of = |info: Option<&Record>| {
        field(record_field(info, "time"), "created").and_then(Value::as_f64)
    };
    let mut failures: Vec<(String, f64)> = messages
        .iter()
        .filter_map(|message| {
            let info = message.info.as_ref();
            if string_field(info, "role") != Some("assistant") {
                return None;
            }
            let mime = unsupported_file_media_type(field(info, "error"))?;
            let parent_id = string_field(info, "parentID")?;
            let has_rejected_file = by_id.get(parent_id).is_some_and(|parent| {
                parent.parts.iter().any(|part| {
                    let record = part.as_object();
                    string_field(record, "type") == Some("file")
                        && string_field(record, "mime")
                            .map(str::to_lowercase)
                            .as_deref()
                            == Some(mime.as_str())
                })
            });
            if !has_rejected_file {
                return None;
            }
            let created = created_of(info)?;
            Some((parent_id.to_string(), created))
        })
        .filter(|(_, created)| {
            messages.iter().all(|message| {
                let info = message.info.as_ref();
                let errored = field(info, "error").is_some_and(is_truthy);
                if string_field(info, "role") != Some("assistant") || errored {
                    return true;
                }
                created_of(info).is_none_or(|time| time <= *created)
            })
        })
        .collect();
    failures.sort_by(|left, right| left.1.total_cmp(&right.1));
    if let Some((message_id, _)) = failures.first() {
        client.revert_session(session_id, message_id).await?;
    }
    Ok(())
}

/// `unsupportedFileMediaType`.
fn unsupported_file_media_type(error: Option<&Value>) -> Option<String> {
    let message = session_error_message(error);
    if !FUNCTIONALITY_NOT_SUPPORTED.is_match(&message) {
        return None;
    }
    FILE_PART_MEDIA_TYPE
        .captures(&message)
        .and_then(|captures| captures.get(1))
        .map(|mime| mime.as_str().to_lowercase())
}

/// `waitForServerUrl`.
async fn wait_for_server_url(start: &Mutex<ServerStart>, timeout_ms: u64) -> Result<String> {
    let started = Instant::now();
    loop {
        {
            let start = start.lock();
            if !start.url.is_empty() {
                return Ok(start.url.clone());
            }
            if let Some(code) = start.exited {
                let code = code.map_or_else(|| "null".to_string(), |code| code.to_string());
                bail!("OpenCode server exited before startup completed (code: {code}).");
            }
        }
        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            bail!("Timed out waiting for OpenCode server");
        }
        sleep(Duration::from_millis(50)).await;
    }
}

impl HarnessAdapter for OpenCodeAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Opencode
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            compact_context: true,
            rewind_last_turn: true,
            respond_question: true,
            keep_question_open: false,
            restore_task_lists: false,
            refresh_catalog: true,
            generate_title: true,
            generate_commit_message: true,
            generate_pr_content: true,
            generate_branch_name: true,
            warmup_text: true,
            run_text_prompt: true,
            stop_text_prompt: true,
        }
    }

    fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        let adapter = self.clone();
        async move {
            adapter
                .send_open_code_turn(input, on_event, on_accepted)
                .await
        }
        .boxed()
    }

    fn compact_context(
        &self,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        let adapter = self.clone();
        async move { adapter.compact_open_code_context(input, on_event).await }.boxed()
    }

    fn rewind_last_turn(
        &self,
        input: RewindLastTurnInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<RewindLastTurnResult>> {
        let adapter = self.clone();
        async move { adapter.rewind_open_code_last_turn(input, on_event).await }.boxed()
    }

    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        let adapter = self.clone();
        async move { adapter.steer_open_code_turn(input).await }.boxed()
    }

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        let adapter = self.clone();
        async move { adapter.cancel_open_code_turn(&session_id).await }.boxed()
    }

    fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        self.respond_open_code_approval(session_id, request_id, decision);
    }

    fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        self.respond_open_code_question(session_id, request_id, reply);
    }

    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        let adapter = self.clone();
        async move {
            adapter.stop_open_code_session(&session_id).await;
            Ok(())
        }
        .boxed()
    }

    fn forget_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        let adapter = self.clone();
        async move {
            adapter.forget_open_code_session(&session_id).await;
            Ok(())
        }
        .boxed()
    }

    fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        _provider_account_id: Option<&str>,
    ) {
        self.bind_open_code_session(thread_id, provider_session_id, cwd);
    }

    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        let refresh = self.inner.refresher.refresh();
        async move {
            refresh.await;
            Ok(())
        }
        .boxed()
    }

    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        let text = self.inner.text.clone();
        async move { Ok(generate_open_code_session_title(&text, &input).await) }.boxed()
    }

    fn generate_commit_message(
        &self,
        cwd: String,
        signal: Option<AbortSignal>,
    ) -> BoxFuture<'_, Result<String>> {
        let text = self.inner.text.clone();
        let git = self.inner.git.clone();
        async move { generate_open_code_commit_message(&text, git.as_ref(), &cwd, signal).await }
            .boxed()
    }

    fn generate_pr_content(
        &self,
        cwd: String,
    ) -> BoxFuture<'_, Result<Option<GeneratedPrContent>>> {
        let text = self.inner.text.clone();
        let git = self.inner.git.clone();
        async move { generate_open_code_pr_content(&text, git.as_ref(), &cwd).await }.boxed()
    }

    fn generate_branch_name(
        &self,
        cwd: String,
        message: String,
    ) -> BoxFuture<'_, Result<Option<String>>> {
        let text = self.inner.text.clone();
        async move { Ok(generate_open_code_branch_name(&text, &cwd, &message).await) }.boxed()
    }

    fn warmup_text(&self, cwd: String) -> BoxFuture<'_, Result<()>> {
        let text = self.inner.text.clone();
        async move {
            text.warmup(&cwd).await;
            Ok(())
        }
        .boxed()
    }

    fn run_text_prompt(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        let text = self.inner.text.clone();
        async move { text.run(input).await }.boxed()
    }

    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        let text = self.inner.text.clone();
        async move {
            text.stop().await;
            Ok(())
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_turn_agent_from_intent_then_settings() {
        let settings =
            |agent: &str| ModelSettings::from([("agent".to_string(), agent.to_string())]);
        assert_eq!(
            open_code_agent_for_turn(Some(TurnIntent::Plan), None),
            "plan"
        );
        assert_eq!(
            open_code_agent_for_turn(Some(TurnIntent::Build), Some(&settings("review"))),
            "build"
        );
        assert_eq!(
            open_code_agent_for_turn(None, Some(&settings(" review "))),
            "review"
        );
        // A configured plan agent only applies through the plan intent.
        assert_eq!(
            open_code_agent_for_turn(None, Some(&settings("plan"))),
            "build"
        );
        assert_eq!(open_code_agent_for_turn(None, None), "build");
    }

    #[test]
    fn compares_directories_without_trailing_slashes() {
        assert!(same_directory("/repo/", "/repo"));
        assert!(same_directory("C:\\repo", "C:/repo"));
        assert!(!same_directory("/repo", "/other"));
    }

    #[test]
    fn reads_the_rejected_media_type_from_a_provider_error() {
        let error = json!({ "data": { "message": "'file part media type application/octet-stream' functionality not supported." } });
        assert_eq!(
            unsupported_file_media_type(Some(&error)).as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(
            unsupported_file_media_type(Some(&json!({ "message": "rate limited" }))),
            None
        );
    }
}
