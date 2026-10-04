use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use futures::FutureExt;
use futures::channel::oneshot;
use monocode_core::attachment::{Attachment, FOLDER_MIME, prompt_text};
use monocode_core::block::{ApprovalDecided, TurnIntent};
use monocode_core::harness::{HarnessId, RuntimeMode};
use monocode_core::harness_event::{
    ApprovalDecision, CompactContextInput, HarnessEvent, HarnessSessionInput, QuestionDecision,
    RewindLastTurnInput, RewindLastTurnResult, SendTurnInput, SteerTurnInput,
};
use monocode_core::user_question::UserQuestionReply;
use parking_lot::{Mutex, ReentrantMutex};
use serde_json::{Value, json};

use super::super::adapter::open_code_agent_for_turn;
use super::super::git::{
    SharedGitSource, generate_open_code_branch_name, generate_open_code_commit_message,
    generate_open_code_pr_content,
};
use super::super::protocol::{permission_title, tool_kind_from_name};
use super::super::text::TextBackend;
use super::super::title::generate_open_code_session_title;
use super::catalog;
use super::client::{Client, HttpError};
use super::events::Decoder;
use super::forms::Form;
use super::protocol::{model_ref, permission_rules};
use super::server::{Options, Server};
use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildEvent, Children, SseEvent, SseEvents};
use crate::core::registry::{
    AcceptedHook, AdapterCapabilities, EventSink, GeneratedPrContent, HarnessAdapter,
    TextPromptInput, TitleInput,
};
use crate::core::session_title::GeneratedSessionTitle;
use crate::core::task::{AbortSignal, BoxFuture, SharedSpawner, sleep};

struct Approval {
    id: String,
    session: String,
    event: HarnessEvent,
    answering: bool,
}

struct State {
    sink: EventSink,
    decoder: Decoder,
    turn: Option<oneshot::Sender<Result<(), String>>>,
    closed: bool,
    next_ui: i64,
    approvals: BTreeMap<i64, Approval>,
    forms: VecDeque<Form>,
    visible_form: Option<(i64, Form)>,
    submitting_forms: HashSet<String>,
    ready_forms: Vec<Form>,
    outbox: Vec<HarnessEvent>,
}

impl State {
    fn next_question(&mut self) -> Result<()> {
        if self.visible_form.is_some() || !self.submitting_forms.is_empty() {
            return Ok(());
        }
        if let Some(mut form) = self.forms.pop_front() {
            if let Some(question) = form.next_question()? {
                let id = self.next_ui;
                self.next_ui += 1;
                self.outbox.push(HarnessEvent::QuestionAsked {
                    request_id: id,
                    title: Some(form.title.clone()),
                    questions: vec![question],
                    call_id: None,
                    auto_resolve_at: None,
                });
                self.visible_form = Some((id, form));
            } else {
                self.submitting_forms.insert(form.id.clone());
                self.ready_forms.push(form);
            }
        }
        Ok(())
    }

    fn finish(&mut self, result: Result<(), String>) {
        if let Some(turn) = self.turn.take() {
            let _ = turn.send(result);
        }
    }

    fn clear_requests(&mut self) {
        for id in self.approvals.keys() {
            self.outbox.push(HarnessEvent::ApprovalResolved {
                request_id: *id,
                decision: ApprovalDecided::Cancelled,
            });
        }
        self.approvals.clear();
        self.forms.clear();
        self.submitting_forms.clear();
        self.ready_forms.clear();
        if let Some((id, _)) = self.visible_form.take() {
            self.outbox.push(HarnessEvent::QuestionResolved {
                request_id: id,
                decision: QuestionDecision::Cancelled,
            });
        }
    }
}

struct Live {
    server: Server,
    client: Client,
    provider_id: String,
    stream: String,
    state: Mutex<State>,
    order: ReentrantMutex<()>,
    turns: smol::lock::Mutex<()>,
}

impl Live {
    fn with<T>(&self, change: impl FnOnce(&mut State) -> T) -> T {
        let _order = self.order.lock();
        let (result, sink, events) = {
            let mut state = self.state.lock();
            let result = change(&mut state);
            (
                result,
                state.sink.clone(),
                std::mem::take(&mut state.outbox),
            )
        };
        for event in events {
            sink(event);
        }
        result
    }

    fn fail(&self, error: String) {
        self.with(|state| {
            if state.closed {
                return;
            }
            state.outbox.push(HarnessEvent::SessionError {
                message: error.clone(),
            });
            state.clear_requests();
            state.finish(Err(error));
        });
    }
}

