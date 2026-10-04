//! Port of src/integrations/harness/providers/opencode/opencodeText.ts:
//! isolated, read-only text generation (titles, commit messages, side
//! questions) on a dedicated `opencode serve` child.
//!
//! The TypeScript kept `live`, `turns`, and `serverUrl` in module globals.
//! Here they are fields of [`OpenCodeText`], which the adapter owns. The
//! `turns` promise chain is an async mutex: prompts run one at a time, and a
//! failed one does not block the next.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use futures::FutureExt;
use monocode_core::block::{ModelSettings, TurnIntent};
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::HarnessEvent;
use monocode_core::js;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use super::client::{OpenCodeClient, PromptInput, parse_event};
use super::deps::stream_text_delta;
use super::protocol::{
    MINIMUM_OPENCODE_VERSION, OpenCodePart, OpenCodePermissionRule, OpenCodePromptPart,
    ParsedOpenCodeModelSlug, PartStore, PartTime, PermissionAction, Record,
    append_open_code_assistant_text_delta, compare_semver, event_session_id, field,
    is_known_hidden_agent, is_truthy, merge_open_code_assistant_text, parse_open_code_model_slug,
    parse_open_code_version, parse_server_url_from_output, record_field, string_field,
    text_delta_event,
};
use crate::core::abort_text_prompt::with_text_prompt_abort;
use crate::core::catalog::SharedCatalog;
use crate::core::child::{BinaryPathChoice, ChildEvent, Children, SseEvent};
use crate::core::json_text::js_string;
use crate::core::registry::{EventSink, TextPromptInput};
use crate::core::task::{SharedSpawner, sleep};

/// Shared prompt builders can use either OpenCode transport.
pub trait TextBackend: Send + Sync {
    fn run_text(&self, input: TextPromptInput) -> crate::core::task::BoxFuture<'_, Result<String>>;
}

impl TextBackend for OpenCodeText {
    fn run_text(&self, input: TextPromptInput) -> crate::core::task::BoxFuture<'_, Result<String>> {
        self.run(input).boxed()
    }
}

pub const TEXT_CHILD_ID: &str = "monocode-opencode-text";
const SERVER_TIMEOUT_MS: u64 = 30_000;
const REQUEST_TIMEOUT_MS: i64 = 45_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Assistant,
    User,
    Hidden,
}

#[derive(Default)]
struct TextState {
    message_role_by_id: HashMap<String, Role>,
    part_by_id: PartStore,
    emitted_text_by_part_id: HashMap<String, String>,
    pending_text_delta_by_part_id: HashMap<String, String>,
    on_event: Option<EventSink>,
    outbox: Vec<HarnessEvent>,
}

/// `LiveText`.
struct LiveText {
    client: OpenCodeClient,
    session_id: String,
    cwd: String,
    model: ParsedOpenCodeModelSlug,
    model_settings_key: String,
    model_settings: Option<ModelSettings>,
    state: Mutex<TextState>,
}

struct TextInner {
    children: Children,
    catalog: SharedCatalog,
    spawner: SharedSpawner,
    live: Mutex<Option<Arc<LiveText>>>,
    turns: smol::lock::Mutex<()>,
    server_url: Mutex<String>,
    processed_events: AtomicUsize,
}

/// The text-generation backend. Clones share one server.
#[derive(Clone)]
pub struct OpenCodeText {
    inner: Arc<TextInner>,
}

impl OpenCodeText {
    pub fn new(children: Children, catalog: SharedCatalog, spawner: SharedSpawner) -> Self {
        Self {
            inner: Arc::new(TextInner {
                children,
                catalog,
                spawner,
                live: Mutex::new(None),
                turns: smol::lock::Mutex::new(()),
                server_url: Mutex::new(String::new()),
                processed_events: AtomicUsize::new(0),
            }),
        }
    }

    /// SSE frames handled so far. Tests wait on it.
    #[cfg(test)]
    pub(crate) fn processed_events(&self) -> usize {
        self.inner.processed_events.load(Ordering::SeqCst)
    }

    /// `stopOpenCodeTextPrompt`.
    pub async fn stop(&self) {
        self.drop_live().await;
    }

