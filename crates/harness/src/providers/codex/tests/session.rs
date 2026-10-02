//! Port of the "codex live turn sequence" tests in
//! src/integrations/harness/providers/codex/codexLive.test.ts. Those tests
//! mock the child process, so they run here over [`super::fake`] rather than
//! as `#[ignore]` tests. Checks that went through the transcript reducer
//! (`applyHarnessEvent`) check the events instead.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use monocode_core::block::TurnIntent;
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::{
    ApprovalDecision, RewindLastTurnInput, RewindLastTurnResult, SendTurnInput,
};
use monocode_core::user_question::UserQuestionReply;

use super::fake::*;
use super::support::{assert_match, assert_no_key};

const S: &str = "codex-live";

fn run(test: impl std::future::Future<Output = ()>) {
    smol::block_on(test);
}

fn answered(custom: &[(&str, &str)]) -> UserQuestionReply {
    UserQuestionReply::Answered {
        answers: Default::default(),
        custom: Some(
            custom
                .iter()
                .map(|(id, text)| (id.to_string(), text.to_string()))
                .collect(),
        ),
    }
}

fn request_id(event: &Value) -> i64 {
    event["requestId"].as_i64().unwrap()
}

async fn finish(h: &Harness, started: Started) {
    complete_turn(h, S, "turn_1");
    started.turn.await.unwrap();
    h.adapter.sessions().stop_session(S).await.unwrap();
}

fn image_item(id: &str) -> Value {
    json!({ "item": { "id": id, "type": "imageGeneration", "result": "aW1hZ2U=" } })
}

