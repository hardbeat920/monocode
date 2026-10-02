//! Port of src/integrations/harness/providers/pi/piText.ts: one-shot,
//! isolated text prompts (titles, side questions) on a shared `--mode rpc`
//! child per flavor.
//!
//! `stateByFlavor` becomes one [`PiText`] per adapter. Prompts run one at a
//! time, as the TypeScript `turns` promise chain made them.

use std::sync::Arc;
use std::sync::LazyLock;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use futures::channel::oneshot;
use parking_lot::Mutex;
use regex::Regex;
use serde_json::{Value, json};

use monocode_core::block::ModelSettings;
use monocode_core::harness_event::HarnessEvent;
use monocode_core::js;

use crate::core::abort_text_prompt::abort_text_prompt_race;
use crate::core::catalog::SharedCatalog;
use crate::core::child::{ChildEvent, Children};
use crate::core::registry::{EventSink, TextPromptInput};
use crate::core::task::{ms, sleep};

use super::client::{DEFAULT_REQUEST_TIMEOUT_MS, PiRpc};
use super::deps::{Rec, join_stream_text};
use super::flavor::PiFlavor;
use super::protocol::{
    PiDeltaKind, PiSpawnOptions, agent_end_will_retry, as_record, assistant_delta_from_event,
    build_pi_prompt, build_pi_spawn_args, is_agent_settled, is_pi_thinking_level,
};

const INIT_TIMEOUT_MS: i64 = 15_000;
const REQUEST_TIMEOUT_MS: i64 = 45_000;

type TurnResult = std::result::Result<(), String>;

#[derive(Default)]
struct LiveTextState {
    collecting: bool,
    output: String,
    closed: bool,
    /// `turnDone` and `turnFailed`; see the family's `TurnHandle`.
    turn_done: Option<oneshot::Sender<TurnResult>>,
    turn_end_pending: bool,
    on_event: Option<EventSink>,
}

struct LiveText {
    rpc: PiRpc,
    cwd: String,
    model: Option<String>,
    settings_key: String,
    state: Mutex<LiveTextState>,
}

struct TextInner {
    flavor: PiFlavor,
    children: Children,
    catalog: SharedCatalog,
    live: Mutex<Option<Arc<LiveText>>>,
    turns: smol::lock::Mutex<()>,
}

/// The isolated text generator for one flavor. Clones share one generator.
#[derive(Clone)]
pub struct PiText(Arc<TextInner>);

/// `modelSettingsKey`.
fn model_settings_key(settings: Option<&ModelSettings>) -> String {
    let get = |key: &str| settings.and_then(|settings| settings.get(key)).cloned();
    let mut key = serde_json::Map::new();
    if let Some(thinking) = get("thinking") {
        key.insert("thinking".into(), Value::String(thinking));
    }
    if let Some(fast) = get("fast") {
        key.insert("fast".into(), Value::String(fast));
    }
    Value::Object(key).to_string()
}

fn command(value: Value) -> Rec {
    value.as_object().cloned().unwrap_or_default()
}

fn send(sender: Option<oneshot::Sender<TurnResult>>, result: TurnResult) {
    if let Some(sender) = sender {
        let _ = sender.send(result);
    }
}

impl PiText {
    pub fn new(flavor: PiFlavor, children: Children, catalog: SharedCatalog) -> Self {
        Self(Arc::new(TextInner {
            flavor,
            children,
            catalog,
            live: Mutex::new(None),
            turns: smol::lock::Mutex::new(()),
        }))
    }

