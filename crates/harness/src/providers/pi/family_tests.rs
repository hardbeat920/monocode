//! Ports of piLive.test.ts, ompLive.test.ts, and the discovery cases of
//! piSkills.test.ts. The TypeScript "Live" tests mocked `core/child`; these
//! run the real family, client, and router over the fake backend in
//! `testing.rs`, so they need no CLI.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;
use serde_json::{Value, json};

use monocode_core::attachment::{ATTACHMENT_ONLY_PROMPT, Attachment, AttachmentKind};
use monocode_core::block::ModelSettings;
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::{
    CompactContextInput, HarnessEvent, HarnessSessionInput, QuestionDecision, RewindLastTurnInput,
    RewindLastTurnResult, SendTurnInput, SteerTurnInput,
};
use monocode_core::user_question::UserQuestionReply;

use crate::core::native_commands::{CommandContext, NativeCommand};
use crate::core::registry::{AcceptedHook, EventSink, event_sink};

use super::catalog::discover_models;
use super::deps::Rec;
use super::family::PiFamily;
use super::flavor::{OMP_FLAVOR, PI_FLAVOR, PiFlavor};
use super::skills::{discover_omp_commands, discover_pi_skills};
use super::testing::{Fake, Responder, WriteReply, settle, wait_for};

type Handler = Arc<dyn Fn(&Responder, &str, &Rec) + Send + Sync>;

#[derive(Default)]
struct Transport {
    prompt: Option<Handler>,
    fast: Option<Handler>,
    fork_data: Option<Value>,
    state_session_id: Option<String>,
}

/// The ompLive.test.ts transport: answers every command, and lets a test
/// take over `prompt` and `set_fast_mode`.
struct Setup {
    fake: Fake,
    transport: Arc<Mutex<Transport>>,
    pi: PiFamily,
    omp: PiFamily,
    events: Arc<Mutex<Vec<HarnessEvent>>>,
}

fn kind(command: &Rec) -> &str {
    command.get("type").and_then(Value::as_str).unwrap_or("")
}

impl Setup {
    fn new() -> Self {
        let fake = Fake::new();
        let transport = Arc::new(Mutex::new(Transport::default()));
        let state = transport.clone();
        fake.on_write(move |responder, session_id, line| {
            let command: Rec = serde_json::from_str(line).unwrap();
            let (prompt, fast, fork_data, state_session_id) = {
                let transport = state.lock();
                (
                    transport.prompt.clone(),
                    transport.fast.clone(),
                    transport.fork_data.clone(),
                    transport.state_session_id.clone(),
                )
            };
            match kind(&command) {
                "extension_ui_response" => return WriteReply::Ok,
                "prompt" => {
                    if let Some(prompt) = prompt {
                        prompt(responder, session_id, &command);
                    }
                    return WriteReply::Ok;
                }
                "set_fast_mode" if fast.is_some() => {
                    fast.unwrap()(responder, session_id, &command);
                    return WriteReply::Ok;
                }
                _ => {}
            }
            let data = match kind(&command) {
                "get_state" => json!({ "sessionId": state_session_id.as_deref().unwrap_or("provider-session") }),
                "get_available_commands" => json!({ "commands": [{ "name": "workflow", "source": "custom" }] }),
                "get_fork_messages" => json!({ "messages": [{ "entryId": "entry", "text": "latest" }] }),
                "fork" => fork_data.unwrap_or_else(|| json!({})),
                _ => json!({}),
            };
            responder.respond(session_id, &command, Some(data));
            WriteReply::Ok
        });
        let pi = PiFamily::new(PI_FLAVOR, fake.children.clone(), Default::default());
        let omp = PiFamily::new(OMP_FLAVOR, fake.children.clone(), Default::default());
        Self {
            fake,
            transport,
            pi,
            omp,
            events: Arc::default(),
        }
    }

    fn sink(&self) -> EventSink {
        let events = self.events.clone();
        event_sink(move |event| events.lock().push(event))
    }

    fn events(&self) -> Vec<HarnessEvent> {
        self.events.lock().clone()
    }

    fn family(&self, flavor: &PiFlavor) -> PiFamily {
        if flavor.is_omp() {
            self.omp.clone()
        } else {
            self.pi.clone()
        }
    }

    fn on_prompt(&self, handler: impl Fn(&Responder, &str, &Rec) + Send + Sync + 'static) {
        self.transport.lock().prompt = Some(Arc::new(handler));
    }

    fn frame(&self, session_id: &str, value: Value) {
        self.fake.frame(session_id, value);
    }

    fn prompts(&self, session_id: &str) -> Vec<Rec> {
        self.fake
            .commands(session_id)
            .into_iter()
            .filter(|command| kind(command) == "prompt")
            .collect()
    }

    fn send(
        &self,
        family: &PiFamily,
        input: SendTurnInput,
        accepted: Option<AcceptedHook>,
    ) -> Running {
        let family = family.clone();
        let sink = self.sink();
        let settled = Arc::new(AtomicBool::new(false));
        let done = settled.clone();
        let turn = smol::spawn(async move {
            let result = family.send_turn(input, sink, accepted).await;
            done.store(true, Ordering::SeqCst);
            result.map_err(|error| error.to_string())
        });
        Running {
            turn,
            settled,
            request: Rec::new(),
        }
    }

    /// `started(turnInput)`: send a turn and wait for its prompt.
    async fn started(&self, family: &PiFlavor, input: SendTurnInput) -> Running {
        let session_id = input.session.session_id.clone();
        let before = self.prompts(&session_id).len();
        let mut running = self.send(&self.family(family), input, None);
        wait_for(|| self.prompts(&session_id).len() > before).await;
        running.request = self.prompts(&session_id).pop().unwrap();
        running
    }
}

struct Running {
    turn: smol::Task<Result<(), String>>,
    settled: Arc<AtomicBool>,
    request: Rec,
}

impl Running {
    fn settled(&self) -> bool {
        self.settled.load(Ordering::SeqCst)
    }

    fn id(&self) -> Value {
        self.request.get("id").cloned().unwrap_or(Value::Null)
    }
}

fn session(session_id: &str, model: &str) -> HarnessSessionInput {
    HarnessSessionInput {
        session_id: session_id.into(),
        cwd: "/repo".into(),
        model: model.into(),
        model_settings: None,
        provider_account_id: None,
        runtime_mode: RuntimeMode::Supervised,
        intent: None,
        controls_agents: None,
        app_access: None,
    }
}