#[test]
fn reports_when_the_provider_accepts_a_turn() {
    run(async {
        let h = Harness::new();
        let accepted = Arc::new(AtomicUsize::new(0));
        let count = accepted.clone();
        let started = start_turn(
            &h,
            S,
            StartTurn {
                on_accepted: Some(Arc::new(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                })),
                ..Default::default()
            },
        )
        .await;
        wait_for("turn acceptance", || accepted.load(Ordering::SeqCst) == 1).await;
        finish(&h, started).await;
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn reopens_a_thread_when_app_access_changes_its_network_policy() {
    run(async {
        let h = Harness::new();
        let first = start_turn(
            &h,
            S,
            StartTurn {
                runtime_mode: Some(RuntimeMode::Auto),
                ..Default::default()
            },
        )
        .await;
        complete_turn(&h, S, "turn_1");
        first.turn.await.unwrap();

        h.clear_sent();
        let app_turn = start_turn(
            &h,
            S,
            StartTurn {
                runtime_mode: Some(RuntimeMode::Auto),
                controls_agents: Some(true),
                expect_resume: Some(true),
                ..Default::default()
            },
        )
        .await;
        assert_match(
            &h.find_method("thread/resume").unwrap()["params"],
            &json!({ "sandboxPolicy": { "networkAccess": true } }),
        );
        assert_match(
            &h.find_method("turn/start").unwrap()["params"],
            &json!({ "sandboxPolicy": { "networkAccess": true } }),
        );
        complete_turn(&h, S, "turn_1");
        app_turn.turn.await.unwrap();

        h.clear_sent();
        let ordinary = start_turn(
            &h,
            S,
            StartTurn {
                runtime_mode: Some(RuntimeMode::Auto),
                expect_resume: Some(true),
                ..Default::default()
            },
        )
        .await;
        let resume = h.find_method("thread/resume").unwrap();
        assert_match(
            &resume["params"],
            &json!({ "sandboxPolicy": { "type": "workspaceWrite" } }),
        );
        assert_no_key(&resume["params"]["sandboxPolicy"], "networkAccess");
        finish(&h, ordinary).await;
    });
}

#[test]
fn materializes_image_generations_before_completing_the_turn() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "item/completed",
            json!({ "item": {
                "id": "image_1", "type": "imageGeneration", "result": "aW1hZ2U=",
                "revisedPrompt": "A clean product photo",
            } }),
        );
        complete_turn(&h, S, "turn_1");
        started.turn.await.unwrap();

        assert_eq!(
            *h.images.saved.lock(),
            vec![("aW1hZ2U=".to_string(), "generated-image".to_string())]
        );
        assert!(started.events.json().contains(&json!({
            "type": "image.generated",
            "itemId": "image_1",
            "path": IMAGE_PATH,
            "name": "generated-image",
            "mimeType": "image/png",
            "size": 8,
            "alt": "A clean product photo",
        })));
        assert_match(
            &started.events.blocks(),
            &json!([{ "role": "image", "image": { "path": IMAGE_PATH, "mimeType": "image/png" } }]),
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn keeps_later_notifications_ordered_after_delayed_image_materialization() {
    run(async {
        let h = Harness::new();
        let release = h.images.hold_next();
        let started = start_turn(&h, S, StartTurn::default()).await;
        let events = started.events.clone();
        h.notify(S, "item/completed", image_item("image_1"));
        h.notify(
            S,
            "item/agentMessage/delta",
            json!({ "itemId": "after_image", "delta": "after image" }),
        );
        complete_turn(&h, S, "turn_1");
        settle().await;
        assert!(!events.message_text().contains(&"after image".to_string()));

        release.send(()).await.unwrap();
        started.turn.await.unwrap();
        settle().await;
        h.notify(
            S,
            "item/agentMessage/delta",
            json!({ "itemId": "post_turn", "delta": "post turn" }),
        );
        wait_for("post turn text", || {
            events.message_text().contains(&"post turn".to_string())
        })
        .await;
        let types: Vec<String> = events
            .json()
            .iter()
            .map(|event| event["type"].as_str().unwrap().to_string())
            .collect();
        let image = types
            .iter()
            .position(|kind| kind == "image.generated")
            .unwrap();
        let after = events
            .json()
            .iter()
            .position(|event| event["text"] == "after image")
            .unwrap();
        assert!(image < after);
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn cleans_up_an_image_that_finishes_saving_after_cancellation() {
    run(async {
        let h = Harness::new();
        let release = h.images.hold_next();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(S, "item/completed", image_item("image_1"));
        wait_for("save started", || !h.images.saved.lock().is_empty()).await;

        let sessions = h.adapter.sessions().clone();
        let cancelling = smol::spawn(async move { sessions.cancel_turn(S).await });
        wait_for("interrupt", || h.find_method("turn/interrupt").is_some()).await;
        let interrupt = h.find_method("turn/interrupt").unwrap();
        h.reply(S, &interrupt["id"], json!({}));
        cancelling.await.unwrap();
        release.send(()).await.unwrap();
        started.turn.await.unwrap();

        wait_for("delete", || !h.images.deleted.lock().is_empty()).await;
        assert_eq!(*h.images.deleted.lock(), vec![vec![IMAGE_PATH.to_string()]]);
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn does_not_flush_queued_notifications_after_the_session_stops() {
    run(async {
        let h = Harness::new();
        let release = h.images.hold_next();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(S, "item/completed", image_item("image_1"));
        h.notify(
            S,
            "item/agentMessage/delta",
            json!({ "itemId": "after_image", "delta": "after image" }),
        );
        complete_turn(&h, S, "turn_1");
        wait_for("save started", || !h.images.saved.lock().is_empty()).await;
        settle().await;
        h.adapter.sessions().stop_session(S).await.unwrap();
        release.send(()).await.unwrap();
        started.turn.await.unwrap();
        wait_for("delete", || !h.images.deleted.lock().is_empty()).await;

        assert!(
            !started
                .events
                .message_text()
                .contains(&"after image".to_string())
        );
        assert!(!started.events.has("image.generated"));
        assert_eq!(*h.images.deleted.lock(), vec![vec![IMAGE_PATH.to_string()]]);
    });
}

#[test]
fn emits_a_final_answer_once_after_commentary() {
    for chunks in [
        vec!["Here is the ", "final answer."],
        vec!["Here is the "],
        vec![],
    ] {
        run(async {
            let h = Harness::new();
            let started = start_turn(&h, S, StartTurn::default()).await;
            let commentary = "I'll inspect the workspace first.\n\n";
            let answer = "Here is the final answer.";
            h.notify(
                S,
                "item/agentMessage/delta",
                json!({ "itemId": "commentary", "delta": commentary }),
            );
            h.notify(
                S,
                "item/completed",
                json!({ "item": { "id": "commentary", "type": "agentMessage", "text": commentary } }),
            );
            for delta in &chunks {
                h.notify(
                    S,
                    "item/agentMessage/delta",
                    json!({ "itemId": "final", "delta": delta }),
                );
            }
            // A repeated completion must also be harmless.
            for _ in 0..2 {
                h.notify(
                    S,
                    "item/completed",
                    json!({ "item": { "id": "final", "type": "agentMessage", "text": answer } }),
                );
            }
            complete_turn(&h, S, "turn_1");
            started.turn.await.unwrap();
            assert_eq!(
                started.events.message_text().join(""),
                format!("{commentary}{answer}")
            );
            assert_match(
                &started.events.blocks(),
                &json!([
                    { "role": "assistant", "text": commentary, "streaming": false },
                    { "role": "assistant", "text": answer, "streaming": false },
                ]),
            );
            h.adapter.sessions().stop_session(S).await.unwrap();
        });
    }
}

#[test]
fn keeps_completion_history_for_separate_items_and_preserves_repeated_tokens() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        let complete = |id: &str, text: &str| {
            h.notify(
                S,
                "item/completed",
                json!({ "item": { "id": id, "type": "agentMessage", "text": text } }),
            )
        };
        complete("first", "Earlier commentary.\n\n");
        for delta in ["very ", "very ", "good."] {
            h.notify(
                S,
                "item/agentMessage/delta",
                json!({ "itemId": "second", "delta": delta }),
            );
        }
        complete("first", "Earlier commentary.\n\n");
        complete("second", "very very good.");
        // The same text in a different item is real new output.
        complete("third", "Earlier commentary.\n\n");
        complete_turn(&h, S, "turn_1");
        started.turn.await.unwrap();
        assert_eq!(
            started.events.message_text(),
            [
                "Earlier commentary.\n\n",
                "very ",
                "very ",
                "good.",
                "Earlier commentary.\n\n"
            ]
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn deduplicates_reasoning_completions_per_item() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        for (item_id, text) in [
            ("reason_1", "First thought."),
            ("reason_2", "Next thought."),
        ] {
            h.notify(
                S,
                "item/reasoning/summaryTextDelta",
                json!({ "itemId": item_id, "summaryIndex": 0, "delta": text }),
            );
            h.notify(
                S,
                "item/completed",
                json!({ "item": {
                    "id": item_id, "type": "reasoning",
                    "summary": [{ "type": "summary_text", "text": text }],
                } }),
            );
        }
        complete_turn(&h, S, "turn_1");
        started.turn.await.unwrap();
        assert_eq!(
            started.events.texts("reasoning.delta"),
            ["First thought.", "Next thought."]
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn resets_text_tracking_between_turns_when_an_item_id_is_reused() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        let text = "The answer.";
        let complete = || {
            h.notify(
                S,
                "item/completed",
                json!({ "item": { "id": "reused_item", "type": "agentMessage", "text": text } }),
            )
        };
        h.notify(
            S,
            "item/agentMessage/delta",
            json!({ "itemId": "reused_item", "delta": text }),
        );
        complete();
        complete_turn(&h, S, "turn_1");
        started.turn.await.unwrap();

        let next = send(
            &h,
            SendTurnInput {
                session: session_input(S, RuntimeMode::Supervised),
                text: "Repeat the answer".into(),
                attachments: None,
            },
            &started.events,
            None,
        );
        wait_for("next turn", || h.methods("turn/start").len() == 2).await;
        let request = h.methods("turn/start")[1].clone();
        h.reply(S, &request["id"], json!({ "turn": { "id": "turn_2" } }));
        h.notify(S, "turn/started", json!({ "turn": { "id": "turn_2" } }));
        wait_for("turn_2", || {
            started.events.of_type("turn.started").len() == 2
        })
        .await;
        complete();
        complete_turn(&h, S, "turn_2");
        next.await.unwrap();
        assert_eq!(started.events.message_text(), [text, text]);
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn keeps_retries_and_http_fallback_out_of_a_successful_turns_transcript() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        let before = started.events.without_turn_identity();
        for attempt in 1..=5 {
            h.notify(
                S,
                "error",
                json!({
                    "threadId": "thr_1", "turnId": "turn_1",
                    "error": { "message": format!("Reconnecting... {attempt}/5") },
                    "willRetry": true,
                }),
            );
        }
        let fallback = "Falling back from WebSockets to HTTPS transport. unexpected status 404 Not Found: Unknown endpoint: GET /v1/responses, url: ws://127.0.0.1:19101/v1/responses";
        h.notify(
            S,
            "warning",
            json!({ "threadId": "thr_1", "message": fallback }),
        );
        settle().await;
        assert!(!started.turn.is_finished());
        assert_eq!(started.events.without_turn_identity(), before);

        h.notify(
            S,
            "item/agentMessage/delta",
            json!({ "delta": "The answer" }),
        );
        complete_turn(&h, S, "turn_1");
        started.turn.await.unwrap();
        assert_eq!(started.events.message_text(), ["The answer"]);
        assert!(!started.events.has("session.error"));
        assert!(!started.events.has("status"));
        assert_match(
            &started.events.blocks(),
            &json!([{ "role": "assistant", "text": "The answer", "streaming": false }]),
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn resumes_a_legacy_thread_when_the_missing_account_resolves_to_default() {
    run(async {
        let h = Harness::new();
        let started = start_turn(
            &h,
            S,
            StartTurn {
                resume: true,
                provider_account_id: Some("default".into()),
                ..Default::default()
            },
        )
        .await;
        assert!(h.find_method("thread/resume").is_some());
        assert!(h.find_method("thread/start").is_none());
        finish(&h, started).await;
    });
}

#[test]
fn does_not_resume_a_legacy_default_thread_under_a_named_account() {
    run(async {
        let h = Harness::new();
        let started = start_turn(
            &h,
            S,
            StartTurn {
                resume: true,
                provider_account_id: Some("account-work".into()),
                expect_resume: Some(false),
                ..Default::default()
            },
        )
        .await;
        assert!(h.find_method("thread/start").is_some());
        assert!(h.find_method("thread/resume").is_none());
        assert_eq!(
            h.fake.spawns.lock()[0]
                .account
                .as_ref()
                .map(|a| a.id.as_str()),
            Some("account-work")
        );
        finish(&h, started).await;
    });
}

#[test]
fn still_surfaces_a_terminal_failure_after_transport_retries() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "error",
            json!({ "error": { "message": "Reconnecting... 5/5" }, "willRetry": true }),
        );
        let message = "Response stream disconnected after too many failed attempts";
        h.notify(
            S,
            "error",
            json!({ "error": { "message": message }, "willRetry": false }),
        );
        wait_for("session error", || started.events.has("session.error")).await;
        assert!(
            started
                .events
                .json()
                .contains(&json!({ "type": "session.error", "message": message }))
        );
        h.notify(
            S,
            "turn/completed",
            json!({ "turn": { "id": "turn_1", "status": "failed", "error": { "message": message } } }),
        );
        started.turn.await.unwrap();
        let blocks = started.events.reduce().blocks;
        assert!(blocks.iter().any(|block| {
            block.role == monocode_core::block::BlockRole::System && block.text == message
        }));
        assert!(
            !blocks
                .iter()
                .any(|block| block.text.contains("Reconnecting"))
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn answers_the_external_clock_before_thread_setup_finishes() {
    for resume in [false, true] {
        run(async {
            let h = Harness::new();
            h.set_now(1_789_000_000_789);
            let started = start_turn(
                &h,
                S,
                StartTurn {
                    resume,
                    before_thread_reply: Some(Box::new(|h: &Harness| {
                        h.request(
                            S,
                            json!("clock_setup"),
                            "currentTime/read",
                            json!({ "threadId": "thr_1" }),
                        );
                    })),
                    ..Default::default()
                },
            )
            .await;
            wait_for("clock reply", || h.reply_to(json!("clock_setup")).is_some()).await;
            assert_eq!(
                h.reply_to(json!("clock_setup")).unwrap(),
                json!({ "id": "clock_setup", "result": { "currentTimeAt": 1_789_000_000 } })
            );
            let events = started.events.clone();
            finish(&h, started).await;
            assert!(!events.has("session.error"));
        });
    }
}

#[test]
fn answers_fresh_clock_reads_without_interrupting_a_pending_question() {
    for intent in [None, Some(TurnIntent::Plan)] {
        run(async {
            let h = Harness::new();
            h.set_now(1_789_000_000_789);
            let started = start_turn(
                &h,
                S,
                StartTurn {
                    intent,
                    ..Default::default()
                },
            )
            .await;
            assert_match(
                &h.find_method("initialize").unwrap()["params"],
                &json!({ "capabilities": { "experimentalApi": true } }),
            );
            assert_match(
                &h.find_method("turn/start").unwrap()["params"],
                &json!({ "collaborationMode": {
                    "mode": if intent == Some(TurnIntent::Plan) { "plan" } else { "default" }
                } }),
            );
            h.request(
                S,
                json!("pending_question"),
                "item/tool/requestUserInput",
                json!({ "itemId": "q1", "questions": [
                    { "id": "choice", "header": "Source", "question": "Which source?" }
                ] }),
            );
            wait_for("question", || started.events.has("question.asked")).await;
            let before = started.events.json();
            for (id, millis) in [
                (json!(91), 1_789_000_000_789_i64),
                (json!("clock_next"), 1_789_000_005_123),
            ] {
                h.set_now(millis);
                h.request(
                    S,
                    id.clone(),
                    "currentTime/read",
                    json!({ "threadId": "thr_1" }),
                );
                wait_for("external clock reply", || h.reply_to(id.clone()).is_some()).await;
                assert_eq!(
                    h.reply_to(id.clone()).unwrap(),
                    json!({ "id": id, "result": { "currentTimeAt": millis / 1000 } })
                );
            }
            assert_eq!(started.events.json(), before);
            assert!(h.reply_to(json!("pending_question")).is_none());
            finish(&h, started).await;
        });
    }
}

#[test]
fn routes_full_access_escalation_after_resume() {
    for resume in [false, true] {
        run(async {
            let h = Harness::new();
            let started = start_turn(
                &h,
                S,
                StartTurn {
                    runtime_mode: Some(RuntimeMode::FullAccess),
                    resume,
                    ..Default::default()
                },
            )
            .await;
            for message in h.sent().iter().filter(|message| {
                ["thread/start", "thread/resume", "turn/start"]
                    .contains(&message["method"].as_str().unwrap_or(""))
            }) {
                assert_match(
                    &message["params"],
                    &json!({ "approvalPolicy": "on-request", "approvalsReviewer": "user" }),
                );
            }
            h.request(
                S,
                json!(91),
                "item/commandExecution/requestApproval",
                json!({ "itemId": "read_1", "command": "git status --short" }),
            );
            wait_for("full-access response", || h.reply_to(json!(91)).is_some()).await;
            assert_eq!(
                h.reply_to(json!(91)).unwrap()["result"],
                json!({ "decision": "accept" })
            );
            assert!(!started.events.has("approval.requested"));
            finish(&h, started).await;
        });
    }
}

#[test]
fn still_waits_for_an_explicit_command_decision_in_supervised_mode() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.request(
            S,
            json!(91),
            "item/commandExecution/requestApproval",
            json!({ "itemId": "cmd_1", "command": "git status --short" }),
        );
        wait_for("approval UI", || started.events.has("approval.requested")).await;
        assert!(h.reply_to(json!(91)).is_none());
        let request = started.events.of_type("approval.requested")[0].clone();
        h.adapter
            .sessions()
            .respond_approval(S, request_id(&request), ApprovalDecision::Deny);
        wait_for("denial", || h.reply_to(json!(91)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "decision": "decline" })
        );
        finish(&h, started).await;
    });
}

#[test]
fn keeps_a_child_approval_answerable_after_a_sibling_completes() {
    for decision in [ApprovalDecision::Allow, ApprovalDecision::Deny] {
        run(async {
            let h = Harness::new();
            let started = start_turn(&h, S, StartTurn::default()).await;
            h.request(
                S,
                json!("child_approval"),
                "item/commandExecution/requestApproval",
                json!({
                    "threadId": "thr_child", "turnId": "turn_child",
                    "itemId": "child_read", "command": "cat ~/.gitconfig",
                }),
            );
            wait_for("approval", || started.events.has("approval.requested")).await;
            let approval = started.events.of_type("approval.requested")[0].clone();
            assert_match(&approval, &json!({ "callId": "child_read" }));
            let before = started.events.without_turn_identity();
            h.notify(
                S,
                "turn/started",
                json!({ "threadId": "thr_sibling", "turn": { "id": "turn_sibling" } }),
            );
            h.notify(
                S,
                "item/agentMessage/delta",
                json!({ "threadId": "thr_sibling", "delta": "Child-only text" }),
            );
            h.notify(
                S,
                "turn/completed",
                json!({ "threadId": "thr_sibling", "turn": { "id": "turn_sibling", "status": "completed" } }),
            );
            h.notify(
                S,
                "error",
                json!({ "threadId": "thr_sibling", "error": { "message": "Child failed" }, "willRetry": false }),
            );
            settle().await;
            assert_eq!(started.events.without_turn_identity(), before);
            assert!(!started.turn.is_finished());
            h.adapter
                .sessions()
                .respond_approval(S, request_id(&approval), decision);
            wait_for("child decision", || {
                h.reply_to(json!("child_approval")).is_some()
            })
            .await;
            assert_eq!(
                h.reply_to(json!("child_approval")).unwrap()["result"],
                json!({ "decision": if decision == ApprovalDecision::Allow { "accept" } else { "decline" } })
            );
            h.notify(
                S,
                "turn/completed",
                json!({ "threadId": "thr_1", "turn": { "id": "turn_1", "status": "completed" } }),
            );
            started.turn.await.unwrap();
            h.adapter.sessions().stop_session(S).await.unwrap();
        });
    }
}

#[test]
fn clears_a_server_resolved_child_approval_using_its_owning_thread() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.request(
            S,
            json!("child_approval"),
            "item/commandExecution/requestApproval",
            json!({ "threadId": "thr_child", "itemId": "child_read", "command": "cat ~/.gitconfig" }),
        );
        wait_for("approval", || started.events.has("approval.requested")).await;
        let approval = started.events.of_type("approval.requested")[0].clone();
        h.notify(
            S,
            "serverRequest/resolved",
            json!({ "threadId": "thr_1", "requestId": "child_approval" }),
        );
        settle().await;
        assert!(!started.events.has("approval.resolved"));
        h.notify(
            S,
            "serverRequest/resolved",
            json!({ "threadId": "thr_child", "requestId": "child_approval" }),
        );
        wait_for("child cleanup", || started.events.has("approval.resolved")).await;
        assert!(started.events.json().contains(&json!({
            "type": "approval.resolved", "requestId": request_id(&approval), "decision": "cancelled",
        })));
        assert!(h.reply_to(json!("child_approval")).is_none());
        finish(&h, started).await;
    });
}

#[test]
fn answers_a_childs_filesystem_permission_request() {
    for decision in [ApprovalDecision::Allow, ApprovalDecision::Deny] {
        run(async {
            let h = Harness::new();
            let started = start_turn(&h, S, StartTurn::default()).await;
            let permissions = json!({ "fileSystem": { "read": ["/home/user/.gitconfig"] } });
            h.request(
                S,
                json!("child_permissions"),
                "item/permissions/requestApproval",
                json!({
                    "threadId": "thr_child", "turnId": "turn_child",
                    "itemId": "child_read", "permissions": permissions,
                }),
            );
            wait_for("approval", || started.events.has("approval.requested")).await;
            let approval = started.events.of_type("approval.requested")[0].clone();
            h.adapter
                .sessions()
                .respond_approval(S, request_id(&approval), decision);
            wait_for("permission response", || {
                h.reply_to(json!("child_permissions")).is_some()
            })
            .await;
            assert_eq!(
                h.reply_to(json!("child_permissions")).unwrap()["result"],
                if decision == ApprovalDecision::Allow {
                    json!({ "scope": "turn", "permissions": permissions })
                } else {
                    json!({ "permissions": {} })
                }
            );
            finish(&h, started).await;
        });
    }
}

#[test]
fn advances_the_question_queue_when_the_server_resolves_a_childs_request() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        for id in ["child_a", "child_b"] {
            h.request(
                S,
                json!(id),
                "item/tool/requestUserInput",
                json!({ "threadId": id, "questions": [
                    { "id": "q", "question": id, "isOther": true, "options": [] }
                ] }),
            );
        }
        wait_for("first question", || started.events.has("question.asked")).await;
        h.notify(
            S,
            "serverRequest/resolved",
            json!({ "threadId": "child_a", "requestId": "child_a" }),
        );
        wait_for("second child question", || {
            started.events.of_type("question.asked").len() == 2
        })
        .await;
        let pending = started.events.reduce().pending_question.unwrap();
        assert_eq!(pending.questions[0].prompt, "child_b");
        assert!(h.reply_to(json!("child_a")).is_none());
        h.adapter
            .sessions()
            .respond_question(S, pending.request_id, UserQuestionReply::Skipped);
        wait_for("second child response", || {
            h.reply_to(json!("child_b")).is_some()
        })
        .await;
        finish(&h, started).await;
    });
}

#[test]
fn fails_the_active_turn_if_a_child_permission_reply_cannot_be_delivered() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.request(
            S,
            json!("child_approval"),
            "item/commandExecution/requestApproval",
            json!({ "threadId": "thr_child", "itemId": "child_read", "command": "cat ~/.gitconfig" }),
        );
        wait_for("approval", || started.events.has("approval.requested")).await;
        let approval = started.events.of_type("approval.requested")[0].clone();
        *h.fake.fail_next_write.lock() = Some("Broken pipe".into());
        h.adapter
            .sessions()
            .respond_approval(S, request_id(&approval), ApprovalDecision::Allow);
        let error = started.turn.await.unwrap_err();
        assert_eq!(error.to_string(), "Broken pipe");
        assert!(
            started
                .events
                .json()
                .contains(&json!({ "type": "session.error", "message": "Broken pipe" }))
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn waits_for_user_input_in_full_access() {
    for intent in [None, Some(TurnIntent::Plan)] {
        run(async {
            let h = Harness::new();
            let started = start_turn(
                &h,
                S,
                StartTurn {
                    runtime_mode: Some(RuntimeMode::FullAccess),
                    intent,
                    ..Default::default()
                },
            )
            .await;
            h.request(
                S,
                json!("question_rpc"),
                "item/tool/requestUserInput",
                json!({ "itemId": "question_1", "questions": [{
                    "id": "permission", "header": "Access", "question": "Read the external source?",
                    "isOther": true, "isSecret": false,
                    "options": [
                        { "label": "Accept", "description": "Read the source." },
                        { "label": "Decline", "description": "Skip." },
                    ],
                }] }),
            );
            wait_for("question UI", || started.events.has("question.asked")).await;
            assert!(h.reply_to(json!("question_rpc")).is_none());
            let request = started.events.of_type("question.asked")[0].clone();
            let questions: Vec<monocode_core::user_question::UserQuestion> =
                serde_json::from_value(request["questions"].clone()).unwrap();
            let decline = questions[0]
                .options
                .iter()
                .find(|option| option.label == "Decline")
                .unwrap()
                .id
                .clone();
            h.adapter.sessions().respond_question(
                S,
                request_id(&request),
                UserQuestionReply::Answered {
                    answers: [("permission".to_string(), vec![decline])].into(),
                    custom: None,
                },
            );
            wait_for("question response", || {
                h.reply_to(json!("question_rpc")).is_some()
            })
            .await;
            assert_eq!(
                h.reply_to(json!("question_rpc")).unwrap()["result"],
                json!({ "answers": { "permission": { "answers": ["Decline"] } } })
            );
            assert!(started.events.json().contains(&json!({
                "type": "question.resolved", "requestId": request_id(&request), "decision": "answered",
            })));
            finish(&h, started).await;
        });
    }
}

#[test]
fn clears_pending_questions() {
    for action in ["skip", "server", "complete", "stop", "cancel"] {
        run(async {
            let h = Harness::new();
            let started = start_turn(&h, S, StartTurn::default()).await;
            h.request(
                S,
                json!(91),
                "item/tool/requestUserInput",
                json!({ "itemId": "q1", "questions": [{
                    "id": "q", "question": "Which source?", "isSecret": false, "isOther": true, "options": null,
                }] }),
            );
            wait_for("question UI", || started.events.has("question.asked")).await;
            let request = started.events.of_type("question.asked")[0].clone();
            match action {
                "skip" => h.adapter.sessions().respond_question(
                    S,
                    request_id(&request),
                    UserQuestionReply::Skipped,
                ),
                "server" => h.notify(
                    S,
                    "serverRequest/resolved",
                    json!({ "threadId": "thr_1", "requestId": 91 }),
                ),
                "complete" => complete_turn(&h, S, "turn_1"),
                "stop" => h.adapter.sessions().stop_session(S).await.unwrap(),
                _ => {
                    let sessions = h.adapter.sessions().clone();
                    let cancelled = smol::spawn(async move { sessions.cancel_turn(S).await });
                    wait_for("interrupt", || h.find_method("turn/interrupt").is_some()).await;
                    let interrupt = h.find_method("turn/interrupt").unwrap();
                    h.reply(S, &interrupt["id"], json!({}));
                    cancelled.await.unwrap();
                }
            }
            wait_for("question cleanup", || {
                started.events.has("question.resolved")
            })
            .await;
            assert!(started.events.json().contains(&json!({
                "type": "question.resolved",
                "requestId": request_id(&request),
                "decision": if action == "skip" { "skipped" } else { "cancelled" },
            })));
            if action == "skip" {
                wait_for("skip reply", || h.reply_to(json!(91)).is_some()).await;
                assert_eq!(
                    h.reply_to(json!(91)).unwrap()["result"],
                    json!({ "answers": {} })
                );
            } else {
                assert!(h.reply_to(json!(91)).is_none());
            }
            h.adapter.sessions().respond_question(
                S,
                request_id(&request),
                answered(&[("q", "too late")]),
            );
            complete_turn(&h, S, "turn_1");
            started.turn.await.unwrap();
            settle().await;
            assert_eq!(h.replies_to(json!(91)), usize::from(action == "skip"));
            h.adapter.sessions().stop_session(S).await.unwrap();
        });
    }
}

#[test]
fn does_not_collect_secret_answers_in_the_transcript_question_ui() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.request(
            S,
            json!(91),
            "item/tool/requestUserInput",
            json!({ "questions": [
                { "id": "secret", "question": "Enter a secret", "isSecret": true, "options": null }
            ] }),
        );
        wait_for("unsupported secret response", || {
            h.reply_to(json!(91)).is_some()
        })
        .await;
        assert!(!started.events.has("question.asked"));
        assert!(
            started
                .events
                .texts("status")
                .iter()
                .any(|text| text.contains("secret input"))
        );
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "answers": {} })
        );
        finish(&h, started).await;
    });
}

