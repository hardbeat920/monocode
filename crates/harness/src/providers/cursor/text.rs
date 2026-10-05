//! Port of src/integrations/harness/providers/cursor/cursorText.ts: isolated
//! text prompts (titles, commit messages, side questions) on one shared
//! `cursor-agent acp` process in ask mode.
//!
//! The TypeScript kept the process and its turn chain in module globals.
//! Here they live in a [`TextRunner`] the adapter owns. A prompt holds the
//! turn lock for its whole run, which is the promise chain's serialization.

use std::sync::{Arc, Weak};

use anyhow::{Result, anyhow};
use monocode_core::block::ModelSettings;
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::HarnessEvent;
use monocode_core::js;
use monocode_core::reducer::join_stream_text;
use parking_lot::Mutex;
use serde_json::{Value, json};
use smol::lock::Mutex as AsyncMutex;

use crate::core::abort_text_prompt::with_text_prompt_abort;
use crate::core::acp::{AcpClient, AcpHandlers};
use crate::core::child::{ChildHandlers, Children};
use crate::core::registry::{EventSink, TextPromptInput};
use crate::core::task::SharedSpawner;

use super::json::{as_record, run_now};
use super::protocol::{
    SessionConfigOption, client_capabilities, js_string_or_empty, resolve_setting_config_id,
};

/// `TEXT_CHILD_ID`.
pub const TEXT_CHILD_ID: &str = "monocode-text";
const INIT_TIMEOUT_MS: i64 = 60_000;
const REQUEST_TIMEOUT_MS: i64 = 20_000;
/// `TEXT_MODEL`.
pub const TEXT_MODEL: &str = "composer-2.5";

/// `LiveText`.
struct LiveText {
    acp: AcpClient,
    state: Mutex<LiveTextState>,
}

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

/// The shared text process and its turn chain.
pub struct TextRunner {
    children: Children,
    spawner: SharedSpawner,
    live: Mutex<Option<Arc<LiveText>>>,
    turns: AsyncMutex<()>,
}

/// `modelSettingsKey`.
fn model_settings_key(settings: Option<&ModelSettings>) -> String {
    serde_json::to_string(&settings.cloned().unwrap_or_default()).unwrap_or_default()
}

impl TextRunner {
    pub fn new(children: Children, spawner: SharedSpawner) -> Arc<Self> {
        Arc::new(Self {
            children,
            spawner,
            live: Mutex::new(None),
            turns: AsyncMutex::new(()),
        })
    }

    /// `stopCursorTextPrompt`.
    pub async fn stop_cursor_text_prompt(&self, child_id: Option<&str>) {
        self.drop_live().await;
        if let Some(child_id) = child_id
            && child_id != TEXT_CHILD_ID
        {
            self.children.unwatch_child(child_id);
            let _ = self.children.kill_child(child_id).await;
        }
    }

    /// `warmupCursorText`: start the shared process in the background so the
    /// first prompt is fast. Failures are swallowed.
    pub async fn warmup_cursor_text(self: &Arc<Self>, cwd: &str) {
        if cwd.is_empty() || cwd == "~" {
            return;
        }
        let _turn = self.turns.lock().await;
        let _ = self.ensure_live(cwd, None, None).await;
    }

    /// `runCursorTextPrompt`: a Cursor ACP turn in ask mode.
    pub async fn run_cursor_text_prompt(
        self: &Arc<Self>,
        input: TextPromptInput,
    ) -> Result<String> {
        let _turn = self.turns.lock().await;
        self.prompt_on_live(input).await
    }

