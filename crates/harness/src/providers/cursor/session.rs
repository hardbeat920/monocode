//! Port of src/integrations/harness/providers/cursor/cursor.ts: live Cursor
//! sessions over ACP (`cursor-agent acp`).
//!
//! The TypeScript kept `liveByThread`, `resumeByThread`, and
//! `cancelledThreads` in module globals. Here they live in a
//! [`CursorSessions`] the adapter owns. Each live session keeps its mutable
//! state behind one lock; handlers compute their events under the lock and
//! emit them after releasing it, so a listener that calls back into the
//! adapter cannot deadlock. `setTimeout` timers become spawned tasks that a
//! dropped [`Timer`] cancels.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use anyhow::{Result, anyhow, bail};
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use monocode_core::attachment::prompt_blocks;
use monocode_core::block::{ApprovalDecided, ToolPreview};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{
    ApprovalDecision, HarnessEvent, QuestionDecision, SendTurnInput, SteerTurnInput,
};
use monocode_core::reducer::{
    ToolTitleInput, compose_tool_title, extract_search_query, extract_shell_command,
    extract_skill_name, extract_tool_preview, is_agent_tool, is_weak_tool_title,
};
use monocode_core::task_list::task_list_from_tool_input;
use monocode_core::user_question::{
    UserQuestionReply, question_prompt_title, questions_from_unknown,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use smol::lock::Mutex as AsyncMutex;

use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::acp_subagents::AcpSubagents;
use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildHandlers, Children};
use crate::core::json_rpc::JsonRpcClientOptions;
use crate::core::registry::EventSink;
use crate::core::task::{self, SharedSpawner};

use super::json::{Rec, as_record, first_nn, js_truthy, object, present, run_now};
use super::labels::{coerce_maybe_string, string_field, tool_detail, tool_label, tool_output};
use super::protocol::{
    ConfigValue, SessionConfigOption, TOOL_ENRICH_MAX_ATTEMPTS, ask_question_call_id,
    ask_question_title, call_id_field, client_capabilities, cursor_agent_title,
    cursor_ask_question_response, cursor_permission_request, cursor_subagent_detail,
    cursor_tool_output_is_background, decision_option, extract_model_config_id,
    is_cursor_agent_input, is_cursor_task_list_input, is_cursor_todo_update,
    needs_cursor_tool_enrichment, pick_auto_option, planning_option, read_config_options,
    resolve_setting_config_id, task_duration, tool_enrichment_delay,
};
use super::store::{StoreReader, StoredCursorSubagentRun, StoredCursorToolCall};
use super::subagents::{cursor_agent_label, cursor_subagent_events, kind_from_cursor_tool_name};

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

/// A list that keeps insertion order, like a JavaScript `Map`.
#[derive(Debug, Clone)]
struct Ordered<V> {
    entries: Vec<(String, V)>,
}

impl<V> Default for Ordered<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<V> Ordered<V> {
    fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    /// `map.set`: an existing key keeps its place.
    fn set(&mut self, key: &str, value: V) {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key.to_string(), value)),
        }
    }
    fn delete(&mut self, key: &str) -> Option<V> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(index).1)
    }
    fn clear(&mut self) {
        self.entries.clear();
    }
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }
    fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }
}

/// `PendingToolEnrichment`.
#[derive(Debug, Clone)]
struct PendingToolEnrichment {
    kind: Option<String>,
    attempts: i64,
}

/// The mutable half of `Live`.
struct LiveState {
    subagents: AcpSubagents,
    model_config_id: String,
    config_options: Vec<SessionConfigOption>,
    mute_updates: bool,
    cancelled: bool,
    runtime_mode: RuntimeMode,
    planning: bool,
    on_event: EventSink,
    approvals: HashMap<i64, oneshot::Sender<ApprovalDecision>>,
    questions: HashMap<i64, oneshot::Sender<UserQuestionReply>>,
    enriched_tools: HashSet<String>,
    pending_tool_enrichments: Ordered<PendingToolEnrichment>,
    tool_enrichment_timer: Option<Timer>,
    tool_enrichment_running: bool,
    tool_statuses: HashMap<String, String>,
    task_list_tools: HashSet<String>,
    agent_tools: Ordered<String>,
    background_agent_tools: Ordered<()>,
    subagent_runs: HashMap<String, StoredCursorSubagentRun>,
    subagent_revisions: HashMap<String, String>,
    subagent_generation: u64,
    subagent_final_polls: i64,
    subagent_timer: Option<Timer>,
    subagent_refresh: Option<Shared<BoxFuture<'static, ()>>>,
    prompt_active: bool,
}

/// `Live`: one running `cursor-agent acp` process for a thread.
struct Live {
    acp: AcpClient,
    acp_session_id: String,
    cwd: String,
    state: Mutex<LiveState>,
    /// `live.turns`: one turn at a time.
    turns: AsyncMutex<()>,
    store: StoreReader,
    spawner: SharedSpawner,
}

impl Live {
    fn sink(&self) -> EventSink {
        self.state.lock().on_event.clone()
    }