#[test]
fn auto_approves_full_access_file_and_permission_requests() {
    run(async {
        let h = Harness::new();
        let started = start_turn(
            &h,
            S,
            StartTurn {
                runtime_mode: Some(RuntimeMode::FullAccess),
                ..Default::default()
            },
        )
        .await;
        h.request(
            S,
            json!(91),
            "item/fileChange/requestApproval",
            json!({ "itemId": "edit_1", "reason": "Edit the requested file" }),
        );
        let permissions = json!({ "network": { "enabled": true } });
        h.request(
            S,
            json!(92),
            "item/permissions/requestApproval",
            json!({ "itemId": "perm_1", "permissions": permissions }),
        );
        wait_for("both grants", || {
            h.reply_to(json!(91)).is_some() && h.reply_to(json!(92)).is_some()
        })
        .await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "decision": "accept" })
        );
        assert_eq!(
            h.reply_to(json!(92)).unwrap()["result"],
            json!({ "scope": "session", "permissions": permissions })
        );
        assert!(!started.events.has("approval.requested"));
        finish(&h, started).await;
    });
}

#[test]
fn queues_concurrent_questions_instead_of_hiding_the_first_one() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        for id in [91, 92] {
            h.request(
                S,
                json!(id),
                "item/tool/requestUserInput",
                json!({ "questions": [{
                    "id": format!("q{id}"), "question": format!("Question {id}"), "options": null, "isOther": true,
                }] }),
            );
        }
        wait_for("first question", || started.events.has("question.asked")).await;
        settle().await;
        let asked = || started.events.of_type("question.asked");
        assert_eq!(asked().len(), 1);
        h.adapter.sessions().respond_question(
            S,
            request_id(&asked()[0]),
            answered(&[("q91", "first answer")]),
        );
        wait_for("second question", || asked().len() == 2).await;
        h.adapter.sessions().respond_question(
            S,
            request_id(&asked()[1]),
            UserQuestionReply::Skipped,
        );
        wait_for("second reply", || h.reply_to(json!(92)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "answers": { "q91": { "answers": ["first answer"] } } })
        );
        assert_eq!(
            h.reply_to(json!(92)).unwrap()["result"],
            json!({ "answers": {} })
        );
        finish(&h, started).await;
    });
}

