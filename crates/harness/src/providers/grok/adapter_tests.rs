//! Port of grokLive.test.ts. The TypeScript mocked `core/child`; these run
//! the real adapter over `core::testing::Fake` with a scripted ACP peer.

use super::*;
use crate::providers::grok::test_support::{Events, Peer, session, settings, turn};
use monocode_core::attachment::{Attachment, AttachmentKind};

fn init_result() -> Value {
    json!({
        "protocolVersion": 1,
        "authMethods": [{ "id": "cached_token" }],
        "_meta": {
            "defaultAuthMethodId": "cached_token",
            "modelState": {
                "currentModelId": "grok-4.6",
                "availableModels": [{
                    "modelId": "grok-4.6",
                    "name": "Grok 4.6",
                    "_meta": { "totalContextTokens": 500000 },
                }],
            },
        },
    })
}

async fn handshake(peer: &Peer, session_id: &str) {
    peer.answer(session_id, "initialize", init_result()).await;
    peer.answer(session_id, "authenticate", json!({})).await;
    peer.answer(
        session_id,
        "session/new",
        json!({ "sessionId": "S1", "models": { "currentModelId": "grok-4.6" } }),
    )
    .await;
}

fn start(adapter: &GrokAdapter, input: SendTurnInput, events: &Events) -> smol::Task<Result<()>> {
    let adapter = adapter.clone();
    let sink = events.sink();
    smol::spawn(async move { adapter.send_turn(input, sink, None).await })
}

#[test]
fn authenticates_selects_the_model_and_prompts() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = GrokAdapter::new(&peer.ctx, GrokHost::default());
        let events = Events::default();
        let mut input = turn(
            "t1",
            "grok:grok-4.6",
            RuntimeMode::Supervised,
            "hey",
            Some(settings(&[("effort", "high")])),
        );
        input.attachments = Some(vec![Attachment {
            id: "image-1".into(),
            name: "screenshot.png".into(),
            mime_type: "image/png".into(),
            kind: AttachmentKind::Image,
            size: 3,
            data: Some("YWJj".into()),
            ..Attachment::default()
        }]);
        let running = start(&adapter, input, &events);
        handshake(&peer, "t1").await;
        peer.answer("t1", "session/set_mode", json!({})).await;
        let prompt = peer.next("session/prompt", |_| true).await;
        assert_eq!(
            prompt["params"]["prompt"],
            json!([
                { "type": "text", "text": "hey" },
                { "type": "image", "mimeType": "image/png", "data": "YWJj" },
            ])
        );
        peer.reply("t1", &prompt["id"], json!({ "stopReason": "end_turn" }));
        running.await.unwrap();
        assert!(events.any(|event| matches!(event, HarnessEvent::SessionProviderBound { .. })));
        assert!(peer.find("authenticate").is_some());
        adapter.stop_session("t1".into()).await.unwrap();
    });
}

#[test]
fn surfaces_a_supervised_permission_request_instead_of_auto_approving() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = GrokAdapter::new(&peer.ctx, GrokHost::default());
        let events = Events::default();
        let running = start(
            &adapter,
            turn(
                "t2",
                "grok:grok-4.6",
                RuntimeMode::Supervised,
                "run git",
                None,
            ),
            &events,
        );
        handshake(&peer, "t2").await;
        let prompt = peer.next("session/prompt", |_| true).await;
        peer.line(
            "t2",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "session/request_permission",
                "params": {
                    "sessionId": "S1",
                    "toolCall": {
                        "toolCallId": "call_a",
                        "title": "Execute `git status`",
                        "kind": "execute",
                        "rawInput": { "variant": "Bash", "command": "git status" },
                    },
                    "options": [
                        { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                        { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" },
                    ],
                },
            }),
        );
        let seen = events.clone();
        peer.wait_for("approval.requested", move |_| {
            seen.any(|event| matches!(event, HarnessEvent::ApprovalRequested { .. }))
        })
        .await;
        adapter.respond_approval("t2", 1, ApprovalDecision::Allow);
        let response = |peer: &Peer| {
            peer.sent()
                .into_iter()
                .find(|message| message["id"] == 1 && message.get("result").is_some())
        };
        peer.wait_for("permission response", |peer| response(peer).is_some())
            .await;
        assert_eq!(
            response(&peer).unwrap()["result"]["outcome"]["optionId"],
            "allow-once"
        );
        peer.reply("t2", &prompt["id"], json!({ "stopReason": "end_turn" }));
        running.await.unwrap();
        adapter.stop_session("t2".into()).await.unwrap();
    });
}