    fn emit_all(&self, events: Vec<HarnessEvent>) {
        if events.is_empty() {
            return;
        }
        let sink = self.sink();
        for event in events {
            sink(event);
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

/// Settings that were constants in the TypeScript. Tests shorten them.
#[derive(Debug, Clone, Default)]
pub struct CursorSessionOptions {
    /// The JSON-RPC client options for each session's ACP client.
    pub rpc: JsonRpcClientOptions,
}

/// The module state of cursor.ts.
pub struct CursorSessions {
    children: Children,
    spawner: SharedSpawner,
    catalog: SharedCatalog,
    store: StoreReader,
    options: CursorSessionOptions,
    threads: Mutex<Threads>,
}

impl CursorSessions {
    pub fn new(
        children: Children,
        spawner: SharedSpawner,
        catalog: SharedCatalog,
        store: StoreReader,
        options: CursorSessionOptions,
    ) -> Arc<Self> {
        Arc::new(Self {
            children,
            spawner,
            catalog,
            store,
            options,
            threads: Mutex::new(Threads::default()),
        })
    }

    fn live(&self, session_id: &str) -> Option<Arc<Live>> {
        self.threads.lock().live_by_thread.get(session_id).cloned()
    }

    /// `sendCursorTurn`.
    pub async fn send_cursor_turn(
        self: &Arc<Self>,
        input: SendTurnInput,
        on_event: EventSink,
    ) -> Result<()> {
        let session_id = input.session.session_id.clone();
        let live = match self.ensure_live(&input, on_event.clone()).await {
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
            state.planning = input.session.intent == Some(monocode_core::block::TurnIntent::Plan);
        }
        let _turn = live.turns.lock().await;
        {
            let mut state = live.state.lock();
            state.cancelled = false;
            state.mute_updates = false;
            schedule_cursor_tool_enrichment(&live, &mut state, 0);
        }
        let result = async {
            self.apply_model_selection(&live, &input).await?;
            if live.state.lock().cancelled {
                return Ok(());
            }
            prompt(&live, &input).await
        }
        .await;
        match result {
            Err(_) if live.state.lock().cancelled => Ok(()),
            other => other,
        }
    }

    /// `steerCursorTurn`.
    pub async fn steer_cursor_turn(&self, input: SteerTurnInput) -> Result<()> {
        let Some(live) = self.live(&input.session_id) else {
            bail!("No active Cursor session");
        };
        let blocks = prompt_blocks(&input.text, input.attachments.as_deref().unwrap_or(&[]))
            .map_err(|e| anyhow!(e))?;
        if blocks.is_empty() {
            return Ok(());
        }
        let params = json!({ "sessionId": live.acp_session_id, "prompt": blocks });
        if live
            .acp
            .notify("session/steer", Some(params.clone()))
            .await
            .is_err()
        {
            live.acp.notify("_session/steer", Some(params)).await?;
        }
        Ok(())
    }

    /// `respondCursorApproval`.
    pub fn respond_cursor_approval(
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

    /// `respondCursorQuestion`.
    pub fn respond_cursor_question(
        &self,
        session_id: &str,
        request_id: i64,
        reply: UserQuestionReply,
    ) {
        if let Some(live) = self.live(session_id)
            && let Some(resolve) = live.state.lock().questions.remove(&request_id)
        {
            let _ = resolve.send(reply);
        }
    }

    /// `cancelCursorTurn`: abort the in-flight prompt without tearing down
    /// the ACP session.
    pub async fn cancel_cursor_turn(&self, session_id: &str) -> Result<()> {
        let Some(live) = self.live(session_id) else {
            self.threads
                .lock()
                .cancelled_threads
                .insert(session_id.to_string());
            return Ok(());
        };
        {
            let mut state = live.state.lock();
            state.cancelled = true;
            state.mute_updates = true;
            state.prompt_active = false;
            retire_cursor_subagent_polling(&mut state);
            state.task_list_tools.clear();
            state.agent_tools.clear();
            state.background_agent_tools.clear();
            state.tool_enrichment_timer = None;
            for (_, resolve) in state.approvals.drain() {
                let _ = resolve.send(ApprovalDecision::Deny);
            }
            for (_, resolve) in state.questions.drain() {
                let _ = resolve.send(UserQuestionReply::Skipped);
            }
        }
        let _ = live
            .acp
            .notify(
                "session/cancel",
                Some(json!({ "sessionId": live.acp_session_id })),
            )
            .await;
        live.acp.reject_pending(Some("cancelled"));
        Ok(())
    }

    /// `stopCursorSession`: kill the process but keep the ACP session id for
    /// `session/load`.
    pub async fn stop_cursor_session(&self, session_id: &str) -> Result<()> {
        let live = {
            let mut threads = self.threads.lock();
            threads.cancelled_threads.remove(session_id);
            threads.live_by_thread.remove(session_id)
        };
        if let Some(live) = &live {
            let mut state = live.state.lock();
            state.mute_updates = true;
            state.prompt_active = false;
            retire_cursor_subagent_polling(&mut state);
            state.tool_enrichment_timer = None;
            state.pending_tool_enrichments.clear();
            state.agent_tools.clear();
            state.background_agent_tools.clear();
            for (_, resolve) in state.approvals.drain() {
                let _ = resolve.send(ApprovalDecision::Deny);
            }
            for (_, resolve) in state.questions.drain() {
                let _ = resolve.send(UserQuestionReply::Skipped);
            }
        }
        if let Some(live) = &live {
            live.acp.close(None);
        }
        self.children.unwatch_child(session_id);
        let _ = self.children.kill_child(session_id).await;
        Ok(())
    }

    /// `forgetCursorSession`: delete or idle detach, so drop the Cursor
    /// conversation too. `stopCursorTitleGeneration` was a no-op.
    pub async fn forget_cursor_session(&self, session_id: &str) -> Result<()> {
        self.threads.lock().resume_by_thread.remove(session_id);
        self.stop_cursor_session(session_id).await
    }

    /// `bindCursorSession`: seed ACP resume state for a restored session.
    pub fn bind_cursor_session(&self, thread_id: &str, acp_session_id: &str, cwd: &str) {
        let session_id = monocode_core::js::trim(acp_session_id);
        if thread_id.is_empty() || session_id.is_empty() || monocode_core::js::trim(cwd).is_empty()
        {
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

    async fn ensure_live(
        self: &Arc<Self>,
        input: &SendTurnInput,
        on_event: EventSink,
    ) -> Result<Arc<Live>> {
        let session_id = input.session.session_id.clone();
        let cwd = input.session.cwd.clone();
        let planning = input.session.intent == Some(monocode_core::block::TurnIntent::Plan);
        if let Some(existing) = self.live(&session_id) {
            if existing.cwd == cwd {
                let mut state = existing.state.lock();
                state.on_event = on_event;
                state.runtime_mode = input.session.runtime_mode;
                state.planning = planning;
                drop(state);
                return Ok(existing);
            }
            self.threads.lock().resume_by_thread.remove(&session_id);
            self.stop_cursor_session(&session_id).await?;
        }

        let resume = {
            let mut threads = self.threads.lock();
            let resume = threads.resume_by_thread.get(&session_id).cloned();
            if resume.as_ref().is_some_and(|resume| resume.cwd != cwd) {
                threads.resume_by_thread.remove(&session_id);
            }
            resume
        };
        let can_load = resume.as_ref().is_some_and(|resume| resume.cwd == cwd);

        let path = self.children.resolve_cursor_binary().await?.path;
        let live_ref: Arc<Mutex<Weak<Live>>> = Arc::new(Mutex::new(Weak::new()));
        let mute_gate = Arc::new(AtomicBool::new(false));
        let handlers = AcpHandlers::default()
            .on_notification({
                let live_ref = live_ref.clone();
                let mute_gate = mute_gate.clone();
                move |method, params| {
                    if mute_gate.load(Ordering::SeqCst) {
                        return;
                    }
                    let Some(live) = live_ref.lock().upgrade() else {
                        return;
                    };
                    if live.state.lock().mute_updates {
                        return;
                    }
                    handle_notification(&live, method, &params);
                }
            })
            .on_request({
                let live_ref = live_ref.clone();
                let spawner = self.spawner.clone();
                move |id, method, params| {
                    let Some(live) = live_ref.lock().upgrade() else {
                        return;
                    };
                    let method = method.to_string();
                    run_now(
                        &spawner,
                        Box::pin(async move {
                            handle_request(&live, id, &method, params).await;
                        }),
                    );
                }
            });
        let acp = AcpClient::with_options(
            &session_id,
            Arc::new(self.children.clone()),
            handlers,
            self.options.rpc.clone(),
        );

        let fallback_sink = on_event.clone();
        let weak_sessions = Arc::downgrade(self);
        self.children.watch_child_with(
            &session_id,
            ChildHandlers {
                on_line: Box::new({
                    let acp = acp.clone();
                    move |line| acp.push_line(&line)
                }),
                on_exit: Box::new({
                    let acp = acp.clone();
                    let live_ref = live_ref.clone();
                    let session_id = session_id.clone();
                    move |code| {
                        acp.close(Some("Cursor CLI exited"));
                        if let Some(sessions) = weak_sessions.upgrade() {
                            sessions.threads.lock().live_by_thread.remove(&session_id);
                        }
                        let live = live_ref.lock().upgrade();
                        let (muted, sink) = match &live {
                            Some(live) => {
                                let mut state = live.state.lock();
                                state.prompt_active = false;
                                retire_cursor_subagent_polling(&mut state);
                                (state.mute_updates, state.on_event.clone())
                            }
                            None => (false, fallback_sink.clone()),
                        };
                        if !muted {
                            sink(HarnessEvent::SessionEnded {
                                code: code.map(i64::from),
                            });
                        }
                    }
                }),
                on_stderr: None,
            },
        );

        self.children
            .spawn_child(
                &session_id,
                &path,
                vec!["acp".into()],
                &cwd,
                None,
                Some(HarnessId::Cursor),
            )
            .await?;

        let setup = async {
            acp.request_value(
                "initialize",
                Some(json!({
                    "protocolVersion": 1,
                    "clientCapabilities": client_capabilities(),
                    "clientInfo": { "name": "monocode", "version": "0.1.0" },
                })),
                0,
            )
            .await?;
            let _ = acp
                .request_value(
                    "authenticate",
                    Some(json!({ "methodId": "cursor_login" })),
                    0,
                )
                .await;

            let mut setup: Option<Value> = None;
            let mut acp_session_id: Option<String> = None;
            let mut did_load = false;

            if can_load && let Some(resume) = &resume {
                mute_gate.store(true, Ordering::SeqCst);
                let loaded = acp
                    .request_value(
                        "session/load",
                        Some(json!({ "sessionId": resume.acp_session_id, "cwd": cwd, "mcpServers": [] })),
                        0,
                    )
                    .await;
                if let Ok(loaded) = loaded {
                    setup = Some(loaded);
                    acp_session_id = Some(resume.acp_session_id.clone());
                    did_load = true;
                }
                mute_gate.store(false, Ordering::SeqCst);
            }

            if acp_session_id.is_none() {
                let created = acp
                    .request_value(
                        "session/new",
                        Some(json!({ "cwd": cwd, "mcpServers": [] })),
                        0,
                    )
                    .await?;
                acp_session_id = created
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(|id| monocode_core::js::trim(id).to_string())
                    .filter(|id| !id.is_empty());
                setup = Some(created);
            }
            let Some(acp_session_id) = acp_session_id else {
                bail!("Cursor did not return a session id");
            };
            Ok::<_, anyhow::Error>((setup, acp_session_id, did_load))
        };

        match setup.await {
            Ok((setup, acp_session_id, did_load)) => {
                let config_options = setup.as_ref().and_then(|setup| setup.get("configOptions"));
                let live = Arc::new(Live {
                    acp: acp.clone(),
                    acp_session_id: acp_session_id.clone(),
                    cwd: cwd.clone(),
                    state: Mutex::new(LiveState {
                        subagents: AcpSubagents::new(),
                        model_config_id: extract_model_config_id(config_options),
                        config_options: read_config_options(config_options),
                        mute_updates: did_load,
                        cancelled: false,
                        runtime_mode: input.session.runtime_mode,
                        planning,
                        on_event: on_event.clone(),
                        approvals: HashMap::new(),
                        questions: HashMap::new(),
                        enriched_tools: HashSet::new(),
                        pending_tool_enrichments: Ordered::default(),
                        tool_enrichment_timer: None,
                        tool_enrichment_running: false,
                        tool_statuses: HashMap::new(),
                        task_list_tools: HashSet::new(),
                        agent_tools: Ordered::default(),
                        background_agent_tools: Ordered::default(),
                        subagent_runs: HashMap::new(),
                        subagent_revisions: HashMap::new(),
                        subagent_generation: 0,
                        subagent_final_polls: 0,
                        subagent_timer: None,
                        subagent_refresh: None,
                        prompt_active: false,
                    }),
                    turns: AsyncMutex::new(()),
                    store: self.store.clone(),
                    spawner: self.spawner.clone(),
                });
                *live_ref.lock() = Arc::downgrade(&live);
                {
                    let mut threads = self.threads.lock();
                    threads
                        .live_by_thread
                        .insert(session_id.clone(), live.clone());
                    threads.resume_by_thread.insert(
                        session_id.clone(),
                        Resume {
                            acp_session_id: acp_session_id.clone(),
                            cwd: cwd.clone(),
                        },
                    );
                }
                on_event(HarnessEvent::SessionProviderBound {
                    provider_session_id: acp_session_id,
                });
                on_event(HarnessEvent::SessionStarted);
                Ok(live)
            }
            Err(error) => {
                acp.close(Some(&error.to_string()));
                let _ = self.stop_cursor_session(&session_id).await;
                Err(error)
            }
        }
    }

    /// `applyModelSelection`.
    async fn apply_model_selection(&self, live: &Arc<Live>, input: &SendTurnInput) -> Result<()> {
        let base = self
            .catalog
            .read()
            .native_model_id_for(&input.session.model);
        let model_config_id = live.state.lock().model_config_id.clone();
        if set_config_option(live, &model_config_id, &base)
            .await
            .is_err()
        {
            let _ = live
                .acp
                .request_value(
                    "session/set_model",
                    Some(json!({ "sessionId": live.acp_session_id, "modelId": base })),
                    0,
                )
                .await;
        }
        for (setting_id, value) in input.session.model_settings.iter().flatten() {
            let config_id = {
                let state = live.state.lock();
                resolve_setting_config_id(&state.config_options, setting_id)
            };
            let Some(config_id) = config_id else {
                continue;
            };
            let _ = set_config_option(live, &config_id, value).await;
        }
        Ok(())
    }
}

/// `setConfigOption`.
async fn set_config_option(live: &Arc<Live>, config_id: &str, value: &str) -> Result<()> {
    {
        let state = live.state.lock();
        let current = state
            .config_options
            .iter()
            .find(|option| option.id == config_id);
        if let Some(current) = current
            && current
                .current_value
                .as_ref()
                .map(ConfigValue::as_js_string)
                .unwrap_or_default()
                == value
        {
            return Ok(());
        }
    }
    let result = live
        .acp
        .request_value(
            "session/set_config_option",
            Some(
                json!({ "sessionId": live.acp_session_id, "configId": config_id, "value": value }),
            ),
            0,
        )
        .await?;
    if let Some(options) = result
        .get("configOptions")
        .filter(|options| js_truthy(options))
    {
        let mut state = live.state.lock();
        state.config_options = read_config_options(Some(options));
        state.model_config_id = extract_model_config_id(Some(options));
    }
    Ok(())
}

/// `prompt`.
async fn prompt(live: &Arc<Live>, input: &SendTurnInput) -> Result<()> {
    let result = async {
        let blocks = prompt_blocks(&input.text, input.attachments.as_deref().unwrap_or(&[]))
            .map_err(|e| anyhow!(e))?;
        if blocks.is_empty() {
            return Ok(false);
        }
        {
            let mut state = live.state.lock();
            state.agent_tools.clear();
            retire_cursor_subagent_polling(&mut state);
            state.subagent_runs.clear();
            state.subagent_revisions.clear();
            state.background_agent_tools.clear();
            state.task_list_tools.clear();
            state.prompt_active = true;
        }
        live.acp
            .request_value(
                "session/prompt",
                Some(json!({ "sessionId": live.acp_session_id, "prompt": blocks })),
                0,
            )
            .await?;
        Ok::<bool, anyhow::Error>(true)
    }
    .await;

    match result {
        Ok(false) => Ok(()),
        Ok(true) => {
            {
                let mut state = live.state.lock();
                state.prompt_active = false;
                if state.cancelled {
                    state.background_agent_tools.clear();
                    return Ok(());
                }
            }
            settle_cursor_background_agents(live, "completed");
            refresh_cursor_subagents(live).await;
            let sink = {
                let mut state = live.state.lock();
                if state.cancelled || state.mute_updates {
                    return Ok(());
                }
                state.subagent_final_polls = 3;
                schedule_cursor_subagents(live, &mut state, 500);
                state.on_event.clone()
            };
            sink(HarnessEvent::MessageCompleted);
            sink(HarnessEvent::ReasoningCompleted);
            wake_cursor_tool_enrichment(live);
            Ok(())
        }
        Err(error) => {
            {
                let mut state = live.state.lock();
                state.prompt_active = false;
                if state.cancelled {
                    return Ok(());
                }
            }
            settle_cursor_background_agents(live, "failed");
            live.sink()(HarnessEvent::SessionError {
                message: error.to_string(),
            });
            Err(error)
        }
    }
}

/// `handleNotification`.
fn handle_notification(live: &Arc<Live>, method: &str, params: &Value) {
    if method == "session/update" {
        handle_session_update(live, params);
        return;
    }
    if is_cursor_todo_update(method) {
        emit_cursor_todo_update(live, params);
    }
    if method == "cursor/task" {
        handle_cursor_task(live, params);
    }
}

/// `handleRequest`.
async fn handle_request(live: &Arc<Live>, id: i64, method: &str, params: Value) {
    match method {
        "session/request_permission" => handle_permission(live, id, &params).await,
        "cursor/ask_question" => handle_ask_question(live, id, &params).await,
        "cursor/create_plan" => {
            if let Some(Value::String(plan)) = params.get("plan")
                && !plan.is_empty()
            {
                live.sink()(HarnessEvent::Plan {
                    text: plan.clone(),
                    key: None,
                    append: None,
                    streaming: None,
                });
            }
            let _ = live
                .acp
                .respond(id, json!({ "outcome": { "outcome": "accepted" } }))
                .await;
        }
        _ if is_cursor_todo_update(method) => {
            emit_cursor_todo_update(live, &params);
            let _ = live.acp.respond(id, json!({})).await;
        }
        "cursor/task" => {
            handle_cursor_task(live, &params);
            let _ = live.acp.respond(id, json!({})).await;
        }
        _ => {
            let _ = live.acp.respond(id, json!({})).await;
        }
    }
}

/// `emitCursorTodoUpdate`.
fn emit_cursor_todo_update(live: &Arc<Live>, params: &Value) {
    let rec = params.as_object();
    let call_id = rec.and_then(call_id_field);
    let mut events = Vec::new();
    if let Some(call_id) = call_id {
        live.state.lock().task_list_tools.insert(call_id.clone());
        events.push(tool_updated(
            &call_id,
            None,
            Some("tasks"),
            None,
            None,
            None,
            None,
        ));
    }
    if let Some(items) = task_list_from_tool_input("updateTodos", params) {
        events.push(HarnessEvent::TasksUpdated {
            key: None,
            explanation: None,
            merge: (rec.and_then(|rec| rec.get("merge")) == Some(&Value::Bool(true)))
                .then_some(true),
            authoritative: None,
            provider_session_id: None,
            items,
        });
    }
    live.emit_all(events);
}

#[allow(clippy::too_many_arguments)]
fn tool_updated(
    call_id: &str,
    title: Option<String>,
    kind: Option<&str>,
    status: Option<String>,
    detail: Option<String>,
    preview: Option<ToolPreview>,
    agent_model: Option<String>,
) -> HarnessEvent {
    HarnessEvent::ToolUpdated {
        agent_model,
        call_id: call_id.to_string(),
        title,
        kind: kind.map(str::to_string),
        status,
        detail,
        preview,
        paths: None,
    }
}

/// `handleCursorTask`.
fn handle_cursor_task(live: &Arc<Live>, params: &Value) {
    let Some(task) = params.as_object() else {
        return;
    };
    let Some(call_id) = call_id_field(task) else {
        return;
    };
    let mut events = Vec::new();
    {
        let mut state = live.state.lock();
        if !state.prompt_active && !state.agent_tools.has(&call_id) {
            return;
        }
        let agent_id = string_field(task, "agentId").or_else(|| string_field(task, "agent_id"));
        let duration_ms = task_duration(task);
        let background = state.background_agent_tools.has(&call_id)
            || (agent_id.is_some() && duration_ms.is_none());
        let title = cursor_agent_title(
            Some(params),
            None,
            state.agent_tools.get(&call_id).map(String::as_str),
        );
        state.agent_tools.set(&call_id, title.clone());
        if background {
            state.background_agent_tools.set(&call_id, ());
        }
        let status = if background {
            "in_progress".to_string()
        } else {
            state
                .tool_statuses
                .get(&call_id)
                .cloned()
                .unwrap_or_else(|| {
                    if duration_ms.is_none() {
                        "in_progress"
                    } else {
                        "completed"
                    }
                    .into()
                })
        };
        state.tool_statuses.insert(call_id.clone(), status.clone());
        events.push(tool_updated(
            &call_id,
            Some(title.clone()),
            Some("agent"),
            Some(status),
            if background {
                cursor_subagent_detail(task)
            } else {
                None
            },
            None,
            string_field(task, "model").map(str::to_string),
        ));
        // A completion notification often carries the first useful description.
        if let Some(cached) = state.subagent_runs.get(&call_id) {
            events.extend(cursor_subagent_events(cached, Some(&title)));
        }
        schedule_cursor_subagents(live, &mut state, 0);
        if cursor_agent_label(Some(&title)).is_none() {
            queue_cursor_tool_enrichment(live, &mut state, &call_id, Some("agent"));
        }
    }
    live.emit_all(events);
}

/// `handleAskQuestion`.
async fn handle_ask_question(live: &Arc<Live>, id: i64, params: &Value) {
    let questions = questions_from_unknown(params);
    let title = ask_question_title(params).unwrap_or_else(|| question_prompt_title(&questions));
    live.sink()(HarnessEvent::QuestionAsked {
        request_id: id,
        title: Some(title),
        questions: questions.clone(),
        call_id: ask_question_call_id(params),
        auto_resolve_at: None,
    });

    let (resolve, reply) = oneshot::channel();
    live.state.lock().questions.insert(id, resolve);
    let reply = reply.await.unwrap_or(UserQuestionReply::Skipped);
    live.state.lock().questions.remove(&id);
    live.sink()(HarnessEvent::QuestionResolved {
        request_id: id,
        decision: match reply {
            UserQuestionReply::Answered { .. } => QuestionDecision::Answered,
            UserQuestionReply::Skipped => QuestionDecision::Skipped,
        },
    });
    let _ = live
        .acp
        .respond(id, cursor_ask_question_response(&reply, &questions))
        .await;
}

/// `handlePermission`.
async fn handle_permission(live: &Arc<Live>, id: i64, params: &Value) {
    let request = cursor_permission_request(params);
    let kind = request.kind.as_deref();
    let (event, sink, planning, runtime_mode) = {
        let mut state = live.state.lock();
        let event = request.call_id.as_ref().map(|call_id| {
            let event = tool_updated(
                call_id,
                Some(request.title.clone()),
                kind,
                state.tool_statuses.get(call_id).cloned(),
                None,
                request.preview.clone(),
                None,
            );
            let has_target = request.preview.as_ref().is_some_and(|preview| {
                preview.path.as_deref().is_some_and(|path| !path.is_empty())
                    || preview
                        .query
                        .as_deref()
                        .is_some_and(|query| !query.is_empty())
            });
            if has_target {
                state.enriched_tools.insert(call_id.clone());
                state.pending_tool_enrichments.delete(call_id);
            } else if needs_cursor_tool_enrichment(
                kind,
                Some(&request.title),
                request.preview.as_ref(),
            ) {
                queue_cursor_tool_enrichment(live, &mut state, call_id, kind);
            }
            event
        });
        (
            event,
            state.on_event.clone(),
            state.planning,
            state.runtime_mode,
        )
    };
    if let Some(event) = event {
        sink(event);
    }

    if planning {
        let option_id = planning_option(
            request.preview.as_ref().map(|preview| preview.kind),
            kind,
            &request.option_ids,
        );
        let _ = live
            .acp
            .respond(
                id,
                json!({ "outcome": { "outcome": "selected", "optionId": option_id } }),
            )
            .await;
        return;
    }

    if let Some(auto) = pick_auto_option(runtime_mode, kind, &request.option_ids) {
        let _ = live
            .acp
            .respond(
                id,
                json!({ "outcome": { "outcome": "selected", "optionId": auto } }),
            )
            .await;
        return;
    }

    live.sink()(HarnessEvent::ApprovalRequested {
        request_id: id,
        title: request.title.clone(),
        kind: request.kind.clone(),
        call_id: request.call_id.clone(),
        preview: request.preview.clone(),
    });

    let (resolve, decision) = oneshot::channel();
    live.state.lock().approvals.insert(id, resolve);
    let decision = decision.await.unwrap_or(ApprovalDecision::Deny);
    live.state.lock().approvals.remove(&id);
    live.sink()(HarnessEvent::ApprovalResolved {
        request_id: id,
        decision: match decision {
            ApprovalDecision::Allow => ApprovalDecided::Allow,
            ApprovalDecision::Deny => ApprovalDecided::Deny,
        },
    });
    let option_id = decision_option(decision == ApprovalDecision::Allow, &request.option_ids);
    let _ = live
        .acp
        .respond(
            id,
            json!({ "outcome": { "outcome": "selected", "optionId": option_id } }),
        )
        .await;
}

/// The text of `update.content ?? update.text` for a message or thought.
fn update_text(update: &Rec, separator: &str) -> String {
    super::labels::text_from_content(first_nn(update, &["content", "text"]), separator)
}

/// `handleSessionUpdate`.
fn handle_session_update(live: &Arc<Live>, params: &Value) {
    let rec = params.as_object();
    let update_value = object(rec.and_then(|rec| rec.get("update"))).or(rec.map(|_| params));
    let Some(update_value) = update_value else {
        return;
    };
    let update = update_value.as_object().expect("an object value");
    let kind = first_nn(update, &["sessionUpdate", "session_update", "type"])
        .map(|value| super::protocol::js_string_or_empty(Some(value)))
        .unwrap_or_default();

    let route = |live: &Arc<Live>, event: HarnessEvent| {
        let routed = live.state.lock().subagents.route(params, vec![event]);
        live.emit_all(routed);
    };

    if kind == "agent_message_chunk" || kind == "agent_message" {
        // Whole-message arrays contain distinct content blocks; chunks are exact deltas.
        let text = update_text(update, if kind == "agent_message" { "\n" } else { "" });
        if !text.is_empty() {
            route(live, HarnessEvent::MessageDelta { text });
        }
        return;
    }
    if kind == "agent_thought_chunk" || kind == "agent_thought" {
        let text = update_text(update, if kind == "agent_thought" { "\n" } else { "" });
        if !text.is_empty() {
            route(live, HarnessEvent::ReasoningDelta { text });
        }
        return;
    }
    if kind != "tool_call" && kind != "tool_call_update" && kind != "tool_call_content_chunk" {
        return;
    }

    let tool_value = object(update.get("toolCall"))
        .or_else(|| object(update.get("tool_call")))
        .unwrap_or(update_value);
    let tool = tool_value.as_object().expect("an object value");
    let call_id = first_nn(tool, &["toolCallId", "tool_call_id"])
        .or_else(|| first_nn(update, &["toolCallId", "tool_call_id"]))
        .map(|value| super::protocol::js_string_or_empty(Some(value)))
        .unwrap_or_default();
    if call_id.is_empty() {
        return;
    }
    let reported_kind =
        coerce_maybe_string(update, "kind").or_else(|| coerce_maybe_string(tool, "kind"));
    let status = coerce_maybe_string(update, "status")
        .or_else(|| coerce_maybe_string(tool, "status"))
        .map(str::to_string);
    let raw_title = tool_label(update, tool);
    let raw_input = ["rawInput", "raw_input", "input"]
        .iter()
        .find_map(|key| super::json::nn(update, key).or_else(|| super::json::nn(tool, key)));
    let detail = tool_detail(update, tool);
    let preview = extract_tool_preview(update, tool);
    let input_values = present(&[
        update.get("rawInput"),
        tool.get("rawInput"),
        update.get("raw_input"),
        tool.get("raw_input"),
        update.get("input"),
        tool.get("input"),
    ]);

    let mut events = Vec::new();
    {
        let mut state = live.state.lock();
        let agent = state.agent_tools.has(&call_id)
            || is_agent_tool(reported_kind, raw_title.as_deref())
            || is_cursor_agent_input(raw_input);
        let task_list = state.task_list_tools.contains(&call_id)
            || is_cursor_task_list_input(raw_input, raw_title.as_deref());
        if task_list {
            state.task_list_tools.insert(call_id.clone());
        }
        let tool_kind = if agent {
            Some("agent")
        } else if task_list {
            Some("tasks")
        } else {
            reported_kind
        };
        let title = if agent {
            Some(cursor_agent_title(
                raw_input,
                raw_title.as_deref(),
                state.agent_tools.get(&call_id).map(String::as_str),
            ))
        } else {
            let query = preview
                .as_ref()
                .and_then(|preview| preview.query.clone())
                .or_else(|| raw_input.and_then(extract_search_query));
            let composed = compose_tool_title(&ToolTitleInput {
                kind: tool_kind,
                title: raw_title.as_deref(),
                command: extract_shell_command(&input_values).as_deref(),
                skill: extract_skill_name(&input_values).as_deref(),
                path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
                query: query.as_deref(),
                preview_kind: preview.as_ref().map(|preview| preview.kind),
                cwd: None,
            });
            if composed.is_empty() {
                raw_title.clone()
            } else {
                Some(composed)
            }
        };

        if state.subagents.is_child(params) {
            // The shared child route decides what is worth keeping on a step.
            let event = tool_updated(
                &call_id,
                title,
                tool_kind,
                status,
                tool_output(update, tool),
                preview,
                None,
            );
            let routed = state.subagents.route(params, vec![event]);
            drop(state);
            live.emit_all(routed);
            return;
        }
        if agent && let Some(title) = &title {
            state.agent_tools.set(&call_id, title.clone());
        }
        let background = agent
            && status.as_deref() == Some("completed")
            && cursor_tool_output_is_background(update, tool);
        if background {
            state.background_agent_tools.set(&call_id, ());
        }
        let displayed_status = if background {
            Some("in_progress".to_string())
        } else {
            status
        };
        if let Some(displayed) = &displayed_status {
            state
                .tool_statuses
                .insert(call_id.clone(), displayed.clone());
        }
        let agent_model = if agent {
            as_record(raw_input)
                .and_then(|input| string_field(input, "model"))
                .map(str::to_string)
        } else {
            None
        };
        let event = tool_updated(
            &call_id,
            title.clone(),
            tool_kind,
            displayed_status,
            detail,
            preview.clone(),
            agent_model,
        );
        events.extend(state.subagents.route(params, vec![event]));
        if agent {
            if let Some(cached) = state.subagent_runs.get(&call_id) {
                events.extend(cursor_subagent_events(cached, title.as_deref()));
            }
            schedule_cursor_subagents(live, &mut state, 0);
        }
        if needs_cursor_tool_enrichment(tool_kind, title.as_deref(), preview.as_ref()) {
            queue_cursor_tool_enrichment(live, &mut state, &call_id, tool_kind);
        } else if state.pending_tool_enrichments.has(&call_id) {
            state.pending_tool_enrichments.delete(&call_id);
            state.enriched_tools.insert(call_id.clone());
        }
    }
    live.emit_all(events);
}

/// `retireCursorSubagentPolling`.
fn retire_cursor_subagent_polling(state: &mut LiveState) {
    state.subagent_timer = None;
    state.subagent_final_polls = 0;
    state.subagent_generation += 1;
}

/// `scheduleCursorSubagents`.
fn schedule_cursor_subagents(live: &Arc<Live>, state: &mut LiveState, delay: i64) {
    if state.mute_updates
        || state.subagent_timer.is_some()
        || state.subagent_refresh.is_some()
        || state.agent_tools.is_empty()
    {
        return;
    }
    let weak = Arc::downgrade(live);
    state.subagent_timer = Some(start_timer(&live.spawner, delay, move |token| {
        let Some(live) = weak.upgrade() else {
            return;
        };
        {
            let mut state = live.state.lock();
            if state.subagent_timer.as_ref().map(|timer| timer.token) != Some(token) {
                return;
            }
            state.subagent_timer = None;
        }
        drop(refresh_cursor_subagents(&live));
    }));
}

/// `refreshCursorSubagents`.
fn refresh_cursor_subagents(live: &Arc<Live>) -> Shared<BoxFuture<'static, ()>> {
    let mut state = live.state.lock();
    if let Some(existing) = &state.subagent_refresh {
        return existing.clone();
    }
    if state.mute_updates || state.agent_tools.is_empty() {
        return futures::future::ready(()).boxed().shared();
    }
    let generation = state.subagent_generation;
    let ids: Vec<String> = state.agent_tools.keys().cloned().collect();
    let ids = ids[ids.len().saturating_sub(256)..].to_vec();
    let revisions = state.subagent_revisions.clone();
    let job_live = live.clone();
    let job: BoxFuture<'static, ()> = async move {
        let live = job_live;
        let runs = live
            .store
            .read_stored_cursor_subagent_runs(&live.acp_session_id, ids, revisions)
            .await
            .unwrap_or_default();
        let events = {
            let mut state = live.state.lock();
            let mut events = Vec::new();
            if !state.mute_updates && state.subagent_generation == generation {
                for run in runs {
                    let Some(title) = state.agent_tools.get(&run.tool_call_id).cloned() else {
                        continue;
                    };
                    state
                        .subagent_revisions
                        .insert(run.agent_id.clone(), run.revision.clone());
                    events.extend(cursor_subagent_events(&run, Some(&title)));
                    state.subagent_runs.insert(run.tool_call_id.clone(), run);
                }
            }
            events
        };
        live.emit_all(events);
        let mut state = live.state.lock();
        state.subagent_refresh = None;
        if state.subagent_generation != generation {
            if state.prompt_active {
                schedule_cursor_subagents(&live, &mut state, 0);
            }
            return;
        }
        if state.prompt_active {
            schedule_cursor_subagents(&live, &mut state, 1_000);
        } else if state.subagent_final_polls > 0 {
            state.subagent_final_polls -= 1;
            schedule_cursor_subagents(&live, &mut state, 500);
        }
    }
    .boxed();
    let shared = job.shared();
    state.subagent_refresh = Some(shared.clone());
    drop(state);
    let detached = shared.clone();
    live.spawner.spawn(Box::pin(detached));
    shared
}

/// `settleCursorBackgroundAgents`.
fn settle_cursor_background_agents(live: &Arc<Live>, status: &str) {
    let events = {
        let mut state = live.state.lock();
        let ids: Vec<String> = state.background_agent_tools.keys().cloned().collect();
        let mut events = Vec::new();
        for call_id in ids {
            state
                .tool_statuses
                .insert(call_id.clone(), status.to_string());
            events.push(tool_updated(
                &call_id,
                state.agent_tools.get(&call_id).cloned(),
                Some("agent"),
                Some(status.to_string()),
                (status == "failed").then(|| "Subagent failed.".to_string()),
                None,
                None,
            ));
        }
        state.background_agent_tools.clear();
        events
    };
    live.emit_all(events);
}

/// `queueCursorToolEnrichment`.
fn queue_cursor_tool_enrichment(
    live: &Arc<Live>,
    state: &mut LiveState,
    call_id: &str,
    kind: Option<&str>,
) {
    if state.enriched_tools.contains(call_id) {
        return;
    }
    let pending = state.pending_tool_enrichments.get(call_id).cloned();
    state.pending_tool_enrichments.set(
        call_id,
        PendingToolEnrichment {
            kind: kind
                .map(str::to_string)
                .or(pending.as_ref().and_then(|pending| pending.kind.clone())),
            attempts: pending.map_or(0, |pending| pending.attempts),
        },
    );
    schedule_cursor_tool_enrichment(live, state, 0);
}

/// `scheduleCursorToolEnrichment`.
fn schedule_cursor_tool_enrichment(live: &Arc<Live>, state: &mut LiveState, delay: i64) {
    if state.mute_updates
        || state.tool_enrichment_running
        || state.tool_enrichment_timer.is_some()
        || state.pending_tool_enrichments.is_empty()
    {
        return;
    }
    let weak = Arc::downgrade(live);
    state.tool_enrichment_timer = Some(start_timer(&live.spawner, delay, move |token| {
        let Some(live) = weak.upgrade() else {
            return;
        };
        {
            let mut state = live.state.lock();
            if state
                .tool_enrichment_timer
                .as_ref()
                .map(|timer| timer.token)
                != Some(token)
            {
                return;
            }
            state.tool_enrichment_timer = None;
        }
        let spawner = live.spawner.clone();
        spawner.spawn(Box::pin(async move {
            refresh_cursor_tool_enrichments(&live).await
        }));
    }));
}

/// `wakeCursorToolEnrichment`.
fn wake_cursor_tool_enrichment(live: &Arc<Live>) {
    let mut state = live.state.lock();
    state.tool_enrichment_timer = None;
    schedule_cursor_tool_enrichment(live, &mut state, 0);
}

/// `refreshCursorToolEnrichments`.
async fn refresh_cursor_tool_enrichments(live: &Arc<Live>) {
    let call_ids: Vec<String> = {
        let mut state = live.state.lock();
        if state.mute_updates
            || state.tool_enrichment_running
            || state.pending_tool_enrichments.is_empty()
        {
            return;
        }
        state.tool_enrichment_running = true;
        state
            .pending_tool_enrichments
            .keys()
            .take(256)
            .cloned()
            .collect()
    };
    let stored = live
        .store
        .read_stored_cursor_tool_calls(&live.acp_session_id, call_ids.clone())
        .await
        .unwrap_or_default();
    if !live.state.lock().mute_updates {
        for stored in stored {
            let pending = live
                .state
                .lock()
                .pending_tool_enrichments
                .get(&stored.tool_call_id)
                .cloned();
            let Some(pending) = pending else {
                continue;
            };
            if apply_stored_cursor_tool_call(live, &stored, pending.kind.as_deref()) {
                live.state
                    .lock()
                    .pending_tool_enrichments
                    .delete(&stored.tool_call_id);
            }
        }
        let mut state = live.state.lock();
        for call_id in &call_ids {
            let Some(pending) = state.pending_tool_enrichments.get(call_id).cloned() else {
                continue;
            };
            let attempts = pending.attempts + 1;
            if attempts >= TOOL_ENRICH_MAX_ATTEMPTS {
                state.pending_tool_enrichments.delete(call_id);
            } else {
                state.pending_tool_enrichments.set(
                    call_id,
                    PendingToolEnrichment {
                        attempts,
                        ..pending
                    },
                );
            }
        }
    }
    let mut state = live.state.lock();
    state.tool_enrichment_running = false;
    let delay = tool_enrichment_delay(
        state
            .pending_tool_enrichments
            .values()
            .map(|pending| pending.attempts),
    );
    schedule_cursor_tool_enrichment(live, &mut state, delay);
}

/// `applyStoredCursorToolCall`.
fn apply_stored_cursor_tool_call(
    live: &Arc<Live>,
    stored: &StoredCursorToolCall,
    kind: Option<&str>,
) -> bool {
    let mapped_kind = kind_from_cursor_tool_name(Some(&stored.tool_name), kind);
    let mut recovered = Rec::new();
    if let Some(kind) = &mapped_kind {
        recovered.insert("kind".into(), Value::from(kind.clone()));
    }
    recovered.insert("name".into(), Value::from(stored.tool_name.clone()));
    recovered.insert("rawInput".into(), stored.args.clone());
    let preview = extract_tool_preview(&recovered, &recovered);
    let mut state = live.state.lock();
    let title = if mapped_kind.as_deref() == Some("agent") {
        cursor_agent_title(
            Some(&stored.args),
            None,
            state
                .agent_tools
                .get(&stored.tool_call_id)
                .map(String::as_str),
        )
    } else {
        let label = tool_label(&recovered, &recovered).unwrap_or_else(|| stored.tool_name.clone());
        let args = present(&[Some(&stored.args)]);
        let query = preview
            .as_ref()
            .and_then(|preview| preview.query.clone())
            .or_else(|| extract_search_query(&stored.args));
        let composed = compose_tool_title(&ToolTitleInput {
            kind: mapped_kind.as_deref(),
            title: Some(&label),
            command: extract_shell_command(&args).as_deref(),
            skill: extract_skill_name(&args).as_deref(),
            path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
            query: query.as_deref(),
            preview_kind: preview.as_ref().map(|preview| preview.kind),
            cwd: None,
        });
        if composed.is_empty() {
            stored.tool_name.clone()
        } else {
            composed
        }
    };

    let has_target = preview.as_ref().is_some_and(|preview| {
        preview.path.as_deref().is_some_and(|path| !path.is_empty())
            || preview
                .query
                .as_deref()
                .is_some_and(|query| !query.is_empty())
    });
    let weak = if mapped_kind.as_deref() == Some("agent") {
        cursor_agent_label(Some(&title)).is_none()
    } else {
        !has_target && is_weak_tool_title(&title)
    };
    if weak {
        return false;
    }

    state.enriched_tools.insert(stored.tool_call_id.clone());
    if mapped_kind.as_deref() == Some("agent") {
        state.agent_tools.set(&stored.tool_call_id, title.clone());
    }
    let mut events = vec![tool_updated(
        &stored.tool_call_id,
        Some(title.clone()),
        mapped_kind.as_deref(),
        state.tool_statuses.get(&stored.tool_call_id).cloned(),
        None,
        preview,
        None,
    )];
    if mapped_kind.as_deref() == Some("agent")
        && let Some(cached) = state.subagent_runs.get(&stored.tool_call_id)
    {
        events.extend(cursor_subagent_events(cached, Some(&title)));
    }
    let sink = state.on_event.clone();
    drop(state);
    for event in events {
        sink(event);
    }
    true
}