#[test]
fn auto_approves_supported_mcp_confirmations_in_full_access() {
    for boolean in [false, true] {
        run(async {
            let h = Harness::new();
            let started = start_turn(
                &h,
                S,
                StartTurn {
                    runtime_mode: Some(RuntimeMode::FullAccess),
                    ..Default::default()
                },
            )
            .await;
            h.request(
                S,
                json!(91),
                "mcpServer/elicitation/request",
                json!({
                    "serverName": "example", "mode": "form", "message": "Read this source?",
                    "requestedSchema": if boolean {
                        json!({ "type": "object", "properties": { "approved": { "type": "boolean" } }, "required": ["approved"] })
                    } else {
                        json!({ "type": "object", "properties": {} })
                    },
                }),
            );
            wait_for("MCP response", || h.reply_to(json!(91)).is_some()).await;
            assert_eq!(
                h.reply_to(json!(91)).unwrap()["result"],
                json!({
                    "action": "accept",
                    "content": if boolean { json!({ "approved": true }) } else { json!({}) },
                    "_meta": null,
                })
            );
            assert!(!started.events.has("approval.requested"));
            finish(&h, started).await;
        });
    }
}

#[test]
fn keeps_plan_mcp_confirmations_explicit_in_full_access() {
    run(async {
        let h = Harness::new();
        let started = start_turn(
            &h,
            S,
            StartTurn {
                runtime_mode: Some(RuntimeMode::FullAccess),
                intent: Some(TurnIntent::Plan),
                ..Default::default()
            },
        )
        .await;
        h.request(
            S,
            json!(91),
            "mcpServer/elicitation/request",
            json!({
                "serverName": "example", "mode": "form", "message": "Read this source?",
                "requestedSchema": { "type": "object", "properties": { "approved": { "type": "boolean" } }, "required": ["approved"] },
            }),
        );
        wait_for("plan MCP approval UI", || {
            started.events.has("approval.requested")
        })
        .await;
        let approval = started.events.of_type("approval.requested")[0].clone();
        h.adapter
            .sessions()
            .respond_approval(S, request_id(&approval), ApprovalDecision::Allow);
        wait_for("MCP response", || h.reply_to(json!(91)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "action": "accept", "content": { "approved": true }, "_meta": null })
        );
        finish(&h, started).await;
    });
}