    /// `pickTextModel`: the requested `provider/model`, else a cheap catalog
    /// model, else the first one.
    fn pick_text_model(&self, requested: Option<&str>) -> Option<String> {
        static CHEAP: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?i-u)haiku|mini|flash|nano|lite|luna").expect("cheap model regex")
        });
        let selected = requested.map(js::trim);
        if let Some(selected) = selected.filter(|selected| selected.contains('/')) {
            return Some(selected.to_string());
        }
        let catalog = self.0.catalog.read();
        let models: Vec<_> = catalog
            .models_for(self.0.flavor.id)
            .iter()
            .filter(|model| {
                model
                    .native_id
                    .as_deref()
                    .is_some_and(|id| id.contains('/'))
            })
            .collect();
        let cheap = models.iter().find(|model| {
            CHEAP.is_match(&format!(
                "{} {} {}",
                model.native_id.as_deref().unwrap_or(""),
                model.name,
                model.id
            ))
        });
        let picked = cheap.or(models.first())?;
        let native = js::trim(picked.native_id.as_deref()?);
        (!native.is_empty()).then(|| native.to_string())
    }

    /// `stopTextPrompt`.
    pub async fn stop_text_prompt(&self) -> Result<()> {
        self.drop_live().await;
        Ok(())
    }

    /// `warmupText`.
    pub async fn warmup_text(&self, cwd: &str) -> Result<()> {
        if cwd.is_empty() || cwd == "~" {
            return Ok(());
        }
        let _turn = self.0.turns.lock().await;
        let _ = self.ensure_live(cwd, None, None).await;
        Ok(())
    }

    /// `runTextPrompt`.
    pub async fn run_text_prompt(&self, input: TextPromptInput) -> Result<String> {
        let _turn = self.0.turns.lock().await;
        self.prompt_on_live(input).await
    }

    async fn prompt_on_live(&self, input: TextPromptInput) -> Result<String> {
        let session = self
            .ensure_live(
                &input.cwd,
                input.model.as_deref(),
                input.model_settings.as_ref(),
            )
            .await?;
        let timeout_ms = input.timeout_ms.unwrap_or(REQUEST_TIMEOUT_MS);
        let result = self.prompt(&session, &input, timeout_ms).await;
        let result = match result {
            Ok(output) => {
                self.drop_live().await;
                Ok(output)
            }
            Err(error) => {
                session.state.lock().turn_done = None;
                let _ = session
                    .rpc
                    .request(
                        command(json!({ "type": "abort" })),
                        DEFAULT_REQUEST_TIMEOUT_MS,
                    )
                    .await;
                self.drop_live().await;
                Err(error)
            }
        };
        {
            let mut state = session.state.lock();
            state.collecting = false;
            state.on_event = None;
            state.turn_done = None;
        }
        result
    }

    /// The `try` block of `promptOnLive`.
    async fn prompt(
        &self,
        session: &Arc<LiveText>,
        input: &TextPromptInput,
        timeout_ms: i64,
    ) -> Result<String> {
        let flavor = self.0.flavor;
        let label = flavor.label;
        let _ = session
            .rpc
            .request(
                command(json!({ "type": "new_session" })),
                DEFAULT_REQUEST_TIMEOUT_MS,
            )
            .await;
        let settings = input.model_settings.as_ref();
        let thinking = settings
            .and_then(|settings| settings.get("thinking"))
            .filter(|level| is_pi_thinking_level(Some(level)))
            .map(String::as_str)
            .unwrap_or("off");
        let _ = session
            .rpc
            .request(
                command(json!({ "type": "set_thinking_level", "level": thinking })),
                DEFAULT_REQUEST_TIMEOUT_MS,
            )
            .await;
        let fast = settings
            .and_then(|settings| settings.get("fast"))
            .map(String::as_str);
        if flavor.is_omp()
            && let Some(fast) = fast.filter(|fast| *fast == "true" || *fast == "false")
        {
            let _ = session
                .rpc
                .request(
                    command(json!({ "type": "set_fast_mode", "enabled": fast == "true" })),
                    DEFAULT_REQUEST_TIMEOUT_MS,
                )
                .await;
        }
        let (sender, receiver) = oneshot::channel::<TurnResult>();
        {
            let mut state = session.state.lock();
            state.output.clear();
            state.collecting = true;
            state.on_event = input.on_event.clone();
            state.turn_end_pending = false;
            state.turn_done = Some(sender);
        }

        let prompt = build_pi_prompt(&input.prompt, &[], false).map_err(|error| anyhow!(error))?;
        session.rpc.request(prompt, timeout_ms).await?;
        let pending = {
            let mut state = session.state.lock();
            if state.turn_end_pending {
                state.turn_end_pending = false;
                state.turn_done.take()
            } else {
                None
            }
        };
        send(pending, Ok(()));

        let turn = async {
            match receiver.await {
                Ok(result) => result.map_err(|error| anyhow!(error)),
                // Dropped without a result: the TypeScript promise never settled.
                Err(_) => futures::future::pending().await,
            }
        };
        let timer = async {
            sleep(ms(timeout_ms)).await;
            Err(anyhow!("{label} text generation timed out"))
        };
        let abort_session = session.clone();
        let race = abort_text_prompt_race(input.signal.as_ref(), move || {
            let failed = abort_session.state.lock().turn_done.take();
            send(failed, Err("By-the-way request cancelled".into()));
            // `void request(abort)`: registered and queued now, never awaited.
            drop(abort_session.rpc.request(
                command(json!({ "type": "abort" })),
                DEFAULT_REQUEST_TIMEOUT_MS,
            ));
            async {}
        });
        let aborted = async move {
            match race {
                Some(race) => Err(race.await),
                None => futures::future::pending().await,
            }
        };
        smol::future::or(turn, smol::future::or(timer, aborted)).await?;

        let mut output = js::trim(&session.state.lock().output).to_string();
        if output.is_empty() {
            let reply = session
                .rpc
                .request(
                    command(json!({ "type": "get_last_assistant_text" })),
                    DEFAULT_REQUEST_TIMEOUT_MS,
                )
                .await
                .ok();
            if let Some(text) = reply
                .as_ref()
                .and_then(|reply| as_record(reply.get("data")))
                .and_then(|data| data.get("text"))
                .and_then(Value::as_str)
            {
                output = js::trim(text).to_string();
            }
        }
        if output.is_empty() {
            return Err(anyhow!("{label} returned empty output."));
        }
        Ok(output)
    }

    async fn ensure_live(
        &self,
        cwd: &str,
        requested_model: Option<&str>,
        model_settings: Option<&ModelSettings>,
    ) -> Result<Arc<LiveText>> {
        let model = self.pick_text_model(requested_model);
        let settings_key = model_settings_key(model_settings);
        let current = self.0.live.lock().clone();
        if let Some(current) = current
            && !current.state.lock().closed
            && current.cwd == cwd
            && current.model == model
            && current.settings_key == settings_key
        {
            return Ok(current);
        }
        self.drop_live().await;
        self.start_live(cwd, model, settings_key).await
    }

    async fn start_live(
        &self,
        cwd: &str,
        model: Option<String>,
        settings_key: String,
    ) -> Result<Arc<LiveText>> {
        let flavor = self.0.flavor;
        let child_id = flavor.text_child_id;
        let path = self.0.children.resolve_binary(flavor.id).await?.path;
        let rpc = PiRpc::new(&self.0.children, child_id, flavor.label);
        let session = Arc::new(LiveText {
            rpc,
            cwd: cwd.to_string(),
            model: model.clone(),
            settings_key,
            state: Mutex::new(LiveTextState::default()),
        });

        let events = self.0.children.watch_child(child_id);
        let pump = session.clone();
        let text = self.clone();
        self.0.children.spawner().spawn(
            async move {
                while let Ok(event) = events.recv().await {
                    match event {
                        ChildEvent::Stdout(line) => {
                            if let Some(rec) = pump.rpc.push_line(&line) {
                                handle_frame(&pump, &rec);
                            }
                        }
                        ChildEvent::Exit(_) => {
                            let exited = format!("{} text generator exited", flavor.label);
                            pump.state.lock().closed = true;
                            {
                                let mut live = text.0.live.lock();
                                if live.as_ref().is_some_and(|live| Arc::ptr_eq(live, &pump)) {
                                    *live = None;
                                }
                            }
                            pump.rpc.close(Some(exited.clone()));
                            let failed = pump.state.lock().turn_done.take();
                            send(failed, Err(exited));
                        }
                        ChildEvent::Stderr(_) => {}
                    }
                }
            }
            .boxed(),
        );

        let started: Result<()> = async {
            let args = build_pi_spawn_args(
                &flavor,
                &PiSpawnOptions {
                    isolated: true,
                    model,
                    ..PiSpawnOptions::default()
                },
            );
            self.0
                .children
                .spawn_child(child_id, &path, args, cwd, None, Some(flavor.id))
                .await?;
            session
                .rpc
                .request(command(json!({ "type": "get_state" })), INIT_TIMEOUT_MS)
                .await?;
            Ok(())
        }
        .await;
        match started {
            Ok(()) => {
                *self.0.live.lock() = Some(session.clone());
                Ok(session)
            }
            Err(error) => {
                session.state.lock().closed = true;
                session.rpc.close(Some(error.to_string()));
                self.0.children.unwatch_child(child_id);
                let _ = self.0.children.kill_child(child_id).await;
                Err(error)
            }
        }
    }

    async fn drop_live(&self) {
        let flavor = self.0.flavor;
        let current = self.0.live.lock().take();
        if let Some(current) = current {
            current.state.lock().closed = true;
            current.rpc.close(None);
            let failed = current.state.lock().turn_done.take();
            send(
                failed,
                Err(format!("{} text generator stopped", flavor.label)),
            );
        }
        self.0.children.unwatch_child(flavor.text_child_id);
        let _ = self.0.children.kill_child(flavor.text_child_id).await;
    }
}