/// `input(sessionId, text)`.
fn input(session_id: &str, text: &str) -> SendTurnInput {
    SendTurnInput {
        session: session(session_id, "omp:default"),
        text: text.into(),
        attachments: None,
    }
}

fn pi_input(session_id: &str, text: &str) -> SendTurnInput {
    SendTurnInput {
        session: session(session_id, "pi:default"),
        text: text.into(),
        attachments: None,
    }
}

fn completed(events: &[HarnessEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, HarnessEvent::MessageCompleted))
        .count()
}

fn statuses(events: &[HarnessEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            HarnessEvent::Status { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn run<F: std::future::Future<Output = ()>>(future: F) {
    smol::block_on(future);
}

/// `events.reduce(applyHarnessEvent, newSession(flavor, "/repo"))`.
fn reduce(flavor: &PiFlavor, events: &[HarnessEvent]) -> monocode_core::session::Session {
    let model = format!("{}:default", flavor.id);
    let session = monocode_core::session::Session::blank("s", flavor.id, model, "/repo");
    monocode_core::reducer::apply_harness_events(&session, events)
}

mod pi_live_session {
    use super::*;

    fn pi_setup(state: Value, fail_set_model: bool) -> Setup {
        let setup = Setup::new();
        setup.fake.on_write(move |responder, session_id, line| {
            let command: Rec = serde_json::from_str(line).unwrap();
            match kind(&command) {
                "get_state" => responder.respond(session_id, &command, Some(state.clone())),
                "compact" => responder.respond(
                    session_id,
                    &command,
                    Some(json!({ "estimatedTokensAfter": 32_000 })),
                ),
                "set_model" if fail_set_model => {
                    responder.fail(session_id, &command, "Model unavailable")
                }
                "extension_ui_response" => {}
                _ => responder.respond(session_id, &command, Some(json!({}))),
            }
            WriteReply::Ok
        });
        setup
    }

    fn compact_input(session_id: &str, model: &str) -> CompactContextInput {
        session(session_id, model)
    }

    #[test]
    fn publishes_the_resolved_pi_default_model_for_provider_usage() {
        let setup = pi_setup(
            json!({
                "sessionId": "pi_default",
                "model": { "provider": "openai-codex", "id": "gpt-5.4", "contextWindow": 200_000 },
            }),
            false,
        );
        run(async {
            setup
                .pi
                .compact_context(compact_input("pi-default", "pi:default"), setup.sink())
                .await
                .unwrap();
            assert!(
                setup
                    .events()
                    .contains(&HarnessEvent::SessionConfigChanged {
                        model: Some("pi:openai-codex/gpt-5.4".into()),
                        model_settings: None,
                    })
            );
            setup.pi.stop_session("pi-default").await.unwrap();
        });
    }

    #[test]
    fn does_not_publish_an_intermediate_default_when_explicit_model_selection_fails() {
        let setup = pi_setup(
            json!({
                "sessionId": "pi_explicit",
                "model": { "provider": "anthropic", "id": "claude-sonnet-5", "contextWindow": 200_000 },
            }),
            true,
        );
        run(async {
            let error = setup
                .pi
                .compact_context(
                    compact_input("pi-explicit", "pi:openai-codex/gpt-5.4"),
                    setup.sink(),
                )
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), "Model unavailable");
            assert!(
                !setup
                    .events()
                    .iter()
                    .any(|event| matches!(event, HarnessEvent::SessionConfigChanged { .. }))
            );
            let set_model = setup.fake.last_command("pi-explicit", "set_model").unwrap();
            assert_eq!(set_model.get("provider"), Some(&json!("openai-codex")));
            assert_eq!(set_model.get("modelId"), Some(&json!("gpt-5.4")));
            setup.pi.stop_session("pi-explicit").await.unwrap();
        });
    }

    #[test]
    fn uses_the_compact_rpc_command_and_publishes_the_post_compact_estimate() {
        let setup = pi_setup(
            json!({ "sessionId": "pi_session", "model": { "contextWindow": 200_000 } }),
            false,
        );
        run(async {
            setup
                .pi
                .compact_context(compact_input("pi-compact", "pi:default"), setup.sink())
                .await
                .unwrap();
            assert!(setup.fake.last_command("pi-compact", "compact").is_some());
            assert!(setup.events().contains(&HarnessEvent::Context {
                used: Some(32_000),
                window: Some(200_000)
            }));
            setup.pi.stop_session("pi-compact").await.unwrap();
        });
    }

    #[test]
    fn publishes_readable_ponytail_status_and_extension_notifications() {
        let setup = pi_setup(
            json!({ "sessionId": "pi_session", "model": { "contextWindow": 200_000 } }),
            false,
        );
        run(async {
            setup
                .pi
                .compact_context(compact_input("pi-ansi", "pi:default"), setup.sink())
                .await
                .unwrap();
            setup.frame(
                "pi-ansi",
                json!({
                    "type": "extension_ui_request",
                    "id": "ponytail-status",
                    "method": "setStatus",
                    "statusKey": "ponytail",
                    "statusText": "\u{1b}[38;5;241m○\u{1b}[39m \u{1b}[38;5;244mponytail:\u{1b}[39m \u{1b}[38;5;188m⚡ FULL\u{1b}[0m",
                }),
            );
            setup.frame(
                "pi-ansi",
                json!({
                    "type": "extension_ui_request",
                    "id": "plugin-notify",
                    "method": "notify",
                    "message": "\u{1b}[32mPlugin ready\u{1b}[0m",
                }),
            );
            setup.frame(
                "pi-ansi",
                json!({ "type": "extension_ui_request", "id": "empty-status", "method": "setStatus", "statusText": "\u{1b}[0m" }),
            );
            wait_for(|| statuses(&setup.events()).len() >= 2).await;
            settle().await;
            assert_eq!(
                statuses(&setup.events()),
                ["○ ponytail: ⚡ FULL", "Plugin ready"]
            );
            setup.pi.stop_session("pi-ansi").await.unwrap();
        });
    }
}

mod pi_family_edit_recovery {
    use super::*;

    fn rewind_input() -> RewindLastTurnInput {
        RewindLastTurnInput {
            session: session("omp-test", "omp:default"),
            provider_turn_id: None,
            text: None,
            attachments: None,
        }
    }

    #[test]
    fn reads_fork_cancellation_from_the_rpc_data_payload() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({ "type": "prompt_result", "id": running.id(), "agentInvoked": false }),
            );
            running.turn.await.unwrap();
            setup.transport.lock().fork_data = Some(json!({ "cancelled": true }));
            let error = setup
                .omp
                .rewind_last_turn(rewind_input(), setup.sink())
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), "Edit cancelled");
            setup.omp.forget_session("omp-test").await.unwrap();
        });
    }

    #[test]
    fn rebinds_to_the_provider_session_created_by_fork() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({ "type": "prompt_result", "id": running.id(), "agentInvoked": false }),
            );
            running.turn.await.unwrap();
            setup.transport.lock().state_session_id = Some("forked-provider-session".into());
            let result = setup
                .omp
                .rewind_last_turn(rewind_input(), setup.sink())
                .await
                .unwrap();
            assert_eq!(result, RewindLastTurnResult { submitted: false });
            assert!(
                setup
                    .events()
                    .contains(&HarnessEvent::SessionProviderBound {
                        provider_session_id: "forked-provider-session".into()
                    })
            );
            assert_eq!(
                setup.omp.provider_session_id("omp-test").as_deref(),
                Some("forked-provider-session")
            );
            setup.omp.forget_session("omp-test").await.unwrap();
        });
    }
}

