//! Port of src/integrations/harness/providers/fx/fx.ts and fxAdapter.ts: the
//! live fx adapter. It spawns `fx acp` and talks ACP. fx accepts no image or
//! audio prompt blocks; the composer hides attachments.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use regex::Regex;
use serde_json::{Value, json};

use monocode_core::block::TurnIntent;
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent, SendTurnInput, SteerTurnInput};
use monocode_core::js;

use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::acp_subagents::AcpSubagents;
use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildHandlers, Children};
use crate::core::register::HarnessContext;
use crate::core::registry::{AcceptedHook, AdapterCapabilities, EventSink, HarnessAdapter};
use crate::core::task::{BoxFuture, SharedSpawner};

use super::catalog::FxCatalog;
use super::protocol::{
    SessionConfigOption, auto_permission_option, events_from_acp_update, extract_model_config_id,
    fx_mode_id, fx_prompt_blocks, permission_option_id, permission_request_from_acp,
    read_config_options, resolve_setting_config_id, session_id_from_result,
};
use super::shared::{
    Wiring, ignore_unsupported_control, initialize_params, respond_method_not_found,
    selected_outcome, spawn_method_not_found,
};

// fx answers `initialize` in well under a second when it can reach a
// credential. A long wait means it is blocked reading the macOS Keychain, not
// working, so fail fast with something actionable instead of stalling.
const INIT_TIMEOUT_MS: i64 = 12_000;
const SESSION_TIMEOUT_MS: i64 = 45_000;
const CONTROL_TIMEOUT_MS: i64 = 15_000;
const PROMPT_TIMEOUT_MS: i64 = 30 * 60_000;

const AUTH_HELP: &str = "fx has no Vercel AI Gateway credential it can read from here. Run `fx login` (or `fx setup`) in a terminal, or export AI_GATEWAY_API_KEY so it does not depend on the macOS Keychain.";

static CREDENTIAL_DETAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)needs access|AI Gateway|API key|Keychain").unwrap());
static TIMED_OUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)timed out").unwrap());
static STDERR_FAILURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)Fx needs access|AI Gateway|not start").unwrap());

/// `fxStartupError`: fx rejects `initialize` itself when it cannot read a
/// credential.
fn fx_startup_error(detail: &str) -> anyhow::Error {
    if CREDENTIAL_DETAIL.is_match(detail) {
        return anyhow!("{}\n\n{AUTH_HELP}", js::trim(detail));
    }
    if TIMED_OUT.is_match(detail) {
        return anyhow!(
            "fx did not answer initialize within {}s. {AUTH_HELP}",
            INIT_TIMEOUT_MS / 1000
        );
    }
    anyhow!("fx did not start. {detail}")
}

/// `fxSpawnArgs`.
fn fx_spawn_args(native: &str) -> Vec<String> {
    let native = js::trim(native);
    if native.is_empty() {
        vec!["acp".into()]
    } else {
        vec!["acp".into(), "--model".into(), native.to_string()]
    }
}

struct LiveState {
    model_config_id: String,
    config_options: Vec<SessionConfigOption>,
    mute_updates: bool,
    cancelled: bool,
    runtime_mode: RuntimeMode,
    planning: bool,
    on_event: EventSink,
}

/// `Live`.
struct Live {
    subagents: Mutex<AcpSubagents>,
    acp: AcpClient,
    acp_session_id: String,
    cwd: String,
    state: Mutex<LiveState>,
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
}

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
    catalog: SharedCatalog,
    threads: Arc<Mutex<Threads>>,
    models: FxCatalog,
}

/// `fxAdapter`.
#[derive(Clone)]
pub struct FxAdapter {
    inner: Arc<Inner>,
}

impl FxAdapter {
    pub fn new(ctx: &HarnessContext) -> Self {
        Self {
            inner: Arc::new(Inner {
                children: ctx.children.clone(),
                spawner: ctx.spawner.clone(),
                catalog: ctx.catalog.clone(),
                threads: Arc::default(),
                models: FxCatalog::new(ctx.children.clone(), ctx.catalog.clone()),
            }),
        }
    }

    /// The model catalog probe.
    pub fn catalog(&self) -> &FxCatalog {
        &self.inner.models
    }
}

/// `ensureFxRegistered`. A second call keeps the live adapter.
pub fn register(ctx: &HarnessContext) {
    if ctx.registry.is_registered(HarnessId::Fx) {
        return;
    }
    ctx.registry.register_harness(Arc::new(FxAdapter::new(ctx)));
}

