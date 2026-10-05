//! Port of src/integrations/harness/providers/grok/grokText.ts: one-shot text
//! prompts (titles, commit messages, side questions) on a separate
//! `grok agent stdio` child with no tool access.
//!
//! The TypeScript kept `live` and `turns` in module globals; here they live
//! on [`GrokText`], which the adapter owns.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::block::ModelSettings;
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::HarnessEvent;
use monocode_core::js;
use monocode_core::reducer::join_stream_text;

use crate::core::abort_text_prompt::with_text_prompt_abort;
use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::child::{ChildHandlers, Children};
use crate::core::registry::{EventSink, TextPromptInput};
use crate::core::task::SharedSpawner;

use super::protocol::{TEXT_MODEL, grok_auth_method_id, grok_effort, grok_text_spawn_args};
use super::shared::{initialize_params, selected_outcome};

pub const TEXT_CHILD_ID: &str = "monocode-grok-text";
const INIT_TIMEOUT_MS: i64 = 60_000;
const REQUEST_TIMEOUT_MS: i64 = 20_000;

struct LiveTextState {
    cwd: String,
    model: String,
    settings_key: String,
    acp_session_id: String,
    collecting: bool,
    output: String,
    closed: bool,
    on_event: Option<EventSink>,
}

/// `LiveText`.
struct LiveText {
    acp: AcpClient,
    state: Mutex<LiveTextState>,
}

impl LiveText {
    fn closed(&self) -> bool {
        self.state.lock().closed
    }

    fn acp_session_id(&self) -> String {
        self.state.lock().acp_session_id.clone()
    }
}

struct Inner {
    children: Children,
    spawner: SharedSpawner,
    live: Mutex<Option<Arc<LiveText>>>,
    /// `turns`: text prompts run one at a time.
    turns: smol::lock::Mutex<()>,
}

/// The Grok text runner. Clones share one runner.
#[derive(Clone)]
pub struct GrokText {
    inner: Arc<Inner>,
}

/// The input of `runGrokTextPrompt` that git and titles use.
#[derive(Clone, Default)]
pub struct GrokTextPrompt {
    pub cwd: String,
    pub model: Option<String>,
    pub model_settings: Option<ModelSettings>,
    pub prompt: String,
    pub timeout_ms: Option<i64>,
    pub signal: Option<crate::core::task::AbortSignal>,
    pub on_event: Option<EventSink>,
}

impl From<TextPromptInput> for GrokTextPrompt {
    fn from(input: TextPromptInput) -> Self {
        Self {
            cwd: input.cwd,
            model: input.model,
            model_settings: input.model_settings,
            prompt: input.prompt,
            timeout_ms: input.timeout_ms,
            signal: input.signal,
            on_event: input.on_event,
        }
    }
}

impl GrokText {
    pub fn new(children: Children, spawner: SharedSpawner) -> Self {
        Self {
            inner: Arc::new(Inner {
                children,
                spawner,
                live: Mutex::new(None),
                turns: smol::lock::Mutex::new(()),
            }),
        }
    }

    /// `stopGrokTextPrompt`.
    pub async fn stop(&self, child_id: Option<&str>) {
        self.drop_live().await;
        if let Some(child_id) = child_id
            && child_id != TEXT_CHILD_ID
        {
            self.inner.children.unwatch_child(child_id);
            let _ = self.inner.children.kill_child(child_id).await;
        }
    }

    /// `warmupGrokText`.
    pub async fn warmup(&self, cwd: &str) {
        if cwd.is_empty() || cwd == "~" {
            return;
        }
        let _turn = self.inner.turns.lock().await;
        let _ = self.ensure_live(cwd, None, None).await;
    }

    /// `runGrokTextPrompt` with the registry's input.
    pub async fn run(&self, input: TextPromptInput) -> Result<String> {
        self.run_prompt(input.into()).await
    }

    /// `runGrokTextPrompt`.
    pub async fn run_prompt(&self, input: GrokTextPrompt) -> Result<String> {
        let _turn = self.inner.turns.lock().await;
        self.prompt_on_live(input).await
    }

    /// `promptOnLive`.
    async fn prompt_on_live(&self, input: GrokTextPrompt) -> Result<String> {
        if let Some(signal) = &input.signal {
            signal.throw_if_aborted()?;
        }
        let session = self
            .ensure_live(
                &input.cwd,
                input.model.as_deref(),
                input.model_settings.as_ref(),
            )
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
        let acp_session_id = session.acp_session_id();
        let request = session.acp.request_value(
            "session/prompt",
            Some(json!({
                "sessionId": acp_session_id,
                "prompt": [{ "type": "text", "text": input.prompt }],
            })),
            input.timeout_ms.unwrap_or(REQUEST_TIMEOUT_MS),
        );
        let on_abort = {
            let acp = session.acp.clone();
            let acp_session_id = acp_session_id.clone();
            move || async move {
                let _ = acp
                    .notify(
                        "session/cancel",
                        Some(json!({ "sessionId": acp_session_id })),
                    )
                    .await;
            }
        };
        let result = with_text_prompt_abort(input.signal.as_ref(), on_abort, request).await;
        let outcome = match result {
            Ok(_) => Ok(session.state.lock().output.clone()),
            Err(error) => {
                let _ = session
                    .acp
                    .notify(
                        "session/cancel",
                        Some(json!({ "sessionId": acp_session_id })),
                    )
                    .await;
                if session.closed() {
                    self.drop_live().await;
                }
                Err(error)
            }
        };
        {
            let mut state = session.state.lock();
            state.collecting = false;
            state.on_event = None;
        }
        self.drop_live().await;
        outcome
    }

