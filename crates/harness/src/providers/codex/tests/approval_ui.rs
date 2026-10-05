//! The protocol side of
//! src/integrations/harness/providers/codex/codexApprovalUi.test.ts: Codex
//! requests become transcript events, and the answer the UI gives goes back
//! over the wire. Rendering the form, the toast, and the notification banner
//! belongs to the view crates.

use std::time::Duration;

use serde_json::{Value, json};

use monocode_core::block::ApprovalDecided;
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::{ApprovalDecision, SendTurnInput};
use monocode_core::user_question::{UserQuestion, UserQuestionReply};

use super::fake::*;

const S: &str = "codex-ui";

fn run(test: impl std::future::Future<Output = ()>) {
    smol::block_on(test);
}

/// The `beforeEach`: a supervised turn on "Inspect the project".
async fn start(h: &Harness) -> Started {
    let events = Events::default();
    let turn = send(
        h,
        SendTurnInput {
            session: session_input(S, RuntimeMode::Supervised),
            text: "Inspect the project".into(),
            attachments: None,
        },
        &events,
        None,
    );
    for (method, result) in [
        ("initialize", json!({})),
        ("thread/start", json!({ "thread": { "id": "thr_ui" } })),
        ("turn/start", json!({ "turn": { "id": "turn_ui" } })),
    ] {
        wait_for(method, || h.find_method(method).is_some()).await;
        let request = h.find_method(method).unwrap();
        h.reply(S, &request["id"], result);
    }
    wait_for("turn.started", || events.has("turn.started")).await;
    Started { events, turn }
}

/// The `afterEach`.
async fn finish(h: &Harness, started: Started) {
    complete_turn(h, S, "turn_ui");
    started.turn.await.unwrap();
    h.adapter.sessions().stop_session(S).await.unwrap();
}

fn option_id(question: &Value, label: &str) -> String {
    let questions: Vec<UserQuestion> =
        serde_json::from_value(question["questions"].clone()).unwrap();
    questions[0]
        .options
        .iter()
        .find(|option| option.label == label)
        .unwrap()
        .id
        .clone()
}

#[test]
fn renders_a_question_and_sends_the_clicked_answer() {
    run(async {
        let h = Harness::new();
        let started = start(&h).await;
        h.request(
            S,
            json!(91),
            "item/tool/requestUserInput",
            json!({ "itemId": "q1", "questions": [{
                "id": "access", "header": "Source", "question": "Read the external source?",
                "isOther": false, "isSecret": false,
                "options": [{ "label": "Accept" }, { "label": "Decline" }],
            }] }),
        );
        wait_for("question", || started.events.has("question.asked")).await;
        let question = started.events.of_type("question.asked")[0].clone();
        assert_eq!(question["title"], "Source");
        assert_eq!(
            question["questions"][0]["prompt"],
            "Read the external source?"
        );
        let pending = started.events.reduce().pending_question.unwrap();
        assert_eq!(pending.title.as_deref(), Some("Source"));
        assert!(h.reply_to(json!(91)).is_none());

        h.adapter.sessions().respond_question(
            S,
            pending.request_id,
            UserQuestionReply::Answered {
                answers: [("access".to_string(), vec![option_id(&question, "Decline")])].into(),
                custom: None,
            },
        );
        wait_for("answer", || h.reply_to(json!(91)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "answers": { "access": { "answers": ["Decline"] } } })
        );
        wait_for("question cleared", || {
            started.events.has("question.resolved")
        })
        .await;
        assert!(started.events.reduce().pending_question.is_none());
        finish(&h, started).await;
    });
}

#[test]
fn keeps_command_approval_visible_and_sends_allow() {
    run(async {
        let h = Harness::new();
        let started = start(&h).await;
        h.request(
            S,
            json!(91),
            "item/commandExecution/requestApproval",
            json!({ "itemId": "cmd1", "command": "git status --short", "reason": "Inspect the workspace" }),
        );
        wait_for("approval", || started.events.has("approval.requested")).await;
        let blocks = started.events.reduce().blocks;
        let approval = blocks
            .iter()
            .find(|block| block.approval.is_some())
            .expect("approval block");
        assert_eq!(approval.approval.as_ref().unwrap().decided, None);
        let request_id = approval.approval.as_ref().unwrap().request_id;

        h.adapter
            .sessions()
            .respond_approval(S, request_id, ApprovalDecision::Allow);
        wait_for("allow", || h.reply_to(json!(91)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "decision": "accept" })
        );
        wait_for("resolved", || started.events.has("approval.resolved")).await;
        let blocks = started.events.reduce().blocks;
        let approval = blocks
            .iter()
            .find(|block| block.approval.is_some())
            .unwrap();
        assert_eq!(
            approval.approval.as_ref().unwrap().decided,
            Some(ApprovalDecided::Allow)
        );
        finish(&h, started).await;
    });
}