const SHORT: Duration = Duration::from_millis(400);

fn short_harness() -> Harness {
    Harness::with(HarnessOptions {
        question_auto_resolve: SHORT,
        ..Default::default()
    })
}

#[test]
fn honors_is_blocking_without_relying_on_deprecated_auto_resolution_ms() {
    for is_blocking in [Some(false), Some(true), None] {
        run(async {
            let h = short_harness();
            let started = start_turn(&h, S, StartTurn::default()).await;
            let mut params = json!({
                "autoResolutionMs": 1,
                "questions": [{ "id": "q", "question": "Choose a source", "options": null }],
            });
            if let Some(is_blocking) = is_blocking {
                params["isBlocking"] = json!(is_blocking);
            }
            h.request(S, json!(91), "item/tool/requestUserInput", params);
            wait_for("question", || started.events.has("question.asked")).await;
            let question = started.events.of_type("question.asked")[0].clone();
            if is_blocking == Some(false) {
                assert_eq!(
                    question["autoResolveAt"],
                    json!(h.now() + SHORT.as_millis() as i64)
                );
            } else {
                assert_no_key(&question, "autoResolveAt");
            }
            settle().await;
            assert!(h.reply_to(json!(91)).is_none());
            smol::Timer::after(SHORT * 2).await;
            if is_blocking == Some(false) {
                wait_for("auto skip", || h.reply_to(json!(91)).is_some()).await;
                assert_eq!(
                    h.reply_to(json!(91)).unwrap()["result"],
                    json!({ "answers": {} })
                );
                assert!(started.events.json().contains(&json!({
                    "type": "question.resolved", "requestId": request_id(&question), "decision": "skipped",
                })));
            } else {
                assert!(h.reply_to(json!(91)).is_none());
            }
            finish(&h, started).await;
        });
    }
}