impl Inner {
    fn native_model_id(&self, model: &str) -> String {
        self.catalog.read().native_model_id_for(model)
    }

    /// `sendFxTurn`.
    async fn send_turn(&self, input: SendTurnInput, on_event: EventSink) -> Result<()> {
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
        let planning = input.session.intent == Some(TurnIntent::Plan);
        {
            let mut state = live.state.lock();
            state.on_event = on_event;
            state.runtime_mode = input.session.runtime_mode;
            state.planning = planning;
        }
        let result = {
            let _turn = live.turns.lock().await;
            {
                let mut state = live.state.lock();
                state.cancelled = false;
                state.mute_updates = false;
            }
            let run = async {
                self.apply_model_selection(&live, &input).await?;
                if live.cancelled() {
                    return Ok(());
                }
                apply_runtime_mode(&live, input.session.runtime_mode, planning).await?;
                if live.cancelled() {
                    return Ok(());
                }
                prompt(&live, &input).await
            };
            match run.await {
                Err(_) if live.cancelled() => Ok(()),
                other => other,
            }
        };
        if let Err(error) = result {
            // A timed-out or failed turn leaves fx's process state unknowable.
            // Keep its provider session id, but recycle the child so the next
            // turn can resume instead of inheriting a wedged transport.
            let current = self.threads.lock().live_by_thread.get(&session_id).cloned();
            if current.is_some_and(|current| Arc::ptr_eq(&current, &live)) {
                self.stop_session(&session_id).await;
            }
            return Err(error);
        }
        Ok(())
    }