#[test]
fn shows_a_required_boolean_mcp_confirmation_and_sends_the_choice() {
    for choice in [ApprovalDecision::Allow, ApprovalDecision::Deny] {
        run(async {
            let h = Harness::new();
            let started = start(&h).await;
            h.request(
                S,
                json!(91),
                "mcpServer/elicitation/request",
                json!({
                    "mode": "form", "serverName": "example", "message": "Confirm access",
                    "requestedSchema": {
                        "type": "object",
                        "properties": { "approved": { "type": "boolean", "title": "Read this source?" } },
                        "required": ["approved"],
                    },
                }),
            );
            wait_for("approval", || started.events.has("approval.requested")).await;
            let approval = started.events.of_type("approval.requested")[0].clone();
            assert!(
                approval["title"]
                    .as_str()
                    .unwrap()
                    .contains("Read this source?")
            );
            assert!(h.reply_to(json!(91)).is_none());
            h.adapter.sessions().respond_approval(
                S,
                approval["requestId"].as_i64().unwrap(),
                choice,
            );
            wait_for("MCP reply", || h.reply_to(json!(91)).is_some()).await;
            let allow = choice == ApprovalDecision::Allow;
            assert_eq!(
                h.reply_to(json!(91)).unwrap()["result"],
                json!({
                    "action": if allow { "accept" } else { "decline" },
                    "content": if allow { json!({ "approved": true }) } else { Value::Null },
                    "_meta": null,
                })
            );
            finish(&h, started).await;
        });
    }
}

#[test]
fn handles_an_optional_question_timeout_or_interaction() {
    const SHORT: Duration = Duration::from_millis(400);
    for action in ["timeout", "click", "keydown", "paste"] {
        run(async {
            let h = Harness::with(HarnessOptions {
                question_auto_resolve: SHORT,
                ..Default::default()
            });
            let started = start(&h).await;
            h.request(
                S,
                json!(91),
                "item/tool/requestUserInput",
                json!({ "isBlocking": false, "questions": [{
                    "id": "q", "question": "Choose a source",
                    "options": [{ "label": "Local" }, { "label": "Remote" }],
                }] }),
            );
            wait_for("question", || started.events.has("question.asked")).await;
            let question = started.events.of_type("question.asked")[0].clone();
            let request_id = question["requestId"].as_i64().unwrap();
            assert!(question.get("autoResolveAt").is_some());
            if action != "timeout" {
                // Any interaction with the form keeps the question open.
                h.adapter.sessions().keep_question_open(S, request_id);
                wait_for("kept open", || started.events.has("question.updated")).await;
                assert_eq!(
                    started
                        .events
                        .reduce()
                        .pending_question
                        .unwrap()
                        .auto_resolve_at,
                    None
                );
            }
            smol::Timer::after(SHORT * 2).await;
            if action == "timeout" {
                wait_for("skip", || h.reply_to(json!(91)).is_some()).await;
                assert_eq!(
                    h.reply_to(json!(91)).unwrap()["result"],
                    json!({ "answers": {} })
                );
                assert!(started.events.reduce().pending_question.is_none());
            } else {
                assert!(h.reply_to(json!(91)).is_none());
                assert!(started.events.reduce().pending_question.is_some());
                if action == "click" {
                    h.adapter.sessions().respond_question(
                        S,
                        request_id,
                        UserQuestionReply::Answered {
                            answers: [("q".to_string(), vec![option_id(&question, "Local")])]
                                .into(),
                            custom: None,
                        },
                    );
                    wait_for("answer", || h.reply_to(json!(91)).is_some()).await;
                    assert_eq!(
                        h.reply_to(json!(91)).unwrap()["result"],
                        json!({ "answers": { "q": { "answers": ["Local"] } } })
                    );
                }
            }
            assert_eq!(started.events.of_type("question.asked").len(), 1);
            finish(&h, started).await;
        });
    }
}
