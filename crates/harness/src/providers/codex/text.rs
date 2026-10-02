//! Port of src/integrations/harness/providers/codex/codexText.ts: one-shot
//! text prompts (titles, commit messages, side questions) on a separate
//! `codex app-server`.

use std::sync::{Arc, OnceLock, Weak};

use anyhow::{Result, anyhow, bail};
use futures::channel::oneshot;
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::block::ModelSettings;
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::js;
use monocode_core::reducer::{join_stream_text, stream_text_delta};

use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildAccount, ChildEvent, Children};
use crate::core::json_rpc::{JsonRpcClient, JsonRpcClientOptions, JsonRpcHandlers, JsonRpcId};
use crate::core::registry::{EventSink, TextPromptInput, ThreadIdHook};
use crate::core::task::{AbortSignal, SharedSpawner, ms, timeout};

use super::json::{as_record, string_field};
use super::protocol::{
    ThreadStartInput, TurnStartInput, build_thread_start_params, build_turn_start_params,
    is_recoverable_thread_resume_error, map_codex_notification,
};

const TEXT_CHILD_ID: &str = "monocode-codex-text";
const INIT_TIMEOUT_MS: i64 = 60_000;
const REQUEST_TIMEOUT_MS: i64 = 45_000;
const TEXT_RUNTIME_MODE: RuntimeMode = RuntimeMode::Supervised;
const TEXT_MODEL: &str = "gpt-5.6-luna";
const TEXT_EFFORT: &str = "low";

struct TextState {
    cwd: String,
    thread_id: String,
    model: String,
    effort: String,
    service_tier: Option<String>,
    collecting: bool,
    output: String,
    closed: bool,
    /// `turnDone` and `turnFailed`.
    turn: Option<oneshot::Sender<Result<(), String>>>,
    on_event: Option<EventSink>,
}

struct LiveText {
    rpc: JsonRpcClient,
    provider_account_id: Option<String>,
    state: Mutex<TextState>,
}

impl LiveText {
    fn closed(&self) -> bool {
        self.state.lock().closed
    }
}

struct TextShared {
    children: Children,
    catalog: SharedCatalog,
    spawner: SharedSpawner,
    live: Mutex<Option<Arc<LiveText>>>,
    /// `turns`: prompts and warmups run one at a time.
    turns: futures::lock::Mutex<()>,
}

/// The shared Codex text runner. Clones share one app-server.
#[derive(Clone)]
pub struct CodexText {
    shared: Arc<TextShared>,
}

/// What `ensureLive` needs from a prompt.
#[derive(Default)]
struct EnsureInput<'a> {
    cwd: &'a str,
    provider_account_id: Option<&'a str>,
    model: Option<&'a str>,
    model_settings: Option<&'a ModelSettings>,
    thread_id: Option<&'a str>,
    on_thread_id: Option<&'a ThreadIdHook>,
}

impl CodexText {
    pub fn new(children: Children, spawner: SharedSpawner, catalog: SharedCatalog) -> Self {
        Self {
            shared: Arc::new(TextShared {
                children,
                catalog,
                spawner,
                live: Mutex::new(None),
                turns: futures::lock::Mutex::new(()),
            }),
        }
    }

    /// `pickTextModel`.
    fn pick_text_model(&self, requested: Option<&str>) -> String {
        if let Some(selected) = requested.map(js::trim).filter(|model| !model.is_empty()) {
            return selected.to_string();
        }
        let catalog = self.shared.catalog.read();
        let luna = catalog.models_for(HarnessId::Codex).iter().find(|model| {
            let haystack = format!(
                "{} {} {}",
                model.native_id.as_deref().unwrap_or(""),
                model.name,
                model.id
            );
            haystack.to_lowercase().contains("5.6-luna")
        });
        luna.and_then(|model| model.native_id.clone())
            .unwrap_or_else(|| TEXT_MODEL.into())
    }

