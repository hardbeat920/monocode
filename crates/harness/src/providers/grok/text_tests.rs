//! Port of grokText.test.ts.

use super::*;
use crate::providers::grok::test_support::{Events, Peer};

#[test]
fn forwards_grok_text_deltas_without_duplicating_snapshots() {
    smol::block_on(async {
        let peer = Peer::new();
        let text = GrokText::new(peer.ctx.children.clone(), peer.ctx.spawner.clone());
        let events = Events::default();
        let result = {
            let text = text.clone();
            let sink = events.sink();
            smol::spawn(async move {
                text.run_prompt(GrokTextPrompt {
                    cwd: "/repo".into(),
                    prompt: "question".into(),
                    on_event: Some(sink),
                    ..GrokTextPrompt::default()
                })
                .await
            })
        };

        peer.answer(TEXT_CHILD_ID, "initialize", json!({})).await;
        peer.answer(
            TEXT_CHILD_ID,
            "session/new",
            json!({ "sessionId": "grok_text" }),
        )
        .await;
        peer.answer(TEXT_CHILD_ID, "session/set_model", json!({}))
            .await;
        peer.answer(TEXT_CHILD_ID, "session/set_mode", json!({}))
            .await;
        let prompt = peer.next("session/prompt", |_| true).await;

        peer.notify(
            TEXT_CHILD_ID,
            "session/update",
            json!({
                "sessionId": "grok_text",
                "update": { "sessionUpdate": "agent_message_chunk", "content": "Hel" },
            }),
        );
        peer.notify(
            TEXT_CHILD_ID,
            "session/update",
            json!({
                "sessionId": "grok_text",
                "update": { "sessionUpdate": "agent_message", "content": "Hello" },
            }),
        );
        peer.reply(TEXT_CHILD_ID, &prompt["id"], json!({}));

        assert_eq!(result.await.unwrap(), "Hello");
        assert_eq!(
            events.all(),
            vec![
                HarnessEvent::MessageDelta { text: "Hel".into() },
                HarnessEvent::MessageDelta { text: "lo".into() },
            ]
        );
        text.stop(None).await;
    });
}

#[test]
fn refuses_tools_and_skips_questions_in_the_text_runner() {
    smol::block_on(async {
        let peer = Peer::new();
        let text = GrokText::new(peer.ctx.children.clone(), peer.ctx.spawner.clone());
        let result = {
            let text = text.clone();
            smol::spawn(async move {
                text.run_prompt(GrokTextPrompt {
                    cwd: "/repo".into(),
                    prompt: "question".into(),
                    ..GrokTextPrompt::default()
                })
                .await
            })
        };
        peer.answer(TEXT_CHILD_ID, "initialize", json!({})).await;
        peer.answer(
            TEXT_CHILD_ID,
            "session/new",
            json!({ "sessionId": "grok_text" }),
        )
        .await;
        peer.answer(TEXT_CHILD_ID, "session/set_model", json!({}))
            .await;
        peer.answer(TEXT_CHILD_ID, "session/set_mode", json!({}))
            .await;
        let prompt = peer.next("session/prompt", |_| true).await;
        peer.line(
            TEXT_CHILD_ID,
            json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "session/request_permission",
                "params": { "options": [{ "optionId": "allow-once" }, { "optionId": "reject-once" }] },
            }),
        );
        peer.line(
            TEXT_CHILD_ID,
            json!({ "jsonrpc": "2.0", "id": 10, "method": "x.ai/ask_user_question", "params": {} }),
        );
        peer.wait_for("text runner replies", |peer| {
            let sent = peer.sent();
            sent.iter().any(|message| message["id"] == 9)
                && sent.iter().any(|message| message["id"] == 10)
        })
        .await;
        let sent = peer.sent();
        let by_id = |id: i64| {
            sent.iter()
                .find(|message| message["id"] == id)
                .unwrap()
                .clone()
        };
        assert_eq!(by_id(9)["result"]["outcome"]["optionId"], "reject-once");
        assert_eq!(by_id(10)["result"], json!({ "outcome": "skip_interview" }));
        peer.reply(TEXT_CHILD_ID, &prompt["id"], json!({}));
        assert_eq!(result.await.unwrap(), "");
        text.stop(None).await;
    });
}