mod omp_command_lifecycle {
    use super::*;

    fn text_result(value: &str) -> Value {
        json!({ "content": [{ "type": "text", "text": value }] })
    }

    #[test]
    fn keeps_a_tool_finished_when_a_progress_update_arrives_after_its_end() {
        for flavor in [PI_FLAVOR, OMP_FLAVOR] {
            let setup = Setup::new();
            let session_id = format!("{}-late-update", flavor.id);
            run(async {
                let model = format!("{}:default", flavor.id);
                let turn_input = SendTurnInput {
                    session: session(&session_id, &model),
                    text: "Run it".into(),
                    attachments: None,
                };
                let running = setup.started(&flavor, turn_input).await;
                let update = json!({ "type": "tool_execution_update", "toolCallId": "sh", "partialResult": text_result("building") });
                setup.frame(
                    &session_id,
                    json!({ "type": "tool_execution_start", "toolCallId": "sh", "toolName": "bash", "args": { "command": "make" } }),
                );
                setup.frame(&session_id, update.clone());
                setup.frame(
                    &session_id,
                    json!({ "type": "tool_execution_end", "toolCallId": "sh", "result": text_result("built"), "isError": false }),
                );
                // omp#12875: steering during bash can deliver an update after the end.
                setup.frame(&session_id, update);
                setup.frame(&session_id, json!({ "type": "agent_end" }));
                running.turn.await.unwrap();
                let updates: Vec<(Option<String>, Option<String>)> = setup
                    .events()
                    .into_iter()
                    .filter_map(|event| match event {
                        HarnessEvent::ToolUpdated {
                            call_id,
                            status,
                            detail,
                            ..
                        } if call_id == "sh" => Some((status, detail)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    updates,
                    [
                        (Some("running".to_string()), None),
                        (Some("running".to_string()), Some("building".to_string())),
                        (Some("completed".to_string()), Some("built".to_string())),
                    ],
                    "{}",
                    flavor.id
                );
                let row = reduce(&flavor, &setup.events())
                    .blocks
                    .into_iter()
                    .find(|block| {
                        block.tool.as_ref().and_then(|tool| tool.call_id.as_deref()) == Some("sh")
                    })
                    .unwrap();
                let tool = row.tool.unwrap();
                assert_eq!(tool.status.as_deref(), Some("completed"));
                assert_eq!(tool.detail.as_deref(), Some("built"));
            });
        }
    }

    #[test]
    fn forwards_extension_result_details_into_the_subagent_trail() {
        for flavor in [PI_FLAVOR, OMP_FLAVOR] {
            let setup = Setup::new();
            let session_id = format!("{}-subagent", flavor.id);
            run(async {
                let model = format!("{}:default", flavor.id);
                let turn_input = SendTurnInput {
                    session: session(&session_id, &model),
                    text: "Investigate auth".into(),
                    attachments: None,
                };
                let running = setup.started(&flavor, turn_input).await;
                setup.frame(
                    &session_id,
                    json!({ "type": "tool_execution_start", "toolCallId": "spawn", "toolName": "task", "args": { "agent": "scout", "task": "Check auth" } }),
                );
                let result = json!({
                    "content": [{ "type": "text", "text": "Found auth" }],
                    "details": { "results": [{
                        "agent": "scout",
                        "task": "Check auth",
                        "exitCode": 0,
                        "messages": [{ "role": "assistant", "content": [{ "type": "text", "text": "Reading auth" }] }],
                    }] },
                });
                setup.frame(&session_id, json!({ "type": "tool_execution_update", "toolCallId": "spawn", "partialResult": result }));
                setup.frame(
                    &session_id,
                    json!({ "type": "tool_execution_end", "toolCallId": "spawn", "result": result, "isError": false }),
                );
                setup.frame(&session_id, json!({ "type": "agent_end" }));
                running.turn.await.unwrap();
                let row = reduce(&flavor, &setup.events())
                    .blocks
                    .into_iter()
                    .find(|block| {
                        block.tool.as_ref().and_then(|tool| tool.call_id.as_deref())
                            == Some("spawn")
                    })
                    .unwrap();
                let steps = &row.agent_run.as_ref().unwrap().steps;
                assert_eq!(steps.len(), 1, "{}", flavor.id);
                assert_eq!(steps[0].kind, monocode_core::block::AgentStepKind::Message);
                assert_eq!(steps[0].text, "Reading auth");
                let tool = row.tool.unwrap();
                assert_eq!(tool.status.as_deref(), Some("completed"));
                assert_eq!(tool.detail.as_deref(), Some("Found auth"));
            });
        }
    }

    #[test]
    fn delivers_attachment_only_prompts_and_steering() {
        for flavor in [PI_FLAVOR, OMP_FLAVOR] {
            let setup = Setup::new();
            let session_id = format!("{}-attachments", flavor.id);
            let attachments = vec![Attachment {
                id: "pdf".into(),
                name: "report.pdf".into(),
                kind: AttachmentKind::File,
                mime_type: "application/pdf".into(),
                size: 100,
                path: Some("/tmp/report.pdf".into()),
                ..Attachment::default()
            }];
            let expected = format!(
                "{ATTACHMENT_ONLY_PROMPT}\n\nAttached file (read from disk): \"/tmp/report.pdf\""
            );
            run(async {
                let model = format!("{}:default", flavor.id);
                let turn_input = SendTurnInput {
                    session: session(&session_id, &model),
                    text: String::new(),
                    attachments: Some(attachments.clone()),
                };
                let running = setup.started(&flavor, turn_input).await;
                assert_eq!(running.request.get("message"), Some(&json!(expected)));
                setup
                    .family(&flavor)
                    .steer_turn(SteerTurnInput {
                        session_id: session_id.clone(),
                        cwd: "/repo".into(),
                        model: model.clone(),
                        model_settings: None,
                        text: String::new(),
                        attachments: Some(attachments.clone()),
                    })
                    .await
                    .unwrap();
                let steer = setup.fake.last_command(&session_id, "steer").unwrap();
                assert_eq!(steer.get("message"), Some(&json!(expected)));
                setup.frame(&session_id, json!({ "type": "agent_end" }));
                running.turn.await.unwrap();
            });
        }
    }

    #[test]
    fn applies_fast_mode_through_omp_rpc_before_prompting() {
        let setup = Setup::new();
        run(async {
            let mut turn_input = input("omp-test", "/workflow foo");
            turn_input.session.model_settings =
                Some(ModelSettings::from([("fast".into(), "true".into())]));
            let running = setup.started(&OMP_FLAVOR, turn_input).await;
            let commands = setup.fake.commands("omp-test");
            let fast = commands
                .iter()
                .position(|command| kind(command) == "set_fast_mode")
                .unwrap();
            let prompt = commands
                .iter()
                .position(|command| kind(command) == "prompt")
                .unwrap();
            assert_eq!(commands[fast].get("enabled"), Some(&json!(true)));
            assert!(fast < prompt);
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
        });
    }

    #[test]
    fn keeps_fast_mode_in_sync_with_omp_config_updates() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({ "type": "config_update", "fastModeEnabled": true }),
            );
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
            assert!(
                setup
                    .events()
                    .contains(&HarnessEvent::SessionConfigChanged {
                        model: None,
                        model_settings: Some(ModelSettings::from([("fast".into(), "true".into())])),
                    })
            );
        });
    }

    #[test]
    fn falls_back_cleanly_when_the_current_model_cannot_use_fast_mode() {
        let setup = Setup::new();
        setup.transport.lock().fast = Some(Arc::new(|responder, session_id, command| {
            responder.fail(
                session_id,
                command,
                "Fast mode is unavailable for the current model.",
            );
        }));
        run(async {
            let mut turn_input = input("omp-test", "/workflow foo");
            turn_input.session.model_settings =
                Some(ModelSettings::from([("fast".into(), "true".into())]));
            let running = setup.started(&OMP_FLAVOR, turn_input).await;
            let fast_requests = setup
                .fake
                .commands("omp-test")
                .into_iter()
                .filter(|command| kind(command) == "set_fast_mode")
                .count();
            assert_eq!(fast_requests, 1);
            let events = setup.events();
            assert!(events.contains(&HarnessEvent::SessionConfigChanged {
                model: None,
                model_settings: Some(ModelSettings::from([("fast".into(), "false".into())])),
            }));
            assert!(events.contains(&HarnessEvent::Status {
                text: "Fast mode is unavailable for the current model.".into()
            }));
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
        });
    }

    #[test]
    fn reflects_command_driven_model_settings_and_session_changes_in_monocode() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({ "type": "config_update", "model": { "provider": "anthropic", "id": "new-model" }, "thinkingLevel": "high" }),
            );
            setup.frame(
                "omp-test",
                json!({ "type": "session_info_update", "sessionId": "new-provider-session", "title": "New session" }),
            );
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
            let events = setup.events();
            let config = events
                .iter()
                .find(|event| matches!(event, HarnessEvent::SessionConfigChanged { .. }))
                .unwrap();
            assert_eq!(
                config,
                &HarnessEvent::SessionConfigChanged {
                    model: Some("omp:anthropic/new-model".into()),
                    model_settings: Some(ModelSettings::from([("thinking".into(), "high".into())])),
                }
            );
            assert!(events.contains(&HarnessEvent::SessionProviderBound {
                provider_session_id: "new-provider-session".into()
            }));
            let mut session = monocode_core::session::Session::blank(
                "s",
                monocode_core::HarnessId::Omp,
                "omp:default",
                "/repo",
            );
            session.model_settings = ModelSettings::from([("existing".into(), "keep".into())]);
            let updated = monocode_core::reducer::apply_harness_event(&session, config);
            assert_eq!(updated.model, "omp:anthropic/new-model");
            assert_eq!(
                updated.model_settings,
                ModelSettings::from([
                    ("existing".into(), "keep".into()),
                    ("thinking".into(), "high".into())
                ])
            );
        });
    }

    #[test]
    fn sends_arguments_unchanged_and_renders_local_output_without_waiting_for_agent_end() {
        let setup = Setup::new();
        setup.on_prompt(|responder, session_id, command| {
            responder.frame(
                session_id,
                json!({ "type": "command_output", "text": "\u{1b}[32mSelected reviewer: careful\u{1b}[0m" }),
            );
            responder.respond(session_id, command, Some(json!({ "agentInvoked": false })));
        });
        run(async {
            setup
                .omp
                .send_turn(input("omp-test", "/workflow foo"), setup.sink(), None)
                .await
                .unwrap();
        });
        assert_eq!(
            setup.prompts("omp-test")[0].get("message"),
            Some(&json!("/workflow foo"))
        );
        let events = setup.events();
        assert!(statuses(&events).contains(&"Selected reviewer: careful".to_string()));
        assert_eq!(completed(&events), 1);
        let spawn = setup.fake.spawns().remove(0);
        assert_eq!(spawn.session_id, "omp-test");
        assert_eq!(spawn.command, "/fake/omp");
        assert_eq!(spawn.args, ["--mode", "rpc"]);
        assert_eq!(spawn.cwd, "/repo");
        assert_eq!(spawn.account, None);
        assert_eq!(spawn.binary_provider, Some(monocode_core::HarnessId::Omp));
    }

    #[test]
    fn reports_when_omp_accepts_a_turn() {
        let setup = Setup::new();
        setup.on_prompt(|responder, session_id, command| {
            responder.respond(session_id, command, Some(json!({ "agentInvoked": false })));
        });
        let accepted = Arc::new(AtomicUsize::new(0));
        let count = accepted.clone();
        run(async {
            let hook: AcceptedHook = Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            });
            setup
                .omp
                .send_turn(input("omp-test", "/workflow foo"), setup.sink(), Some(hook))
                .await
                .unwrap();
        });
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn handles_prompt_result_before_and_after_its_acknowledgement() {
        for before in [true, false] {
            let setup = Setup::new();
            setup.on_prompt(move |responder, session_id, command| {
                if before {
                    responder.frame(
                        session_id,
                        json!({ "type": "prompt_result", "id": command.get("id"), "agentInvoked": false }),
                    );
                }
                responder.respond(session_id, command, None);
            });
            run(async {
                let running = setup
                    .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                    .await;
                if !before {
                    setup.frame(
                        "omp-test",
                        json!({ "type": "prompt_result", "id": running.id(), "agentInvoked": false }),
                    );
                }
                running.turn.await.unwrap();
            });
            assert_eq!(completed(&setup.events()), 1, "before: {before}");
        }
    }

    #[test]
    fn ignores_results_belonging_to_another_request_and_nonterminal_agent_end() {
        let setup = Setup::new();
        setup.on_prompt(|responder, session_id, command| {
            responder.respond(session_id, command, None)
        });
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({ "type": "prompt_result", "id": "another-request", "agentInvoked": false }),
            );
            setup.frame(
                "omp-test",
                json!({ "type": "prompt_result", "id": running.id(), "agentInvoked": true }),
            );
            setup.frame(
                "omp-test",
                json!({ "type": "agent_end", "isTerminal": false }),
            );
            settle().await;
            assert!(!running.settled());
            assert_eq!(completed(&setup.events()), 0);
            setup.frame(
                "omp-test",
                json!({ "type": "agent_end", "isTerminal": true }),
            );
            running.turn.await.unwrap();
        });
    }

    #[test]
    fn keeps_normal_chat_and_agent_invoking_commands_active_until_terminal_completion() {
        let setup = Setup::new();
        setup.on_prompt(|responder, session_id, command| {
            responder.respond(session_id, command, Some(json!({ "agentInvoked": true })))
        });
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "Explain this code"))
                .await;
            assert_eq!(
                running.request.get("message"),
                Some(&json!("Explain this code"))
            );
            settle().await;
            assert!(!running.settled());
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
        });
    }

    #[test]
    fn reports_prompt_errors_delivered_after_the_acknowledgement() {
        let setup = Setup::new();
        setup.on_prompt(|responder, session_id, command| {
            responder.respond(session_id, command, Some(json!({ "agentInvoked": true })))
        });
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({ "type": "response", "command": "prompt", "id": running.id(), "success": false, "error": "Workflow failed" }),
            );
            assert_eq!(running.turn.await.unwrap_err(), "Workflow failed");
            assert!(setup.events().contains(&HarnessEvent::SessionError {
                message: "Workflow failed".into()
            }));
        });
    }

    #[test]
    fn cancels_an_unacknowledged_command_and_ignores_its_late_completion_during_the_next_turn() {
        let setup = Setup::new();
        run(async {
            let first = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.omp.cancel_turn("omp-test").await.unwrap();
            first.turn.await.unwrap();
            setup.fake.clear_requests();
            setup.on_prompt(|responder, session_id, command| {
                responder.respond(session_id, command, None)
            });
            let second = setup.started(&OMP_FLAVOR, input("omp-test", "hello")).await;
            let first_id = first.request.get("id").cloned().unwrap();
            setup.frame(
                "omp-test",
                json!({ "type": "prompt_result", "id": first_id, "agentInvoked": false }),
            );
            setup.frame(
                "omp-test",
                json!({ "type": "response", "command": "prompt", "id": first_id, "success": true, "data": { "agentInvoked": false } }),
            );
            settle().await;
            assert!(!second.settled());
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            second.turn.await.unwrap();
        });
    }

    #[test]
    fn uses_prompt_for_a_slash_command_submitted_while_streaming_without_finishing_the_main_turn() {
        let setup = Setup::new();
        setup.on_prompt(|responder, session_id, command| {
            responder.respond(session_id, command, None)
        });
        run(async {
            let running = setup.started(&OMP_FLAVOR, input("omp-test", "hello")).await;
            setup.on_prompt(|responder, session_id, command| {
                responder.respond(session_id, command, Some(json!({ "agentInvoked": false })))
            });
            setup
                .omp
                .steer_turn(SteerTurnInput {
                    session_id: "omp-test".into(),
                    cwd: "/repo".into(),
                    model: "omp:default".into(),
                    model_settings: None,
                    text: "/usage".into(),
                    attachments: None,
                })
                .await
                .unwrap();
            let last = setup.fake.commands("omp-test").pop().unwrap();
            assert_eq!(kind(&last), "prompt");
            assert_eq!(last.get("message"), Some(&json!("/usage")));
            assert_eq!(last.get("streamingBehavior"), Some(&json!("steer")));
            settle().await;
            assert!(!running.settled());
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
        });
    }

    #[test]
    fn keeps_pis_normal_retry_and_completion_path_intact() {
        let setup = Setup::new();
        setup.on_prompt(|responder, session_id, command| {
            responder.respond(session_id, command, Some(json!({ "agentInvoked": false })))
        });
        run(async {
            let running = setup
                .started(&PI_FLAVOR, pi_input("pi-test", "/skill:architect foo"))
                .await;
            setup.frame("pi-test", json!({ "type": "agent_end", "willRetry": true }));
            settle().await;
            assert!(!running.settled());
            setup.frame("pi-test", json!({ "type": "agent_settled" }));
            running.turn.await.unwrap();
        });
        assert_eq!(completed(&setup.events()), 1);
    }

    #[test]
    fn surfaces_displayed_omp_advisor_notes_and_hides_internal_messages() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            let advisor = json!({
                "role": "custom",
                "customType": "advisor",
                "display": true,
                "content": "raw advisory envelope",
                "details": { "notes": [
                    { "note": "Minor note", "severity": "nit" },
                    { "note": "Stop here", "severity": "blocker" },
                ] },
            });
            setup.frame(
                "omp-test",
                json!({ "type": "message_start", "message": advisor }),
            );
            setup.frame(
                "omp-test",
                json!({ "type": "message_end", "message": advisor }),
            );
            setup.frame(
                "omp-test",
                json!({ "type": "message_start", "message": {
                    "role": "custom", "customType": "xdev-mount-notice", "display": false, "content": "internal mount details"
                } }),
            );
            setup.frame("omp-test", json!({ "type": "advisor_yielded" }));
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
            let events = setup.events();
            let interjections: Vec<&HarnessEvent> = events
                .iter()
                .filter(|event| matches!(event, HarnessEvent::Interjection { .. }))
                .collect();
            assert_eq!(
                interjections,
                [&HarnessEvent::Interjection {
                    text: "Minor note\n\nStop here".into(),
                    custom_type: "advisor".into(),
                    severity: Some(monocode_core::block::InterjectionSeverity::Blocker),
                }]
            );
            assert!(
                !serde_json::to_string(&events)
                    .unwrap()
                    .contains("internal mount details")
            );
        });
    }

    #[test]
    fn emits_a_transient_status_when_the_omp_advisor_yields() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            settle().await;
            let start = setup.events().len();
            setup.frame("omp-test", json!({ "type": "advisor_yielded" }));
            wait_for(|| setup.events().len() > start).await;
            settle().await;
            let tail = setup.events()[start..].to_vec();
            assert_eq!(tail.len(), 1);
            assert!(matches!(tail[0], HarnessEvent::Status { .. }));
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
        });
    }

    #[test]
    fn uses_generic_content_for_other_displayed_omp_custom_messages() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({ "type": "message_start", "message": {
                    "role": "custom",
                    "customType": "extension-notice",
                    "display": true,
                    "content": [
                        { "type": "text", "text": "Extension changed the plan." },
                        { "type": "image", "data": "ignored", "mimeType": "image/png" },
                        { "type": "text", "text": "Review it." },
                    ],
                } }),
            );
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
            assert!(setup.events().contains(&HarnessEvent::Interjection {
                text: "Extension changed the plan.\nReview it.".into(),
                custom_type: "extension-notice".into(),
                severity: None,
            }));
        });
    }

    #[test]
    fn does_not_reinterpret_pi_custom_messages_as_omp_interjections() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&PI_FLAVOR, pi_input("pi-test", "hello"))
                .await;
            setup.frame(
                "pi-test",
                json!({ "type": "message_start", "message": {
                    "role": "custom", "customType": "advisor", "display": true, "content": "Pi-owned custom frame"
                } }),
            );
            setup.frame("pi-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
            assert!(
                !setup
                    .events()
                    .iter()
                    .any(|event| matches!(event, HarnessEvent::Interjection { .. }))
            );
        });
    }
}