    async fn prompt_on_live(self: &Arc<Self>, input: TextPromptInput) -> Result<String> {
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
        let acp_session_id = {
            let mut state = session.state.lock();
            state.output.clear();
            state.collecting = true;
            state.on_event = input.on_event.clone();
            state.acp_session_id.clone()
        };
        let on_abort = {
            let acp = session.acp.clone();
            let id = acp_session_id.clone();
            move || async move {
                let _ = acp
                    .notify("session/cancel", Some(json!({ "sessionId": id })))
                    .await;
            }
        };
        let prompt = session.acp.request_value(
            "session/prompt",
            Some(json!({
                "sessionId": acp_session_id,
                "prompt": [{ "type": "text", "text": input.prompt }],
            })),
            input.timeout_ms.unwrap_or(REQUEST_TIMEOUT_MS),
        );
        let result = with_text_prompt_abort(input.signal.as_ref(), on_abort, prompt).await;
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
                if session.state.lock().closed {
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

    async fn ensure_live(
        self: &Arc<Self>,
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
        let current = self.live.lock().clone();
        if let Some(live) = current
            && !live.state.lock().closed
        {
            let same = {
                let state = live.state.lock();
                state.cwd == cwd && state.model == model && state.settings_key == settings_key
            };
            if same {
                return Ok(live);
            }
            match open_session(&live, cwd, &model, model_settings).await {
                Ok(()) => return Ok(live),
                Err(_) => self.drop_live().await,
            }
        }
        self.start_live(cwd, &model, model_settings).await
    }

    async fn start_live(
        self: &Arc<Self>,
        cwd: &str,
        model: &str,
        model_settings: Option<&ModelSettings>,
    ) -> Result<Arc<LiveText>> {
        self.drop_live().await;
        let path = self.children.resolve_cursor_binary().await?.path;
        let session_ref: Arc<Mutex<Weak<LiveText>>> = Arc::new(Mutex::new(Weak::new()));
        let handlers = AcpHandlers::default()
            .on_notification({
                let session_ref = session_ref.clone();
                move |method, params| {
                    let Some(session) = session_ref.lock().upgrade() else {
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
                        let next = join_stream_text(&state.output, &text_from_update(&params));
                        state.output = next;
                        // `joinStreamText` keeps the old text as a prefix.
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
                let session_ref = session_ref.clone();
                let spawner = self.spawner.clone();
                move |id, method, params| {
                    let Some(session) = session_ref.lock().upgrade() else {
                        return;
                    };
                    let acp = session.acp.clone();
                    let method = method.to_string();
                    run_now(
                        &spawner,
                        Box::pin(async move {
                            handle_text_request(&acp, id, &method, &params).await;
                        }),
                    );
                }
            });
        let acp = AcpClient::new(TEXT_CHILD_ID, Arc::new(self.children.clone()), handlers);
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
        *session_ref.lock() = Arc::downgrade(&session);

        let weak_runner = Arc::downgrade(self);
        let weak_session = Arc::downgrade(&session);
        self.children.watch_child_with(
            TEXT_CHILD_ID,
            ChildHandlers {
                on_line: Box::new({
                    let acp = acp.clone();
                    move |line| acp.push_line(&line)
                }),
                on_exit: Box::new({
                    let acp = acp.clone();
                    move |_| {
                        if let Some(session) = weak_session.upgrade() {
                            session.state.lock().closed = true;
                            if let Some(runner) = weak_runner.upgrade() {
                                let mut live = runner.live.lock();
                                if live
                                    .as_ref()
                                    .is_some_and(|live| Arc::ptr_eq(live, &session))
                                {
                                    *live = None;
                                }
                            }
                        }
                        acp.close(Some("Cursor text generator exited"));
                    }
                }),
                on_stderr: None,
            },
        );

        let setup = async {
            self.children
                .spawn_child(
                    TEXT_CHILD_ID,
                    &path,
                    vec!["acp".into()],
                    cwd,
                    None,
                    Some(HarnessId::Cursor),
                )
                .await?;
            acp.request_value(
                "initialize",
                Some(json!({
                    "protocolVersion": 1,
                    "clientCapabilities": client_capabilities(),
                    "clientInfo": { "name": "monocode-text", "version": "0.1.0" },
                })),
                INIT_TIMEOUT_MS,
            )
            .await?;
            let _ = acp
                .request_value(
                    "authenticate",
                    Some(json!({ "methodId": "cursor_login" })),
                    REQUEST_TIMEOUT_MS,
                )
                .await;
            open_session(&session, cwd, model, model_settings).await
        };
        match setup.await {
            Ok(()) => {
                *self.live.lock() = Some(session.clone());
                Ok(session)
            }
            Err(error) => {
                session.state.lock().closed = true;
                acp.close(Some(&error.to_string()));
                self.children.unwatch_child(TEXT_CHILD_ID);
                let _ = self.children.kill_child(TEXT_CHILD_ID).await;
                Err(error)
            }
        }
    }

    /// `dropLive`.
    async fn drop_live(&self) {
        let current = self.live.lock().take();
        if let Some(current) = current {
            current.state.lock().closed = true;
            current.acp.close(None);
        }
        self.children.unwatch_child(TEXT_CHILD_ID);
        let _ = self.children.kill_child(TEXT_CHILD_ID).await;
    }
}

/// `openSession`: a fresh ask-mode session with the requested model.
async fn open_session(
    session: &LiveText,
    cwd: &str,
    model: &str,
    model_settings: Option<&ModelSettings>,
) -> Result<()> {
    let acp = &session.acp;
    let setup = acp
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
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow!("Cursor did not return a session id"))?
        .to_string();

    let _ = acp
        .request_value(
            "session/set_mode",
            Some(json!({ "sessionId": acp_session_id, "modeId": "ask" })),
            REQUEST_TIMEOUT_MS,
        )
        .await;

    let config_options = setup.get("configOptions");
    let model_config_id = text_model_config_id(config_options);
    let set_model = acp
        .request_value(
            "session/set_config_option",
            Some(
                json!({ "sessionId": acp_session_id, "configId": model_config_id, "value": model }),
            ),
            REQUEST_TIMEOUT_MS,
        )
        .await;
    if set_model.is_err() {
        let _ = acp
            .request_value(
                "session/set_model",
                Some(json!({ "sessionId": acp_session_id, "modelId": model })),
                REQUEST_TIMEOUT_MS,
            )
            .await;
    }

    let options = text_setting_options(config_options);
    for (setting_id, value) in model_settings.into_iter().flatten() {
        let Some(config_id) = resolve_setting_config_id(&options, setting_id) else {
            continue;
        };
        let _ = acp
            .request_value(
                "session/set_config_option",
                Some(json!({ "sessionId": acp_session_id, "configId": config_id, "value": value })),
                REQUEST_TIMEOUT_MS,
            )
            .await;
    }

    let mut state = session.state.lock();
    state.cwd = cwd.to_string();
    state.model = model.to_string();
    state.settings_key = model_settings_key(model_settings);
    state.acp_session_id = acp_session_id;
    Ok(())
}

/// `handleTextRequest`: text generation never approves anything or answers
/// questions.
async fn handle_text_request(acp: &AcpClient, id: i64, method: &str, params: &Value) {
    let response = match method {
        "session/request_permission" => {
            let option_id = permission_option_ids(params)
                .into_iter()
                .find(|value| {
                    let lower = value.to_lowercase();
                    lower.contains("reject") || lower.contains("deny") || lower.contains("cancel")
                })
                .unwrap_or_else(|| "reject-once".into());
            json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
        }
        "cursor/ask_question" => json!({
            "outcome": { "outcome": "skipped", "reason": "Text generation does not answer questions" }
        }),
        _ => json!({}),
    };
    let _ = acp.respond(id, response).await;
}

/// The id fields the text runner reads, with `category` as a trimmed string.
fn id_and_category(item: &Value) -> Option<(String, String)> {
    let rec = item.as_object()?;
    let id = rec
        .get("id")
        .filter(|value| !value.is_null())
        .or_else(|| rec.get("configId").filter(|value| !value.is_null()));
    let id = js::trim(&js_string_or_empty(id)).to_string();
    let category = js::trim(&js_string_or_empty(rec.get("category"))).to_string();
    (!id.is_empty()).then_some((id, category))
}

/// The options `resolveSettingConfigId` reads in cursorText.ts.
fn text_setting_options(raw: Option<&Value>) -> Vec<SessionConfigOption> {
    let Some(Value::Array(items)) = raw else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(id_and_category)
        .map(|(id, category)| SessionConfigOption {
            id,
            category: Some(category),
            current_value: None,
        })
        .collect()
}

/// `extractModelConfigId` in cursorText.ts.
fn text_model_config_id(raw: Option<&Value>) -> String {
    let Some(Value::Array(items)) = raw else {
        return "model".into();
    };
    items
        .iter()
        .filter_map(id_and_category)
        .find(|(id, category)| category == "model" || id == "model")
        .map(|(id, _)| id)
        .unwrap_or_else(|| "model".into())
}

/// `permissionOptionIds`.
fn permission_option_ids(params: &Value) -> Vec<String> {
    match params.get("options") {
        Some(Value::Array(options)) => options
            .iter()
            .filter_map(|item| {
                as_record(Some(item))?
                    .get("optionId")?
                    .as_str()
                    .map(str::to_string)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `textFromUpdate`: the agent message text in a `session/update`.
fn text_from_update(params: &Value) -> String {
    let rec = params.as_object();
    let update = as_record(rec.and_then(|rec| rec.get("update"))).or(rec);
    let Some(update) = update else {
        return String::new();
    };
    let kind = ["sessionUpdate", "session_update", "type"]
        .iter()
        .find_map(|key| update.get(*key).filter(|value| !value.is_null()))
        .map(|value| js_string_or_empty(Some(value)))
        .unwrap_or_default();
    if kind != "agent_message_chunk" && kind != "agent_message" {
        return String::new();
    }
    let content = update
        .get("content")
        .filter(|value| !value.is_null())
        .or_else(|| update.get("text"));
    text_from_content(content)
}

/// `textFromContent` in cursorText.ts: array parts join with nothing.
fn text_from_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(rec)) => {
            if let Some(Value::String(text)) = rec.get("text") {
                return text.clone();
            }
            match rec.get("content").filter(|value| !value.is_null()) {
                Some(nested) => text_from_content(Some(nested)),
                None => String::new(),
            }
        }
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| text_from_content(Some(item)))
            .collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_message_text_from_updates() {
        let chunk = json!({ "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Hi" } } });
        assert_eq!(text_from_update(&chunk), "Hi");
        let whole = json!({ "sessionUpdate": "agent_message", "content": [{ "text": "a" }, { "text": "b" }] });
        assert_eq!(text_from_update(&whole), "ab");
        let thought =
            json!({ "update": { "sessionUpdate": "agent_thought_chunk", "content": "x" } });
        assert_eq!(text_from_update(&thought), "");
    }

    #[test]
    fn reads_config_ids_like_cursor_text() {
        let raw = json!([{ "configId": " picker ", "category": " model " }, { "id": "effort", "category": "thought_level" }]);
        assert_eq!(text_model_config_id(Some(&raw)), "picker");
        assert_eq!(text_model_config_id(None), "model");
        let options = text_setting_options(Some(&raw));
        assert_eq!(
            resolve_setting_config_id(&options, "reasoning").as_deref(),
            Some("effort")
        );
    }

    #[test]
    fn rejects_permissions_by_any_reject_like_option() {
        let params =
            json!({ "options": [{ "optionId": "allow-once" }, { "optionId": "Deny-Always" }] });
        assert_eq!(
            permission_option_ids(&params),
            ["allow-once", "Deny-Always"]
        );
    }
}