    /// `pickTextEffort`.
    fn pick_text_effort(&self, model_id: &str, model_settings: Option<&ModelSettings>) -> String {
        let requested = model_settings.and_then(|settings| {
            [settings.get("reasoningEffort"), settings.get("effort")]
                .into_iter()
                .flatten()
                .map(|value| js::trim(value))
                .find(|value| !value.is_empty())
                .map(str::to_string)
        });
        let catalog = self.shared.catalog.read();
        let setting = catalog
            .models_for(HarnessId::Codex)
            .iter()
            .find(|model| model.native_id.as_deref() == Some(model_id))
            .and_then(|model| model.settings.as_ref())
            .and_then(|settings| settings.iter().find(|entry| entry.id == "reasoningEffort"));
        let options: Vec<&str> = setting
            .map(|setting| {
                setting
                    .options
                    .iter()
                    .map(|option| option.value.as_str())
                    .collect()
            })
            .unwrap_or_default();
        if let Some(requested) = requested
            && (options.is_empty() || options.contains(&requested.as_str()))
        {
            return requested;
        }
        if options.contains(&"low") {
            return "low".into();
        }
        if options.contains(&"none") {
            return "none".into();
        }
        if let Some(setting) = setting
            && !setting.value.is_empty()
            && options.contains(&setting.value.as_str())
        {
            return setting.value.clone();
        }
        TEXT_EFFORT.into()
    }

    /// `stopCodexTextPrompt`.
    pub async fn stop_text_prompt(&self) -> Result<()> {
        self.drop_live().await;
        Ok(())
    }

    /// `warmupCodexText`: start the shared app-server in the background so
    /// the first prompt is fast.
    pub async fn warmup_text(&self, cwd: &str) -> Result<()> {
        if cwd.is_empty() || cwd == "~" {
            return Ok(());
        }
        let _turn = self.shared.turns.lock().await;
        let _ = self
            .ensure_live(&EnsureInput {
                cwd,
                ..Default::default()
            })
            .await;
        Ok(())
    }

    /// `runCodexTextPrompt`: a Codex app-server turn on the shared text
    /// process.
    pub async fn run_text_prompt(&self, input: TextPromptInput) -> Result<String> {
        if let Some(model) = &input.model
            && js::trim(model).is_empty()
        {
            bail!("The selected Codex model is unavailable.");
        }
        let _turn = self.shared.turns.lock().await;
        self.prompt_on_live(&input).await
    }

    async fn prompt_on_live(&self, input: &TextPromptInput) -> Result<String> {
        if let Some(signal) = &input.signal {
            signal.throw_if_aborted()?;
        }
        let session = self
            .ensure_live(&EnsureInput {
                cwd: &input.cwd,
                provider_account_id: input.provider_account_id.as_deref(),
                model: input.model.as_deref(),
                model_settings: input.model_settings.as_ref(),
                thread_id: input.thread_id.as_deref(),
                on_thread_id: input.on_thread_id.as_ref(),
            })
            .await?;
        if let Some(signal) = &input.signal {
            signal.throw_if_aborted()?;
        }
        {
            let mut state = session.state.lock();
            state.output.clear();
            state.collecting = true;
            state.on_event = input.on_event.clone();
        }
        let timeout_ms = input.timeout_ms.unwrap_or(REQUEST_TIMEOUT_MS);

        let result = self.collect_turn(&session, input, timeout_ms).await;
        if result.is_err() {
            let thread_id = session.state.lock().thread_id.clone();
            let _ = session
                .rpc
                .request_value("turn/interrupt", Some(json!({ "threadId": thread_id })), 0)
                .await;
            if session.closed() {
                self.drop_live().await;
            }
        }
        {
            let mut state = session.state.lock();
            state.collecting = false;
            state.turn = None;
        }
        // TODO(port): the TypeScript drops the process after every prompt, so
        // the "warm" process only serves one prompt. Ported as is.
        self.drop_live().await;
        result
    }