mod omp_live_command_inventories {
    use super::*;

    fn names(commands: &[NativeCommand]) -> Vec<&str> {
        commands
            .iter()
            .map(|command| command.name.as_str())
            .collect()
    }

    #[test]
    fn loads_through_the_active_process_and_isolates_push_updates_by_session_and_directory() {
        let setup = Setup::new();
        let provider = setup.omp.command_provider();
        let collect = || {
            let seen: Arc<Mutex<Vec<Vec<NativeCommand>>>> = Arc::default();
            let sink = seen.clone();
            (
                seen,
                Arc::new(move |commands: Vec<NativeCommand>| sink.lock().push(commands)),
            )
        };
        let (a, on_a) = collect();
        let (b, on_b) = collect();
        let (other_cwd, on_other) = collect();
        let context = |session_id: &str, cwd: &str| CommandContext {
            cwd: cwd.into(),
            session_id: Some(session_id.into()),
        };
        let unsubscribe = [
            provider
                .subscribe(context("omp-test", "/repo/"), on_a)
                .unwrap(),
            provider.subscribe(context("other", "/repo"), on_b).unwrap(),
            provider
                .subscribe(context("omp-test", "/elsewhere"), on_other)
                .unwrap(),
        ];
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            let found = provider
                .discover(context("omp-test", "/repo"))
                .await
                .unwrap();
            assert_eq!(names(&found), ["workflow"]);
            setup.frame(
                "omp-test",
                json!({ "type": "available_commands_update", "commands": [{ "name": "review", "source": "extension" }] }),
            );
            setup.frame(
                "omp-test",
                json!({ "type": "available_commands_update", "commands": null }),
            );
            wait_for(|| !a.lock().is_empty()).await;
            settle().await;
            assert_eq!(a.lock().len(), 1);
            assert!(b.lock().is_empty());
            assert!(other_cwd.lock().is_empty());
            let found = provider
                .discover(context("omp-test", "/repo"))
                .await
                .unwrap();
            assert_eq!(names(&found), ["review"]);
            for unsubscribe in unsubscribe {
                unsubscribe();
            }
            setup.frame(
                "omp-test",
                json!({ "type": "available_commands_update", "commands": [] }),
            );
            settle().await;
            assert_eq!(a.lock().len(), 1);
            setup.frame("omp-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
        });
    }
}