    /// `cancelFxTurn`.
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
        let _ = live
            .acp
            .notify(
                "session/cancel",
                Some(json!({ "sessionId": live.acp_session_id })),
            )
            .await;
        live.acp.reject_pending(Some("cancelled"));
    }

    /// `stopFxSession`.
    async fn stop_session(&self, session_id: &str) {
        let live = {
            let mut threads = self.threads.lock();
            threads.cancelled_threads.remove(session_id);
            threads.live_by_thread.remove(session_id)
        };
        if let Some(live) = &live {
            live.state.lock().mute_updates = true;
            live.acp.close(None);
        }
        self.children.unwatch_child(session_id);
        let _ = self.children.kill_child(session_id).await;
    }

    async fn forget_session(&self, session_id: &str) {
        self.threads.lock().resume_by_thread.remove(session_id);
        self.stop_session(session_id).await;
    }

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
    async fn ensure_live(&self, input: &SendTurnInput, on_event: EventSink) -> Result<Arc<Live>> {
        let session = &input.session;
        let session_id = session.session_id.clone();
        let existing = self.threads.lock().live_by_thread.get(&session_id).cloned();
        if let Some(existing) = &existing
            && existing.cwd == session.cwd
        {
            let mut state = existing.state.lock();
            state.on_event = on_event;
            state.runtime_mode = session.runtime_mode;
            return Ok(existing.clone());
        }
        if existing.is_some() {
            self.threads.lock().resume_by_thread.remove(&session_id);
            self.stop_session(&session_id).await;
        }

        let resume = {
            let mut threads = self.threads.lock();
            let resume = threads.resume_by_thread.get(&session_id).cloned();
            if resume
                .as_ref()
                .is_some_and(|resume| resume.cwd != session.cwd)
            {
                threads.resume_by_thread.remove(&session_id);
            }
            resume.filter(|resume| resume.cwd == session.cwd)
        };

        let path = self.children.resolve_fx_binary().await?.path;
        let wiring: Arc<Wiring<Live>> = Wiring::new();
        let handlers = AcpHandlers::default()
            .on_notification({
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
                    handle_notification(&live, method, &params);
                }
            })
            .on_request({
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
                            handle_request(&live, id, &method, &params).await;
                        }));
                    }
                }
            });
        let acp = AcpClient::new(&session_id, Arc::new(self.children.clone()), handlers);
        wiring.set_acp(&acp);

        // These handlers outlive the turn that created them. Routing through
        // the live record keeps them on the current turn's listener;
        // capturing the first turn's listener dropped every later exit and
        // stderr error, leaving the session on "Working…" forever.
        let emit = {
            let wiring = wiring.clone();
            let fallback = on_event.clone();
            Arc::new(move |event: HarnessEvent| match wiring.live() {
                Some(live) => live.emit(event),
                None => fallback(event),
            })
        };
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
                        let live = wiring.live();
                        acp.close(Some("fx exited"));
                        threads.lock().live_by_thread.remove(&session_id);
                        let ended = HarnessEvent::SessionEnded {
                            code: code.map(i64::from),
                        };
                        match &live {
                            Some(live) => live.emit(ended),
                            None => emit(ended),
                        }
                    }
                }),
                on_stderr: Some(Box::new({
                    let emit = emit.clone();
                    move |line| {
                        log::debug!("[monocode] fx stderr {line}");
                        if STDERR_FAILURE.is_match(&line) {
                            emit(HarnessEvent::SessionError {
                                message: js::trim(&line).to_string(),
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
                fx_spawn_args(&self.native_model_id(&session.model)),
                &session.cwd,
                None,
                Some(HarnessId::Fx),
            )
            .await?;

        match self
            .start_session(input, &acp, &wiring, resume.as_ref(), on_event)
            .await
        {
            Ok(live) => Ok(live),
            Err(error) => {
                acp.close(Some(&error.to_string()));
                wiring.clear();
                self.stop_session(&session_id).await;
                Err(error)
            }
        }
    }

    /// The `try` block of `ensureLive`.
    async fn start_session(
        &self,
        input: &SendTurnInput,
        acp: &AcpClient,
        wiring: &Arc<Wiring<Live>>,
        resume: Option<&Resume>,
        on_event: EventSink,
    ) -> Result<Arc<Live>> {
        let session = &input.session;
        acp.request_value(
            "initialize",
            Some(initialize_params("monocode")),
            INIT_TIMEOUT_MS,
        )
        .await
        .map_err(|error| fx_startup_error(&error.to_string()))?;

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
                                "cwd": session.cwd,
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
                    Some(json!({ "cwd": session.cwd, "mcpServers": [] })),
                    SESSION_TIMEOUT_MS,
                )
                .await?;
            acp_session_id = session_id_from_result(&setup);
        }
        let Some(acp_session_id) = acp_session_id else {
            return Err(anyhow!("fx did not return a session id"));
        };

        let config_options =
            read_config_options(setup.get("configOptions").unwrap_or(&Value::Null));
        let live = Arc::new(Live {
            subagents: Mutex::new(AcpSubagents::new()),
            acp: acp.clone(),
            acp_session_id: acp_session_id.clone(),
            cwd: session.cwd.clone(),
            state: Mutex::new(LiveState {
                model_config_id: extract_model_config_id(&config_options),
                config_options,
                mute_updates: did_load,
                cancelled: false,
                runtime_mode: session.runtime_mode,
                planning: session.intent == Some(TurnIntent::Plan),
                on_event,
            }),
            turns: smol::lock::Mutex::new(()),
        });
        wiring.set_live(&live);
        {
            let mut threads = self.threads.lock();
            threads
                .live_by_thread
                .insert(session.session_id.clone(), live.clone());
            threads.resume_by_thread.insert(
                session.session_id.clone(),
                Resume {
                    acp_session_id: acp_session_id.clone(),
                    cwd: session.cwd.clone(),
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
    async fn apply_model_selection(&self, live: &Live, input: &SendTurnInput) -> Result<()> {
        let base = self.native_model_id(&input.session.model);
        let model_config_id = {
            let current = live.state.lock().model_config_id.clone();
            if current == "provider" {
                "model".to_string()
            } else {
                current
            }
        };

        if let Err(error) = set_config_option(live, &model_config_id, &base).await {
            ignore_unsupported_control("fx", "set_config_option", error)?;
        }
        if model_config_id != "model"
            && let Err(error) = set_config_option(live, "model", &base).await
        {
            ignore_unsupported_control("fx", "set_config_option", error)?;
        }

        // TODO(port): the TypeScript walked `modelSettings` in insertion
        // order; `ModelSettings` is a sorted map.
        for (setting_id, value) in input.session.model_settings.iter().flatten() {
            let options = live.state.lock().config_options.clone();
            let Some(config_id) = resolve_setting_config_id(&options, setting_id) else {
                continue;
            };
            if config_id == "provider" {
                continue;
            }
            if let Err(error) = set_config_option(live, &config_id, value).await {
                ignore_unsupported_control("fx", "set_config_option", error)?;
            }
        }
        Ok(())
    }
}

/// `applyRuntimeMode`. Unsupported mode control is non-fatal because
/// `handlePermission` remains a backstop. Transport failures and timeouts are
/// rethrown so the wedged child is recycled instead of leaving the turn
/// pending forever.
async fn apply_runtime_mode(live: &Live, runtime_mode: RuntimeMode, planning: bool) -> Result<()> {
    let mode_id = if planning {
        "ask"
    } else {
        fx_mode_id(runtime_mode)
    };
    if let Err(error) = live
        .acp
        .request_value(
            "session/set_mode",
            Some(json!({ "sessionId": live.acp_session_id, "modeId": mode_id })),
            CONTROL_TIMEOUT_MS,
        )
        .await
    {
        ignore_unsupported_control("fx", "set_mode", error)?;
    }
    Ok(())
}

/// `setConfigOption`.
async fn set_config_option(live: &Live, config_id: &str, value: &str) -> Result<()> {
    let unchanged = live
        .state
        .lock()
        .config_options
        .iter()
        .find(|option| option.id == config_id)
        .is_some_and(|option| option.current_text() == value);
    if unchanged {
        return Ok(());
    }
    let result = live
        .acp
        .request_value(
            "session/set_config_option",
            Some(
                json!({ "sessionId": live.acp_session_id, "configId": config_id, "value": value }),
            ),
            CONTROL_TIMEOUT_MS,
        )
        .await?;
    if let Some(options) = result
        .get("configOptions")
        .filter(|options| truthy(options))
    {
        let options = read_config_options(options);
        let mut state = live.state.lock();
        state.model_config_id = extract_model_config_id(&options);
        state.config_options = options;
    }
    Ok(())
}

/// JavaScript truthiness.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `prompt`.
async fn prompt(live: &Live, input: &SendTurnInput) -> Result<()> {
    let run = async {
        let blocks = fx_prompt_blocks(&input.text);
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
            let detail = error.to_string();
            live.emit(HarnessEvent::SessionError {
                message: if CREDENTIAL_DETAIL.is_match(&detail) {
                    format!("{}\n\n{AUTH_HELP}", js::trim(&detail))
                } else {
                    detail
                },
            });
            Err(error)
        }
    }
}

/// `handleNotification`.
fn handle_notification(live: &Live, method: &str, params: &Value) {
    if method != "session/update" {
        return;
    }
    let events = events_from_acp_update(params);
    let routed = live.subagents.lock().route(params, events);
    for event in routed {
        live.emit(event);
    }
}

/// `handleRequest`.
async fn handle_request(live: &Live, id: i64, method: &str, params: &Value) {
    if method == "session/request_permission" {
        if let Err(error) = handle_permission(live, id, params).await {
            log::debug!("[monocode] fx permission reply {error:#}");
        }
        return;
    }
    respond_method_not_found(&live.acp, id, method).await;
}

/// `handlePermission`. fx polices its own permissions in `code` mode, so
/// anything that still reaches MonoCode is answered at once. A turn never
/// parks on an approval; that left sessions on "Working…" with an empty
/// transcript.
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
    let (planning, runtime_mode) = {
        let state = live.state.lock();
        (state.planning, state.runtime_mode)
    };
    let option_id = if planning {
        let read_only = matches!(request.kind.as_deref(), Some("read" | "search"));
        permission_option_id(
            if read_only {
                ApprovalDecision::Allow
            } else {
                ApprovalDecision::Deny
            },
            &request.option_ids,
        )
    } else {
        auto_permission_option(runtime_mode, &request.option_ids)
            .unwrap_or_else(|| permission_option_id(ApprovalDecision::Allow, &request.option_ids))
    };
    live.acp.respond(id, selected_outcome(&option_id)).await
}

impl HarnessAdapter for FxAdapter {
    fn id(&self) -> HarnessId {
        HarnessId::Fx
    }

    fn can_steer(&self) -> bool {
        false
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            refresh_catalog: true,
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

    fn steer_turn(&self, _input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Err(anyhow!("fx does not support steering an in-flight turn")) })
    }

    fn cancel_turn(&self, session_id: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.inner.cancel_turn(&session_id).await;
            Ok(())
        })
    }

    /// fx auto-approves in `code` mode, so there is never a pending approval.
    fn respond_approval(&self, _session_id: &str, _request_id: i64, _decision: ApprovalDecision) {}

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
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