struct Inner {
    children: Children,
    catalog: SharedCatalog,
    spawner: SharedSpawner,
    git: Option<SharedGitSource>,
    options: Options,
    live: Mutex<HashMap<String, Arc<Live>>>,
    resumes: Mutex<HashMap<String, String>>,
    cancelled: Mutex<HashSet<String>>,
    text_threads: Mutex<HashSet<String>>,
    setup: smol::lock::Mutex<()>,
    warm: Mutex<Option<Server>>,
    starting: Mutex<HashMap<String, (Server, Option<String>)>>,
}

#[derive(Clone)]
pub struct Adapter {
    inner: Arc<Inner>,
}

impl Adapter {
    pub fn new(
        children: Children,
        catalog: SharedCatalog,
        spawner: SharedSpawner,
        git: Option<SharedGitSource>,
    ) -> Self {
        Self::with_options(children, catalog, spawner, git, Options::default())
    }

    pub fn with_options(
        children: Children,
        catalog: SharedCatalog,
        spawner: SharedSpawner,
        git: Option<SharedGitSource>,
        options: Options,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                children,
                catalog,
                spawner,
                git,
                options,
                live: Mutex::new(HashMap::new()),
                resumes: Mutex::new(HashMap::new()),
                cancelled: Mutex::new(HashSet::new()),
                text_threads: Mutex::new(HashSet::new()),
                setup: smol::lock::Mutex::new(()),
                warm: Mutex::new(None),
                starting: Mutex::new(HashMap::new()),
            }),
        }
    }

    fn live(&self, thread: &str) -> Option<Arc<Live>> {
        self.inner.live.lock().get(thread).cloned()
    }

    async fn ensure(
        &self,
        input: &HarnessSessionInput,
        sink: EventSink,
        readonly: bool,
    ) -> Result<Arc<Live>> {
        let _setup = self.inner.setup.lock().await;
        if let Some(live) = self.live(&input.session_id) {
            if !live.with(|state| state.closed)
                && same_directory(&live.client.directory, &input.cwd)
            {
                live.with(|state| state.sink = sink);
                return Ok(live);
            }
            self.stop(&input.session_id).await;
        }
        let warm = if readonly {
            self.inner.warm.lock().take()
        } else {
            None
        };
        let server = if let Some(server) = warm {
            server
        } else {
            Server::start_for_session(
                self.inner.children.clone(),
                &input.cwd,
                &self.inner.options,
                Some(&input.session_id),
            )
            .await?
        };
        self.inner
            .starting
            .lock()
            .insert(input.session_id.clone(), (server.clone(), None));
        let result = async {
            if self.inner.cancelled.lock().contains(&input.session_id) { bail!("OpenCode startup was cancelled"); }
            let client = server.at(&input.cwd);
            let permissions = if readonly { json!([{"action":"*","resource":"*","effect":"deny"}]) } else { permission_rules(input.runtime_mode) };
            let model = self.model(input)?;
            let agent = open_code_agent_for_turn(input.intent, input.model_settings.as_ref());
            let resumed = self.inner.resumes.lock().get(&input.session_id).cloned();
            let session = if let Some(id) = resumed {
                let existing = client.session(&id).await?;
                if existing.pointer("/location/directory").and_then(Value::as_str).is_some_and(|cwd| !same_directory(cwd, &input.cwd)) {
                    let fork = client.fork(&id).await?;
                    let id = required_id(&fork)?;
                    client.move_to(id).await?;
                    fork
                } else { existing }
            } else {
                client.create_session(json!({"location":{"directory":input.cwd},"model":model,"agent":agent,"permissions":permissions})).await?
            };
            let provider_id = required_id(&session)?.to_string();
            let stream = format!("monocode-opencode-v2-events-{}", uuid::Uuid::new_v4());
            let events = client.subscribe(&stream).await?;
            if let Some((_, opened)) = self.inner.starting.lock().get_mut(&input.session_id) { *opened = Some(stream.clone()); }
            let mut decoder = Decoder::new(provider_id.clone());
            // Seed the durable watermark without replaying persisted transcript blocks.
            for event in client.log(&provider_id, 0).await? { decoder.checkpoint(&event); }
            if self.inner.cancelled.lock().contains(&input.session_id) {
                client.close_events(&stream).await;
                bail!("OpenCode startup was cancelled");
            }
            let live = Arc::new(Live { server: server.clone(), client, provider_id: provider_id.clone(), stream, state: Mutex::new(State { sink, decoder, turn: None, closed: false, next_ui: 1, approvals: BTreeMap::new(), forms: VecDeque::new(), visible_form: None, submitting_forms: HashSet::new(), ready_forms: Vec::new(), outbox: Vec::new() }), order: ReentrantMutex::new(()), turns: smol::lock::Mutex::new(()) });
            self.inner.resumes.lock().insert(input.session_id.clone(), provider_id.clone());
            self.inner.live.lock().insert(input.session_id.clone(), live.clone());
            self.pump(&live, events);
            live.with(|state| {
                state.outbox.push(HarnessEvent::SessionProviderBound { provider_session_id: provider_id });
                state.outbox.push(HarnessEvent::SessionStarted);
            });
            Ok(live)
        }.await;
        let starting = self.inner.starting.lock().remove(&input.session_id);
        if result.is_err() {
            if let Some((_, Some(stream))) = starting {
                server.client.close_events(&stream).await;
            }
            server.stop().await;
        }
        result
    }

    fn model(&self, input: &HarnessSessionInput) -> Result<Value> {
        let native = self.inner.catalog.read().native_model_id_for(&input.model);
        let variant = input
            .model_settings
            .as_ref()
            .and_then(|settings| settings.get("variant"))
            .map(String::as_str);
        model_ref(&native, variant)
    }

    async fn configure(
        &self,
        live: &Live,
        input: &HarnessSessionInput,
        readonly: bool,
    ) -> Result<()> {
        let rules = if readonly {
            json!([{"action":"*","resource":"*","effect":"deny"}])
        } else {
            permission_rules(input.runtime_mode)
        };
        live.client
            .update_session(&live.provider_id, json!({"permissions":rules}))
            .await?;
        live.client
            .switch_model(&live.provider_id, self.model(input)?)
            .await?;
        let window = self.inner.catalog.read().model_context_window(&input.model);
        live.with(|state| state.decoder.context_window = window);
        live.client
            .switch_agent(
                &live.provider_id,
                &open_code_agent_for_turn(input.intent, input.model_settings.as_ref()),
            )
            .await?;
        Ok(())
    }

    async fn send(
        &self,
        input: SendTurnInput,
        sink: EventSink,
        accepted: Option<AcceptedHook>,
        readonly: bool,
    ) -> Result<()> {
        let id = format!("msg_{}", uuid::Uuid::new_v4().simple());
        let body = prompt_body(
            &id,
            &input.text,
            input.attachments.as_deref().unwrap_or_default(),
            "steer",
        )?;
        self.inner
            .cancelled
            .lock()
            .remove(&input.session.session_id);
        let live = self.ensure(&input.session, sink, readonly).await?;
        let _turn = live.turns.lock().await;
        self.configure(&live, &input.session, readonly).await?;
        if self
            .inner
            .cancelled
            .lock()
            .contains(&input.session.session_id)
        {
            return Ok(());
        }
        let (done, receiver) = oneshot::channel();
        live.with(|state| {
            state
                .decoder
                .begin(input.session.intent == Some(TurnIntent::Plan));
            state.turn = Some(done);
        });
        if let Err(error) = live.client.prompt(&live.provider_id, body).await {
            live.with(|state| state.finish(Err(error.to_string())));
            return Err(error);
        }
        if let Some(accepted) = accepted {
            accepted();
        }
        live.with(|state| {
            state.outbox.push(HarnessEvent::TurnStarted {
                provider_turn_id: id,
            })
        });
        receiver
            .await
            .map_err(|_| anyhow!("OpenCode 2 turn closed before completion"))?
            .map_err(anyhow::Error::msg)
    }

    fn pump(&self, live: &Arc<Live>, mut events: SseEvents) {
        let adapter = self.clone();
        let exit_live = live.clone();
        let live = live.clone();
        self.inner.spawner.spawn(async move {
            loop {
                if live.with(|state| state.closed) { break; }
                match events.recv().await {
                    Ok(SseEvent::Data(data)) => if let Ok(event) = serde_json::from_str::<Value>(&data) { adapter.event(&live, &event); },
                    _ => {
                        if live.with(|state| state.closed) { break; }
                        let mut recovered = None;
                        for attempt in 0..3 {
                            sleep(Duration::from_millis(100 * (attempt + 1))).await;
                            if live.with(|state| state.closed) { break; }
                            live.client.close_events(&live.stream).await;
                            if let Ok(next) = live.client.subscribe(&live.stream).await {
                                let sessions = live.with(|state| state.decoder.sessions());
                                let result = async {
                                    for session in sessions {
                                        let after = live.with(|state| state.decoder.sequence.get(&session).copied().unwrap_or_default());
                                        for event in live.client.log(&session, after).await? { adapter.event(&live, &event); }
                                    }
                                    let permissions = live.client.pending_permissions().await?;
                                    for request in permissions.as_array().into_iter().flatten() { adapter.event(&live, &json!({"type":"permission.asked","data":request})); }
                                    let forms = live.client.pending_forms(&live.provider_id).await?;
                                    for form in forms.as_array().into_iter().flatten() { adapter.event(&live, &json!({"type":"form.created","data":{"form":form}})); }
                                    Ok::<_, anyhow::Error>(())
                                }.await;
                                if result.is_ok() { recovered = Some(next); break; }
                            }
                        }
                        if let Some(next) = recovered { events = next; }
                        else {
                            live.fail("OpenCode 2 disconnected and could not recover its event stream".into());
                            live.with(|state| state.closed = true);
                            live.client.close_events(&live.stream).await;
                            live.server.stop().await;
                            break;
                        }
                    }
                }
            }
        }.boxed());
        let live = exit_live;
        self.inner.spawner.spawn(
            async move {
                while let Ok(event) = live.server.events.recv().await {
                    if let ChildEvent::Exit(code) = event {
                        if !live.with(|state| state.closed) {
                            live.fail(format!("OpenCode 2 server exited with code {code:?}"));
                            live.with(|state| {
                                state.closed = true;
                                state.outbox.push(HarnessEvent::SessionEnded {
                                    code: code.map(i64::from),
                                });
                            });
                            live.client.close_events(&live.stream).await;
                        }
                        break;
                    }
                }
            }
            .boxed(),
        );
    }

    fn event(&self, live: &Arc<Live>, event: &Value) {
        let kind = event["type"].as_str().unwrap_or_default();
        let data = &event["data"];
        live.with(|state| {
            if state.closed {
                return;
            }
            match kind {
                "permission.asked"
                    if state.turn.is_some()
                        && state
                            .decoder
                            .belongs(data["sessionID"].as_str().unwrap_or_default()) =>
                {
                    let Some(id) = data["id"].as_str() else {
                        return;
                    };
                    if state.approvals.values().any(|request| request.id == id) {
                        return;
                    }
                    let ui = state.next_ui;
                    state.next_ui += 1;
                    let action = data["action"].as_str().unwrap_or("tool");
                    let resources = data["resources"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>();
                    let event = HarnessEvent::ApprovalRequested {
                        request_id: ui,
                        title: data["message"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| permission_title(action, &resources)),
                        kind: Some(tool_kind_from_name(action)),
                        call_id: data
                            .pointer("/source/id")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        preview: None,
                    };
                    state.approvals.insert(
                        ui,
                        Approval {
                            id: id.into(),
                            session: data["sessionID"].as_str().unwrap().into(),
                            event: event.clone(),
                            answering: false,
                        },
                    );
                    state.outbox.push(event);
                }
                "permission.replied" => {
                    let id = data["requestID"].as_str().unwrap_or_default();
                    if let Some(ui) = state
                        .approvals
                        .iter()
                        .find_map(|(ui, request)| (request.id == id).then_some(*ui))
                    {
                        state.approvals.remove(&ui);
                        state.outbox.push(HarnessEvent::ApprovalResolved {
                            request_id: ui,
                            decision: if data["reply"] == "reject" {
                                ApprovalDecided::Deny
                            } else {
                                ApprovalDecided::Allow
                            },
                        });
                    }
                }
                "form.created" if state.turn.is_some() => {
                    let form = &data["form"];
                    if !state
                        .decoder
                        .belongs(form["sessionID"].as_str().unwrap_or_default())
                    {
                        return;
                    }
                    let id = form["id"].as_str().unwrap_or_default();
                    if state
                        .visible_form
                        .as_ref()
                        .is_some_and(|(_, form)| form.id == id)
                        || state.forms.iter().any(|form| form.id == id)
                        || state.submitting_forms.contains(id)
                    {
                        return;
                    }
                    match Form::new(form) {
                        Ok(form) => {
                            state.forms.push_back(form);
                            if let Err(error) = state.next_question() {
                                state.outbox.push(HarnessEvent::SessionError {
                                    message: error.to_string(),
                                });
                                state.finish(Err(error.to_string()));
                            }
                        }
                        Err(error) => {
                            state.outbox.push(HarnessEvent::SessionError {
                                message: error.to_string(),
                            });
                            state.finish(Err(error.to_string()));
                        }
                    }
                }
                "form.cancelled" | "form.replied" => {
                    let id = data["id"].as_str().unwrap_or_default();
                    state.forms.retain(|form| form.id != id);
                    if state
                        .visible_form
                        .as_ref()
                        .is_some_and(|(_, form)| form.id == id)
                    {
                        let (ui, _) = state.visible_form.take().unwrap();
                        state.outbox.push(HarnessEvent::QuestionResolved {
                            request_id: ui,
                            decision: if kind == "form.replied" {
                                QuestionDecision::Answered
                            } else {
                                QuestionDecision::Cancelled
                            },
                        });
                        let _ = state.next_question();
                    }
                }
                _ if state.turn.is_some() => {
                    let decoded = state.decoder.decode(event);
                    state.outbox.extend(decoded.events);
                    if let Some(done) = decoded.ended {
                        state.clear_requests();
                        state.finish(done);
                    }
                }
                _ => state.decoder.checkpoint(event),
            }
        });
        self.submit_ready_forms(live);
    }

    fn submit_ready_forms(&self, live: &Arc<Live>) {
        for form in live.with(|state| std::mem::take(&mut state.ready_forms)) {
            self.submit_form(live.clone(), form, false);
        }
    }

    fn submit_form(&self, live: Arc<Live>, mut form: Form, skipped: bool) {
        let adapter = self.clone();
        self.inner.spawner.spawn(
            async move {
                let result = if skipped {
                    live.client.cancel_form(&form.session, &form.id).await
                } else {
                    live.client
                        .reply_form(&form.session, &form.id, form.answer())
                        .await
                };
                live.with(|state| {
                    state.submitting_forms.remove(&form.id);
                    if state.closed {
                        return;
                    }
                    if let Err(error) = result {
                        if let Some(invalid) = error
                            .downcast_ref::<HttpError>()
                            .filter(|error| error.tag == "FormInvalidAnswerError")
                        {
                            form.retry_rejected(&invalid.message);
                            state.outbox.push(HarnessEvent::Status {
                                text: invalid.message.clone(),
                            });
                            state.forms.push_front(form);
                        } else {
                            state.outbox.push(HarnessEvent::SessionError {
                                message: error.to_string(),
                            });
                            state.finish(Err(error.to_string()));
                        }
                    }
                    let _ = state.next_question();
                });
                adapter.submit_ready_forms(&live);
            }
            .boxed(),
        );
    }

    async fn stop(&self, thread: &str) {
        let live = self.inner.live.lock().remove(thread);
        if let Some(live) = live {
            live.with(|state| {
                state.closed = true;
                state.clear_requests();
                state.finish(Ok(()));
            });
            live.client.close_events(&live.stream).await;
            live.server.stop().await;
        }
        let starting = self.inner.starting.lock().remove(thread);
        if let Some((server, stream)) = starting {
            if let Some(stream) = stream {
                server.client.close_events(&stream).await;
            }
            server.stop().await;
        }
    }
}