    /// `ensureLive`.
    async fn ensure_live(
        &self,
        cwd: &str,
        requested_model: Option<&str>,
        model_settings: Option<&ModelSettings>,
    ) -> Result<Arc<LiveText>> {
        let model = requested_model
            .map(js::trim)
            .filter(|model| !model.is_empty())
            .unwrap_or(TEXT_MODEL)
            .to_string();
        let settings_key = model_settings_key(model_settings);
        let current = self.inner.live.lock().clone();
        if let Some(live) = current.filter(|live| !live.closed()) {
            let same = {
                let state = live.state.lock();
                state.cwd == cwd && state.model == model && state.settings_key == settings_key
            };
            if same {
                return Ok(live);
            }
            match self.open_session(&live, cwd, &model, model_settings).await {
                Ok(()) => return Ok(live),
                Err(_) => self.drop_live().await,
            }
        }
        self.start_live(cwd, &model, model_settings).await
    }

    /// `startLive`.
    async fn start_live(
        &self,
        cwd: &str,
        model: &str,
        model_settings: Option<&ModelSettings>,
    ) -> Result<Arc<LiveText>> {
        self.drop_live().await;
        let path = self.inner.children.resolve_grok_binary().await?.path;
        // The handlers reach the session through a weak cell, so the client's
        // own handlers never keep it alive.
        let session_cell: Arc<Mutex<std::sync::Weak<LiveText>>> = Arc::default();
        let handlers = AcpHandlers::default()
            .on_notification({
                let session_cell = session_cell.clone();
                move |method: &str, params: Value| {
                    let Some(session) = session_cell.lock().upgrade() else {
                        return;
                    };
                    if method != "session/update" {
                        return;
                    }
                    let (delta, sink) = {
                        let mut state = session.state.lock();
                        if !state.collecting {
                            return;
                        }
                        let previous_len = state.output.len();
                        state.output = join_stream_text(&state.output, &text_from_update(&params));
                        let delta = state.output.get(previous_len..).unwrap_or("").to_string();
                        (delta, state.on_event.clone())
                    };
                    if !delta.is_empty()
                        && let Some(sink) = sink
                    {
                        sink(HarnessEvent::MessageDelta { text: delta });
                    }
                }
            })
            .on_request({
                let session_cell = session_cell.clone();
                let spawner = self.inner.spawner.clone();
                move |id: i64, method: &str, params: Value| {
                    let Some(acp) = session_cell
                        .lock()
                        .upgrade()
                        .map(|session| session.acp.clone())
                    else {
                        return;
                    };
                    let method = method.to_string();
                    spawner.spawn(Box::pin(async move {
                        handle_text_request(&acp, id, &method, &params).await;
                    }));
                }
            });
        let acp = AcpClient::new(
            TEXT_CHILD_ID,
            Arc::new(self.inner.children.clone()),
            handlers,
        );
        let session = Arc::new(LiveText {
            acp: acp.clone(),
            state: Mutex::new(LiveTextState {
                cwd: cwd.to_string(),
                model: model.to_string(),
                settings_key: model_settings_key(model_settings),
                acp_session_id: String::new(),
                collecting: false,
                output: String::new(),
                closed: false,
                on_event: None,
            }),
        });
        *session_cell.lock() = Arc::downgrade(&session);

        let inner = Arc::downgrade(&self.inner);
        self.inner.children.watch_child_with(
            TEXT_CHILD_ID,
            ChildHandlers {
                on_line: Box::new({
                    let acp = acp.clone();
                    move |line| acp.push_line(&line)
                }),
                on_exit: Box::new({
                    let session_cell = session_cell.clone();
                    let acp = acp.clone();
                    move |_code| {
                        if let Some(session) = session_cell.lock().upgrade() {
                            session.state.lock().closed = true;
                            if let Some(inner) = inner.upgrade() {
                                let mut live = inner.live.lock();
                                if live
                                    .as_ref()
                                    .is_some_and(|live| Arc::ptr_eq(live, &session))
                                {
                                    *live = None;
                                }
                            }
                        }
                        acp.close(Some("Grok Build text generator exited"));
                    }
                }),
                on_stderr: None,
            },
        );

        let started = async {
            self.inner
                .children
                .spawn_child(
                    TEXT_CHILD_ID,
                    &path,
                    grok_text_spawn_args(),
                    cwd,
                    None,
                    Some(HarnessId::Grok),
                )
                .await?;
            let init = acp
                .request_value(
                    "initialize",
                    Some(initialize_params("monocode-text")),
                    INIT_TIMEOUT_MS,
                )
                .await?;
            if let Some(method_id) = grok_auth_method_id(&init) {
                let _ = acp
                    .request_value(
                        "authenticate",
                        Some(json!({ "methodId": method_id, "_meta": { "headless": true } })),
                        REQUEST_TIMEOUT_MS,
                    )
                    .await;
            }
            self.open_session(&session, cwd, model, model_settings)
                .await
        };
        match started.await {
            Ok(()) => {
                *self.inner.live.lock() = Some(session.clone());
                Ok(session)
            }
            Err(error) => {
                session.state.lock().closed = true;
                acp.close(Some(&error.to_string()));
                self.inner.children.unwatch_child(TEXT_CHILD_ID);
                let _ = self.inner.children.kill_child(TEXT_CHILD_ID).await;
                Err(error)
            }
        }
    }