#[test]
fn keeps_an_optional_question_open_after_interaction_and_preserves_its_answer() {
    run(async {
        let h = short_harness();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.request(
            S,
            json!(91),
            "item/tool/requestUserInput",
            json!({ "isBlocking": false, "questions": [
                { "id": "q", "question": "Choose a source", "options": null }
            ] }),
        );
        wait_for("question", || started.events.has("question.asked")).await;
        let question = started.events.of_type("question.asked")[0].clone();
        h.adapter
            .sessions()
            .keep_question_open(S, request_id(&question));
        smol::Timer::after(SHORT * 3).await;
        assert!(h.reply_to(json!(91)).is_none());
        let pending = started.events.reduce().pending_question.unwrap();
        assert_eq!(pending.auto_resolve_at, None);
        h.adapter.sessions().respond_question(
            S,
            request_id(&question),
            answered(&[("q", "chosen source")]),
        );
        wait_for("answer", || h.reply_to(json!(91)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "answers": { "q": { "answers": ["chosen source"] } } })
        );
        finish(&h, started).await;
    });
}

#[test]
fn starts_each_queued_optional_questions_deadline_when_it_is_shown() {
    run(async {
        let h = short_harness();
        let started = start_turn(&h, S, StartTurn::default()).await;
        let shown_at = h.now();
        for id in [91, 92] {
            h.request(
                S,
                json!(id),
                "item/tool/requestUserInput",
                json!({ "isBlocking": false, "questions": [
                    { "id": "q", "question": format!("Question {id}"), "options": null }
                ] }),
            );
        }
        wait_for("first question", || started.events.has("question.asked")).await;
        // The second question is shown later, so its deadline starts later.
        h.set_now(shown_at + 120_000);
        let questions = started.events.of_type("question.asked");
        assert_eq!(questions.len(), 1);
        assert_eq!(
            questions[0]["autoResolveAt"],
            json!(shown_at + SHORT.as_millis() as i64)
        );
        wait_for("first skip", || h.reply_to(json!(91)).is_some()).await;
        wait_for("second question", || {
            started.events.of_type("question.asked").len() == 2
        })
        .await;
        assert_eq!(h.replies_to(json!(91)), 1);
        assert!(h.reply_to(json!(92)).is_none());
        let questions = started.events.of_type("question.asked");
        assert_eq!(
            questions[1]["autoResolveAt"],
            json!(shown_at + 120_000 + SHORT.as_millis() as i64)
        );
        wait_for("second skip", || h.reply_to(json!(92)).is_some()).await;
        assert_eq!(h.replies_to(json!(92)), 1);
        assert!(started.events.reduce().pending_question.is_none());
        finish(&h, started).await;
    });
}

