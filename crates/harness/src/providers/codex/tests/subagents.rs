//! Port of the "codex subagents" tests in
//! src/integrations/harness/providers/codex/codexLive.test.ts.

use serde_json::{Value, json};

use super::fake::*;
use super::support::assert_match;

const S: &str = "s1";

fn run(test: impl std::future::Future<Output = ()>) {
    smol::block_on(test);
}

async fn finish(h: &Harness, started: Started) {
    complete_turn(h, S, "turn_1");
    started.turn.await.unwrap();
    h.adapter.sessions().stop_session(S).await.unwrap();
}

fn spawn_item(extra: Value) -> Value {
    let mut item = json!({
        "id": "collab_1",
        "type": "collabAgentToolCall",
        "tool": "spawnAgent",
        "agentsStates": { "thr_child": { "status": "running" } },
    });
    for (key, value) in extra.as_object().unwrap() {
        item[key] = value.clone();
    }
    item
}

/// Every tool row event for one call id, in order.
fn rows_for(events: &Events, call_id: &str) -> Vec<Value> {
    events
        .json()
        .into_iter()
        .filter(|event| {
            (event["type"] == "tool.started" || event["type"] == "tool.updated")
                && event["callId"] == call_id
        })
        .collect()
}

#[test]
fn mirrors_a_child_threads_work_onto_the_row_that_spawned_it() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "item/started",
            json!({ "threadId": "thr_1", "item": spawn_item(json!({
                "status": "inProgress",
                "prompt": "Correctness review\n\nLook for regressions in the diff.",
            })) }),
        );
        h.notify(
            S,
            "item/started",
            json!({ "threadId": "thr_child", "item": {
                "id": "child_cmd", "type": "commandExecution", "command": "npm test", "status": "inProgress",
            } }),
        );
        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_child", "item": {
                "id": "child_msg", "type": "agentMessage", "text": "No regressions found.",
            } }),
        );
        let events = started.events.clone();
        finish(&h, started).await;

        // The spawn is named from its brief, not from the tool that made it.
        assert!(events.json().iter().any(|event| {
            event["type"] == "tool.started"
                && event["kind"] == "agent"
                && event["title"] == "Correctness review"
        }));
        let steps = events.of_type("agent.step");
        assert!(steps.iter().all(|step| step["callId"] == "collab_1"));
        let pairs: Vec<(String, String)> = steps
            .iter()
            .map(|step| {
                (
                    step["kind"].as_str().unwrap().to_string(),
                    step["text"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(
            pairs,
            [
                ("tool".to_string(), "npm test".to_string()),
                ("message".to_string(), "No regressions found.".to_string()),
            ]
        );
    });
}

#[test]
fn materializes_images_generated_by_child_threads() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "item/started",
            json!({ "threadId": "thr_1", "item": spawn_item(json!({ "status": "inProgress" })) }),
        );
        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_child", "item": {
                "id": "child_image", "type": "imageGeneration", "result": "aW1hZ2U=",
            } }),
        );
        let events = started.events.clone();
        wait_for("child image", || events.has("image.generated")).await;
        finish(&h, started).await;
        assert!(events.json().contains(&json!({
            "type": "image.generated",
            "itemId": "child_image",
            "path": IMAGE_PATH,
            "name": "generated-image",
            "mimeType": "image/png",
            "size": 8,
        })));
    });
}

