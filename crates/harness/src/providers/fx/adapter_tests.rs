//! Port of fxLive.test.ts. The TypeScript mocked `core/child`; these run the
//! real adapter over `core::testing::Fake` with a scripted ACP peer.

use super::*;
use crate::core::testing::Call;
use crate::providers::fx::test_support::{Events, Peer, settings, turn};

fn start(adapter: &FxAdapter, input: SendTurnInput, events: &Events) -> smol::Task<Result<()>> {
    let adapter = adapter.clone();
    let sink = events.sink();
    smol::spawn(async move { adapter.send_turn(input, sink, None).await })
}

fn input(session_id: &str, text: &str) -> SendTurnInput {
    turn(
        session_id,
        "fx:zai/glm-5.2",
        RuntimeMode::Supervised,
        text,
        Some(settings(&[])),
    )
}

#[test]
fn auto_approves_a_permission_request_instead_of_blocking_the_turn() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = FxAdapter::new(&peer.ctx);
        let events = Events::default();

        let turn1 = start(&adapter, input("t1", "hey"), &events);
        peer.answer("t1", "initialize", json!({ "protocolVersion": 1 }))
            .await;
        peer.answer(
            "t1",
            "session/new",
            json!({
                "sessionId": "S1",
                "configOptions": [
                    { "id": "provider", "category": "model", "currentValue": "gateway" },
                    { "id": "model", "category": "model", "currentValue": "zai/glm-5.2" },
                    { "id": "mode", "category": "mode", "currentValue": "ask" },
                ],
            }),
        )
        .await;
        // The model already matches, so set_config_option is skipped.
        peer.answer("t1", "session/set_mode", json!({})).await;
        peer.answer("t1", "session/prompt", json!({ "stopReason": "end_turn" }))
            .await;
        turn1.await.unwrap();
        assert!(peer.find("session/set_config_option").is_none());
        assert_eq!(
            peer.spawned()[0].1,
            vec!["acp".to_string(), "--model".into(), "zai/glm-5.2".into()]
        );

        peer.clear();
        let turn2 = start(&adapter, input("t1", "check the repo"), &events);
        peer.answer("t1", "session/set_mode", json!({})).await;
        let prompt = peer.next("session/prompt", |_| true).await;
        peer.notify(
            "t1",
            "session/update",
            json!({
                "sessionId": "S1",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call_a",
                    "title": "Running",
                    "kind": "execute",
                    "status": "pending",
                },
            }),
        );
        // fx numbers its own requests from 1.
        peer.line(
            "t1",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "session/request_permission",
                "params": {
                    "sessionId": "S1",
                    "toolCall": {
                        "toolCallId": "call_a",
                        "title": "terminal.exec git status -s",
                        "kind": "execute",
                        "status": "pending",
                        "rawInput": { "action": "exec", "command": "git status -s", "cwd": "/repo" },
                    },
                    "options": [
                        { "optionId": "allow_once", "name": "Allow once", "kind": "allow_once" },
                        { "optionId": "allow_always", "name": "Allow for this session", "kind": "allow_always" },
                        { "optionId": "reject_once", "name": "Reject", "kind": "reject_once" },
                    ],
                },
            }),
        );
        let response = |peer: &Peer| {
            peer.sent()
                .into_iter()
                .find(|message| message["id"] == 1 && message.get("result").is_some())
        };
        peer.wait_for("auto permission response", |peer| response(peer).is_some())
            .await;
        assert_eq!(
            response(&peer).unwrap()["result"]["outcome"]["optionId"],
            "allow_always"
        );
        assert!(
            !events.any(|event| matches!(event, HarnessEvent::ApprovalRequested { .. })),
            "fx must never park a turn on an approval"
        );

        peer.reply("t1", &prompt["id"], json!({ "stopReason": "end_turn" }));
        turn2.await.unwrap();
        adapter.stop_session("t1".into()).await.unwrap();
    });
}

#[test]
fn routes_a_late_exit_to_the_current_turns_listener_not_turn_1s() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = FxAdapter::new(&peer.ctx);
        let turn1_events = Events::default();
        let turn2_events = Events::default();

        let turn1 = start(&adapter, input("t2", "hey"), &turn1_events);
        peer.answer("t2", "initialize", json!({ "protocolVersion": 1 }))
            .await;
        peer.answer(
            "t2",
            "session/new",
            json!({
                "sessionId": "S2",
                "configOptions": [{ "id": "model", "category": "model", "currentValue": "zai/glm-5.2" }],
            }),
        )
        .await;
        peer.answer("t2", "session/set_mode", json!({})).await;
        peer.answer("t2", "session/prompt", json!({ "stopReason": "end_turn" }))
            .await;
        turn1.await.unwrap();

        // Turn 2 registers a new listener; fx then dies mid-turn.
        peer.clear();
        let turn2 = start(&adapter, input("t2", "again"), &turn2_events);
        peer.answer("t2", "session/set_mode", json!({})).await;
        peer.next("session/prompt", |_| true).await;
        peer.exit("t2", 1);
        let _ = turn2.await;

        let ended = |event: &HarnessEvent| matches!(event, HarnessEvent::SessionEnded { .. });
        assert!(
            turn2_events.any(ended),
            "session.ended must reach the turn that is actually running"
        );
        assert!(!turn1_events.any(ended));
        adapter.stop_session("t2".into()).await.unwrap();
    });
}

#[test]
fn reads_the_catalog_from_models_and_status_json() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = FxAdapter::new(&peer.ctx);
        let models = adapter.catalog().discover(Some("/repo")).await.unwrap();
        // The fake answers every exec with a version line, which lists no models.
        assert!(models.is_empty());
        let execs: Vec<Vec<String>> = peer
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Exec(request) => Some(request.args),
                _ => None,
            })
            .collect();
        assert!(execs.contains(&vec!["models".to_string(), "--json".into()]));
        assert!(execs.contains(&vec!["status".to_string(), "--json".into()]));
    });
}

#[test]
fn registers_once_and_refuses_to_steer() {
    let peer = Peer::new();
    register(&peer.ctx);
    let first = peer.ctx.registry.get_harness(HarnessId::Fx).unwrap();
    register(&peer.ctx);
    let second = peer.ctx.registry.get_harness(HarnessId::Fx).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!second.can_steer());
}
