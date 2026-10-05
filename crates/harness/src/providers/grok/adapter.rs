//! Port of src/integrations/harness/providers/grok/grok.ts and grokAdapter.ts:
//! the live Grok Build adapter. It spawns `grok agent stdio` and talks ACP.
//! Grok accepts ACP image blocks despite advertising `image: false`.
//!
//! The TypeScript kept `liveByThread`, `resumeByThread`, and
//! `cancelledThreads` in module globals; here they live on the adapter.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::block::{ApprovalDecided, TurnIntent};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, HarnessEvent, HarnessSessionInput, QuestionDecision,
    SendTurnInput, SteerTurnInput,
};
use monocode_core::js;
use monocode_core::user_question::{UserQuestionReply, question_prompt_title};

use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::acp_subagents::AcpSubagents;
use crate::core::child::{ChildHandlers, Children};
use crate::core::register::HarnessContext;
use crate::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, GeneratedPrContent, HarnessAdapter,
    TextPromptInput, TitleInput,
};
use crate::core::session_title::GeneratedSessionTitle;
use crate::core::task::{AbortSignal, BoxFuture, SharedSpawner};

use super::catalog::GrokCatalog;
use super::git::SharedGitSource;
use super::protocol::{
    AUTH_HELP, GrokSpawnInput, as_record, ask_question_response, ask_questions_from_acp,
    context_window_from_setup, current_model_id, events_from_acp_update, grok_auth_error,
    grok_auth_method_id, grok_effort, grok_prompt_blocks, grok_session_new_params, grok_spawn_args,
    is_grok_auth_detail, permission_option_id, permission_request_from_acp, pick_auto_option,
    plan_from_exit_plan, session_id_from_result,
};
use super::shared::{
    Wiring, ignore_unsupported_control, initialize_params, respond_method_not_found,
    selected_outcome, spawn_method_not_found,
};
use super::text::GrokText;

const INIT_TIMEOUT_MS: i64 = 12_000;
const AUTH_TIMEOUT_MS: i64 = 15_000;
const SESSION_TIMEOUT_MS: i64 = 45_000;
const CONTROL_TIMEOUT_MS: i64 = 15_000;
const PROMPT_TIMEOUT_MS: i64 = 30 * 60_000;

/// The fields of `Live` a turn changes.
struct LiveState {
    model_id: String,
    mute_updates: bool,
    cancelled: bool,
    runtime_mode: RuntimeMode,
    on_event: EventSink,
}

/// `Live`: one running `grok agent stdio` child and its ACP session.
struct Live {
    subagents: Mutex<AcpSubagents>,
    acp: AcpClient,
    acp_session_id: String,
    cwd: String,
    context_window: Option<i64>,
    full_access: bool,
    planning: bool,
    state: Mutex<LiveState>,
    approvals: Mutex<HashMap<i64, oneshot::Sender<ApprovalDecision>>>,
    questions: Mutex<HashMap<i64, oneshot::Sender<UserQuestionReply>>>,
    /// `live.turns`: turns on one child run one at a time.
    turns: smol::lock::Mutex<()>,
}

impl Live {
    fn emit(&self, event: HarnessEvent) {
        let sink = self.state.lock().on_event.clone();
        sink(event);
    }

    fn cancelled(&self) -> bool {
        self.state.lock().cancelled
    }

    /// Resolve every parked approval as deny and every question as skipped.
    fn release_waiters(&self) {
        for (_, waiter) in self.approvals.lock().drain() {
            let _ = waiter.send(ApprovalDecision::Deny);
        }
        for (_, waiter) in self.questions.lock().drain() {
            let _ = waiter.send(UserQuestionReply::Skipped);
        }
    }
}

/// `Resume`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Resume {
    acp_session_id: String,
    cwd: String,
}

#[derive(Default)]
struct Threads {
    live_by_thread: HashMap<String, Arc<Live>>,
    resume_by_thread: HashMap<String, Resume>,
    cancelled_threads: HashSet<String>,
}

struct Inner {
    children: Children,
    spawner: SharedSpawner,
    catalog: crate::core::catalog::SharedCatalog,
    git: Option<SharedGitSource>,
    threads: Arc<Mutex<Threads>>,
    text: GrokText,
    models: GrokCatalog,
}

/// App services the Grok adapter needs beyond [`HarnessContext`].
#[derive(Clone, Default)]
pub struct GrokHost {
    /// Staged and branch diffs for commit messages and pull request text.
    /// Without it, those requests fail with "Git context is not available".
    pub git: Option<SharedGitSource>,
}