mod omp_workflow_dialogs {
    use super::*;

    fn question(events: &[HarnessEvent]) -> Option<(i64, Vec<String>)> {
        events.iter().find_map(|event| match event {
            HarnessEvent::QuestionAsked {
                request_id,
                questions,
                ..
            } => Some((
                *request_id,
                questions[0]
                    .options
                    .iter()
                    .map(|option| option.label.clone())
                    .collect(),
            )),
            _ => None,
        })
    }

    #[test]
    fn returns_the_actual_selected_option_with_its_original_text() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({
                    "type": "extension_ui_request",
                    "id": "choose",
                    "method": "select",
                    "title": "Choose reviewer",
                    "options": ["fast", "\u{1b}[32mcareful\u{1b}[0m"],
                }),
            );
            wait_for(|| question(&setup.events()).is_some()).await;
            let (request_id, labels) = question(&setup.events()).unwrap();
            assert_eq!(labels, ["fast", "careful"]);
            setup.omp.respond_question(
                "omp-test",
                request_id,
                UserQuestionReply::Answered {
                    answers: [("choose".to_string(), vec!["1".to_string()])].into(),
                    custom: None,
                },
            );
            wait_for(|| {
                setup
                    .fake
                    .last_command("omp-test", "extension_ui_response")
                    .is_some()
            })
            .await;
            assert_eq!(
                Value::Object(
                    setup
                        .fake
                        .last_command("omp-test", "extension_ui_response")
                        .unwrap()
                ),
                json!({ "type": "extension_ui_response", "id": "choose", "value": "\u{1b}[32mcareful\u{1b}[0m" })
            );
            setup.frame(
                "omp-test",
                json!({ "type": "prompt_result", "id": running.id(), "agentInvoked": false }),
            );
            running.turn.await.unwrap();
        });
    }

    #[test]
    fn returns_input_and_editor_text_to_the_workflow() {
        for method in ["input", "editor"] {
            let setup = Setup::new();
            run(async {
                let running = setup
                    .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                    .await;
                setup.frame(
                    "omp-test",
                    json!({ "type": "extension_ui_request", "id": "text", "method": method, "title": "Instructions" }),
                );
                wait_for(|| question(&setup.events()).is_some()).await;
                let (request_id, _) = question(&setup.events()).unwrap();
                setup.omp.respond_question(
                    "omp-test",
                    request_id,
                    UserQuestionReply::Answered {
                        answers: Default::default(),
                        custom: Some(
                            [(
                                "text".to_string(),
                                "Review security\nand correctness".to_string(),
                            )]
                            .into(),
                        ),
                    },
                );
                wait_for(|| {
                    setup
                        .fake
                        .last_command("omp-test", "extension_ui_response")
                        .is_some()
                })
                .await;
                let response = setup
                    .fake
                    .last_command("omp-test", "extension_ui_response")
                    .unwrap();
                assert_eq!(
                    response.get("value"),
                    Some(&json!("Review security\nand correctness")),
                    "{method}"
                );
                setup.frame(
                    "omp-test",
                    json!({ "type": "prompt_result", "id": running.id(), "agentInvoked": false }),
                );
                running.turn.await.unwrap();
            });
        }
    }

    #[test]
    fn resolves_pending_questions_when_the_user_stops_a_command() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&OMP_FLAVOR, input("omp-test", "/workflow foo"))
                .await;
            setup.frame(
                "omp-test",
                json!({ "type": "extension_ui_request", "id": "text", "method": "input", "title": "Instructions" }),
            );
            wait_for(|| question(&setup.events()).is_some()).await;
            setup.omp.cancel_turn("omp-test").await.unwrap();
            running.turn.await.unwrap();
            wait_for(|| {
                setup
                    .fake
                    .last_command("omp-test", "extension_ui_response")
                    .is_some()
            })
            .await;
            assert_eq!(
                Value::Object(
                    setup
                        .fake
                        .last_command("omp-test", "extension_ui_response")
                        .unwrap()
                ),
                json!({ "type": "extension_ui_response", "id": "text", "cancelled": true })
            );
            wait_for(|| {
                setup.events().iter().any(|event| {
                    matches!(
                        event,
                        HarnessEvent::QuestionResolved {
                            decision: QuestionDecision::Skipped,
                            ..
                        }
                    )
                })
            })
            .await;
        });
    }
}