#[test]
fn routes_a_late_exit_to_the_current_turns_listener() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = GrokAdapter::new(&peer.ctx, GrokHost::default());
        let turn1_events = Events::default();
        let turn2_events = Events::default();

        let turn1 = start(
            &adapter,
            turn("t3", "grok:grok-4.6", RuntimeMode::Supervised, "hey", None),
            &turn1_events,
        );
        handshake(&peer, "t3").await;
        peer.answer("t3", "session/prompt", json!({ "stopReason": "end_turn" }))
            .await;
        turn1.await.unwrap();

        peer.clear();
        let turn2 = start(
            &adapter,
            turn(
                "t3",
                "grok:grok-4.6",
                RuntimeMode::Supervised,
                "again",
                None,
            ),
            &turn2_events,
        );
        peer.next("session/prompt", |_| true).await;
        peer.exit("t3", 1);
        let _ = turn2.await;

        let ended = |event: &HarnessEvent| matches!(event, HarnessEvent::SessionEnded { .. });
        assert!(turn2_events.any(ended));
        assert!(!turn1_events.any(ended));
        adapter.stop_session("t3".into()).await.unwrap();
    });
}

#[test]
fn compacts_with_groks_acp_extension_instead_of_a_slash_command_prompt() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = GrokAdapter::new(&peer.ctx, GrokHost::default());
        let events = Events::default();
        let running = start(
            &adapter,
            turn("t4", "grok:grok-4.6", RuntimeMode::Supervised, "hey", None),
            &events,
        );
        handshake(&peer, "t4").await;
        peer.answer("t4", "session/prompt", json!({ "stopReason": "end_turn" }))
            .await;
        running.await.unwrap();
        peer.clear();

        let compact = {
            let adapter = adapter.clone();
            let sink = events.sink();
            smol::spawn(async move {
                adapter
                    .compact_context(
                        session("t4", "grok:grok-4.6", RuntimeMode::Supervised, None),
                        sink,
                    )
                    .await
            })
        };
        let request = peer.next("_x.ai/compact_conversation", |_| true).await;
        assert_eq!(request["params"], json!({ "sessionId": "S1" }));
        assert!(peer.find("session/prompt").is_none());
        peer.reply("t4", &request["id"], json!({}));
        compact.await.unwrap();
        adapter.stop_session("t4".into()).await.unwrap();
    });
}

#[test]
fn fills_the_context_window_from_setup_and_answers_questions() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = GrokAdapter::new(&peer.ctx, GrokHost::default());
        let events = Events::default();
        let running = start(
            &adapter,
            turn(
                "t5",
                "grok:grok-4.6",
                RuntimeMode::Supervised,
                "ask me",
                None,
            ),
            &events,
        );
        handshake(&peer, "t5").await;
        let prompt = peer.next("session/prompt", |_| true).await;
        peer.notify(
            "t5",
            "session/update",
            json!({ "sessionId": "S1", "update": { "sessionUpdate": "usage_update", "used": 1000 } }),
        );
        peer.line(
            "t5",
            json!({
                "jsonrpc": "2.0",
                "id": 40,
                "method": "_x.ai/ask_user_question",
                "params": { "questions": [{ "question": "Which colour?", "options": [{ "label": "Red" }] }] },
            }),
        );
        let seen = events.clone();
        peer.wait_for("question.asked", move |_| {
            seen.any(|event| matches!(event, HarnessEvent::QuestionAsked { .. }))
        })
        .await;
        adapter.respond_question("t5", 40, UserQuestionReply::Skipped);
        peer.wait_for("question reply", |peer| {
            peer.sent().iter().any(|message| message["id"] == 40)
        })
        .await;
        let reply = peer
            .sent()
            .into_iter()
            .find(|message| message["id"] == 40)
            .unwrap();
        assert_eq!(reply["result"], json!({ "outcome": "skip_interview" }));
        peer.reply("t5", &prompt["id"], json!({ "stopReason": "end_turn" }));
        running.await.unwrap();
        assert!(events.any(|event| {
            matches!(
                event,
                HarnessEvent::Context {
                    used: Some(1000),
                    window: Some(500_000)
                }
            )
        }));
        assert!(events.any(|event| matches!(event, HarnessEvent::MessageCompleted)));
        adapter.stop_session("t5".into()).await.unwrap();
    });
}

#[test]
fn registers_once() {
    let peer = Peer::new();
    register(&peer.ctx);
    let first = peer.ctx.registry.get_harness(HarnessId::Grok).unwrap();
    register(&peer.ctx);
    let second = peer.ctx.registry.get_harness(HarnessId::Grok).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!second.can_steer());
    assert!(second.capabilities().compact_context);
}