/// `grokAdapter`.
#[derive(Clone)]
pub struct GrokAdapter {
    inner: Arc<Inner>,
}

impl GrokAdapter {
    pub fn new(ctx: &HarnessContext, host: GrokHost) -> Self {
        Self {
            inner: Arc::new(Inner {
                children: ctx.children.clone(),
                spawner: ctx.spawner.clone(),
                catalog: ctx.catalog.clone(),
                git: host.git,
                threads: Arc::default(),
                text: GrokText::new(ctx.children.clone(), ctx.spawner.clone()),
                models: GrokCatalog::new(
                    ctx.children.clone(),
                    ctx.spawner.clone(),
                    ctx.catalog.clone(),
                ),
            }),
        }
    }

    /// The text runner behind titles, commit messages, and side questions.
    pub fn text(&self) -> &GrokText {
        &self.inner.text
    }

    /// The model catalog probe.
    pub fn catalog(&self) -> &GrokCatalog {
        &self.inner.models
    }
}

/// `ensureGrokRegistered`, with no host services. See [`register_with`].
pub fn register(ctx: &HarnessContext) {
    register_with(ctx, GrokHost::default());
}

/// `ensureGrokRegistered`. A second call keeps the live adapter.
pub fn register_with(ctx: &HarnessContext, host: GrokHost) {
    if ctx.registry.is_registered(HarnessId::Grok) {
        return;
    }
    ctx.registry
        .register_harness(Arc::new(GrokAdapter::new(ctx, host)));
}

impl Inner {
    fn native_model_id(&self, model: &str) -> String {
        self.catalog.read().native_model_id_for(model)
    }

    /// `sendGrokTurn`.
    async fn send_turn(&self, input: SendTurnInput, on_event: EventSink) -> Result<()> {
        let session_id = input.session.session_id.clone();
        let live = match self.ensure_live(&input.session, on_event.clone()).await {
            Ok(live) => live,
            Err(error) => {
                self.threads.lock().cancelled_threads.remove(&session_id);
                return Err(error);
            }
        };
        if self.threads.lock().cancelled_threads.remove(&session_id) {
            return Ok(());
        }
        {
            let mut state = live.state.lock();
            state.on_event = on_event;
            state.runtime_mode = input.session.runtime_mode;
        }
        let result = {
            let _turn = live.turns.lock().await;
            {
                let mut state = live.state.lock();
                state.cancelled = false;
                state.mute_updates = false;
            }
            let run = async {
                self.apply_model_selection(&live, &input.session).await?;
                if live.cancelled() {
                    return Ok(());
                }
                self.prompt(&live, &input).await
            };
            match run.await {
                Err(_) if live.cancelled() => Ok(()),
                other => other,
            }
        };
        if let Err(error) = result {
            let current = self.threads.lock().live_by_thread.get(&session_id).cloned();
            if current.is_some_and(|current| Arc::ptr_eq(&current, &live)) {
                self.stop_session(&session_id).await;
            }
            return Err(error);
        }
        Ok(())
    }

    /// `compactGrokContext`.
    async fn compact_context(&self, input: CompactContextInput, on_event: EventSink) -> Result<()> {
        let session_id = input.session_id.clone();
        let existing = self.threads.lock().live_by_thread.get(&session_id).cloned();
        let live = match existing {
            Some(live) if live.cwd == input.cwd => live,
            _ => self.ensure_live(&input, on_event.clone()).await?,
        };
        if self.threads.lock().cancelled_threads.remove(&session_id) {
            return Ok(());
        }
        live.state.lock().on_event = on_event;
        let _turn = live.turns.lock().await;
        {
            let mut state = live.state.lock();
            state.cancelled = false;
            state.mute_updates = false;
        }
        let result = live
            .acp
            .request_value(
                "_x.ai/compact_conversation",
                Some(json!({ "sessionId": live.acp_session_id })),
                PROMPT_TIMEOUT_MS,
            )
            .await;
        match result {
            Err(_) if live.cancelled() => Ok(()),
            Err(error) => Err(error),
            Ok(_) => Ok(()),
        }
    }

    fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        let live = self.threads.lock().live_by_thread.get(session_id).cloned();
        if let Some(waiter) = live.and_then(|live| live.approvals.lock().remove(&request_id)) {
            let _ = waiter.send(decision);
        }
    }

    fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        let live = self.threads.lock().live_by_thread.get(session_id).cloned();
        if let Some(waiter) = live.and_then(|live| live.questions.lock().remove(&request_id)) {
            let _ = waiter.send(reply);
        }
    }

    /// `cancelGrokTurn`.
    async fn cancel_turn(&self, session_id: &str) {
        let live = {
            let mut threads = self.threads.lock();
            match threads.live_by_thread.get(session_id).cloned() {
                Some(live) => live,
                None => {
                    threads.cancelled_threads.insert(session_id.to_string());
                    return;
                }
            }
        };
        {
            let mut state = live.state.lock();
            state.cancelled = true;
            state.mute_updates = true;
        }
        live.release_waiters();
        let _ = live
            .acp
            .notify(
                "session/cancel",
                Some(json!({ "sessionId": live.acp_session_id })),
            )
            .await;
        live.acp.reject_pending(Some("cancelled"));
    }

    /// `stopGrokSession`.
    async fn stop_session(&self, session_id: &str) {
        let live = {
            let mut threads = self.threads.lock();
            threads.cancelled_threads.remove(session_id);
            threads.live_by_thread.remove(session_id)
        };
        if let Some(live) = &live {
            live.state.lock().mute_updates = true;
            live.release_waiters();
            live.acp.close(None);
        }
        self.children.unwatch_child(session_id);
        let _ = self.children.kill_child(session_id).await;
    }

    /// `forgetGrokSession`.
    async fn forget_session(&self, session_id: &str) {
        self.threads.lock().resume_by_thread.remove(session_id);
        self.stop_session(session_id).await;
    }

    /// `bindGrokSession`.
    fn bind_session(&self, thread_id: &str, acp_session_id: &str, cwd: &str) {
        let session_id = js::trim(acp_session_id);
        if thread_id.is_empty() || session_id.is_empty() || js::trim(cwd).is_empty() {
            return;
        }
        self.threads.lock().resume_by_thread.insert(
            thread_id.to_string(),
            Resume {
                acp_session_id: session_id.to_string(),
                cwd: cwd.to_string(),
            },
        );
    }

    /// `ensureLive`.
    async fn ensure_live(
        &self,
        input: &HarnessSessionInput,
        on_event: EventSink,
    ) -> Result<Arc<Live>> {
        let session_id = input.session_id.clone();
        let want_planning = input.intent == Some(TurnIntent::Plan);
        let want_full_access = input.runtime_mode == RuntimeMode::FullAccess && !want_planning;
        let existing = self.threads.lock().live_by_thread.get(&session_id).cloned();
        if let Some(existing) = &existing
            && existing.cwd == input.cwd
            && existing.full_access == want_full_access
            && existing.planning == want_planning
        {
            let mut state = existing.state.lock();
            state.on_event = on_event;
            state.runtime_mode = input.runtime_mode;
            return Ok(existing.clone());
        }
        if existing.is_some() {
            self.stop_session(&session_id).await;
        }

        let resume = {
            let mut threads = self.threads.lock();
            let resume = threads.resume_by_thread.get(&session_id).cloned();
            if resume
                .as_ref()
                .is_some_and(|resume| resume.cwd != input.cwd)
            {
                threads.resume_by_thread.remove(&session_id);
            }
            resume.filter(|resume| resume.cwd == input.cwd)
        };

        let path = self.children.resolve_grok_binary().await?.path;
        let wiring: Arc<Wiring<Live>> = Wiring::new();
        let handlers = {
            let on_notification = {
                let wiring = wiring.clone();
                move |method: &str, params: Value| {
                    if wiring.muted() {
                        return;
                    }
                    let Some(live) = wiring.live() else {
                        return;
                    };
                    if live.state.lock().mute_updates {
                        return;
                    }
                    handle_notification(&live, method, params);
                }
            };
            let on_request = {
                let wiring = wiring.clone();
                let spawner = self.spawner.clone();
                move |id: i64, method: &str, params: Value| match wiring.live() {
                    None => {
                        if let Some(acp) = wiring.acp() {
                            spawn_method_not_found(&spawner, acp, id, method);
                        }
                    }
                    Some(live) => {
                        let method = method.to_string();
                        spawner.spawn(Box::pin(async move {
                            handle_request(&live, id, &method, params).await;
                        }));
                    }
                }
            };
            AcpHandlers::default()
                .on_notification(on_notification)
                .on_request(on_request)
        };
        let acp = AcpClient::new(&session_id, Arc::new(self.children.clone()), handlers);
        wiring.set_acp(&acp);

        let emit = {
            let wiring = wiring.clone();
            let fallback = on_event.clone();
            move |event: HarnessEvent| match wiring.live() {
                Some(live) => live.emit(event),
                None => fallback(event),
            }
        };
        let emit = Arc::new(emit);
        self.children.watch_child_with(
            &session_id,
            ChildHandlers {
                on_line: Box::new({
                    let acp = acp.clone();
                    move |line| acp.push_line(&line)
                }),
                on_exit: Box::new({
                    let acp = acp.clone();
                    let threads = self.threads.clone();
                    let session_id = session_id.clone();
                    let emit = emit.clone();
                    let wiring = wiring.clone();
                    move |code| {
                        // Hold the record so the exit still reaches its listener.
                        let live = wiring.live();
                        acp.close(Some("Grok Build exited"));
                        threads.lock().live_by_thread.remove(&session_id);
                        match &live {
                            Some(live) => live.emit(HarnessEvent::SessionEnded {
                                code: code.map(i64::from),
                            }),
                            None => emit(HarnessEvent::SessionEnded {
                                code: code.map(i64::from),
                            }),
                        }
                    }
                }),
                on_stderr: Some(Box::new({
                    let emit = emit.clone();
                    move |line| {
                        log::debug!("[monocode] grok stderr {line}");
                        if is_unauthenticated_line(&line) {
                            emit(HarnessEvent::SessionError {
                                message: format!("{}\n\n{AUTH_HELP}", js::trim(&line)),
                            });
                        }
                    }
                })),
            },
        );

        self.children
            .spawn_child(
                &session_id,
                &path,
                grok_spawn_args(GrokSpawnInput {
                    model: &input.model,
                    effort: grok_effort(input.model_settings.as_ref()).as_deref(),
                    full_access: want_full_access,
                    plan: want_planning,
                }),
                &input.cwd,
                None,
                Some(HarnessId::Grok),
            )
            .await?;

        let started = self
            .start_session(
                input,
                &acp,
                &wiring,
                resume.as_ref(),
                want_full_access,
                want_planning,
                on_event,
            )
            .await;
        match started {
            Ok(live) => Ok(live),
            Err(error) => {
                acp.close(Some(&error.to_string()));
                wiring.clear();
                self.stop_session(&session_id).await;
                Err(error)
            }
        }
    }

    /// The `try` block of `ensureLive`: initialize, authenticate, and load,
    /// resume, or create the ACP session.
    #[allow(clippy::too_many_arguments)]
    async fn start_session(
        &self,
        input: &HarnessSessionInput,
        acp: &AcpClient,
        wiring: &Arc<Wiring<Live>>,
        resume: Option<&Resume>,
        full_access: bool,
        planning: bool,
        on_event: EventSink,
    ) -> Result<Arc<Live>> {
        let init = acp
            .request_value(
                "initialize",
                Some(initialize_params("monocode")),
                INIT_TIMEOUT_MS,
            )
            .await
            .map_err(|error| grok_auth_error(&error.to_string()))?;

        if let Some(method_id) = grok_auth_method_id(&init)
            && let Err(error) = acp
                .request_value(
                    "authenticate",
                    Some(json!({ "methodId": method_id, "_meta": { "headless": true } })),
                    AUTH_TIMEOUT_MS,
                )
                .await
        {
            log::debug!("[monocode] grok authenticate {error:#}");
        }

        let mut setup = Value::Null;
        let mut acp_session_id: Option<String> = None;
        let mut did_load = false;
        if let Some(resume) = resume {
            match acp
                .request_value(
                    "session/resume",
                    Some(json!({ "sessionId": resume.acp_session_id })),
                    SESSION_TIMEOUT_MS,
                )
                .await
            {
                Ok(result) => {
                    acp_session_id = Some(
                        session_id_from_result(&result)
                            .unwrap_or_else(|| resume.acp_session_id.clone()),
                    );
                    setup = result;
                    did_load = true;
                }
                Err(_) => {
                    wiring.set_muted(true);
                    let loaded = acp
                        .request_value(
                            "session/load",
                            Some(json!({
                                "sessionId": resume.acp_session_id,
                                "cwd": input.cwd,
                                "mcpServers": [],
                            })),
                            SESSION_TIMEOUT_MS,
                        )
                        .await;
                    if let Ok(result) = loaded {
                        acp_session_id = Some(
                            session_id_from_result(&result)
                                .unwrap_or_else(|| resume.acp_session_id.clone()),
                        );
                        setup = result;
                        did_load = true;
                    }
                    wiring.set_muted(false);
                }
            }
        }

        if acp_session_id.is_none() {
            setup = acp
                .request_value(
                    "session/new",
                    Some(grok_session_new_params(&input.cwd, input.runtime_mode)),
                    SESSION_TIMEOUT_MS,
                )
                .await
                .map_err(|error| grok_auth_error(&error.to_string()))?;
            acp_session_id = session_id_from_result(&setup);
        }
        let Some(acp_session_id) = acp_session_id else {
            return Err(anyhow!("Grok Build did not return a session id"));
        };

        let live = Arc::new(Live {
            subagents: Mutex::new(AcpSubagents::new()),
            acp: acp.clone(),
            acp_session_id: acp_session_id.clone(),
            cwd: input.cwd.clone(),
            context_window: context_window_from_setup(&setup)
                .or_else(|| context_window_from_setup(&init)),
            full_access,
            planning,
            state: Mutex::new(LiveState {
                model_id: current_model_id(&setup)
                    .unwrap_or_else(|| self.native_model_id(&input.model)),
                mute_updates: did_load,
                cancelled: false,
                runtime_mode: input.runtime_mode,
                on_event,
            }),
            approvals: Mutex::default(),
            questions: Mutex::default(),
            turns: smol::lock::Mutex::new(()),
        });
        wiring.set_live(&live);
        {
            let mut threads = self.threads.lock();
            threads
                .live_by_thread
                .insert(input.session_id.clone(), live.clone());
            threads.resume_by_thread.insert(
                input.session_id.clone(),
                Resume {
                    acp_session_id: acp_session_id.clone(),
                    cwd: input.cwd.clone(),
                },
            );
        }
        live.emit(HarnessEvent::SessionProviderBound {
            provider_session_id: acp_session_id,
        });
        live.emit(HarnessEvent::SessionStarted);
        Ok(live)
    }

    /// `applyModelSelection`.
    async fn apply_model_selection(&self, live: &Live, input: &HarnessSessionInput) -> Result<()> {
        let base = self.native_model_id(&input.model);
        let current = live.state.lock().model_id.clone();
        if !base.is_empty() && base != current {
            match live
                .acp
                .request_value(
                    "session/set_model",
                    Some(json!({ "sessionId": live.acp_session_id, "modelId": base })),
                    CONTROL_TIMEOUT_MS,
                )
                .await
            {
                Ok(_) => live.state.lock().model_id = base,
                Err(error) => ignore_unsupported_control("grok", "set_model", error)?,
            }
        }

        let Some(effort) = grok_effort(input.model_settings.as_ref()) else {
            return Ok(());
        };
        if let Err(error) = live
            .acp
            .request_value(
                "session/set_mode",
                Some(json!({ "sessionId": live.acp_session_id, "modeId": effort })),
                CONTROL_TIMEOUT_MS,
            )
            .await
        {
            ignore_unsupported_control("grok", "set_mode", error)?;
        }
        Ok(())
    }

    /// `prompt`.
    async fn prompt(&self, live: &Live, input: &SendTurnInput) -> Result<()> {
        let run = async {
            let attachments = input.attachments.as_deref().unwrap_or(&[]);
            let blocks =
                grok_prompt_blocks(&input.text, attachments).map_err(|error| anyhow!(error))?;
            if blocks.is_empty() {
                return Ok::<(), anyhow::Error>(());
            }
            live.acp
                .request_value(
                    "session/prompt",
                    Some(json!({ "sessionId": live.acp_session_id, "prompt": blocks })),
                    PROMPT_TIMEOUT_MS,
                )
                .await?;
            if live.cancelled() {
                return Ok(());
            }
            live.emit(HarnessEvent::MessageCompleted);
            live.emit(HarnessEvent::ReasoningCompleted);
            Ok(())
        };
        match run.await {
            Ok(()) => Ok(()),
            Err(_) if live.cancelled() => Ok(()),
            Err(error) => {
                // TODO(port): Grok puts the real cause in the error `data`
                // (for example a 402 "usage balance exhausted") behind
                // "Internal error". The TypeScript showed only the message.
                let detail = error.to_string();
                live.emit(HarnessEvent::SessionError {
                    message: if is_grok_auth_detail(&detail) {
                        format!("{}\n\n{AUTH_HELP}", js::trim(&detail))
                    } else {
                        detail
                    },
                });
                Err(error)
            }
        }
    }
}