    /// `warmupOpenCodeText`: start the server early. Never fails.
    pub async fn warmup(&self, cwd: &str) {
        if cwd.is_empty() || cwd == "~" {
            return;
        }
        let _turn = self.inner.turns.lock().await;
        let _ = self.ensure_live(cwd, None, None).await;
    }

    /// `runOpenCodeTextPrompt`.
    pub async fn run(&self, input: TextPromptInput) -> Result<String> {
        let _turn = self.inner.turns.lock().await;
        self.prompt_on_live(input).await
    }

    async fn prompt_on_live(&self, input: TextPromptInput) -> Result<String> {
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
        session.state.lock().on_event = input.on_event.clone();

        let result = self.prompt_once(&session, &input).await;

        session.state.lock().on_event = None;
        self.drop_live().await;
        result
    }

    async fn prompt_once(
        &self,
        session: &Arc<LiveText>,
        input: &TextPromptInput,
    ) -> Result<String> {
        let prompt = PromptInput {
            session_id: session.session_id.clone(),
            model: session.model.clone(),
            agent: Some(open_code_text_agent(
                input.intent,
                session.model_settings.as_ref(),
            )),
            variant: session
                .model_settings
                .as_ref()
                .and_then(|settings| settings.get("variant").cloned()),
            parts: vec![OpenCodePromptPart::Text {
                text: input.prompt.clone(),
            }],
        };
        let timeout = Some(input.timeout_ms.unwrap_or(REQUEST_TIMEOUT_MS));
        let client = session.client.clone();
        let session_id = session.session_id.clone();
        // `abortTextPromptRace`: on abort, stop the server's turn, then reject.
        let result = with_text_prompt_abort(
            input.signal.as_ref(),
            move || async move { client.abort_session(&session_id).await },
            session.client.prompt(&prompt, timeout),
        )
        .await?;
        let error = result
            .info
            .as_ref()
            .and_then(|info| info.get("error"))
            .filter(|error| is_truthy(error));
        if let Some(error) = error {
            let message = match error.as_object().and_then(|error| error.get("message")) {
                Some(message) => js_string(message),
                None => "OpenCode text generation failed".into(),
            };
            bail!(message);
        }
        let text = get_open_code_text_response(result.parts.as_deref());
        if text.is_empty() {
            bail!("OpenCode returned empty output.");
        }
        Ok(text)
    }

    async fn ensure_live(
        &self,
        cwd: &str,
        requested_model: Option<&str>,
        model_settings: Option<&ModelSettings>,
    ) -> Result<Arc<LiveText>> {
        let model = self.pick_text_model(requested_model);
        let settings_key = model_settings_key(model_settings);
        let current = self.inner.live.lock().clone();
        if let Some(live) = &current
            && live.cwd == cwd
            && live.model == model
            && live.model_settings_key == settings_key
        {
            return Ok(live.clone());
        }
        if current.is_some() {
            self.drop_live().await;
        }
        self.start_live(cwd, model, model_settings).await
    }

    async fn start_live(
        &self,
        cwd: &str,
        model: ParsedOpenCodeModelSlug,
        model_settings: Option<&ModelSettings>,
    ) -> Result<Arc<LiveText>> {
        let children = &self.inner.children;
        let binary = children.resolve_open_code_binary().await?;
        let version_out = children
            .exec_child(
                &binary.path,
                vec!["--version".into()],
                Some(cwd),
                Some(HarnessId::Opencode),
                BinaryPathChoice::Runtime,
            )
            .await
            .unwrap_or_default();
        let version = parse_open_code_version(&version_out);
        if version
            .as_deref()
            .is_none_or(|version| compare_semver(version, MINIMUM_OPENCODE_VERSION) < 0)
        {
            bail!(
                "OpenCode v{} is too old for text generation.",
                version.as_deref().unwrap_or("unknown")
            );
        }

        self.inner.server_url.lock().clear();
        self.watch_server();

        let port = children.free_harness_port().await?;
        children
            .spawn_child(
                TEXT_CHILD_ID,
                &binary.path,
                vec![
                    "serve".into(),
                    "--hostname=127.0.0.1".into(),
                    format!("--port={port}"),
                ],
                cwd,
                None,
                Some(HarnessId::Opencode),
            )
            .await?;

        match self.connect(cwd, model, model_settings).await {
            Ok(session) => Ok(session),
            Err(error) => {
                self.drop_live().await;
                Err(error)
            }
        }
    }