/// `handleFrame` for the text generator.
fn handle_frame(session: &LiveText, rec: &Rec) {
    let mut events = Vec::new();
    let finished = {
        let mut state = session.state.lock();
        if !state.collecting {
            return;
        }
        if let Some(delta) = assistant_delta_from_event(rec) {
            match delta.kind {
                PiDeltaKind::Text => {
                    state.output = join_stream_text(&state.output, &delta.text);
                    events.push(HarnessEvent::MessageDelta { text: delta.text });
                }
                PiDeltaKind::Thinking => {
                    events.push(HarnessEvent::ReasoningDelta { text: delta.text })
                }
            }
        }
        let mut finished = None;
        if is_agent_settled(rec) || agent_end_will_retry(rec) == Some(false) {
            if state.turn_done.is_some() {
                state.turn_end_pending = false;
                finished = state.turn_done.take();
            } else {
                state.turn_end_pending = true;
            }
        }
        let sink = state.on_event.clone();
        (sink, finished)
    };
    let (sink, finished) = finished;
    if let Some(sink) = sink {
        for event in events {
            sink(event);
        }
    }
    send(finished, Ok(()));
}

#[cfg(test)]
mod tests {
    use super::super::flavor::PI_FLAVOR;
    use super::super::testing::{Fake, WriteReply};
    use super::*;
    use crate::core::registry::event_sink;