fn is_unauthenticated_line(line: &str) -> bool {
    static UNAUTHENTICATED: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)not authenticated|Authentication required|XAI_API_KEY").unwrap()
    });
    UNAUTHENTICATED.is_match(line)
}

/// `handleNotification`.
fn handle_notification(live: &Live, method: &str, params: Value) {
    let update_params = match method {
        "session/update" => params,
        "_x.ai/session_notification" | "x.ai/session_notification" => {
            unwrap_session_notification(params)
        }
        _ => return,
    };
    let events = events_from_acp_update(&update_params);
    let routed = live.subagents.lock().route(&update_params, events);
    for event in routed {
        match event {
            HarnessEvent::Context { used, window: None } if live.context_window.is_some() => {
                live.emit(HarnessEvent::Context {
                    used,
                    window: live.context_window,
                });
            }
            other => live.emit(other),
        }
    }
}

/// `unwrapSessionNotification`.
fn unwrap_session_notification(params: Value) -> Value {
    let Some(rec) = params.as_object() else {
        return params;
    };
    let present = |key: &str| rec.get(key).is_some_and(|value| !value.is_null());
    if present("update") || present("sessionUpdate") {
        return params;
    }
    let nested = as_record(rec.get("notification")).or_else(|| as_record(rec.get("payload")));
    match nested {
        Some(nested) => Value::Object(nested.clone()),
        None => params,
    }
}