    /// `openSession`.
    async fn open_session(
        &self,
        session: &LiveText,
        cwd: &str,
        model: &str,
        model_settings: Option<&ModelSettings>,
    ) -> Result<()> {
        let setup = session
            .acp
            .request_value(
                "session/new",
                Some(json!({ "cwd": cwd, "mcpServers": [] })),
                REQUEST_TIMEOUT_MS,
            )
            .await?;
        let acp_session_id = setup
            .get("sessionId")
            .and_then(Value::as_str)
            .map(js::trim)
            .unwrap_or("")
            .to_string();
        if acp_session_id.is_empty() {
            return Err(anyhow!("Grok Build did not return a session id"));
        }
        let _ = session
            .acp
            .request_value(
                "session/set_model",
                Some(json!({ "sessionId": acp_session_id, "modelId": model })),
                REQUEST_TIMEOUT_MS,
            )
            .await;
        let _ = session
            .acp
            .request_value(
                "session/set_mode",
                Some(json!({
                    "sessionId": acp_session_id,
                    "modeId": grok_effort(model_settings).unwrap_or_else(|| "low".into()),
                })),
                REQUEST_TIMEOUT_MS,
            )
            .await;
        let mut state = session.state.lock();
        state.cwd = cwd.to_string();
        state.model = model.to_string();
        state.settings_key = model_settings_key(model_settings);
        state.acp_session_id = acp_session_id;
        Ok(())
    }

    /// `dropLive`.
    async fn drop_live(&self) {
        let current = self.inner.live.lock().take();
        if let Some(current) = current {
            current.state.lock().closed = true;
            current.acp.close(None);
        }
        self.inner.children.unwatch_child(TEXT_CHILD_ID);
        let _ = self.inner.children.kill_child(TEXT_CHILD_ID).await;
    }
}

/// `handleTextRequest`: the text runner never grants a tool or asks the user.
async fn handle_text_request(acp: &AcpClient, id: i64, method: &str, params: &Value) {
    match method {
        "session/request_permission" => {
            let option_id = permission_option_ids(params)
                .into_iter()
                .find(|value| is_refusal(value))
                .unwrap_or_else(|| "reject-once".into());
            let _ = acp.respond(id, selected_outcome(&option_id)).await;
        }
        "_x.ai/ask_user_question" | "x.ai/ask_user_question" => {
            let _ = acp
                .respond(id, json!({ "outcome": "skip_interview" }))
                .await;
        }
        _ => {
            let _ = acp.respond(id, json!({})).await;
        }
    }
}

fn is_refusal(value: &str) -> bool {
    let lower = value.to_lowercase();
    lower.contains("reject") || lower.contains("deny") || lower.contains("cancel")
}

/// `permissionOptionIds`.
fn permission_option_ids(params: &Value) -> Vec<String> {
    params
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .filter_map(|item| {
                    item.as_object()?
                        .get("optionId")?
                        .as_str()
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `textFromUpdate`: agent message text only.
fn text_from_update(params: &Value) -> String {
    let rec = params.as_object();
    let update = rec
        .and_then(|rec| rec.get("update"))
        .and_then(Value::as_object)
        .or(rec);
    let Some(update) = update else {
        return String::new();
    };
    let kind = update
        .get("sessionUpdate")
        .filter(|value| !value.is_null())
        .or_else(|| {
            update
                .get("session_update")
                .filter(|value| !value.is_null())
        })
        .or_else(|| update.get("type").filter(|value| !value.is_null()))
        .map(crate::core::json_text::js_string)
        .unwrap_or_default();
    if kind != "agent_message_chunk" && kind != "agent_message" {
        return String::new();
    }
    text_from_content(
        update
            .get("content")
            .filter(|value| !value.is_null())
            .or_else(|| update.get("text")),
    )
}

fn text_from_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(rec)) => match rec.get("text") {
            Some(Value::String(text)) => text.clone(),
            _ => match rec.get("content").filter(|value| !value.is_null()) {
                Some(inner) => text_from_content(Some(inner)),
                None => String::new(),
            },
        },
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| text_from_content(Some(item)))
            .collect(),
        _ => String::new(),
    }
}

/// `modelSettingsKey`.
fn model_settings_key(settings: Option<&ModelSettings>) -> String {
    json!({ "effort": grok_effort(settings).unwrap_or_else(|| "low".into()) }).to_string()
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