#[test]
fn banks_a_childs_opening_moves_until_its_row_is_known() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "thread/started",
            json!({ "thread": { "id": "thr_child", "model": "gpt-5.6-sol" } }),
        );
        // Codex streams the child's first calls before the spawn item reports
        // which thread it created.
        h.notify(
            S,
            "item/started",
            json!({ "threadId": "thr_child", "item": {
                "id": "child_cmd", "type": "commandExecution", "command": "git diff", "status": "inProgress",
            } }),
        );
        settle().await;
        assert!(!started.events.has("agent.step"));

        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_1", "item": {
                "id": "sa_1", "type": "subAgentActivity", "kind": "started",
                "agentPath": "/root/explore-auth", "agentThreadId": "thr_child",
            } }),
        );
        let events = started.events.clone();
        finish(&h, started).await;

        let session = events.reduce();
        let run = session
            .blocks
            .iter()
            .find(|block| {
                block.tool.as_ref().and_then(|tool| tool.call_id.as_deref()) == Some("sa_1")
            })
            .and_then(|block| block.agent_run.clone())
            .expect("agent run");
        assert_match(
            &serde_json::to_value(&run).unwrap(),
            &json!({ "name": "Explore Auth subagent", "model": "gpt-5.6-sol" }),
        );
        assert_eq!(run.steps.len(), 1);
        let steps: Vec<(String, String, String)> = events
            .of_type("agent.step")
            .iter()
            .map(|step| {
                (
                    step["callId"].as_str().unwrap().to_string(),
                    step["kind"].as_str().unwrap().to_string(),
                    step["text"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(
            steps,
            [(
                "sa_1".to_string(),
                "tool".to_string(),
                "git diff".to_string()
            )]
        );
    });
}

#[test]
fn shows_one_row_per_spawned_agent_however_codex_describes_it() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "item/started",
            json!({ "threadId": "thr_1", "item": spawn_item(json!({
                "status": "inProgress", "prompt": "Correctness review",
            })) }),
        );
        // The same agent, described again by the older item type.
        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_1", "item": {
                "id": "sa_1", "type": "subAgentActivity", "kind": "started",
                "agentPath": "/root/explore-auth", "agentThreadId": "thr_child",
            } }),
        );
        let events = started.events.clone();
        finish(&h, started).await;

        let mut ids: Vec<String> = Vec::new();
        for event in events.json() {
            if (event["type"] == "tool.started" || event["type"] == "tool.updated")
                && event["kind"] == "agent"
            {
                let id = event["callId"].as_str().unwrap().to_string();
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        // One row, however many times its state is reported.
        assert_eq!(ids, ["collab_1"]);
    });
}

#[test]
fn still_gives_a_failed_duplicate_its_own_row() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "item/started",
            json!({ "threadId": "thr_1", "item": spawn_item(json!({
                "status": "inProgress", "prompt": "Correctness review",
            })) }),
        );
        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_1", "item": {
                "id": "sa_1", "type": "subAgentActivity", "kind": "interrupted", "agentThreadId": "thr_child",
            } }),
        );
        let events = started.events.clone();
        finish(&h, started).await;
        assert!(events.json().iter().any(|event| {
            event["type"] == "tool.updated"
                && event["callId"] == "sa_1"
                && event["status"] == "failed"
        }));
    });
}

#[test]
fn keeps_a_spawned_agent_running_until_its_own_state_says_otherwise() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        let spawn = spawn_item(json!({ "prompt": "Correctness review" }));
        h.notify(
            S,
            "item/started",
            json!({ "threadId": "thr_1", "item": spawn }),
        );
        // The spawn call returns almost at once. The agent it started has not
        // finished, so the row must not settle here.
        let mut done = spawn.clone();
        done["status"] = json!("completed");
        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_1", "item": done }),
        );
        settle().await;
        let before_wait: Vec<Value> = rows_for(&started.events, "collab_1")
            .into_iter()
            .filter(|event| event["type"] == "tool.updated")
            .collect();
        assert!(
            before_wait
                .iter()
                .all(|event| event["status"] == "in_progress")
        );

        // Waiting on the agent is where Codex reports what became of it.
        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_1", "item": {
                "id": "collab_2", "type": "collabAgentToolCall", "tool": "wait", "status": "completed",
                "receiverThreadIds": ["thr_child"],
                "agentsStates": { "thr_child": { "status": "completed", "message": "ok" } },
            } }),
        );
        let events = started.events.clone();
        finish(&h, started).await;
        let last = rows_for(&events, "collab_1").pop().unwrap();
        assert_match(&last, &json!({ "kind": "agent", "status": "completed" }));
    });
}

#[test]
fn never_leaves_an_agent_row_running_once_the_turn_is_over() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "item/started",
            json!({ "threadId": "thr_1", "item": spawn_item(json!({ "prompt": "Correctness review" })) }),
        );
        // Codex never reports a closing state for this child.
        let events = started.events.clone();
        finish(&h, started).await;
        let last = rows_for(&events, "collab_1").pop().unwrap();
        assert_match(
            &last,
            &json!({ "kind": "agent", "title": "Correctness review", "status": "completed" }),
        );
    });
}

#[test]
fn keeps_a_child_thread_out_of_the_parents_own_transcript() {
    run(async {
        let h = Harness::new();
        let started = start_turn(&h, S, StartTurn::default()).await;
        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_1", "item": {
                "id": "sa_1", "type": "subAgentActivity", "kind": "started",
                "agentPath": "/root/explore-auth", "agentThreadId": "thr_child",
            } }),
        );
        h.notify(
            S,
            "item/completed",
            json!({ "threadId": "thr_child", "item": {
                "id": "child_msg", "type": "agentMessage", "text": "Child talking.",
            } }),
        );
        let events = started.events.clone();
        finish(&h, started).await;
        assert!(
            !events
                .message_text()
                .iter()
                .any(|text| text.contains("Child talking."))
        );
    });
}