mod omp_approvals {
    use super::*;
    use monocode_core::harness_event::ApprovalDecision;

    #[test]
    fn confirms_an_extension_dialog_through_an_approval() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&PI_FLAVOR, pi_input("pi-test", "hello"))
                .await;
            setup.frame(
                "pi-test",
                json!({ "type": "extension_ui_request", "id": "ui-1", "method": "confirm", "title": "Dangerous", "message": "Allow rm?" }),
            );
            let requested = || {
                setup.events().iter().find_map(|event| match event {
                    HarnessEvent::ApprovalRequested {
                        request_id, title, ..
                    } => Some((*request_id, title.clone())),
                    _ => None,
                })
            };
            wait_for(|| requested().is_some()).await;
            let (request_id, title) = requested().unwrap();
            assert_eq!(title, "Dangerous — Allow rm?");
            setup
                .pi
                .respond_approval("pi-test", request_id, ApprovalDecision::Allow);
            wait_for(|| {
                setup
                    .fake
                    .last_command("pi-test", "extension_ui_response")
                    .is_some()
            })
            .await;
            assert_eq!(
                Value::Object(
                    setup
                        .fake
                        .last_command("pi-test", "extension_ui_response")
                        .unwrap()
                ),
                json!({ "type": "extension_ui_response", "id": "ui-1", "confirmed": true })
            );
            setup.frame("pi-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
        });
    }

    #[test]
    fn reports_a_failed_turn_once_no_retry_follows() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&PI_FLAVOR, pi_input("pi-test", "hello"))
                .await;
            setup.frame(
                "pi-test",
                json!({ "type": "message_end", "message": { "role": "assistant", "stopReason": "error", "errorMessage": "first" } }),
            );
            setup.frame("pi-test", json!({ "type": "agent_end", "willRetry": true }));
            setup.frame(
                "pi-test",
                json!({ "type": "message_end", "message": { "role": "assistant", "stopReason": "error" } }),
            );
            setup.frame("pi-test", json!({ "type": "agent_end" }));
            running.turn.await.unwrap();
            let errors: Vec<HarnessEvent> = setup
                .events()
                .into_iter()
                .filter(|event| matches!(event, HarnessEvent::SessionError { .. }))
                .collect();
            assert_eq!(
                errors,
                [HarnessEvent::SessionError {
                    message: "Pi turn failed".into()
                }]
            );
        });
    }

    #[test]
    fn ends_the_session_when_the_child_exits() {
        let setup = Setup::new();
        run(async {
            let running = setup
                .started(&PI_FLAVOR, pi_input("pi-test", "hello"))
                .await;
            setup.fake.responder.exit("pi-test", Some(3));
            assert_eq!(running.turn.await.unwrap_err(), "Pi exited");
            assert!(
                setup
                    .events()
                    .contains(&HarnessEvent::SessionEnded { code: Some(3) })
            );
            assert!(setup.events().contains(&HarnessEvent::SessionError {
                message: "Pi exited".into()
            }));
        });
    }

    #[test]
    fn resumes_a_bound_session_with_each_flavors_flag() {
        let setup = Setup::new();
        setup.on_prompt(|responder, session_id, command| {
            responder.respond(session_id, command, Some(json!({ "agentInvoked": false })))
        });
        setup
            .omp
            .bind_session("omp-test", " stored-session ", "/repo");
        run(async {
            setup
                .omp
                .send_turn(input("omp-test", "/workflow foo"), setup.sink(), None)
                .await
                .unwrap();
        });
        assert_eq!(
            setup.fake.spawns()[0].args,
            ["--mode", "rpc", "--resume", "stored-session"]
        );
    }
}