fn required_id(value: &Value) -> Result<&str> {
    value["id"]
        .as_str()
        .filter(|id| id.starts_with("ses_"))
        .ok_or_else(|| anyhow!("OpenCode 2 returned an invalid session identifier"))
}

fn same_directory(a: &str, b: &str) -> bool {
    a.replace('\\', "/").trim_end_matches('/') == b.replace('\\', "/").trim_end_matches('/')
}

pub fn prompt_body(
    id: &str,
    text: &str,
    attachments: &[Attachment],
    delivery: &str,
) -> Result<Value> {
    let mut text = prompt_text(text, attachments);
    let mut files = Vec::new();
    for attachment in attachments {
        if attachment.mime_type == FOLDER_MIME {
            if let Some(path) = &attachment.path {
                text.push_str(&format!("\n\nAttached folder: {path}"));
            }
            continue;
        }
        let uri = if let Some(data) = &attachment.data {
            format!("data:{};base64,{data}", attachment.mime_type)
        } else if let Some(path) = &attachment.path {
            url::Url::from_file_path(path)
                .map_err(|_| anyhow!("Attachment requires an absolute file path"))?
                .to_string()
        } else {
            bail!("Attachment has no file path or inline data");
        };
        files.push(json!({"uri":uri,"name":attachment.name}));
    }
    Ok(json!({"id":id,"text":text,"files":files,"delivery":delivery,"resume":true}))
}