#[test]
fn clears_optional_question_timers_without_a_late_reply() {
    for action in ["answer", "server", "stop", "complete"] {
        run(async {
            let h = short_harness();
            let started = start_turn(&h, S, StartTurn::default()).await;
            h.request(
                S,
                json!(91),
                "item/tool/requestUserInput",
                json!({ "isBlocking": false, "questions": [
                    { "id": "q", "question": "Choose a source", "options": null }
                ] }),
            );
            wait_for("question", || started.events.has("question.asked")).await;
            let question = started.events.of_type("question.asked")[0].clone();
            match action {
                "answer" => h.adapter.sessions().respond_question(
                    S,
                    request_id(&question),
                    UserQuestionReply::Skipped,
                ),
                "server" => h.notify(
                    S,
                    "serverRequest/resolved",
                    json!({ "threadId": "thr_1", "requestId": 91 }),
                ),
                "stop" => h.adapter.sessions().stop_session(S).await.unwrap(),
                _ => complete_turn(&h, S, "turn_1"),
            }
            smol::Timer::after(SHORT * 3).await;
            h.adapter.sessions().respond_question(
                S,
                request_id(&question),
                answered(&[("q", "too late")]),
            );
            settle().await;
            assert_eq!(h.replies_to(json!(91)), usize::from(action == "answer"));
            assert_eq!(started.events.of_type("question.resolved").len(), 1);
            complete_turn(&h, S, "turn_1");
            started.turn.await.unwrap();
            h.adapter.sessions().stop_session(S).await.unwrap();
        });
    }
}

#[test]
fn reports_unsupported_mcp_forms_instead_of_returning_an_empty_success() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.request(
            S,
            json!(90),
            "item/tool/requestUserInput",
            json!({ "questions": [{ "id": "q", "question": "Choose a name", "options": null }] }),
        );
        h.request(
            S,
            json!(91),
            "mcpServer/elicitation/request",
            json!({
                "mode": "form",
                "requestedSchema": { "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] },
            }),
        );
        wait_for("MCP cancel", || h.reply_to(json!(91)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "action": "cancel", "content": null, "_meta": null })
        );
        assert!(
            started
                .events
                .texts("status")
                .iter()
                .any(|text| text.contains("does not support yet"))
        );
        h.request(S, json!(92), "future/requestApproval", json!({}));
        wait_for("protocol error", || h.reply_to(json!(92)).is_some()).await;
        assert_match(
            &h.reply_to(json!(92)).unwrap()["error"],
            &json!({ "code": -32601 }),
        );
        let mut busy = monocode_core::session::Session::blank(
            "codex-test",
            monocode_core::harness::HarnessId::Codex,
            "codex:gpt-5.4",
            "/repo",
        );
        busy.busy = Some(true);
        let session = started.events.reduce_onto(busy);
        assert_eq!(session.busy, Some(true));
        let pending = session.pending_question.unwrap();
        assert_eq!(pending.questions[0].id, "q");
        h.adapter.sessions().respond_question(
            S,
            pending.request_id,
            answered(&[("q", "chosen name")]),
        );
        wait_for("remaining answer", || h.reply_to(json!(90)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(90)).unwrap()["result"],
            json!({ "answers": { "q": { "answers": ["chosen name"] } } })
        );
        finish(&h, started).await;
    });
}

#[test]
fn clears_server_resolved_approvals_without_replying_twice() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.request(
            S,
            json!(91),
            "item/commandExecution/requestApproval",
            json!({ "itemId": "cmd", "command": "git status" }),
        );
        wait_for("approval", || started.events.has("approval.requested")).await;
        let request = started.events.of_type("approval.requested")[0].clone();
        h.notify(
            S,
            "serverRequest/resolved",
            json!({ "threadId": "unrelated", "requestId": 91 }),
        );
        settle().await;
        assert!(!started.events.has("approval.resolved"));
        h.notify(
            S,
            "serverRequest/resolved",
            json!({ "threadId": "thr_1", "requestId": 91 }),
        );
        wait_for("approval cleanup", || {
            started.events.has("approval.resolved")
        })
        .await;
        assert!(started.events.json().contains(&json!({
            "type": "approval.resolved", "requestId": request_id(&request), "decision": "cancelled",
        })));
        h.adapter
            .sessions()
            .respond_approval(S, request_id(&request), ApprovalDecision::Allow);
        settle().await;
        assert!(h.reply_to(json!(91)).is_none());
        finish(&h, started).await;
    });
}

#[test]
fn applies_a_queued_access_change_only_when_that_turn_starts() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        let queued = send(
            &h,
            SendTurnInput {
                session: session_input(S, RuntimeMode::FullAccess),
                text: "Continue".into(),
                attachments: None,
            },
            &started.events,
            None,
        );
        settle().await;
        h.request(
            S,
            json!(91),
            "item/commandExecution/requestApproval",
            json!({ "itemId": "cmd", "command": "git status" }),
        );
        wait_for("current turn approval", || {
            started.events.has("approval.requested")
        })
        .await;
        assert!(h.reply_to(json!(91)).is_none());
        let request = started.events.of_type("approval.requested")[0].clone();
        h.adapter
            .sessions()
            .respond_approval(S, request_id(&request), ApprovalDecision::Deny);
        wait_for("current turn decision", || h.reply_to(json!(91)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "decision": "decline" })
        );
        complete_turn(&h, S, "turn_1");
        started.turn.await.unwrap();

        wait_for("queued turn", || h.methods("turn/start").len() == 2).await;
        let next = h.methods("turn/start")[1].clone();
        assert_match(
            &next["params"],
            &json!({
                "approvalPolicy": "on-request",
                "approvalsReviewer": "user",
                "sandboxPolicy": { "type": "dangerFullAccess" },
            }),
        );
        h.reply(S, &next["id"], json!({ "turn": { "id": "turn_2" } }));
        h.notify(S, "turn/started", json!({ "turn": { "id": "turn_2" } }));
        h.request(
            S,
            json!(92),
            "item/commandExecution/requestApproval",
            json!({ "itemId": "cmd2", "command": "git status" }),
        );
        wait_for("queued turn approval", || h.reply_to(json!(92)).is_some()).await;
        assert_eq!(
            h.reply_to(json!(92)).unwrap()["result"],
            json!({ "decision": "accept" })
        );
        complete_turn(&h, S, "turn_2");
        queued.await.unwrap();
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn stays_busy_after_an_agent_message_until_turn_completed() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "item/completed",
            json!({ "item": { "id": "msg_1", "type": "agentMessage", "text": "I'll inspect the changelog first." } }),
        );
        smol::Timer::after(Duration::from_millis(100)).await;
        assert!(!started.turn.is_finished());
        h.notify(
            S,
            "item/started",
            json!({ "item": { "id": "cmd_1", "type": "commandExecution", "command": "git log -1", "status": "inProgress" } }),
        );
        wait_for("tool", || started.events.has("tool.started")).await;
        assert!(!started.turn.is_finished());
        finish(&h, started).await;
    });
}

