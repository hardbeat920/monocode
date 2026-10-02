//! Port of hermesLive.test.ts. The TypeScript mocked `core/child`; these run
//! the real adapter over `core::testing::Fake` with a scripted ACP peer and
//! scripted transcript files.

use super::*;
use crate::providers::grok::test_support::{Events, Peer, turn};
use monocode_core::attachment::{Attachment, AttachmentKind};
use std::sync::atomic::{AtomicBool, Ordering};

async fn initialize(peer: &Peer, session_id: &str) {
    peer.answer(session_id, "initialize", json!({ "protocolVersion": 1 }))
        .await;
}

async fn new_session(peer: &Peer, session_id: &str) {
    peer.answer(
        session_id,
        "session/new",
        json!({ "sessionId": "hermes-session-1", "models": { "currentModelId": "nous:hermes-4" } }),
    )
    .await;
}

fn start(adapter: &HermesAdapter, input: SendTurnInput, events: &Events) -> smol::Task<Result<()>> {
    let adapter = adapter.clone();
    let sink = events.sink();
    smol::spawn(async move { adapter.send_turn(input, sink, None).await })
}

#[test]
fn starts_hermes_acp_selects_model_and_mode_and_sends_attachments() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = HermesAdapter::new(&peer.ctx);
        let events = Events::default();
        let mut input = turn(
            "hermes-live-new",
            "hermes:openrouter:gpt-5",
            RuntimeMode::AutoAcceptEdits,
            "inspect this",
            None,
        );
        input.attachments = Some(vec![Attachment {
            id: "image-1".into(),
            name: "screen.png".into(),
            mime_type: "image/png".into(),
            kind: AttachmentKind::Image,
            size: 4,
            data: Some("AAAA".into()),
            ..Attachment::default()
        }]);
        let running = start(&adapter, input, &events);

        peer.next("initialize", |_| true).await;
        peer.stderr(
            "hermes-live-new",
            "2026-09-17 10:24:42 [WARNING] agent.credential_pool: Copilot token exchange degraded to RAW token (exchange unavailable)",
        );
        initialize(&peer, "hermes-live-new").await;
        new_session(&peer, "hermes-live-new").await;
        let set_model = peer.next("session/set_model", |_| true).await;
        assert_eq!(set_model["params"]["modelId"], "openrouter:gpt-5");
        peer.reply("hermes-live-new", &set_model["id"], json!({}));
        let set_mode = peer.next("session/set_mode", |_| true).await;
        assert_eq!(set_mode["params"]["modeId"], "accept_edits");
        peer.reply("hermes-live-new", &set_mode["id"], json!({}));
        let prompt = peer.next("session/prompt", |_| true).await;
        assert_eq!(
            prompt["params"]["prompt"],
            json!([
                { "type": "text", "text": "inspect this" },
                { "type": "image", "mimeType": "image/png", "data": "AAAA" },
            ])
        );
        peer.reply(
            "hermes-live-new",
            &prompt["id"],
            json!({ "stopReason": "end_turn" }),
        );

        running.await.unwrap();
        assert!(events.all().contains(&HarnessEvent::SessionProviderBound {
            provider_session_id: "hermes-session-1".into()
        }));
        assert!(!events.any(|event| matches!(event, HarnessEvent::SessionError { .. })));
        adapter
            .stop_session("hermes-live-new".into())
            .await
            .unwrap();
    });
}

#[test]
fn loads_a_bound_hermes_session_instead_of_creating_a_new_one() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = HermesAdapter::new(&peer.ctx);
        adapter.bind_session("hermes-live-load", "persisted-session", "/repo", None);
        let running = start(
            &adapter,
            turn(
                "hermes-live-load",
                "hermes:nous:hermes-4",
                RuntimeMode::Supervised,
                "continue",
                None,
            ),
            &Events::default(),
        );
        initialize(&peer, "hermes-live-load").await;
        let load = peer.next("session/load", |_| true).await;
        assert_eq!(load["params"]["sessionId"], "persisted-session");
        peer.reply(
            "hermes-live-load",
            &load["id"],
            json!({ "models": { "currentModelId": "nous:hermes-4" } }),
        );
        peer.answer("hermes-live-load", "session/set_mode", json!({}))
            .await;
        peer.answer(
            "hermes-live-load",
            "session/prompt",
            json!({ "stopReason": "end_turn" }),
        )
        .await;

        running.await.unwrap();
        assert!(peer.find("session/new").is_none());
        assert!(peer.find("session/resume").is_none());
        adapter
            .stop_session("hermes-live-load".into())
            .await
            .unwrap();
    });
}