    /// The `try` block of `promptOnLive`.
    async fn collect_turn(
        &self,
        session: &Arc<LiveText>,
        input: &TextPromptInput,
        timeout_ms: i64,
    ) -> Result<String> {
        let (done, turn_done) = oneshot::channel();
        let (params, thread_id) = {
            let mut state = session.state.lock();
            state.turn = Some(done);
            let params = build_turn_start_params(&TurnStartInput {
                thread_id: &state.thread_id,
                runtime_mode: TEXT_RUNTIME_MODE,
                prompt: Some(&input.prompt),
                model: Some(state.model.as_str()).filter(|model| !model.is_empty()),
                effort: Some(&state.effort),
                service_tier: state.service_tier.as_deref(),
                intent: input.intent,
                ..Default::default()
            })
            .map_err(|error| anyhow!(error))?;
            (params, state.thread_id.clone())
        };

        let turn_start =
            session
                .rpc
                .request_value("turn/start", Some(Value::Object(params)), timeout_ms);
        self.race_abort(session, &thread_id, input.signal.as_ref(), async {
            turn_start.await.map(|_| ())
        })
        .await?;

        let settled = async {
            match timeout(ms(timeout_ms), turn_done).await {
                Some(Ok(Ok(()))) | Some(Err(_)) => Ok(()),
                Some(Ok(Err(error))) => Err(anyhow!(error)),
                None => Err(anyhow!("Codex text generation timed out")),
            }
        };
        self.race_abort(session, &thread_id, input.signal.as_ref(), settled)
            .await?;
        Ok(session.state.lock().output.clone())
    }

    /// `Promise.race([work, abortPromise])`. On abort, interrupt the turn
    /// and fail with "By-the-way request cancelled".
    async fn race_abort(
        &self,
        session: &Arc<LiveText>,
        thread_id: &str,
        signal: Option<&AbortSignal>,
        work: impl std::future::Future<Output = Result<()>>,
    ) -> Result<()> {
        let Some(signal) = signal else {
            return work.await;
        };
        let aborted = async {
            signal.aborted().await;
            let rpc = session.rpc.clone();
            let thread_id = thread_id.to_string();
            self.shared.spawner.spawn(Box::pin(async move {
                let _ = rpc
                    .request_value("turn/interrupt", Some(json!({ "threadId": thread_id })), 0)
                    .await;
            }));
            Err(anyhow!("By-the-way request cancelled"))
        };
        smol::future::or(work, aborted).await
    }

    async fn ensure_live(&self, input: &EnsureInput<'_>) -> Result<Arc<LiveText>> {
        let model = self.pick_text_model(input.model);
        let effort = self.pick_text_effort(&model, input.model_settings);
        let service_tier = input
            .model_settings
            .and_then(|settings| settings.get("serviceTier"))
            .map(|value| js::trim(value).to_string())
            .filter(|value| !value.is_empty());
        let requested_thread_id = input
            .thread_id
            .map(js::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string);
        let notify_thread = |session: &LiveText| {
            if let Some(hook) = input.on_thread_id {
                hook(session.state.lock().thread_id.clone());
            }
        };

        let current = self
            .shared
            .live
            .lock()
            .clone()
            .filter(|live| !live.closed());
        if let Some(live) = current {
            let reusable = {
                let state = live.state.lock();
                state.cwd == input.cwd
                    && state.model == model
                    && state.effort == effort
                    && state.service_tier == service_tier
                    && live.provider_account_id.as_deref() == input.provider_account_id
                    && requested_thread_id
                        .as_ref()
                        .is_none_or(|requested| *requested == state.thread_id)
            };
            if reusable {
                notify_thread(&live);
                return Ok(live);
            }
            if live.provider_account_id.as_deref() != input.provider_account_id {
                self.drop_live().await;
                let started = self
                    .start_live(input, model, effort, service_tier, requested_thread_id)
                    .await?;
                notify_thread(&started);
                return Ok(started);
            }
            {
                let mut state = live.state.lock();
                state.model = model;
                state.effort = effort;
                state.service_tier = service_tier;
            }
            match open_thread(&live, input.cwd, requested_thread_id.as_deref()).await {
                Ok(()) => {
                    notify_thread(&live);
                    return Ok(live);
                }
                Err(error) => {
                    self.drop_live().await;
                    return Err(error);
                }
            }
        }
        let started = self
            .start_live(input, model, effort, service_tier, requested_thread_id)
            .await?;
        notify_thread(&started);
        Ok(started)
    }