    #[test]
    fn forwards_pi_text_and_reasoning_deltas_to_an_isolated_prompt() {
        let fake = Fake::new();
        fake.on_write(|responder, session_id, line| {
            let rec: Rec = serde_json::from_str(line).unwrap();
            responder.respond(session_id, &rec, Some(json!({})));
            WriteReply::Ok
        });
        let text = PiText::new(PI_FLAVOR, fake.children.clone(), SharedCatalog::new());
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen = events.clone();
        smol::block_on(async {
            let run = smol::spawn({
                let text = text.clone();
                async move {
                    text.run_text_prompt(TextPromptInput {
                        cwd: "/repo".into(),
                        model: Some("anthropic/claude-haiku".into()),
                        prompt: "question".into(),
                        on_event: Some(event_sink(move |event| seen.lock().push(event))),
                        ..TextPromptInput::default()
                    })
                    .await
                }
            });
            fake.wait_for(|| fake.last_command("monocode-pi-text", "prompt").is_some())
                .await;
            super::super::testing::settle().await;
            let session = "monocode-pi-text";
            fake.frame(
                session,
                json!({ "type": "message_update", "assistantMessageEvent": { "type": "thinking_delta", "delta": "Checking" } }),
            );
            fake.frame(
                session,
                json!({ "type": "message_update", "assistantMessageEvent": { "type": "text_delta", "delta": "Partial answer" } }),
            );
            fake.frame(session, json!({ "type": "agent_settled" }));
            assert_eq!(run.await.unwrap(), "Partial answer");
            text.stop_text_prompt().await.unwrap();
        });
        assert_eq!(
            *events.lock(),
            vec![
                HarnessEvent::ReasoningDelta {
                    text: "Checking".into()
                },
                HarnessEvent::MessageDelta {
                    text: "Partial answer".into()
                },
            ]
        );
        let spawn = fake.spawns().remove(0);
        assert_eq!(
            spawn.args,
            [
                "--mode",
                "rpc",
                "--no-session",
                "--no-extensions",
                "--no-tools",
                "--no-skills",
                "--no-context-files",
                "--model",
                "anthropic/claude-haiku",
            ]
        );
    }