#[test]
fn uses_hermes_permission_option_ids_for_supervised_approvals() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = HermesAdapter::new(&peer.ctx);
        let events = Events::default();
        let running = start(
            &adapter,
            turn(
                "hermes-live-permission",
                "hermes:nous:hermes-4",
                RuntimeMode::Supervised,
                "run git",
                None,
            ),
            &events,
        );
        initialize(&peer, "hermes-live-permission").await;
        new_session(&peer, "hermes-live-permission").await;
        peer.answer("hermes-live-permission", "session/set_mode", json!({}))
            .await;
        let prompt = peer.next("session/prompt", |_| true).await;

        peer.line(
            "hermes-live-permission",
            json!({
                "jsonrpc": "2.0",
                "id": 91,
                "method": "session/request_permission",
                "params": {
                    "sessionId": "hermes-session-1",
                    "toolCall": {
                        "toolCallId": "tool-1",
                        "title": "Run git status",
                        "kind": "execute",
                        "rawInput": { "command": "git status" },
                    },
                    "options": [
                        { "optionId": "allow_once", "name": "Allow once", "kind": "allow_once" },
                        { "optionId": "deny", "name": "Deny", "kind": "reject_once" },
                    ],
                },
            }),
        );
        let seen = events.clone();
        peer.wait_for("approval.requested", move |_| {
            seen.any(|event| matches!(event, HarnessEvent::ApprovalRequested { .. }))
        })
        .await;
        adapter.respond_approval("hermes-live-permission", 91, ApprovalDecision::Allow);
        let response = |peer: &Peer| {
            peer.sent()
                .into_iter()
                .find(|message| message["id"] == 91 && message.get("result").is_some())
        };
        peer.wait_for("permission response", |peer| response(peer).is_some())
            .await;
        assert_eq!(
            response(&peer).unwrap()["result"]["outcome"]["optionId"],
            "allow_once"
        );
        peer.reply(
            "hermes-live-permission",
            &prompt["id"],
            json!({ "stopReason": "end_turn" }),
        );

        running.await.unwrap();
        adapter
            .stop_session("hermes-live-permission".into())
            .await
            .unwrap();
    });
}

#[test]
fn stays_busy_and_resumes_after_hermes_background_subagents_finish() {
    smol::block_on(async {
        let (peer, files) = Peer::with_files();
        let adapter = HermesAdapter::new(&peer.ctx);
        let events = Events::default();
        let transcript = "/tmp/deleg_abcd/task-0.log";
        let manifest = "/tmp/deleg_abcd/manifest.json";
        files.set(
            manifest,
            &json!({ "tasks": [{ "index": 0, "status": "running" }] }).to_string(),
        );
        files.set(
            transcript,
            "=== Hermes subagent live transcript ===\n12:00:01 assistant | Found the lifecycle race.\n12:00:02 final | end status=completed",
        );

        let settled = Arc::new(AtomicBool::new(false));
        let running = {
            let adapter = adapter.clone();
            let sink = events.sink();
            let settled = settled.clone();
            let input = turn(
                "hermes-live-background",
                "hermes:nous:hermes-4",
                RuntimeMode::Supervised,
                "investigate the race",
                None,
            );
            smol::spawn(async move {
                let result = adapter.send_turn(input, sink, None).await;
                settled.store(true, Ordering::SeqCst);
                result
            })
        };

        initialize(&peer, "hermes-live-background").await;
        new_session(&peer, "hermes-live-background").await;
        peer.answer("hermes-live-background", "session/set_mode", json!({}))
            .await;
        let first_prompt = peer.next("session/prompt", |_| true).await;

        peer.notify(
            "hermes-live-background",
            "session/update",
            json!({
                "sessionId": "hermes-session-1",
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "delegate-call",
                    "kind": "agent",
                    "title": "Delegate task",
                    "status": "completed",
                    "content": [{
                        "type": "text",
                        "text": json!({
                            "status": "dispatched",
                            "mode": "background",
                            "delegation_id": "deleg_abcd",
                            "live_transcripts": [transcript],
                        })
                        .to_string(),
                    }],
                },
            }),
        );
        peer.reply(
            "hermes-live-background",
            &first_prompt["id"],
            json!({ "stopReason": "end_turn" }),
        );

        smol::Timer::after(std::time::Duration::from_millis(20)).await;
        assert!(!settled.load(Ordering::SeqCst));
        let delegate_rows = |events: &Events| -> Vec<HarnessEvent> {
            events
                .all()
                .into_iter()
                .filter(|event| {
                    matches!(event, HarnessEvent::ToolUpdated { call_id, .. } if call_id == "delegate-call")
                })
                .collect()
        };
        match delegate_rows(&events).first() {
            Some(HarnessEvent::ToolUpdated { status, kind, .. }) => {
                assert_eq!(status.as_deref(), Some("in_progress"));
                assert_eq!(kind.as_deref(), Some("agent"));
            }
            other => panic!("no delegate row: {other:?}"),
        }

        files.set(
            manifest,
            &json!({ "completed": "2026-09-21 12:00:02", "tasks": [{ "index": 0, "status": "completed" }] })
                .to_string(),
        );
        peer.wait_for("background continuation prompt", |peer| {
            peer.count("session/prompt") == 2
        })
        .await;
        let continuation = peer
            .sent()
            .into_iter()
            .filter(|message| message["method"] == "session/prompt")
            .nth(1)
            .unwrap();
        let text = continuation["params"]["prompt"][0]["text"]
            .as_str()
            .unwrap();
        assert!(text.contains("Found the lifecycle race"));
        assert!(text.contains(transcript));
        match delegate_rows(&events).last() {
            Some(HarnessEvent::ToolUpdated { status, kind, .. }) => {
                assert_eq!(status.as_deref(), Some("completed"));
                assert_eq!(kind.as_deref(), Some("agent"));
            }
            other => panic!("no delegate row: {other:?}"),
        }

        peer.reply(
            "hermes-live-background",
            &continuation["id"],
            json!({ "stopReason": "end_turn" }),
        );
        running.await.unwrap();
        assert!(settled.load(Ordering::SeqCst));
        adapter
            .stop_session("hermes-live-background".into())
            .await
            .unwrap();
    });
}