    async fn start_live(
        &self,
        input: &EnsureInput<'_>,
        model: String,
        effort: String,
        service_tier: Option<String>,
        requested_thread_id: Option<String>,
    ) -> Result<Arc<LiveText>> {
        self.drop_live().await;
        let binary = self.shared.children.resolve_codex_binary().await?;
        let session_ref: Arc<OnceLock<Weak<LiveText>>> = Arc::new(OnceLock::new());
        let handlers = {
            let notify_ref = session_ref.clone();
            let request_ref = session_ref.clone();
            let spawner = self.shared.spawner.clone();
            JsonRpcHandlers::default()
                .on_notification(move |method, params| {
                    let session = notify_ref.get().and_then(Weak::upgrade);
                    handle_notification(session.as_deref(), method, &params);
                })
                .on_request(move |id, method, _params| {
                    if let Some(session) = request_ref.get().and_then(Weak::upgrade) {
                        let rpc = session.rpc.clone();
                        let method = method.to_string();
                        spawner.spawn(Box::pin(async move {
                            handle_server_request(&rpc, id, &method).await;
                        }));
                    }
                })
        };
        let rpc = JsonRpcClient::new(
            TEXT_CHILD_ID,
            Arc::new(self.shared.children.clone()),
            handlers,
            JsonRpcClientOptions {
                include_jsonrpc: false,
                label: "codex-text".into(),
                ..Default::default()
            },
        );
        let session = Arc::new(LiveText {
            rpc: rpc.clone(),
            provider_account_id: input.provider_account_id.map(str::to_string),
            state: Mutex::new(TextState {
                cwd: input.cwd.to_string(),
                thread_id: String::new(),
                model,
                effort,
                service_tier,
                collecting: false,
                output: String::new(),
                closed: false,
                turn: None,
                on_event: None,
            }),
        });
        let _ = session_ref.set(Arc::downgrade(&session));

        let events = self.shared.children.watch_child(TEXT_CHILD_ID);
        {
            let rpc = rpc.clone();
            let weak = Arc::downgrade(&session);
            let shared = Arc::downgrade(&self.shared);
            self.shared.spawner.spawn(Box::pin(async move {
                while let Ok(event) = events.recv().await {
                    match event {
                        ChildEvent::Stdout(line) => rpc.push_line(&line),
                        ChildEvent::Stderr(_) => {}
                        ChildEvent::Exit(_) => {
                            if let Some(session) = weak.upgrade() {
                                let turn = {
                                    let mut state = session.state.lock();
                                    state.closed = true;
                                    state.turn.take()
                                };
                                if let Some(shared) = shared.upgrade() {
                                    let mut live = shared.live.lock();
                                    if live
                                        .as_ref()
                                        .is_some_and(|live| Arc::ptr_eq(live, &session))
                                    {
                                        *live = None;
                                    }
                                }
                                if let Some(turn) = turn {
                                    let _ = turn.send(Err("Codex text generator exited".into()));
                                }
                            }
                            rpc.close(Some("Codex text generator exited"));
                        }
                    }
                }
            }));
        }

        let opened: Result<()> = async {
            self.shared
                .children
                .spawn_child(
                    TEXT_CHILD_ID,
                    &binary.path,
                    vec!["app-server".into()],
                    input.cwd,
                    Some(ChildAccount {
                        provider: HarnessId::Codex,
                        id: input.provider_account_id.unwrap_or("default").to_string(),
                    }),
                    Some(HarnessId::Codex),
                )
                .await?;
            rpc.request_value(
                "initialize",
                Some(json!({
                    "clientInfo": { "name": "monocode-text", "title": "MonoCode", "version": "0.1.0" },
                    "capabilities": { "experimentalApi": true },
                })),
                INIT_TIMEOUT_MS,
            )
            .await?;
            rpc.notify("initialized", None).await?;
            open_thread(&session, input.cwd, requested_thread_id.as_deref()).await
        }
        .await;

        match opened {
            Ok(()) => {
                *self.shared.live.lock() = Some(session.clone());
                Ok(session)
            }
            Err(error) => {
                session.state.lock().closed = true;
                rpc.close(Some(&error.to_string()));
                self.shared.children.unwatch_child(TEXT_CHILD_ID);
                let _ = self.shared.children.kill_child(TEXT_CHILD_ID).await;
                Err(error)
            }
        }
    }

    /// `dropLive`.
    async fn drop_live(&self) {
        let current = self.shared.live.lock().take();
        if let Some(current) = current {
            current.state.lock().closed = true;
            current.rpc.close(None);
        }
        self.shared.children.unwatch_child(TEXT_CHILD_ID);
        let _ = self.shared.children.kill_child(TEXT_CHILD_ID).await;
    }
}