/// `handleRequest`.
async fn handle_request(live: &Live, id: i64, method: &str, params: Value) {
    match method {
        "session/request_permission" => {
            if let Err(error) = handle_permission(live, id, &params).await {
                log::debug!("[monocode] grok permission reply {error:#}");
            }
        }
        "_x.ai/ask_user_question" | "x.ai/ask_user_question" => {
            handle_ask_question(live, id, &params).await
        }
        "_x.ai/exit_plan_mode" | "x.ai/exit_plan_mode" => {
            let plan = plan_from_exit_plan(&params);
            if !plan.is_empty() {
                live.emit(HarnessEvent::Plan {
                    text: plan,
                    key: None,
                    append: None,
                    streaming: None,
                });
            }
            // End the provider-owned plan turn without approving
            // implementation. MonoCode's separate Build turn is the only
            // approval boundary.
            let _ = live
                .acp
                .respond(id, json!({ "outcome": "abandoned" }))
                .await;
        }
        _ => respond_method_not_found(&live.acp, id, method).await,
    }
}

/// `handlePermission`.
async fn handle_permission(live: &Live, id: i64, params: &Value) -> Result<()> {
    let request = permission_request_from_acp(params);
    if let Some(call_id) = &request.call_id {
        live.emit(HarnessEvent::ToolUpdated {
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

    if live.planning {
        let read_only = matches!(request.kind.as_deref(), Some("read" | "search"));
        let option_id = permission_option_id(
            if read_only {
                ApprovalDecision::Allow
            } else {
                ApprovalDecision::Deny
            },
            &request.option_ids,
        );
        return live.acp.respond(id, selected_outcome(&option_id)).await;
    }

    let runtime_mode = live.state.lock().runtime_mode;
    if let Some(auto) = pick_auto_option(runtime_mode, request.kind.as_deref(), &request.option_ids)
    {
        return live.acp.respond(id, selected_outcome(&auto)).await;
    }

    live.emit(HarnessEvent::ApprovalRequested {
        request_id: id,
        title: request.title.clone(),
        kind: request.kind.clone(),
        call_id: request.call_id.clone(),
        preview: request.preview.clone(),
    });
    let (tx, rx) = oneshot::channel();
    live.approvals.lock().insert(id, tx);
    let decision = rx.await.unwrap_or(ApprovalDecision::Deny);
    live.approvals.lock().remove(&id);
    live.emit(HarnessEvent::ApprovalResolved {
        request_id: id,
        decision: decided(decision),
    });
    live.acp
        .respond(
            id,
            selected_outcome(&permission_option_id(decision, &request.option_ids)),
        )
        .await
}

/// `handleAskQuestion`.
async fn handle_ask_question(live: &Live, id: i64, params: &Value) {
    let questions = ask_questions_from_acp(params);
    live.emit(HarnessEvent::QuestionAsked {
        request_id: id,
        title: Some(question_prompt_title(&questions)),
        questions: questions.clone(),
        call_id: None,
        auto_resolve_at: None,
    });
    let (tx, rx) = oneshot::channel();
    live.questions.lock().insert(id, tx);
    let reply = rx.await.unwrap_or(UserQuestionReply::Skipped);
    live.questions.lock().remove(&id);
    live.emit(HarnessEvent::QuestionResolved {
        request_id: id,
        decision: question_decision(&reply),
    });
    let _ = live
        .acp
        .respond(id, ask_question_response(&reply, &questions))
        .await;
}

/// An approval decision as the `approval.resolved` event records it.
pub(crate) fn decided(decision: ApprovalDecision) -> ApprovalDecided {
    match decision {
        ApprovalDecision::Allow => ApprovalDecided::Allow,
        ApprovalDecision::Deny => ApprovalDecided::Deny,
    }
}

/// `reply.kind`.
pub(crate) fn question_decision(reply: &UserQuestionReply) -> QuestionDecision {
    match reply {
        UserQuestionReply::Answered { .. } => QuestionDecision::Answered,
        UserQuestionReply::Skipped => QuestionDecision::Skipped,
    }
}

impl HarnessAdapter for GrokAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Grok
    }

    fn can_steer(&self) -> bool {
        false
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            compact_context: true,
            respond_question: true,
            refresh_catalog: true,
            generate_title: true,
            generate_commit_message: true,
            generate_pr_content: true,
            generate_branch_name: true,
            warmup_text: true,
            run_text_prompt: true,
            stop_text_prompt: true,
            ..AdapterCapabilities::default()
        }
    }

    fn send_turn(
        &self,
        input: SendTurnInput,
        on_event: EventSink,
        _on_accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.inner.send_turn(input, on_event).await })
    }

    fn compact_context(
        &self,
        input: CompactContextInput,
        on_event: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.inner.compact_context(input, on_event).await })
    }

    fn steer_turn(&self, _input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            Err(anyhow!(
                "Grok Build does not support steering an in-flight turn"
            ))
        })
    }

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.inner.cancel_turn(&session_id).await;
            Ok(())
        })
    }

    fn respond_approval(&self, session_id: &str, request_id: i64, decision: ApprovalDecision) {
        self.inner
            .respond_approval(session_id, request_id, decision);
    }

    fn respond_question(&self, session_id: &str, request_id: i64, reply: UserQuestionReply) {
        self.inner.respond_question(session_id, request_id, reply);
    }

    fn stop_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.inner.stop_session(&session_id).await;
            Ok(())
        })
    }

    fn forget_session(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.inner.forget_session(&session_id).await;
            Ok(())
        })
    }

    fn bind_session(
        &self,
        thread_id: &str,
        provider_session_id: &str,
        cwd: &str,
        _provider_account_id: Option<&str>,
    ) {
        self.inner.bind_session(thread_id, provider_session_id, cwd);
    }

    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.inner.models.refresh().await;
            Ok(())
        })
    }

    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        Box::pin(async move {
            Ok(super::title::generate_grok_session_title(&self.inner.text, &input).await)
        })
    }

    fn generate_commit_message(
        &self,
        cwd: String,
        signal: Option<AbortSignal>,
    ) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move {
            super::git::generate_grok_commit_message(
                &self.inner.text,
                self.inner.git.as_ref(),
                &cwd,
                signal,
            )
            .await
        })
    }

    fn generate_pr_content(
        &self,
        cwd: String,
    ) -> BoxFuture<'_, Result<Option<GeneratedPrContent>>> {
        Box::pin(async move {
            super::git::generate_grok_pr_content(&self.inner.text, self.inner.git.as_ref(), &cwd)
                .await
        })
    }

    fn generate_branch_name(
        &self,
        cwd: String,
        message: String,
    ) -> BoxFuture<'_, Result<Option<String>>> {
        Box::pin(async move {
            Ok(super::git::generate_grok_branch_name(&self.inner.text, &cwd, &message).await)
        })
    }

    fn warmup_text(&self, cwd: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.inner.text.warmup(&cwd).await;
            Ok(())
        })
    }

    fn run_text_prompt(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { self.inner.text.run(input).await })
    }

    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.inner.text.stop(None).await;
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