#[test]
fn steers_with_a_concurrent_prompt() {
    smol::block_on(async {
        let peer = Peer::new();
        let adapter = HermesAdapter::new(&peer.ctx);
        let running = start(
            &adapter,
            turn(
                "hermes-live-steer",
                "hermes:nous:hermes-4",
                RuntimeMode::Supervised,
                "first",
                None,
            ),
            &Events::default(),
        );
        initialize(&peer, "hermes-live-steer").await;
        new_session(&peer, "hermes-live-steer").await;
        peer.answer("hermes-live-steer", "session/set_mode", json!({}))
            .await;
        let first = peer.next("session/prompt", |_| true).await;
        let steer = {
            let adapter = adapter.clone();
            smol::spawn(async move {
                adapter
                    .steer_turn(SteerTurnInput {
                        session_id: "hermes-live-steer".into(),
                        cwd: "/repo".into(),
                        model: "hermes:nous:hermes-4".into(),
                        model_settings: None,
                        text: "also this".into(),
                        attachments: None,
                    })
                    .await
            })
        };
        peer.wait_for("steer prompt", |peer| peer.count("session/prompt") == 2)
            .await;
        let second = peer
            .sent()
            .into_iter()
            .filter(|message| message["method"] == "session/prompt")
            .nth(1)
            .unwrap();
        assert_eq!(second["params"]["prompt"][0]["text"], "also this");
        peer.reply("hermes-live-steer", &second["id"], json!({}));
        steer.await.unwrap();
        peer.reply(
            "hermes-live-steer",
            &first["id"],
            json!({ "stopReason": "end_turn" }),
        );
        running.await.unwrap();
        assert!(adapter.can_steer());
        adapter
            .stop_session("hermes-live-steer".into())
            .await
            .unwrap();
    });
}

#[test]
fn reads_manifest_paths_and_completion() {
    assert_eq!(manifest_path("/tmp/d/task-0.log"), "/tmp/d/manifest.json");
    assert_eq!(manifest_path("C:\\d\\task-0.log"), "C:\\d\\manifest.json");
    assert_eq!(manifest_path("task-0.log"), "");
    assert!(!manifest_complete(r#"{"tasks":[{"status":"completed"}]}"#));
    assert!(!manifest_complete(r#"{"completed":true,"tasks":[]}"#));
    assert!(!manifest_complete(
        r#"{"completed":true,"tasks":[{"status":"finalizing"}]}"#
    ));
    assert!(manifest_complete(
        r#"{"completed":"now","tasks":[{"status":"FAILED"}]}"#
    ));
    assert_eq!(slice_tail("abcdef", 3), "def");
    assert_eq!(slice_tail("abc", 6), "abc");
}
