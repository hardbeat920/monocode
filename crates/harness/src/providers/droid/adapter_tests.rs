//! Port of droidLive.test.ts. The TypeScript mocked `core/child` and the
//! catalog module; these run the real adapter over `core::testing::Fake`
//! with a scripted ACP peer, and see the catalog probe as its spawn.

use super::*;
use crate::core::testing::Call;
use crate::providers::grok::test_support::{Events, Peer, fake_resolving, settings, turn};

fn config(model: &str, efforts: &[&str], effort: &str) -> Value {
    json!([
        {
            "id": "autonomy_level",
            "category": "mode",
            "currentValue": "normal",
            "options": [
                { "value": "normal", "name": "Auto (Off)" },
                { "value": "spec", "name": "Spec" },
                { "value": "auto-low", "name": "Auto (Low)" },
            ],
        },
        {
            "id": "model",
            "category": "model",
            "currentValue": model,
            "options": [
                { "value": "gpt-6-luna", "name": "GPT-6 Luna" },
                { "value": "claude-opus-5-5", "name": "Opus 5.5" },
            ],
        },
        {
            "id": "reasoning_effort",
            "category": "thought_level",
            "currentValue": effort,
            "options": efforts.iter().map(|value| json!({ "value": value, "name": value })).collect::<Vec<_>>(),
        },
    ])
}

fn update(peer: &Peer, session_id: &str, update: Value) {
    peer.notify(
        session_id,
        "session/update",
        json!({ "sessionId": "droid-session-1", "update": update }),
    );
}

async fn start_session(peer: &Peer, session_id: &str) {
    peer.answer(session_id, "initialize", json!({ "protocolVersion": 1 }))
        .await;
    peer.answer(
        session_id,
        "session/new",
        json!({
            "sessionId": "droid-session-1",
            "models": { "currentModelId": "gpt-6-luna", "availableModels": [] },
            "configOptions": config("gpt-6-luna", &["none", "low", "medium"], "medium"),
        }),
    )
    .await;
}

fn start(adapter: &DroidAdapter, input: SendTurnInput, events: &Events) -> smol::Task<Result<()>> {
    let adapter = adapter.clone();
    let sink = events.sink();
    smol::spawn(async move { adapter.send_turn(input, sink, None).await })
}

fn probe_spawns(peer: &Peer) -> usize {
    peer.calls()
        .iter()
        .filter(|call| matches!(call, Call::Spawn(request) if request.session_id.starts_with("monocode-droid-probe-")))
        .count()
}

#[test]
fn spawns_droid_acp_switches_model_then_effort_sets_autonomy_and_prompts() {
    smol::block_on(async {
        let peer = Peer::with_fake(fake_resolving(HarnessId::Droid, "/fake/droid"));
        let adapter = DroidAdapter::new(&peer.ctx);
        let events = Events::default();
        let running = start(
            &adapter,
            turn(
                "droid-live-new",
                "droid:claude-opus-5-5",
                RuntimeMode::AutoAcceptEdits,
                "inspect this",
                Some(settings(&[("effort", "xhigh")])),
            ),
            &events,
        );
        start_session(&peer, "droid-live-new").await;
        assert_eq!(
            peer.spawned()[0],
            (
                "/fake/droid".to_string(),
                vec!["exec".to_string(), "--output-format".into(), "acp".into()]
            )
        );

        let set_model = peer
            .next("session/set_config_option", |message| {
                message["params"]["configId"] == "model"
            })
            .await;
        assert_eq!(set_model["params"]["value"], "claude-opus-5-5");
        // Droid answers with `{}` and announces the new per-model levels.
        update(
            &peer,
            "droid-live-new",
            json!({
                "sessionUpdate": "config_option_update",
                "configOptions": config("claude-opus-5-5", &["low", "high", "xhigh", "max"], "high"),
            }),
        );
        peer.reply("droid-live-new", &set_model["id"], json!({}));

        let set_effort = peer
            .next("session/set_config_option", |message| {
                message["params"]["configId"] == "reasoning_effort"
            })
            .await;
        assert_eq!(set_effort["params"]["value"], "xhigh");
        peer.reply("droid-live-new", &set_effort["id"], json!({}));

        let set_mode = peer.next("session/set_mode", |_| true).await;
        assert_eq!(set_mode["params"]["modeId"], "auto-low");
        peer.reply("droid-live-new", &set_mode["id"], json!({}));

        let prompt = peer.next("session/prompt", |_| true).await;
        assert_eq!(
            prompt["params"],
            json!({
                "sessionId": "droid-session-1",
                "prompt": [{ "type": "text", "text": "inspect this" }],
            })
        );
        update(
            &peer,
            "droid-live-new",
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "done" } }),
        );
        peer.reply(
            "droid-live-new",
            &prompt["id"],
            json!({ "stopReason": "end_turn" }),
        );

        running.await.unwrap();
        let all = events.all();
        assert!(all.contains(&HarnessEvent::SessionProviderBound {
            provider_session_id: "droid-session-1".into()
        }));
        assert!(all.contains(&HarnessEvent::MessageDelta {
            text: "done".into()
        }));
        // The first live session kicks off the effort probe.
        peer.wait_for("catalog probe", |peer| probe_spawns(peer) == 1)
            .await;
        adapter.stop_session("droid-live-new".into()).await.unwrap();
    });
}