/// `openThread`: resume the requested thread, or start a new one.
async fn open_thread(
    session: &LiveText,
    cwd: &str,
    requested_thread_id: Option<&str>,
) -> Result<()> {
    let (model, service_tier) = {
        let state = session.state.lock();
        (state.model.clone(), state.service_tier.clone())
    };
    let thread_params = || {
        build_thread_start_params(&ThreadStartInput {
            cwd,
            runtime_mode: TEXT_RUNTIME_MODE,
            controls_agents: false,
            model: Some(model.as_str()).filter(|model| !model.is_empty()),
            service_tier: service_tier.as_deref(),
        })
    };
    let thread_of = |opened: &Value| {
        opened
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .map(|id| js::trim(id).to_string())
            .filter(|id| !id.is_empty())
    };

    let mut thread_id: Option<String> = None;
    if let Some(requested) = requested_thread_id {
        let mut params = super::json::Record::new();
        params.insert("threadId".into(), json!(requested));
        params.extend(thread_params());
        match session
            .rpc
            .request_value(
                "thread/resume",
                Some(Value::Object(params)),
                INIT_TIMEOUT_MS,
            )
            .await
        {
            Ok(opened) => thread_id = thread_of(&opened),
            Err(error) => {
                if !is_recoverable_thread_resume_error(&error.to_string()) {
                    return Err(error);
                }
            }
        }
    }
    if thread_id.is_none() {
        let opened = session
            .rpc
            .request_value(
                "thread/start",
                Some(Value::Object(thread_params())),
                INIT_TIMEOUT_MS,
            )
            .await?;
        thread_id = thread_of(&opened);
    }
    let Some(thread_id) = thread_id else {
        bail!("Codex did not return a thread id");
    };
    let mut state = session.state.lock();
    state.cwd = cwd.to_string();
    state.thread_id = thread_id;
    Ok(())
}

/// `handleNotification` for the text runner.
fn handle_notification(session: Option<&LiveText>, method: &str, params: &Value) {
    let Some(session) = session else {
        return;
    };
    let (collecting, on_event) = {
        let state = session.state.lock();
        (state.collecting, state.on_event.clone())
    };
    if !collecting {
        if method == "turn/completed" {
            let turn = session.state.lock().turn.take();
            if let Some(turn) = turn {
                let _ = turn.send(Ok(()));
            }
        }
        return;
    }

    if let Some(on_event) = &on_event {
        for event in map_codex_notification(method, params).events {
            on_event(event);
        }
    }

    if method == "item/agentMessage/delta" {
        let delta = stream_text_delta(as_record(Some(params)).and_then(|rec| rec.get("delta")));
        if !delta.is_empty() {
            let mut state = session.state.lock();
            state.output = join_stream_text(&state.output, delta);
        }
        return;
    }

    if method == "turn/completed" {
        let turn = as_record(as_record(Some(params)).and_then(|rec| rec.get("turn")));
        let status = string_field(turn, "status").unwrap_or("completed");
        let outcome = if status == "failed" {
            let message = string_field(
                as_record(turn.and_then(|turn| turn.get("error"))),
                "message",
            )
            .unwrap_or("Codex turn failed");
            Err(message.to_string())
        } else {
            Ok(())
        };
        let turn = session.state.lock().turn.take();
        if let Some(turn) = turn {
            let _ = turn.send(outcome);
        }
    }
}

/// `handleServerRequest` for the text runner: it never approves anything.
async fn handle_server_request(rpc: &JsonRpcClient, id: JsonRpcId, method: &str) {
    let result = match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({ "decision": "decline" })
        }
        "item/permissions/requestApproval" => json!({ "permissions": {} }),
        _ => json!({}),
    };
    let _ = rpc.respond(id, result).await;
}

/// `pickTextModel`. Test seam.
#[cfg(test)]
pub(super) fn pick_model_for_test(text: &CodexText, requested: Option<&str>) -> String {
    text.pick_text_model(requested)
}

/// `pickTextEffort`. Test seam.
#[cfg(test)]
pub(super) fn pick_effort_for_test(
    text: &CodexText,
    model: &str,
    settings: Option<&ModelSettings>,
) -> String {
    text.pick_text_effort(model, settings)
}