mod pi_skills_discovery {
    use super::*;

    fn fake_with(data: Value) -> Fake {
        let fake = Fake::new();
        fake.on_write(move |responder, session_id, line| {
            let command: Rec = serde_json::from_str(line).unwrap();
            if kind(&command) != "extension_ui_response" {
                responder.respond(session_id, &command, Some(data.clone()));
            }
            WriteReply::Ok
        });
        fake
    }

    #[test]
    fn queries_omp_in_the_project_with_config_skills_and_extensions_enabled() {
        let fake = fake_with(
            json!({ "commands": [{ "name": "orchestrate", "source": "custom", "description": "Select agents" }] }),
        );
        let commands =
            smol::block_on(discover_omp_commands(&fake.children, "/repo-worktree")).unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "orchestrate");
        assert_eq!(commands[0].invocation, "orchestrate");
        assert_eq!(commands[0].origin.as_deref(), Some("custom"));
        let spawn = fake.spawns().remove(0);
        assert!(spawn.session_id.starts_with("monocode-omp-skills-"));
        assert_eq!(spawn.command, "/fake/omp");
        assert_eq!(spawn.args, ["--mode", "rpc", "--no-session"]);
        assert_eq!(spawn.cwd, "/repo-worktree");
        assert_eq!(spawn.binary_provider, Some(monocode_core::HarnessId::Omp));
        let request = fake.commands(&spawn.session_id).remove(0);
        assert_eq!(kind(&request), "get_available_commands");
    }

    #[test]
    fn loads_skills_through_an_isolated_sessionless_pi_probe() {
        let fake = fake_with(
            json!({ "commands": [{ "name": "skill:architect", "description": "Design first.", "source": "skill" }] }),
        );
        let skills = smol::block_on(discover_pi_skills(&fake.children, "/repo")).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "architect");
        assert_eq!(skills[0].description, "Design first.");
        assert_eq!(skills[0].invocation, "skill:architect");
        assert_eq!(skills[0].source, monocode_core::HarnessId::Pi);
        let spawn = fake.spawns().remove(0);
        assert!(spawn.session_id.starts_with("monocode-pi-skills-"));
        assert_eq!(spawn.command, "/fake/pi");
        assert_eq!(spawn.args, ["--mode", "rpc", "--no-session"]);
        assert_eq!(spawn.cwd, "/repo");
        assert_eq!(kind(&fake.commands(&spawn.session_id)[0]), "get_commands");
        assert_eq!(fake.kills(), [spawn.session_id]);
    }

    #[test]
    fn denies_extension_ui_requests_that_require_a_reply() {
        let fake = Fake::new();
        let answered = Arc::new(AtomicBool::new(false));
        let gate = answered.clone();
        fake.on_write(move |responder, session_id, line| {
            let command: Rec = serde_json::from_str(line).unwrap();
            match kind(&command) {
                "get_commands" => responder.frame(
                    session_id,
                    json!({ "type": "extension_ui_request", "id": "ui-1", "method": "confirm", "title": "Continue?" }),
                ),
                "extension_ui_response" => {
                    gate.store(true, Ordering::SeqCst);
                    responder.frame(
                        session_id,
                        json!({ "type": "response", "id": "mc_1", "command": "get_commands", "success": true, "data": { "commands": [] } }),
                    );
                }
                _ => {}
            }
            WriteReply::Ok
        });
        let skills = smol::block_on(discover_pi_skills(&fake.children, "/repo")).unwrap();
        assert!(skills.is_empty());
        assert!(answered.load(Ordering::SeqCst));
        let spawn = fake.spawns().remove(0);
        let response = fake
            .commands(&spawn.session_id)
            .into_iter()
            .find(|command| kind(command) == "extension_ui_response")
            .unwrap();
        assert_eq!(
            Value::Object(response),
            json!({ "type": "extension_ui_response", "id": "ui-1", "cancelled": true })
        );
    }

    #[test]
    fn cleans_up_after_request_and_response_failures() {
        let fake = Fake::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        fake.on_write(move |responder, session_id, line| {
            let command: Rec = serde_json::from_str(line).unwrap();
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                responder.fail(session_id, &command, "rpc failed");
            } else {
                responder.respond(session_id, &command, Some(json!({})));
            }
            WriteReply::Ok
        });
        smol::block_on(async {
            let error = discover_pi_skills(&fake.children, "/repo")
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), "rpc failed");
            assert_eq!(fake.kills().len(), 1);
            let error = discover_pi_skills(&fake.children, "/repo")
                .await
                .unwrap_err();
            assert!(error.to_string().contains("commands"), "{error}");
            assert_eq!(fake.kills().len(), 2);
        });
    }

    #[test]
    fn uses_a_unique_child_id_for_each_probe() {
        let fake = fake_with(json!({ "commands": [] }));
        smol::block_on(async {
            discover_pi_skills(&fake.children, "/repo").await.unwrap();
            discover_pi_skills(&fake.children, "/repo").await.unwrap();
        });
        let spawns = fake.spawns();
        assert_ne!(spawns[0].session_id, spawns[1].session_id);
    }

    #[test]
    fn discovers_models_with_a_unique_probe() {
        let fake = fake_with(json!({ "models": [] }));
        let models =
            smol::block_on(discover_models(&fake.children, &OMP_FLAVOR, Some("/repo"))).unwrap();
        assert!(models.is_empty());
        assert!(
            fake.spawns()[0]
                .session_id
                .starts_with("monocode-omp-probe-")
        );
    }
}