impl HarnessAdapter for Adapter {
    fn id(&self) -> HarnessId {
        HarnessId::Opencode
    }
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            compact_context: true,
            rewind_last_turn: true,
            respond_question: true,
            refresh_catalog: true,
            generate_title: true,
            generate_commit_message: true,
            generate_pr_content: true,
            generate_branch_name: true,
            warmup_text: true,
            run_text_prompt: true,
            stop_text_prompt: true,
            ..Default::default()
        }
    }
    fn send_turn(
        &self,
        input: SendTurnInput,
        sink: EventSink,
        accepted: Option<AcceptedHook>,
    ) -> BoxFuture<'_, Result<()>> {
        self.send(input, sink, accepted, false).boxed()
    }
    fn compact_context(
        &self,
        input: CompactContextInput,
        sink: EventSink,
    ) -> BoxFuture<'_, Result<()>> {
        async move {
            let live = self.ensure(&input, sink, false).await?;
            let _turn = live.turns.lock().await;
            self.configure(&live, &input, false).await?;
            let (sender, receiver) = oneshot::channel();
            live.with(|state| {
                state.turn = Some(sender);
                state.decoder.begin(false);
            });
            if let Err(error) = live.client.compact(&live.provider_id).await {
                live.with(|state| state.finish(Err(error.to_string())));
                return Err(error);
            }
            receiver
                .await
                .map_err(|_| anyhow!("OpenCode compaction ended before completion"))?
                .map_err(anyhow::Error::msg)
        }
        .boxed()
    }
    fn rewind_last_turn(
        &self,
        input: RewindLastTurnInput,
        sink: EventSink,
    ) -> BoxFuture<'_, Result<RewindLastTurnResult>> {
        async move {
            let live = self.ensure(&input.session, sink, false).await?;
            let _turn = live.turns.lock().await;
            let messages = live.client.messages(&live.provider_id).await?;
            let message = match input.provider_turn_id {
                Some(id)
                    if messages
                        .iter()
                        .any(|message| message["id"] == id && message["type"] == "user") =>
                {
                    id
                }
                Some(_) => bail!("OpenCode cannot find the requested user turn"),
                None => messages
                    .iter()
                    .rev()
                    .find(|message| message["type"] == "user")
                    .and_then(|message| message["id"].as_str())
                    .ok_or_else(|| anyhow!("OpenCode has no user turn to rewind"))?
                    .into(),
            };
            live.client
                .stage_revert(&live.provider_id, &message)
                .await?;
            live.client.commit_revert(&live.provider_id).await?;
            Ok(RewindLastTurnResult { submitted: false })
        }
        .boxed()
    }
    fn steer_turn(&self, input: SteerTurnInput) -> BoxFuture<'_, Result<()>> {
        async move {
            let live = self
                .live(&input.session_id)
                .filter(|live| live.with(|state| state.turn.is_some() && !state.closed))
                .ok_or_else(|| anyhow!("OpenCode has no active turn to steer"))?;
            let body = prompt_body(
                &format!("msg_{}", uuid::Uuid::new_v4().simple()),
                &input.text,
                input.attachments.as_deref().unwrap_or_default(),
                "steer",
            )?;
            if body["text"]
                .as_str()
                .is_none_or(|text| text.trim().is_empty())
                && body["files"].as_array().is_none_or(Vec::is_empty)
            {
                return Ok(());
            }
            let native = self.inner.catalog.read().native_model_id_for(&input.model);
            let variant = input
                .model_settings
                .as_ref()
                .and_then(|settings| settings.get("variant"))
                .map(String::as_str);
            live.client
                .switch_model(&live.provider_id, model_ref(&native, variant)?)
                .await?;
            let window = self.inner.catalog.read().model_context_window(&input.model);
            live.with(|state| state.decoder.context_window = window);
            if let Some(agent) = input
                .model_settings
                .as_ref()
                .and_then(|settings| settings.get("agent"))
            {
                live.client.switch_agent(&live.provider_id, agent).await?;
            }
            live.client.prompt(&live.provider_id, body).await?;
            Ok(())
        }
        .boxed()
    }
    fn cancel_turn(&self, thread: String) -> BoxFuture<'_, Result<()>> {
        async move {
            self.inner.cancelled.lock().insert(thread.clone());
            if let Some(live) = self.live(&thread) {
                live.client.interrupt(&live.provider_id).await?;
                live.with(|state| {
                    state.clear_requests();
                    state.finish(Ok(()));
                });
            }
            Ok(())
        }
        .boxed()
    }
    fn respond_approval(&self, thread: &str, ui: i64, decision: ApprovalDecision) {
        let Some(live) = self.live(thread) else {
            return;
        };
        let request = live.with(|state| {
            let request = state.approvals.get_mut(&ui)?;
            if request.answering {
                return None;
            }
            request.answering = true;
            Some((request.session.clone(), request.id.clone()))
        });
        let Some((session, id)) = request else {
            return;
        };
        self.inner.spawner.spawn(
            async move {
                let result = live
                    .client
                    .reply_permission(
                        &session,
                        &id,
                        if decision == ApprovalDecision::Allow {
                            "once"
                        } else {
                            "reject"
                        },
                    )
                    .await;
                live.with(|state| match result {
                    Ok(_) => {
                        if state.approvals.remove(&ui).is_some() {
                            state.outbox.push(HarnessEvent::ApprovalResolved {
                                request_id: ui,
                                decision: if decision == ApprovalDecision::Allow {
                                    ApprovalDecided::Allow
                                } else {
                                    ApprovalDecided::Deny
                                },
                            });
                        }
                    }
                    Err(error) => {
                        if let Some(request) = state.approvals.get_mut(&ui) {
                            request.answering = false;
                            state.outbox.push(HarnessEvent::Status {
                                text: error.to_string(),
                            });
                            state.outbox.push(request.event.clone());
                        }
                    }
                });
            }
            .boxed(),
        );
    }
    fn respond_question(&self, thread: &str, ui: i64, reply: UserQuestionReply) {
        let Some(live) = self.live(thread) else {
            return;
        };
        let pending = live.with(|state| {
            if !state.visible_form.as_ref().is_some_and(|(id, _)| *id == ui) {
                return None;
            }
            let (_, mut form) = state.visible_form.take().unwrap();
            let skipped = matches!(reply, UserQuestionReply::Skipped);
            if !skipped && let Err(error) = form.apply(reply) {
                state.outbox.push(HarnessEvent::Status {
                    text: error.to_string(),
                });
                state.forms.push_front(form);
                let _ = state.next_question();
                return None;
            }
            state.outbox.push(HarnessEvent::QuestionResolved {
                request_id: ui,
                decision: if skipped {
                    QuestionDecision::Skipped
                } else {
                    QuestionDecision::Answered
                },
            });
            if !skipped {
                match form.next_question() {
                    Ok(Some(_)) => {
                        state.forms.push_front(form);
                        let _ = state.next_question();
                        return None;
                    }
                    Err(error) => {
                        state.outbox.push(HarnessEvent::SessionError {
                            message: error.to_string(),
                        });
                        state.finish(Err(error.to_string()));
                        return None;
                    }
                    Ok(None) => {}
                }
            }
            state.submitting_forms.insert(form.id.clone());
            Some((form, skipped))
        });
        self.submit_ready_forms(&live);
        if let Some((form, skipped)) = pending {
            self.submit_form(live, form, skipped);
        }
    }
    fn stop_session(&self, thread: String) -> BoxFuture<'_, Result<()>> {
        async move {
            self.stop(&thread).await;
            Ok(())
        }
        .boxed()
    }
    fn forget_session(&self, thread: String) -> BoxFuture<'_, Result<()>> {
        async move {
            self.stop(&thread).await;
            self.inner.resumes.lock().remove(&thread);
            self.inner.cancelled.lock().remove(&thread);
            Ok(())
        }
        .boxed()
    }
    fn bind_session(&self, thread: &str, provider: &str, _cwd: &str, _account: Option<&str>) {
        self.inner
            .resumes
            .lock()
            .insert(thread.into(), provider.into());
    }
    fn refresh_catalog(&self) -> BoxFuture<'_, Result<()>> {
        async move {
            let directory = self.inner.children.home_dir().await?;
            let models =
                catalog::discover(self.inner.children.clone(), &directory, &self.inner.options)
                    .await?;
            self.inner
                .catalog
                .set_harness_models(HarnessId::Opencode, models);
            Ok(())
        }
        .boxed()
    }
    fn generate_title(
        &self,
        input: TitleInput,
    ) -> BoxFuture<'_, Result<Option<GeneratedSessionTitle>>> {
        async move { Ok(generate_open_code_session_title(self, &input).await) }.boxed()
    }
    fn generate_commit_message(
        &self,
        cwd: String,
        signal: Option<AbortSignal>,
    ) -> BoxFuture<'_, Result<String>> {
        async move {
            generate_open_code_commit_message(self, self.inner.git.as_ref(), &cwd, signal).await
        }
        .boxed()
    }
    fn generate_pr_content(
        &self,
        cwd: String,
    ) -> BoxFuture<'_, Result<Option<GeneratedPrContent>>> {
        async move { generate_open_code_pr_content(self, self.inner.git.as_ref(), &cwd).await }
            .boxed()
    }
    fn generate_branch_name(
        &self,
        cwd: String,
        message: String,
    ) -> BoxFuture<'_, Result<Option<String>>> {
        async move { Ok(generate_open_code_branch_name(self, &cwd, &message).await) }.boxed()
    }
    fn warmup_text(&self, cwd: String) -> BoxFuture<'_, Result<()>> {
        async move {
            let _setup = self.inner.setup.lock().await;
            if self.inner.warm.lock().is_none() {
                let server =
                    Server::start(self.inner.children.clone(), &cwd, &self.inner.options).await?;
                *self.inner.warm.lock() = Some(server);
            }
            Ok(())
        }
        .boxed()
    }
    fn run_text_prompt(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        self.run_text(input)
    }
    fn stop_text_prompt(&self) -> BoxFuture<'_, Result<()>> {
        async move {
            let threads: Vec<_> = self.inner.text_threads.lock().drain().collect();
            for thread in threads {
                self.inner.cancelled.lock().insert(thread.clone());
                self.stop(&thread).await;
                let _ = self.inner.children.kill_child(&thread).await;
                self.inner.resumes.lock().remove(&thread);
            }
            let _setup = self.inner.setup.lock().await;
            let warm = self.inner.warm.lock().take();
            if let Some(server) = warm {
                server.stop().await;
            }
            Ok(())
        }
        .boxed()
    }
}