    async fn connect(
        &self,
        cwd: &str,
        model: ParsedOpenCodeModelSlug,
        model_settings: Option<&ModelSettings>,
    ) -> Result<Arc<LiveText>> {
        let url = self.wait_for_url(SERVER_TIMEOUT_MS).await?;
        let client = OpenCodeClient::new(&url, cwd, self.inner.children.clone());
        let deny_all = [OpenCodePermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Deny,
        }];
        let created = client.create_session(None, Some(&deny_all)).await?;
        let session = Arc::new(LiveText {
            client,
            session_id: created.id,
            cwd: cwd.to_string(),
            model,
            model_settings_key: model_settings_key(model_settings),
            model_settings: model_settings.cloned(),
            state: Mutex::new(TextState::default()),
        });
        *self.inner.live.lock() = Some(session.clone());
        let events = session.client.subscribe_events(TEXT_CHILD_ID).await?;
        let pump = session.clone();
        let inner = self.inner.clone();
        self.inner.spawner.spawn(
            async move {
                while let Ok(event) = events.recv().await {
                    if let SseEvent::Data(data) = event
                        && let Some(event) = parse_event(&data)
                    {
                        handle_text_event(&pump, &event);
                    }
                    inner.processed_events.fetch_add(1, Ordering::SeqCst);
                }
            }
            .boxed(),
        );
        Ok(session)
    }

    /// `watchChild(TEXT_CHILD_ID, ...)`: read the listening URL from stdout or
    /// stderr, and forget the server when it exits.
    fn watch_server(&self) {
        let events = self.inner.children.watch_child(TEXT_CHILD_ID);
        let inner = self.inner.clone();
        self.inner.spawner.spawn(
            async move {
                while let Ok(event) = events.recv().await {
                    match event {
                        ChildEvent::Stdout(line) | ChildEvent::Stderr(line) => {
                            if let Some(url) = parse_server_url_from_output(&line) {
                                *inner.server_url.lock() = url;
                            }
                        }
                        ChildEvent::Exit(_) => {
                            inner.live.lock().take();
                        }
                    }
                }
            }
            .boxed(),
        );
    }

    async fn wait_for_url(&self, timeout_ms: u64) -> Result<String> {
        let started = Instant::now();
        loop {
            let url = self.inner.server_url.lock().clone();
            if !url.is_empty() {
                return Ok(url);
            }
            if started.elapsed() >= Duration::from_millis(timeout_ms) {
                bail!("Timed out waiting for OpenCode text server");
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    /// `dropLive`.
    async fn drop_live(&self) {
        let current = self.inner.live.lock().take();
        if let Some(current) = current {
            current.client.abort_session(&current.session_id).await;
            current.client.close_events(TEXT_CHILD_ID).await;
        }
        let children = &self.inner.children;
        children.unwatch_child(TEXT_CHILD_ID);
        let _ = children.kill_child(TEXT_CHILD_ID).await;
    }

    /// `pickTextModel`.
    fn pick_text_model(&self, requested: Option<&str>) -> ParsedOpenCodeModelSlug {
        let selected = requested.map(js::trim).unwrap_or_default();
        if !selected.is_empty() {
            let model_slug = selected.strip_prefix("opencode:").unwrap_or(selected);
            if let Some(parsed) = parse_open_code_model_slug(Some(model_slug)) {
                return parsed;
            }
            if !model_slug.is_empty() {
                return ParsedOpenCodeModelSlug {
                    provider_id: "opencode".into(),
                    model_id: model_slug.into(),
                };
            }
        }
        let catalog = self.inner.catalog.read();
        for model in catalog.models_for(HarnessId::Opencode) {
            let slug = model.native_id.as_deref().unwrap_or(&model.id);
            if let Some(parsed) = parse_open_code_model_slug(Some(slug)) {
                return parsed;
            }
        }
        ParsedOpenCodeModelSlug {
            provider_id: "opencode".into(),
            model_id: "glm-5".into(),
        }
    }
}

/// `modelSettingsKey`: `JSON.stringify({ agent, variant })`.
fn model_settings_key(settings: Option<&ModelSettings>) -> String {
    #[derive(Serialize)]
    struct Key<'a> {
        #[serde(skip_serializing_if = "Option::is_none")]
        agent: Option<&'a String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        variant: Option<&'a String>,
    }
    serde_json::to_string(&Key {
        agent: settings.and_then(|settings| settings.get("agent")),
        variant: settings.and_then(|settings| settings.get("variant")),
    })
    .unwrap_or_default()
}

/// `openCodeTextAgent`.
fn open_code_text_agent(intent: Option<TurnIntent>, settings: Option<&ModelSettings>) -> String {
    match intent {
        Some(TurnIntent::Plan) => return "plan".into(),
        Some(TurnIntent::Build) => return "build".into(),
        _ => {}
    }
    let configured = settings
        .and_then(|settings| settings.get("agent"))
        .map(|agent| js::trim(agent))
        .unwrap_or_default();
    if configured.is_empty() {
        "build".into()
    } else {
        configured.to_string()
    }
}

/// `getOpenCodeTextResponse`: the text parts of a finished message, joined.
pub fn get_open_code_text_response(parts: Option<&[Value]>) -> String {
    let text: String = parts
        .unwrap_or_default()
        .iter()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect();
    js::trim(&text).to_string()
}

fn handle_text_event(session: &LiveText, event: &Record) {
    let (events, sink) = {
        let mut state = session.state.lock();
        apply_text_event(session, &mut state, event);
        (std::mem::take(&mut state.outbox), state.on_event.clone())
    };
    if let Some(sink) = sink {
        for event in events {
            sink(event);
        }
    }
}

/// `handleTextEvent`.
fn apply_text_event(session: &LiveText, state: &mut TextState, event: &Record) {
    if let Some(session_id) = event_session_id(event)
        && session_id != session.session_id
    {
        return;
    }
    let empty = Record::new();
    let properties = record_field(Some(event), "properties").unwrap_or(&empty);
    let properties = Some(properties);
    let event_type = event.get("type").and_then(Value::as_str);

    if event_type == Some("message.updated") {
        let info = record_field(properties, "info");
        let (Some(id), role) = (string_field(info, "id"), string_field(info, "role")) else {
            return;
        };
        let agent = string_field(info, "agent");
        let resolved = match role {
            Some("assistant") if agent.is_some_and(is_known_hidden_agent) => Role::Hidden,
            Some("assistant") => Role::Assistant,
            Some("user") => Role::User,
            _ => return,
        };
        state.message_role_by_id.insert(id.to_string(), resolved);
        if resolved == Role::Assistant {
            for part in state.part_by_id.of_message(id) {
                emit_text_part(state, &part);
            }
        }
        return;
    }

    if event_type == Some("message.part.updated") {
        let Some(mut part) = parse_text_part(field(properties, "part")) else {
            return;
        };
        let pending_delta = state
            .pending_text_delta_by_part_id
            .get(&part.id)
            .filter(|delta| !delta.is_empty())
            .cloned();
        if let Some(pending_delta) = pending_delta {
            if part.time.and_then(|time| time.end).is_none() {
                part.text = Some(
                    merge_open_code_assistant_text(
                        Some(&pending_delta),
                        part.text.as_deref().unwrap_or_default(),
                    )
                    .latest_text,
                );
            }
            state.pending_text_delta_by_part_id.remove(&part.id);
        }
        let part = merge_text_part(state.part_by_id.get(&part.id), part);
        state.part_by_id.set(part.clone());
        if text_part_role(state, &part) == Some(Role::Assistant) {
            emit_text_part(state, &part);
        }
        return;
    }

    if event_type != Some("message.part.delta") {
        return;
    }
    let part_id = string_field(properties, "partID");
    let delta = stream_text_delta(field(properties, "delta"));
    let (Some(part_id), false) = (part_id, delta.is_empty()) else {
        return;
    };
    let Some(existing) = state.part_by_id.get(part_id).cloned() else {
        state
            .pending_text_delta_by_part_id
            .entry(part_id.to_string())
            .or_default()
            .push_str(&delta);
        return;
    };
    // OpenCode publishes the completed part snapshot with time.end after all
    // text deltas. If SSE delivery reorders those publications, the snapshot
    // already contains any delta that arrives after it.
    if existing.time.and_then(|time| time.end).is_some() {
        return;
    }
    let previous = state
        .emitted_text_by_part_id
        .get(&existing.id)
        .cloned()
        .or_else(|| existing.text.clone())
        .unwrap_or_default();
    let next = append_open_code_assistant_text_delta(&previous, &delta);
    let next_part = OpenCodePart {
        text: Some(next.next_text.clone()),
        ..existing
    };
    state.part_by_id.set(next_part.clone());
    if text_part_role(state, &next_part) != Some(Role::Assistant) {
        return;
    }
    state
        .emitted_text_by_part_id
        .insert(next_part.id.clone(), next.next_text);
    if let Some(mapped) = text_delta_event(&next_part, &next.delta_to_emit) {
        state.outbox.push(mapped);
    }
}

/// `emitTextPart`.
fn emit_text_part(state: &mut TextState, part: &OpenCodePart) {
    if part.part_type != "text" && part.part_type != "reasoning" {
        return;
    }
    let Some(text) = part.text.as_deref() else {
        return;
    };
    let previous = state.emitted_text_by_part_id.get(&part.id).cloned();
    let next = merge_open_code_assistant_text(previous.as_deref(), text);
    state
        .emitted_text_by_part_id
        .insert(part.id.clone(), next.latest_text);
    if let Some(mapped) = text_delta_event(part, &next.delta_to_emit) {
        state.outbox.push(mapped);
    }
}

/// `textPartRole`.
fn text_part_role(state: &TextState, part: &OpenCodePart) -> Option<Role> {
    part.message_id
        .as_ref()
        .and_then(|id| state.message_role_by_id.get(id).copied())
}

/// `parseTextPart`: a text or reasoning part, or `None`.
fn parse_text_part(value: Option<&Value>) -> Option<OpenCodePart> {
    let record = value.and_then(Value::as_object)?;
    let record = Some(record);
    let id = string_field(record, "id")?;
    let part_type = string_field(record, "type")?;
    if part_type != "text" && part_type != "reasoning" {
        return None;
    }
    let time = record_field(record, "time");
    let start = field(time, "start").and_then(Value::as_f64);
    let end = field(time, "end").and_then(Value::as_f64);
    Some(OpenCodePart {
        id: id.into(),
        part_type: part_type.into(),
        message_id: string_field(record, "messageID").map(str::to_string),
        text: field(record, "text")
            .and_then(Value::as_str)
            .map(str::to_string),
        time: (start.is_some() || end.is_some()).then_some(PartTime { start, end }),
        ..Default::default()
    })
}

/// `mergeTextPart`: a stale snapshot never replaces a completed one.
fn merge_text_part(previous: Option<&OpenCodePart>, next: OpenCodePart) -> OpenCodePart {
    let Some(previous) = previous else {
        return next;
    };
    let ended = |part: &OpenCodePart| part.time.and_then(|time| time.end).is_some();
    if ended(previous) && !ended(&next) {
        return previous.clone();
    }
    let text = merge_open_code_assistant_text(
        previous.text.as_deref(),
        next.text.as_deref().unwrap_or_default(),
    )
    .latest_text;
    OpenCodePart {
        text: Some(text),
        time: next.time.or(previous.time),
        ..next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::event_sink;
    use crate::providers::opencode::test_support::{FakeHost, path_of, wait_for};
    use serde_json::json;

    struct Harness {
        host: FakeHost,
        text: OpenCodeText,
        events: Arc<Mutex<Vec<HarnessEvent>>>,
        injected: usize,
    }

    impl Harness {
        fn new() -> Self {
            let host = FakeHost::new();
            host.respond_with(|request| {
                if request.method == "POST" && path_of(&request.url) == "/session" {
                    return (200, r#"{"id":"text_session"}"#.into());
                }
                (204, String::new())
            });
            let text = OpenCodeText::new(host.children(), SharedCatalog::new(), host.spawner());
            Self {
                host,
                text,
                events: Arc::new(Mutex::new(Vec::new())),
                injected: 0,
            }
        }

        fn run(&self) -> smol::Task<Result<String>> {
            let text = self.text.clone();
            let events = self.events.clone();
            smol::spawn(async move {
                text.run(TextPromptInput {
                    cwd: "/repo".into(),
                    model: Some("openrouter/anthropic/claude-haiku".into()),
                    prompt: "question".into(),
                    on_event: Some(event_sink(move |event| events.lock().push(event))),
                    ..Default::default()
                })
                .await
            })
        }

        fn sse(&mut self, event: Value) {
            self.host.sse(TEXT_CHILD_ID, event);
            self.injected += 1;
        }

        fn message(&mut self, id: &str, role: &str) {
            self.sse(json!({
                "type": "message.updated",
                "properties": { "info": { "id": id, "role": role, "sessionID": "text_session" } },
            }));
        }

        fn part(&mut self, message_id: &str, text: &str, ended: bool) {
            let time = if ended {
                json!({ "start": 1, "end": 2 })
            } else {
                json!({ "start": 1 })
            };
            self.sse(json!({
                "type": "message.part.updated",
                "properties": { "part": {
                    "id": format!("part_{message_id}"),
                    "messageID": message_id,
                    "sessionID": "text_session",
                    "type": "text",
                    "text": text,
                    "time": time,
                } },
            }));
        }

        fn delta(&mut self, message_id: &str, text: &str) {
            self.sse(json!({
                "type": "message.part.delta",
                "properties": {
                    "sessionID": "text_session",
                    "partID": format!("part_{message_id}"),
                    "delta": text,
                },
            }));
        }

        /// Wait for the stream to handle every frame sent so far, then let
        /// the prompt reply with `text`.
        async fn finish(
            &self,
            reply: futures::channel::oneshot::Sender<(u16, String)>,
            text: &str,
        ) {
            let injected = self.injected;
            wait_for("events handled", || {
                self.text.processed_events() >= injected
            })
            .await;
            let body = json!({ "info": {}, "parts": [{ "type": "text", "text": text }] });
            reply.send((200, body.to_string())).unwrap();
        }

        fn deltas(&self) -> Vec<HarnessEvent> {
            self.events.lock().clone()
        }
    }

    fn delta(text: &str) -> HarnessEvent {
        HarnessEvent::MessageDelta { text: text.into() }
    }

    async fn prompt_started(harness: &Harness) {
        wait_for("prompt", || {
            !harness
                .host
                .calls_to("/session/text_session/message")
                .is_empty()
        })
        .await;
    }

    #[test]
    fn forwards_only_incremental_opencode_assistant_text() {
        smol::block_on(async {
            let mut harness = Harness::new();
            let reply = harness.host.defer("POST", "/session/text_session/message");
            let result = harness.run();
            prompt_started(&harness).await;
            harness.message("user_message", "user");
            harness.part("user_message", "question", false);
            harness.message("assistant_message", "assistant");
            harness.part("assistant_message", "Hel", false);
            harness.part("assistant_message", "Hello", false);
            harness.finish(reply, "Hello").await;

            assert_eq!(result.await.unwrap(), "Hello");
            assert_eq!(harness.deltas(), vec![delta("Hel"), delta("lo")]);
        });
    }

    #[test]
    fn does_not_replay_a_delta_after_a_stale_snapshot() {
        smol::block_on(async {
            let mut harness = Harness::new();
            let reply = harness.host.defer("POST", "/session/text_session/message");
            let result = harness.run();
            prompt_started(&harness).await;
            harness.message("assistant_message", "assistant");
            harness.part("assistant_message", "Hello", false);
            harness.part("assistant_message", "Hel", false);
            harness.delta("assistant_message", "!");
            harness.message("assistant_message", "assistant");
            harness.finish(reply, "Hello!").await;

            assert_eq!(result.await.unwrap(), "Hello!");
            assert_eq!(harness.deltas(), vec![delta("Hello"), delta("!")]);
        });
    }

    #[test]
    fn does_not_replay_a_delta_after_an_out_of_order_completed_snapshot() {
        smol::block_on(async {
            let mut harness = Harness::new();
            let reply = harness.host.defer("POST", "/session/text_session/message");
            let result = harness.run();
            prompt_started(&harness).await;
            harness.message("assistant_message", "assistant");
            harness.part("assistant_message", "Hello", true);
            harness.part("assistant_message", "", false);
            harness.delta("assistant_message", "lo");
            harness.finish(reply, "Hello").await;

            assert_eq!(result.await.unwrap(), "Hello");
            assert_eq!(harness.deltas(), vec![delta("Hello")]);
        });
    }

    #[test]
    fn buffers_a_delta_that_arrives_before_its_part_snapshot() {
        smol::block_on(async {
            let mut harness = Harness::new();
            let reply = harness.host.defer("POST", "/session/text_session/message");
            let result = harness.run();
            prompt_started(&harness).await;
            harness.delta("assistant_message", "Hel");
            harness.message("assistant_message", "assistant");
            harness.part("assistant_message", "", false);
            harness.delta("assistant_message", "lo");
            harness.part("assistant_message", "Hello", true);
            harness.finish(reply, "Hello").await;

            assert_eq!(result.await.unwrap(), "Hello");
            assert_eq!(harness.deltas(), vec![delta("Hel"), delta("lo")]);
        });
    }

    #[test]
    fn sends_the_model_agent_and_a_deny_all_session() {
        smol::block_on(async {
            let harness = Harness::new();
            let reply = harness.host.defer("POST", "/session/text_session/message");
            let result = harness.run();
            prompt_started(&harness).await;
            harness.finish(reply, "  Title  ").await;
            assert_eq!(result.await.unwrap(), "Title");

            let create = &harness.host.calls_to("/session?")[0];
            assert_eq!(
                create.body.as_deref(),
                Some(r#"{"permission":[{"permission":"*","pattern":"*","action":"deny"}]}"#)
            );
            let prompt = &harness.host.calls_to("/session/text_session/message")[0];
            assert_eq!(
                prompt.body.as_deref(),
                Some(
                    r#"{"model":{"providerID":"openrouter","modelID":"anthropic/claude-haiku"},"agent":"build","parts":[{"type":"text","text":"question"}]}"#
                )
            );
            assert_eq!(prompt.timeout_ms, Some(REQUEST_TIMEOUT_MS));
            // The server is dropped after every prompt.
            assert!(
                !harness
                    .host
                    .calls_to("/session/text_session/abort")
                    .is_empty()
            );
            assert!(harness.host.kills().contains(&TEXT_CHILD_ID.to_string()));
        });
    }

    #[test]
    fn rejects_a_model_error_and_empty_output() {
        smol::block_on(async {
            let harness = Harness::new();
            let reply = harness.host.defer("POST", "/session/text_session/message");
            let result = harness.run();
            prompt_started(&harness).await;
            let body = json!({ "info": { "error": { "message": "Quota exceeded" } }, "parts": [] });
            reply.send((200, body.to_string())).unwrap();
            assert_eq!(result.await.unwrap_err().to_string(), "Quota exceeded");

            let reply = harness.host.defer("POST", "/session/text_session/message");
            let result = harness.run();
            wait_for("second prompt", || {
                harness.host.calls_to("/session/text_session/message").len() == 2
            })
            .await;
            reply
                .send((200, r#"{"info":{},"parts":[]}"#.into()))
                .unwrap();
            assert_eq!(
                result.await.unwrap_err().to_string(),
                "OpenCode returned empty output."
            );
        });
    }

    #[test]
    fn refuses_an_opencode_older_than_the_minimum() {
        smol::block_on(async {
            let harness = Harness::new();
            harness.host.set_exec_output("opencode 1.14.18");
            let error = harness.run().await.unwrap_err();
            assert_eq!(
                error.to_string(),
                "OpenCode v1.14.18 is too old for text generation."
            );
            assert!(harness.host.spawns().is_empty());
        });
    }

    #[test]
    fn picks_text_models_from_the_request_or_the_catalog() {
        let host = FakeHost::new();
        let text = OpenCodeText::new(host.children(), SharedCatalog::new(), host.spawner());
        let slug = |provider: &str, model: &str| ParsedOpenCodeModelSlug {
            provider_id: provider.into(),
            model_id: model.into(),
        };
        assert_eq!(
            text.pick_text_model(Some("opencode:openai/gpt-5.4")),
            slug("openai", "gpt-5.4")
        );
        assert_eq!(
            text.pick_text_model(Some(" kimi ")),
            slug("opencode", "kimi")
        );
        assert_eq!(text.pick_text_model(None), slug("opencode", "glm-5"));
        assert_eq!(
            get_open_code_text_response(Some(&[
                json!({ "type": "text", "text": " a" }),
                json!({ "type": "reasoning", "text": "x" }),
                json!({ "type": "text", "text": "b " }),
            ])),
            "ab"
        );
        assert_eq!(model_settings_key(None), "{}");
    }
}