#[test]
fn keeps_plan_turns_read_only_without_surfacing_approval_prompts() {
    run(async {
        let h = Harness::new();
        let started = start_turn(
            &h,
            S,
            StartTurn {
                runtime_mode: Some(RuntimeMode::Auto),
                intent: Some(TurnIntent::Plan),
                ..Default::default()
            },
        )
        .await;
        assert_match(
            &h.find_method("turn/start").unwrap()["params"],
            &json!({
                "approvalPolicy": "never",
                "sandboxPolicy": { "type": "readOnly" },
                "collaborationMode": { "mode": "plan" },
            }),
        );
        h.request(
            S,
            json!(91),
            "item/commandExecution/requestApproval",
            json!({ "itemId": "cmd_1", "command": "git status --short" }),
        );
        wait_for("silent plan denial", || h.reply_to(json!(91)).is_some()).await;
        assert!(!started.events.has("approval.requested"));
        assert_eq!(
            h.reply_to(json!(91)).unwrap()["result"],
            json!({ "decision": "decline" })
        );
        finish(&h, started).await;
    });
}

#[test]
fn uses_thread_compact_start_and_waits_for_its_turn_to_complete() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        complete_turn(&h, S, "turn_1");
        started.turn.await.unwrap();
        h.clear_sent();

        let sessions = h.adapter.sessions().clone();
        let compact = smol::spawn(async move {
            sessions
                .compact_context(
                    session_input(S, RuntimeMode::Supervised),
                    Events::default().sink(),
                )
                .await
        });
        wait_for("thread/compact/start", || {
            h.find_method("thread/compact/start").is_some()
        })
        .await;
        let request = h.find_method("thread/compact/start").unwrap();
        assert_eq!(request["params"], json!({ "threadId": "thr_1" }));
        h.reply(S, &request["id"], json!({}));
        h.notify(
            S,
            "turn/started",
            json!({ "turn": { "id": "compact_1", "status": "inProgress" } }),
        );
        smol::Timer::after(Duration::from_millis(30)).await;
        assert!(!compact.is_finished());
        complete_turn(&h, S, "compact_1");
        compact.await.unwrap();
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

fn rewind_input(provider_turn_id: Option<&str>) -> RewindLastTurnInput {
    RewindLastTurnInput {
        session: session_input(S, RuntimeMode::Supervised),
        provider_turn_id: provider_turn_id.map(str::to_string),
        text: None,
        attachments: None,
    }
}

async fn finished_turn(h: &Harness) {
    let started = start_turn(h, S, StartTurn::default()).await;
    complete_turn(h, S, "turn_1");
    started.turn.await.unwrap();
    h.clear_sent();
}

#[test]
fn reverts_before_the_latest_user_turn_after_compaction() {
    run(async {
        let h = Harness::new();
        finished_turn(&h).await;
        let sessions = h.adapter.sessions().clone();
        let rollback = smol::spawn(async move {
            sessions
                .rewind_last_turn(rewind_input(None), Events::default().sink())
                .await
        });
        wait_for("thread/turns/list", || {
            h.find_method("thread/turns/list").is_some()
        })
        .await;
        let list = h.find_method("thread/turns/list").unwrap();
        assert_eq!(
            list["params"],
            json!({ "threadId": "thr_1", "limit": 100, "sortDirection": "desc", "itemsView": "summary" })
        );
        h.reply(
            S,
            &list["id"],
            json!({ "data": [
                { "id": "compact_1", "items": [{ "type": "contextCompaction", "id": "compact_item" }] },
                { "id": "turn_1", "items": [{ "type": "userMessage", "id": "user_item" }] },
            ] }),
        );
        wait_for("thread/revert", || h.find_method("thread/revert").is_some()).await;
        let request = h.find_method("thread/revert").unwrap();
        assert_eq!(
            request["params"],
            json!({ "threadId": "thr_1", "beforeTurnId": "turn_1" })
        );
        h.reply(S, &request["id"], json!({}));
        assert_eq!(
            rollback.await.unwrap(),
            RewindLastTurnResult { submitted: false }
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn uses_the_persisted_provider_turn_boundary_without_listing_turns() {
    run(async {
        let h = Harness::new();
        finished_turn(&h).await;
        let sessions = h.adapter.sessions().clone();
        let rollback = smol::spawn(async move {
            sessions
                .rewind_last_turn(rewind_input(Some("turn_exact")), Events::default().sink())
                .await
        });
        wait_for("thread/revert", || h.find_method("thread/revert").is_some()).await;
        assert!(h.find_method("thread/turns/list").is_none());
        let request = h.find_method("thread/revert").unwrap();
        assert_eq!(
            request["params"],
            json!({ "threadId": "thr_1", "beforeTurnId": "turn_exact" })
        );
        h.reply(S, &request["id"], json!({}));
        assert_eq!(
            rollback.await.unwrap(),
            RewindLastTurnResult { submitted: false }
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn rejects_editing_when_the_provider_exposes_no_user_turn() {
    run(async {
        let h = Harness::new();
        finished_turn(&h).await;
        let sessions = h.adapter.sessions().clone();
        let rollback = smol::spawn(async move {
            sessions
                .rewind_last_turn(rewind_input(None), Events::default().sink())
                .await
        });
        wait_for("thread/turns/list", || {
            h.find_method("thread/turns/list").is_some()
        })
        .await;
        let list = h.find_method("thread/turns/list").unwrap();
        h.reply(
            S,
            &list["id"],
            json!({ "data": [
                { "id": "compact_1", "items": [{ "type": "contextCompaction", "id": "compact_item" }] },
            ] }),
        );
        let error = rollback.await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "Codex did not expose a user turn id to edit"
        );
        assert!(h.find_method("thread/revert").is_none());
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn reports_a_spent_usage_limit_before_the_turn_settles() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "account/rateLimits/updated",
            json!({ "rateLimits": {
                "limitId": "codex",
                "primary": { "usedPercent": 100, "windowDurationMins": 300, "resetsAt": 1_900_000_000 },
            } }),
        );
        h.notify(
            S,
            "turn/completed",
            json!({ "turn": { "id": "turn_1", "status": "failed", "error": {
                "message": "You've hit your usage limit.", "codexErrorInfo": "usageLimitExceeded",
            } } }),
        );
        started.turn.await.unwrap();
        let types: Vec<Value> = started
            .events
            .json()
            .into_iter()
            .filter(|event| event["type"] == "usage.limited")
            .collect();
        assert_eq!(
            types,
            vec![json!({ "type": "usage.limited", "resetsAt": 1_900_000_000_000_i64 })]
        );
        h.adapter.sessions().stop_session(S).await.unwrap();
    });
}

#[test]
fn reports_session_ended_when_the_app_server_exits() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        let router = h.fake.router.lock().clone().unwrap();
        router.on_exit(S, Some(1), 7);
        let error = started.turn.await.unwrap_err();
        assert_eq!(error.to_string(), "Codex app-server exited");
        assert!(
            started
                .events
                .json()
                .contains(&json!({ "type": "session.ended", "code": 1 }))
        );
    });
}