impl TextBackend for Adapter {
    fn run_text(&self, input: TextPromptInput) -> BoxFuture<'_, Result<String>> {
        async move {
            if let Some(signal) = &input.signal {
                signal.throw_if_aborted()?;
            }
            let thread = format!("monocode-opencode-v2-text-{}", uuid::Uuid::new_v4());
            self.inner.text_threads.lock().insert(thread.clone());
            if let Some(provider) = &input.thread_id {
                self.bind_session(&thread, provider, &input.cwd, None);
            }
            let text = Arc::new(Mutex::new(String::new()));
            let sink = {
                let text = text.clone();
                let original = input.on_event.clone();
                let bound = input.on_thread_id.clone();
                Arc::new(move |event: HarnessEvent| {
                    if let HarnessEvent::MessageDelta { text: delta } = &event {
                        text.lock().push_str(delta);
                    }
                    if let HarnessEvent::SessionProviderBound {
                        provider_session_id,
                    } = &event
                        && let Some(bound) = &bound
                    {
                        bound(provider_session_id.clone());
                    }
                    if let Some(original) = &original {
                        original(event);
                    }
                }) as EventSink
            };
            let model = input.model.clone().unwrap_or_else(|| {
                self.inner
                    .catalog
                    .read()
                    .default_model_id(HarnessId::Opencode)
            });
            let request = SendTurnInput {
                session: HarnessSessionInput {
                    session_id: thread.clone(),
                    cwd: input.cwd.clone(),
                    model,
                    model_settings: input.model_settings.clone(),
                    provider_account_id: None,
                    runtime_mode: RuntimeMode::Supervised,
                    intent: input.intent,
                    controls_agents: None,
                    app_access: None,
                },
                text: input.prompt.clone(),
                attachments: None,
            };
            let run = self.send(request, sink, None, true);
            let deadline = Duration::from_millis(input.timeout_ms.unwrap_or(90_000).max(1) as u64);
            let cancelled = async {
                if let Some(signal) = &input.signal {
                    signal.aborted().await;
                } else {
                    futures::future::pending::<()>().await;
                }
                Err(anyhow!("OpenCode text prompt was aborted"))
            };
            let timeout = async {
                sleep(deadline).await;
                Err(anyhow!("OpenCode text prompt timed out"))
            };
            let result = smol::future::or(run, smol::future::or(cancelled, timeout)).await;
            if result.is_err()
                && let Some(live) = self.live(&thread)
            {
                let _ = live.client.interrupt(&live.provider_id).await;
            }
            self.stop(&thread).await;
            // A timeout can cancel startup before the server has returned its ready record.
            let _ = self.inner.children.kill_child(&thread).await;
            self.inner.text_threads.lock().remove(&thread);
            self.inner.resumes.lock().remove(&thread);
            self.inner.cancelled.lock().remove(&thread);
            result?;
            let text = text.lock().clone();
            if text.trim().is_empty() {
                bail!("OpenCode 2 text prompt returned no assistant text");
            }
            Ok(text)
        }
        .boxed()
    }
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