#[test]
fn loads_a_bound_droid_session_instead_of_creating_a_new_one() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = DroidAdapter::new(&peer.ctx);
        adapter.bind_session("droid-live-load", "persisted-session", "/repo", None);
        let events = Events::default();
        let running = start(
            &adapter,
            turn(
                "droid-live-load",
                "droid:gpt-6-luna",
                RuntimeMode::Supervised,
                "continue",
                None,
            ),
            &events,
        );
        peer.answer(
            "droid-live-load",
            "initialize",
            json!({ "protocolVersion": 1 }),
        )
        .await;
        let load = peer.next("session/load", |_| true).await;
        assert_eq!(load["params"]["sessionId"], "persisted-session");
        peer.reply(
            "droid-live-load",
            &load["id"],
            json!({
                "models": { "currentModelId": "gpt-6-luna" },
                "configOptions": config("gpt-6-luna", &["low"], "low"),
            }),
        );
        let set_mode = peer.next("session/set_mode", |_| true).await;
        assert_eq!(set_mode["params"]["modeId"], "normal");
        peer.reply("droid-live-load", &set_mode["id"], json!({}));
        peer.answer(
            "droid-live-load",
            "session/prompt",
            json!({ "stopReason": "end_turn" }),
        )
        .await;

        running.await.unwrap();
        assert!(peer.find("session/new").is_none());
        assert!(peer.find("session/set_config_option").is_none());
        adapter
            .stop_session("droid-live-load".into())
            .await
            .unwrap();
    });
}

#[test]
fn asks_monocode_before_running_a_command_in_supervised_mode() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = DroidAdapter::new(&peer.ctx);
        let events = Events::default();
        let running = start(
            &adapter,
            turn(
                "droid-live-approval",
                "droid:gpt-6-luna",
                RuntimeMode::Supervised,
                "run tests",
                None,
            ),
            &events,
        );
        start_session(&peer, "droid-live-approval").await;
        peer.answer("droid-live-approval", "session/set_mode", json!({}))
            .await;
        let prompt = peer.next("session/prompt", |_| true).await;

        peer.line(
            "droid-live-approval",
            json!({
                "jsonrpc": "2.0",
                "id": 900,
                "method": "session/request_permission",
                "params": {
                    "sessionId": "droid-session-1",
                    "toolCall": {
                        "toolCallId": "call-1",
                        "title": "npm test",
                        "kind": "execute",
                        "rawInput": { "command": "npm test" },
                    },
                    "options": [
                        { "optionId": "proceed_once", "name": "Allow", "kind": "allow_once" },
                        { "optionId": "cancel", "name": "Deny", "kind": "reject_once" },
                    ],
                },
            }),
        );
        let seen = events.clone();
        peer.wait_for("approval.requested", move |_| {
            seen.any(|event| matches!(event, HarnessEvent::ApprovalRequested { .. }))
        })
        .await;
        adapter.respond_approval("droid-live-approval", 900, ApprovalDecision::Allow);
        let answer = |peer: &Peer| {
            peer.sent()
                .into_iter()
                .find(|message| message["id"] == 900 && message.get("result").is_some())
        };
        peer.wait_for("permission reply", |peer| answer(peer).is_some())
            .await;
        assert_eq!(
            answer(&peer).unwrap()["result"]["outcome"]["outcome"],
            "selected"
        );

        peer.reply(
            "droid-live-approval",
            &prompt["id"],
            json!({ "stopReason": "end_turn" }),
        );
        running.await.unwrap();
        adapter
            .stop_session("droid-live-approval".into())
            .await
            .unwrap();
    });
}

#[test]
fn reports_droids_hidden_error_detail_once_without_the_streamed_echo() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = DroidAdapter::new(&peer.ctx);
        let events = Events::default();
        let running = start(
            &adapter,
            turn(
                "droid-live-limit",
                "droid:gpt-6-luna",
                RuntimeMode::Supervised,
                "hi",
                None,
            ),
            &events,
        );
        start_session(&peer, "droid-live-limit").await;
        peer.answer("droid-live-limit", "session/set_mode", json!({}))
            .await;
        let prompt = peer.next("session/prompt", |_| true).await;
        let data = "402 {\"detail\":\"You've reached your 5-hour Droid Core usage limit.\",\"status\":402}";
        update(
            &peer,
            "droid-live-limit",
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": format!("Error: {data}") } }),
        );
        peer.fail(
            "droid-live-limit",
            &prompt["id"],
            json!({ "code": -32603, "message": "Internal error: Agent error", "data": data }),
        );

        assert!(running.await.is_err());
        assert!(!events.any(|event| matches!(event, HarnessEvent::MessageDelta { .. })));
        assert!(events.all().contains(&HarnessEvent::SessionError {
            message: "You've reached your 5-hour Droid Core usage limit.".into()
        }));
    });
}

#[test]
fn registers_once_and_refuses_to_steer() {
    let peer = Peer::new();
    register(&peer.ctx);
    let first = peer.ctx.registry.get_harness(HarnessId::Droid).unwrap();
    register(&peer.ctx);
    let second = peer.ctx.registry.get_harness(HarnessId::Droid).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!second.can_steer());
}