    #[test]
    fn cancels_a_prompt_when_its_signal_aborts() {
        let fake = Fake::new();
        fake.on_write(|responder, session_id, line| {
            let rec: Rec = serde_json::from_str(line).unwrap();
            responder.respond(session_id, &rec, Some(json!({})));
            WriteReply::Ok
        });
        let text = PiText::new(PI_FLAVOR, fake.children.clone(), SharedCatalog::new());
        let signal = crate::core::task::AbortSignal::new();
        smol::block_on(async {
            let run = smol::spawn({
                let text = text.clone();
                let signal = signal.clone();
                async move {
                    text.run_text_prompt(TextPromptInput {
                        cwd: "/repo".into(),
                        model: Some("anthropic/claude-haiku".into()),
                        prompt: "question".into(),
                        signal: Some(signal),
                        ..TextPromptInput::default()
                    })
                    .await
                }
            });
            fake.wait_for(|| fake.last_command("monocode-pi-text", "prompt").is_some())
                .await;
            signal.abort();
            let error = run.await.unwrap_err();
            assert_eq!(error.to_string(), "By-the-way request cancelled");
            assert!(fake.last_command("monocode-pi-text", "abort").is_some());
            assert!(fake.kills().iter().any(|id| id == "monocode-pi-text"));
        });
    }

    #[test]
    fn picks_a_cheap_catalog_model_for_text_jobs() {
        use monocode_core::HarnessId;
        use monocode_core::models::AgentModel;
        let fake = Fake::new();
        let catalog = SharedCatalog::new();
        let text = PiText::new(PI_FLAVOR, fake.children.clone(), catalog.clone());
        assert_eq!(
            text.pick_text_model(Some(" openai/gpt-5 ")).as_deref(),
            Some("openai/gpt-5")
        );
        catalog.set_harness_models(
            HarnessId::Pi,
            vec![
                AgentModel::new("pi:anthropic/opus", HarnessId::Pi, "Opus")
                    .with_native_id("anthropic/opus"),
                AgentModel::new("pi:google/gemini-flash", HarnessId::Pi, "Gemini Flash")
                    .with_native_id("google/gemini-FLASH"),
            ],
        );
        assert_eq!(
            text.pick_text_model(None).as_deref(),
            Some("google/gemini-FLASH")
        );
        assert_eq!(
            text.pick_text_model(Some("default")).as_deref(),
            Some("google/gemini-FLASH")
        );
        catalog.set_harness_models(
            HarnessId::Pi,
            vec![
                AgentModel::new("pi:anthropic/opus", HarnessId::Pi, "Opus")
                    .with_native_id("anthropic/opus"),
            ],
        );
        assert_eq!(
            text.pick_text_model(None).as_deref(),
            Some("anthropic/opus")
        );
    }
}
