//! Port of antigravityLive.test.ts on a scripted child backend.
//!
//! The TypeScript advanced fake timers past the real timeouts. Here each
//! timing case shortens the matching [`AntigravityOptions`] field instead.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use monocode_core::harness::HarnessId;
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent, SendTurnInput};
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::core::catalog::SharedCatalog;
use crate::core::child::Children;
use crate::core::json_rpc::JsonRpcClientOptions;
use crate::core::registry::EventSink;
use crate::core::task::SmolSpawner;

use super::catalog::CatalogRefresh;
use super::session::{AntigravityOptions, AntigravitySessions};

// With the cursor feature on, the file is already Cursor's test module.
#[cfg(feature = "cursor")]
use crate::providers::cursor::fake;
#[cfg(not(feature = "cursor"))]
#[path = "../cursor/fake.rs"]
mod fake;

use fake::{Wire, fake_children, flush, wait_for};

const PATH: &str = "/fake/agy_acp_server.par";
const THREAD: &str = "thread";

/// The `mock` flags in antigravityLive.test.ts.
#[derive(Default)]
struct Mock {
    fail: Mutex<HashSet<String>>,
    silent: Mutex<HashSet<String>>,
    auto_prompt: AtomicBool,
    prompt_stop: Mutex<String>,
    setup_config_options: Mutex<Option<Value>>,
    set_config_result: Mutex<Option<Value>>,
}

fn default_config_options() -> Value {
    json!([
        { "id": "model", "category": "model", "currentValue": "m1", "options": [
            { "value": "m1", "name": "Model One" }, { "value": "m2", "name": "Model Two" },
        ] },
        { "id": "thinking", "category": "thought_level", "currentValue": "low", "options": [
            { "value": "low", "name": "Low" }, { "value": "high", "name": "High" },
        ] },
    ])
}

/// The mock `writeChild`: answer requests on the next tick.
fn respond(mock: &Mock, message: &Value) -> Vec<Value> {
    let (Some(method), Some(id)) = (
        message.get("method").and_then(Value::as_str),
        message.get("id"),
    ) else {
        return Vec::new();
    };
    if mock.silent.lock().contains(method) {
        return Vec::new();
    }
    if method == "session/prompt" && !mock.auto_prompt.load(Ordering::SeqCst) {
        return Vec::new();
    }
    if mock.fail.lock().contains(method) {
        let text = if method == "session/new" {
            "Authentication required"
        } else {
            "unsupported"
        };
        return vec![
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": text } }),
        ];
    }
    let mut lines = Vec::new();
    if method == "session/load" {
        lines.push(json!({ "jsonrpc": "2.0", "method": "session/update", "params": {
            "update": { "sessionUpdate": "agent_message_chunk", "content": { "text": "OLD HISTORY" } }
        } }));
    }
    let setup = json!({
        "sessionId": "provider-session",
        "configOptions": mock.setup_config_options.lock().clone().unwrap_or_else(default_config_options),
    });
    let result = match method {
        "session/new" | "session/load" | "session/resume" => setup,
        "session/prompt" => json!({ "stopReason": *mock.prompt_stop.lock() }),
        "session/set_config_option" => mock
            .set_config_result
            .lock()
            .clone()
            .unwrap_or_else(|| json!({})),
        _ => json!({}),
    };
    lines.push(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    lines
}

struct H {
    sessions: Arc<AntigravitySessions>,
    wire: Arc<Wire>,
    mock: Arc<Mock>,
    children: Children,
    catalog: SharedCatalog,
}

/// Generous timeouts: only the timing cases shorten them.
fn test_options() -> AntigravityOptions {
    AntigravityOptions {
        init_timeout_ms: 5_000,
        session_timeout_ms: 5_000,
        control_timeout_ms: 5_000,
        prompt_timeout_ms: 10_000,
        stall_notify_ms: 120_000,
        rpc: JsonRpcClientOptions {
            write_timeout: Duration::from_secs(5),
            ..JsonRpcClientOptions::default()
        },
    }
}

fn harness_with(options: AntigravityOptions) -> H {
    let (children, wire) = fake_children(PATH, Some(vec!["--uid=".into()]));
    let mock = Arc::new(Mock::default());
    *mock.prompt_stop.lock() = "end_turn".into();
    let responder_mock = mock.clone();
    wire.set_responder(move |_, _, message| respond(&responder_mock, message));
    let catalog = SharedCatalog::new();
    let sessions = AntigravitySessions::new(
        children.clone(),
        Arc::new(SmolSpawner),
        catalog.clone(),
        options,
    );
    H {
        sessions,
        wire,
        mock,
        children,
        catalog,
    }
}

fn harness() -> H {
    harness_with(test_options())
}

type Events = Arc<Mutex<Vec<HarnessEvent>>>;

fn recorder() -> (Events, EventSink) {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    (events, Arc::new(move |event| sink.lock().push(event)))
}

fn input_with(patch: Value) -> SendTurnInput {
    let mut base = json!({
        "sessionId": THREAD, "cwd": "/repo", "model": "antigravity:m1", "text": "hi", "runtimeMode": "supervised",
    });
    for (key, value) in patch.as_object().unwrap() {
        base[key] = value.clone();
    }
    serde_json::from_value(base).unwrap()
}

fn input() -> SendTurnInput {
    input_with(json!({}))
}

/// `provider.send(input)`: the startup is submitted now, the rest runs on a task.
fn send(h: &H, input: SendTurnInput, sink: &EventSink) -> smol::Task<anyhow::Result<()>> {
    smol::spawn(h.sessions.send_antigravity_turn(input, sink.clone()))
}

fn json_events(events: &Events) -> Vec<Value> {
    events
        .lock()
        .iter()
        .map(|event| serde_json::to_value(event).unwrap())
        .collect()
}

fn has_type(events: &Events, kind: &str) -> bool {
    json_events(events)
        .iter()
        .any(|event| event["type"] == kind)
}

fn count_type(events: &Events, kind: &str) -> usize {
    json_events(events)
        .iter()
        .filter(|event| event["type"] == kind)
        .count()
}

fn contains(events: &Events, expected: Value) -> bool {
    json_events(events).contains(&expected)
}

fn is_generation(key: &str) -> bool {
    key.strip_prefix("thread#")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// `liveKey`: the newest generation's child key.
fn live_key(h: &H) -> String {
    h.wire
        .spawned_ids()
        .into_iter()
        .rfind(|id| is_generation(id))
        .unwrap()
}

fn killed_generation(h: &H) -> bool {
    h.wire.kills.lock().iter().any(|key| is_generation(key))
}

/// `permission(kind, id)`.
fn permission(h: &H, kind: &str, id: i64) {
    h.wire.request(
        &live_key(h),
        id,
        "session/request_permission",
        json!({
            "toolCall": { "toolCallId": "tool-1", "title": "Do work", "kind": kind },
            "options": [{ "optionId": "yes", "kind": "allow_once" }, { "optionId": "no", "kind": "reject_once" }],
        }),
    );
}

/// `finishPrompt`.
fn finish_prompt(h: &H) {
    let prompt = h.wire.last_outbound("session/prompt").unwrap();
    let stop = h.mock.prompt_stop.lock().clone();
    h.wire.reply(
        &prompt.session,
        &prompt.message["id"],
        json!({ "stopReason": stop }),
    );
}

fn response(h: &H, id: i64) -> Option<Value> {
    h.wire.response(id)
}

async fn wait_prompt(h: &H) {
    wait_for(|| h.wire.count("session/prompt") > 0, "session/prompt").await;
}

fn sent_params(h: &H, method: &str) -> Value {
    h.wire
        .outbound(method)
        .map(|message| message["params"].clone())
        .unwrap_or(Value::Null)
}

#[test]
fn spawns_the_exact_endpoint_sends_settings_and_images_and_routes_real_approvals() {
    smol::block_on(async {
        let h = harness();
        let (events, sink) = recorder();
        let turn = send(
            &h,
            input_with(json!({
                "modelSettings": { "effort": "high" },
                "attachments": [{ "id": "img", "name": "img.png", "kind": "image", "mimeType": "image/png", "size": 4, "data": "aGV5" }],
            })),
            &sink,
        );
        wait_prompt(&h).await;
        let spawn = h.wire.spawns.lock()[0].clone();
        assert!(is_generation(&spawn.session_id));
        assert_eq!(spawn.command, PATH);
        assert_eq!(spawn.args, vec!["--uid=".to_string()]);
        assert_eq!(spawn.cwd, "/fake/");
        assert_eq!(spawn.account, None);
        assert_eq!(spawn.binary_provider, Some(HarnessId::Antigravity));
        assert_eq!(sent_params(&h, "initialize")["protocolVersion"], 1);
        let set = sent_params(&h, "session/set_config_option");
        assert_eq!(
            (set["configId"].clone(), set["value"].clone()),
            (json!("thinking"), json!("high"))
        );
        let prompt = sent_params(&h, "session/prompt")["prompt"].clone();
        assert_eq!(prompt[0], json!({ "type": "text", "text": "hi" }));
        assert_eq!(
            (prompt[1]["type"].clone(), prompt[1]["data"].clone()),
            (json!("image"), json!("aGV5"))
        );
        assert_eq!(sent_params(&h, "session/new")["cwd"], "/repo");
        permission(&h, "execute", 100);
        wait_for(|| has_type(&events, "approval.requested"), "approval").await;
        assert_eq!(response(&h, 100), None);
        h.sessions
            .respond_antigravity_approval(THREAD, 100, ApprovalDecision::Allow);
        wait_for(
            || {
                response(&h, 100)
                    == Some(json!({ "outcome": { "outcome": "selected", "optionId": "yes" } }))
            },
            "allow reply",
        )
        .await;
        finish_prompt(&h);
        turn.await.unwrap();
        assert!(contains(
            &events,
            json!({ "type": "session.providerBound", "providerSessionId": "provider-session" })
        ));
        assert!(contains(&events, json!({ "type": "message.completed" })));
    });
}

#[test]
fn denies_edits_in_plan_intent_even_with_full_access_selected() {
    smol::block_on(async {
        let h = harness();
        let (events, sink) = recorder();
        let turn = send(
            &h,
            input_with(json!({ "runtimeMode": "full-access", "intent": "plan" })),
            &sink,
        );
        wait_prompt(&h).await;
        assert_eq!(sent_params(&h, "session/set_mode")["modeId"], "default");
        permission(&h, "edit", 100);
        wait_for(
            || {
                response(&h, 100)
                    == Some(json!({ "outcome": { "outcome": "selected", "optionId": "no" } }))
            },
            "deny reply",
        )
        .await;
        assert!(!has_type(&events, "approval.requested"));
        finish_prompt(&h);
        turn.await.unwrap();
    });
}

#[test]
fn rejects_permission_requests_outside_an_active_prompt() {
    smol::block_on(async {
        let h = harness();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (events, sink) = recorder();
        send(
            &h,
            input_with(json!({ "runtimeMode": "full-access" })),
            &sink,
        )
        .await
        .unwrap();
        permission(&h, "execute", 101);
        wait_for(
            || response(&h, 101) == Some(json!({ "outcome": { "outcome": "cancelled" } })),
            "cancelled reply",
        )
        .await;
        assert!(!has_type(&events, "approval.requested"));
        assert!(!has_type(&events, "tool.updated"));
    });
}

#[test]
fn cancels_a_pending_approval_and_suppresses_late_text() {
    smol::block_on(async {
        let h = harness();
        let (events, sink) = recorder();
        let turn = send(&h, input(), &sink);
        wait_prompt(&h).await;
        permission(&h, "execute", 100);
        wait_for(|| has_type(&events, "approval.requested"), "approval").await;
        h.sessions.cancel_antigravity_turn_now(THREAD);
        turn.await.unwrap();
        wait_for(
            || response(&h, 100) == Some(json!({ "outcome": { "outcome": "cancelled" } })),
            "cancelled reply",
        )
        .await;
        h.wire.notify(
            &live_key(&h),
            "session/update",
            json!({ "update": { "sessionUpdate": "agent_message_chunk", "content": { "text": "late" } } }),
        );
        flush().await;
        assert!(!has_type(&events, "message.delta"));
        wait_for(|| h.wire.count("session/cancel") > 0, "session/cancel").await;
    });
}

#[test]
fn fails_closed_when_the_provider_rejects_mode_selection() {
    smol::block_on(async {
        let h = harness();
        h.mock.fail.lock().insert("session/set_mode".into());
        let (_, sink) = recorder();
        let error = send(&h, input(), &sink).await.unwrap_err();
        assert!(error.to_string().contains("unsupported"), "{error}");
        assert_eq!(h.wire.count("session/prompt"), 0);
        assert!(killed_generation(&h));
    });
}

fn uses_the_branch_of_session_recovery_without_replay(branch: &str) {
    smol::block_on(async {
        let h = harness();
        h.sessions
            .bind_antigravity_session(THREAD, "saved-session", "/repo");
        if branch != "resume" {
            h.mock.fail.lock().insert("session/resume".into());
        }
        if branch == "new" {
            h.mock.fail.lock().insert("session/load".into());
        }
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (events, sink) = recorder();
        send(&h, input(), &sink).await.unwrap();
        let methods = h.wire.methods();
        assert!(methods.iter().any(|m| m == "session/resume"));
        assert_eq!(
            methods.iter().any(|m| m == "session/load"),
            branch != "resume"
        );
        assert_eq!(methods.iter().any(|m| m == "session/new"), branch == "new");
        assert!(!has_type(&events, "message.delta"));
    });
}

#[test]
fn uses_the_resume_branch_of_session_recovery_without_replay() {
    uses_the_branch_of_session_recovery_without_replay("resume");
}

#[test]
fn uses_the_load_branch_of_session_recovery_without_replay() {
    uses_the_branch_of_session_recovery_without_replay("load");
}

#[test]
fn uses_the_new_branch_of_session_recovery_without_replay() {
    uses_the_branch_of_session_recovery_without_replay("new");
}

#[test]
fn parks_and_resumes_then_forgets_the_provider_binding() {
    smol::block_on(async {
        let h = harness();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (_, sink) = recorder();
        send(&h, input(), &sink).await.unwrap();
        h.sessions.stop_antigravity_session(THREAD).await.unwrap();
        h.wire.clear_sent();
        send(&h, input(), &sink).await.unwrap();
        assert!(h.wire.count("session/resume") > 0);
        h.sessions.forget_antigravity_session(THREAD).await.unwrap();
        h.wire.clear_sent();
        send(&h, input(), &sink).await.unwrap();
        assert!(h.wire.count("session/new") > 0);
        assert_eq!(h.wire.count("session/resume"), 0);
    });
}

#[test]
fn adds_actionable_authentication_help_and_cleans_up_failed_setup() {
    smol::block_on(async {
        let h = harness();
        h.mock.fail.lock().insert("session/new".into());
        let (_, sink) = recorder();
        let error = send(&h, input(), &sink).await.unwrap_err();
        assert!(error.to_string().contains("agy` once"), "{error}");
        assert!(killed_generation(&h));
    });
}

#[test]
fn probes_catalogs_over_acp_once_kills_the_probe_and_preserves_models_on_failure() {
    smol::block_on(async {
        let h = harness();
        let refresh = Arc::new(CatalogRefresh::default());
        let spawner: crate::core::task::SharedSpawner = Arc::new(SmolSpawner);
        let first = refresh.refresh(h.children.clone(), spawner.clone(), h.catalog.clone());
        let second = refresh.refresh(h.children.clone(), spawner.clone(), h.catalog.clone());
        assert!(first.ptr_eq(&second));
        first.await;
        let native_ids = |h: &H| {
            h.catalog
                .read()
                .models_for(HarnessId::Antigravity)
                .iter()
                .filter_map(|model| model.native_id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(native_ids(&h), ["m1", "m2"]);
        let probe = h
            .wire
            .spawns
            .lock()
            .iter()
            .find(|spawn| spawn.session_id.starts_with("monocode-antigravity-probe-"))
            .cloned()
            .unwrap();
        assert_eq!(probe.command, PATH);
        assert_eq!(probe.args, vec!["--uid=".to_string()]);
        assert_eq!(probe.cwd, "/fake/");
        assert_eq!(probe.binary_provider, Some(HarnessId::Antigravity));
        assert!(h.wire.killed(&probe.session_id));
        assert_eq!(h.wire.spawn_count(), 1);
        h.mock.fail.lock().insert("session/new".into());
        refresh
            .refresh(h.children.clone(), spawner, h.catalog.clone())
            .await;
        assert_eq!(native_ids(&h), ["m1", "m2"]);
        assert_eq!(h.wire.count("authenticate"), 0);
    });
}

#[test]
fn serializes_two_concurrent_first_sends_through_one_startup() {
    smol::block_on(async {
        let h = harness();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (_, sink) = recorder();
        let (second_events, second_sink) = recorder();
        let first = send(&h, input(), &sink);
        let second = send(&h, input_with(json!({ "text": "second" })), &second_sink);
        first.await.unwrap();
        second.await.unwrap();
        assert_eq!(h.wire.spawn_count(), 1);
        assert_eq!(h.wire.count("session/prompt"), 2);
        assert!(contains(
            &second_events,
            json!({ "type": "message.completed" })
        ));
    });
}

#[test]
fn keeps_the_running_turns_listener_and_policy_while_a_send_is_queued() {
    smol::block_on(async {
        let h = harness();
        let (events, sink) = recorder();
        let first = send(&h, input(), &sink);
        wait_prompt(&h).await;
        let (second_events, second_sink) = recorder();
        let second = send(
            &h,
            input_with(json!({ "text": "next", "runtimeMode": "full-access" })),
            &second_sink,
        );
        permission(&h, "execute", 100);
        // The queued send's full-access mode must not auto-approve turn 1's
        // prompt, and turn 2's listener must not see turn 1's approval card.
        wait_for(|| has_type(&events, "approval.requested"), "approval").await;
        assert!(!has_type(&second_events, "approval.requested"));
        assert_eq!(response(&h, 100), None);
        h.sessions
            .respond_antigravity_approval(THREAD, 100, ApprovalDecision::Allow);
        wait_for(|| response(&h, 100).is_some(), "reply").await;
        finish_prompt(&h);
        first.await.unwrap();
        wait_for(|| h.wire.count("session/prompt") == 2, "second prompt").await;
        finish_prompt(&h);
        second.await.unwrap();
        assert!(contains(
            &second_events,
            json!({ "type": "message.completed" })
        ));
    });
}

#[test]
fn recycles_the_transport_after_a_mid_prompt_cancel_instead_of_reusing_it() {
    smol::block_on(async {
        let h = harness();
        let (events, sink) = recorder();
        let first = send(&h, input(), &sink);
        wait_prompt(&h).await;
        h.sessions.cancel_antigravity_turn_now(THREAD);
        first.await.unwrap();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        h.wire.clear_sent();
        send(&h, input_with(json!({ "text": "again" })), &sink)
            .await
            .unwrap();
        assert!(killed_generation(&h));
        assert_eq!(h.wire.spawn_count(), 2);
        assert!(h.wire.count("session/resume") > 0);
        assert!(h.wire.count("session/prompt") > 0);
        assert!(contains(&events, json!({ "type": "message.completed" })));
    });
}

#[test]
fn reuses_the_live_transport_when_a_cancel_landed_between_turns() {
    smol::block_on(async {
        let h = harness();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (_, sink) = recorder();
        send(&h, input(), &sink).await.unwrap();
        h.sessions.cancel_antigravity_turn_now(THREAD);
        h.wire.clear_sent();
        send(&h, input_with(json!({ "text": "again" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 1);
        assert_eq!(h.wire.count("session/resume"), 0);
        assert_eq!(h.wire.count("session/prompt"), 1);
    });
}

#[test]
fn flags_a_wedged_prompt_after_two_quiet_minutes_and_recovers_on_resend() {
    smol::block_on(async {
        let stall = 300u64;
        let h = harness_with(AntigravityOptions {
            stall_notify_ms: stall as i64,
            ..test_options()
        });
        let (events, sink) = recorder();
        let turn = send(&h, input(), &sink);
        wait_prompt(&h).await;
        let quiet = || {
            !json_events(&events)
                .iter()
                .any(|event| event["type"] == "status")
        };
        // Traffic resets the silence clock.
        smol::Timer::after(Duration::from_millis(stall * 2 / 3)).await;
        h.wire.notify(
            &live_key(&h),
            "session/update",
            json!({ "update": { "sessionUpdate": "agent_message_chunk", "content": { "text": "still here" } } }),
        );
        smol::Timer::after(Duration::from_millis(stall * 2 / 3)).await;
        assert!(quiet());
        wait_for(
            || {
                json_events(&events).iter().any(|event| {
                    event["type"] == "status"
                        && event["text"].as_str().unwrap_or("").contains("quiet")
                })
            },
            "stall status",
        )
        .await;
        h.sessions.cancel_antigravity_turn_now(THREAD);
        turn.await.unwrap();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        send(&h, input_with(json!({ "text": "recover" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 2);
        assert!(h.wire.count("session/resume") > 0);
    });
}

#[test]
fn settles_a_pending_approval_and_the_turn_when_the_process_exits() {
    smol::block_on(async {
        let h = harness();
        let (events, sink) = recorder();
        let turn = send(&h, input(), &sink);
        wait_prompt(&h).await;
        permission(&h, "execute", 100);
        wait_for(|| has_type(&events, "approval.requested"), "approval").await;
        h.wire.exit(&live_key(&h), 1);
        turn.await.unwrap();
        wait_for(
            || response(&h, 100) == Some(json!({ "outcome": { "outcome": "cancelled" } })),
            "cancelled reply",
        )
        .await;
        assert!(contains(
            &events,
            json!({ "type": "session.ended", "code": 1 })
        ));
        assert!(contains(
            &events,
            json!({ "type": "approval.resolved", "requestId": 100, "decision": "deny" })
        ));
    });
}

#[test]
fn fails_a_resume_timeout_instead_of_stacking_a_fresh_session_on_top() {
    smol::block_on(async {
        let h = harness_with(AntigravityOptions {
            session_timeout_ms: 200,
            ..test_options()
        });
        h.sessions
            .bind_antigravity_session(THREAD, "saved-session", "/repo");
        h.mock.silent.lock().insert("session/resume".into());
        let (_, sink) = recorder();
        let error = send(&h, input(), &sink).await.unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        assert_eq!(h.wire.count("session/load"), 0);
        assert_eq!(h.wire.count("session/new"), 0);
    });
}

#[test]
fn treats_cancelled_and_refused_stop_reasons_as_ended_turns_not_completions() {
    smol::block_on(async {
        let h = harness();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (events, sink) = recorder();
        let errors = |pattern: &str| {
            json_events(&events).iter().any(|event| {
                event["type"] == "session.error"
                    && event["message"].as_str().unwrap_or("").contains(pattern)
            })
        };
        *h.mock.prompt_stop.lock() = "cancelled".into();
        send(&h, input(), &sink).await.unwrap();
        assert!(!has_type(&events, "message.completed"));
        *h.mock.prompt_stop.lock() = "refusal".into();
        send(&h, input(), &sink).await.unwrap();
        assert!(errors("refusal"));
        *h.mock.prompt_stop.lock() = "max_tokens".into();
        send(&h, input(), &sink).await.unwrap();
        assert!(errors("max_tokens"));
        assert_eq!(count_type(&events, "message.completed"), 0);
    });
}

#[test]
fn stays_quiet_past_the_watchdog_while_an_approval_awaits_the_user() {
    smol::block_on(async {
        let stall = 150u64;
        let h = harness_with(AntigravityOptions {
            stall_notify_ms: stall as i64,
            ..test_options()
        });
        let (events, sink) = recorder();
        let turn = send(&h, input(), &sink);
        wait_prompt(&h).await;
        permission(&h, "execute", 100);
        wait_for(|| has_type(&events, "approval.requested"), "approval").await;
        // The approval wait is user time, not provider silence.
        smol::Timer::after(Duration::from_millis(stall * 3)).await;
        assert!(!has_type(&events, "status"));
        h.sessions
            .respond_antigravity_approval(THREAD, 100, ApprovalDecision::Allow);
        wait_for(|| response(&h, 100).is_some(), "reply").await;
        finish_prompt(&h);
        turn.await.unwrap();
        assert!(contains(&events, json!({ "type": "message.completed" })));
    });
}

#[test]
fn recycles_a_prompt_that_outlives_the_hard_timeout_and_resumes_once() {
    smol::block_on(async {
        let h = harness_with(AntigravityOptions {
            prompt_timeout_ms: 200,
            ..test_options()
        });
        let (_, sink) = recorder();
        let error = send(&h, input(), &sink).await.unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        h.wire.clear_sent();
        send(&h, input_with(json!({ "text": "retry" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 2);
        assert_eq!(h.wire.count("session/resume"), 1);
        assert_eq!(h.wire.count("session/new"), 0);
    });
}

#[test]
fn ignores_a_retired_process_emitting_after_the_transport_was_recycled() {
    smol::block_on(async {
        let h = harness();
        let (_, sink) = recorder();
        let first = send(&h, input(), &sink);
        wait_prompt(&h).await;
        let old_key = live_key(&h);
        h.sessions.cancel_antigravity_turn_now(THREAD);
        first.await.unwrap();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (second_events, second_sink) = recorder();
        let second = send(&h, input_with(json!({ "text": "next" })), &second_sink);
        wait_for(|| h.wire.spawn_count() == 2, "respawn").await;
        // The replacement runs under a new generation key, so the retired
        // child's stdout has no route to the new client.
        assert_ne!(live_key(&h), old_key);
        h.wire.notify(
            &old_key,
            "session/update",
            json!({ "update": { "sessionUpdate": "agent_message_chunk", "content": { "text": "stale" } } }),
        );
        second.await.unwrap();
        assert!(!contains(
            &second_events,
            json!({ "type": "message.delta", "text": "stale" })
        ));
        assert!(contains(
            &second_events,
            json!({ "type": "message.completed" })
        ));
    });
}

#[test]
fn suppresses_a_send_that_was_queued_when_the_running_turn_was_cancelled() {
    smol::block_on(async {
        let h = harness();
        let (_, sink) = recorder();
        let first = send(&h, input(), &sink);
        wait_prompt(&h).await;
        let (_, second_sink) = recorder();
        let second = send(&h, input_with(json!({ "text": "queued" })), &second_sink);
        h.sessions.cancel_antigravity_turn_now(THREAD);
        first.await.unwrap();
        second.await.unwrap();
        // The queued send was pending at cancel time: it must not prompt later.
        assert_eq!(h.wire.count("session/prompt"), 1);
        // A send issued after the cancel proceeds on a recycled transport.
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        send(&h, input_with(json!({ "text": "after" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 2);
        assert!(h.wire.count("session/resume") > 0);
        assert_eq!(h.wire.count("session/prompt"), 2);
    });
}

#[test]
fn retires_an_in_flight_startup_when_the_session_is_cancelled_or_forgotten() {
    smol::block_on(async {
        let h = harness();
        let (_, sink) = recorder();
        for end in ["cancel", "forget"] {
            let release = h.wire.hold_next_spawn();
            let pending = send(&h, input(), &sink);
            wait_for(|| h.wire.spawn_count() == 1, "spawn").await;
            if end == "cancel" {
                h.sessions.cancel_antigravity_turn_now(THREAD);
            } else {
                h.sessions.forget_antigravity_session(THREAD).await.unwrap();
            }
            release.send(Ok(())).await.unwrap();
            pending.await.unwrap();
            // The retired startup kills only its own child and never
            // publishes: no initialize or session setup reached the wire.
            wait_for(|| killed_generation(&h), "generation kill").await;
            assert_eq!(h.wire.count("initialize"), 0, "{end}");
            h.wire.clear_sent();
            h.wire.spawns.lock().clear();
            h.wire.kills.lock().clear();
        }
        // A send after the forgotten session starts cleanly.
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        send(&h, input_with(json!({ "text": "again" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 1);
        assert!(h.wire.count("session/new") > 0);
    });
}

#[test]
fn unwinds_a_cancel_or_stop_that_lands_while_session_setup_is_in_flight() {
    smol::block_on(async {
        let h = harness();
        let (events, sink) = recorder();
        for end in ["cancel", "stop"] {
            h.mock.silent.lock().insert("session/new".into());
            let pending = send(&h, input(), &sink);
            wait_for(|| h.wire.count("session/new") > 0, "session/new").await;
            let key = live_key(&h);
            let started = Instant::now();
            if end == "cancel" {
                h.sessions.cancel_antigravity_turn_now(THREAD);
            } else {
                h.sessions.stop_antigravity_session(THREAD).await.unwrap();
            }
            // A real bridge can still deliver the late exit.
            h.wire.exit(&key, 0);
            // Without the pending-setup abort this would wait out the
            // session-request timeout.
            pending.await.unwrap();
            assert!(started.elapsed() < Duration::from_secs(2), "{end}");
            wait_for(|| killed_generation(&h), "generation kill").await;
            // A setup retired by user intent emits no session.ended.
            assert!(!has_type(&events, "session.ended"), "{end}");
            h.mock.silent.lock().remove("session/new");
            h.wire.clear_sent();
            h.wire.spawns.lock().clear();
            h.wire.kills.lock().clear();
            events.lock().clear();
        }
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        send(&h, input_with(json!({ "text": "again" })), &sink)
            .await
            .unwrap();
        assert!(h.wire.count("session/new") > 0);
    });
}

#[test]
fn never_calls_set_config_option_for_options_the_session_did_not_advertise() {
    smol::block_on(async {
        let h = harness();
        *h.mock.setup_config_options.lock() = Some(json!([]));
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (events, sink) = recorder();
        send(
            &h,
            input_with(json!({ "modelSettings": { "effort": "high" } })),
            &sink,
        )
        .await
        .unwrap();
        assert_eq!(h.wire.count("session/set_config_option"), 0);
        assert!(has_type(&events, "message.completed"));
    });
}

#[test]
fn skips_only_the_unadvertised_option_still_applying_advertised_settings() {
    smol::block_on(async {
        let h = harness();
        *h.mock.setup_config_options.lock() = Some(json!([
            { "id": "thinking", "category": "thought_level", "currentValue": "low", "options": [
                { "value": "low", "name": "Low" }, { "value": "high", "name": "High" },
            ] },
        ]));
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (_, sink) = recorder();
        send(
            &h,
            input_with(json!({ "model": "antigravity:m2", "modelSettings": { "effort": "high" } })),
            &sink,
        )
        .await
        .unwrap();
        assert_eq!(h.wire.count("session/set_config_option"), 1);
        let set = sent_params(&h, "session/set_config_option");
        assert_eq!(
            (set["configId"].clone(), set["value"].clone()),
            (json!("thinking"), json!("high"))
        );
    });
}

#[test]
fn advertises_config_options_support_and_encodes_boolean_options_typed() {
    smol::block_on(async {
        let h = harness();
        *h.mock.setup_config_options.lock() = Some(json!([
            { "id": "model", "category": "model", "currentValue": "m1", "options": [
                { "value": "m1", "name": "Model One" }, { "value": "m2", "name": "Model Two" },
            ] },
            { "id": "fast", "type": "boolean", "currentValue": false },
        ]));
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (_, sink) = recorder();
        send(
            &h,
            input_with(json!({ "modelSettings": { "fast": "true" } })),
            &sink,
        )
        .await
        .unwrap();
        assert_eq!(
            sent_params(&h, "initialize")["clientCapabilities"]["session"],
            json!({ "configOptions": { "boolean": {} } })
        );
        let set = h
            .wire
            .messages()
            .into_iter()
            .find(|m| {
                m["method"] == "session/set_config_option" && m["params"]["configId"] == "fast"
            })
            .unwrap();
        // Boolean options go out as the typed boolean variant, not a string.
        assert_eq!(set["params"]["type"], "boolean");
        assert_eq!(set["params"]["value"], json!(true));
    });
}

#[test]
fn preserves_config_options_after_a_malformed_set_config_response() {
    smol::block_on(async {
        let h = harness();
        *h.mock.set_config_result.lock() = Some(json!({ "configOptions": { "invalid": true } }));
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (_, sink) = recorder();
        send(
            &h,
            input_with(json!({ "modelSettings": { "effort": "high" } })),
            &sink,
        )
        .await
        .unwrap();
        h.wire.clear_sent();
        send(&h, input_with(json!({ "model": "antigravity:m2" })), &sink)
            .await
            .unwrap();
        let set = sent_params(&h, "session/set_config_option");
        assert_eq!(
            (set["configId"].clone(), set["value"].clone()),
            (json!("model"), json!("m2"))
        );
    });
}

#[test]
fn preserves_config_options_after_a_malformed_config_update() {
    smol::block_on(async {
        let h = harness();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let (_, sink) = recorder();
        send(&h, input(), &sink).await.unwrap();
        h.wire.notify(
            &live_key(&h),
            "session/update",
            json!({ "update": { "sessionUpdate": "config_option_update", "configOptions": { "invalid": true } } }),
        );
        flush().await;
        h.wire.clear_sent();
        send(
            &h,
            input_with(json!({ "modelSettings": { "effort": "high" } })),
            &sink,
        )
        .await
        .unwrap();
        let set = sent_params(&h, "session/set_config_option");
        assert_eq!(
            (set["configId"].clone(), set["value"].clone()),
            (json!("thinking"), json!("high"))
        );
    });
}

#[test]
fn routes_a_cancel_reentered_from_session_started_through_the_live_path() {
    smol::block_on(async {
        let h = harness();
        let (events, _) = recorder();
        let fired = Arc::new(AtomicBool::new(false));
        let sessions = Arc::downgrade(&h.sessions);
        let sink: EventSink = {
            let events = events.clone();
            let fired = fired.clone();
            Arc::new(move |event: HarnessEvent| {
                let started = matches!(event, HarnessEvent::SessionStarted);
                events.lock().push(event);
                if started
                    && !fired.swap(true, Ordering::SeqCst)
                    && let Some(sessions) = sessions.upgrade()
                {
                    sessions.cancel_antigravity_turn_now(THREAD);
                }
            })
        };
        send(&h, input(), &sink).await.unwrap();
        assert_eq!(h.wire.count("session/prompt"), 0);
        // The transport survived: the next send reuses it.
        let (_, plain) = recorder();
        let turn = send(&h, input(), &plain);
        wait_prompt(&h).await;
        finish_prompt(&h);
        turn.await.unwrap();
        assert_eq!(h.wire.spawn_count(), 1);
    });
}

#[test]
fn drops_a_live_whose_publish_listener_throws_so_the_next_send_respawns() {
    smol::block_on(async {
        let h = harness();
        let sink: EventSink = Arc::new(|event: HarnessEvent| {
            if matches!(event, HarnessEvent::SessionProviderBound { .. }) {
                panic!("listener boom");
            }
        });
        let error = send(&h, input(), &sink).await.unwrap_err();
        assert!(error.to_string().contains("listener boom"), "{error}");
        // The dead transport must not linger for reuse.
        let (_, plain) = recorder();
        let turn = send(&h, input(), &plain);
        wait_prompt(&h).await;
        finish_prompt(&h);
        turn.await.unwrap();
        assert_eq!(h.wire.spawn_count(), 2);
    });
}

#[test]
fn lets_the_next_waiter_retry_once_a_shared_startup_fails() {
    smol::block_on(async {
        let h = harness();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        let fail = h.wire.hold_next_spawn();
        fail.send(Err("spawn boom".into())).await.unwrap();
        let (_, sink) = recorder();
        let first = send(&h, input(), &sink);
        let second = send(&h, input_with(json!({ "text": "second" })), &sink);
        let error = first.await.unwrap_err();
        assert!(error.to_string().contains("spawn boom"), "{error}");
        second.await.unwrap();
        // The first failure does not poison the chain: the second step
        // rechecks ownership and retries the spawn exactly once.
        assert_eq!(h.wire.spawn_count(), 2);
        assert_eq!(h.wire.count("session/prompt"), 1);
    });
}

#[test]
fn does_not_resurrect_a_session_forgotten_while_sends_were_queued_behind_startup() {
    smol::block_on(async {
        let h = harness();
        let release = h.wire.hold_next_spawn();
        let (_, sink) = recorder();
        let first = send(&h, input(), &sink);
        let second = send(&h, input_with(json!({ "text": "queued" })), &sink);
        wait_for(|| h.wire.spawn_count() == 1, "spawn").await;
        h.sessions.forget_antigravity_session(THREAD).await.unwrap();
        release.send(Ok(())).await.unwrap();
        first.await.unwrap();
        second.await.unwrap();
        // The queued step captured the pre-forget epoch: it never spawns.
        assert_eq!(h.wire.spawn_count(), 1);
        assert_eq!(h.wire.count("session/new"), 0);
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        send(&h, input_with(json!({ "text": "after" })), &sink)
            .await
            .unwrap();
        assert!(h.wire.count("session/new") > 0);
    });
}

#[test]
fn queues_a_different_cwd_send_without_interrupting_the_running_turn() {
    smol::block_on(async {
        let h = harness();
        let (events, sink) = recorder();
        let first = send(&h, input(), &sink);
        wait_prompt(&h).await;
        let second = send(
            &h,
            input_with(json!({ "cwd": "/other", "text": "queued" })),
            &sink,
        );
        flush().await;
        // The eager path must not tear down the live turn for another cwd.
        assert!(h.wire.kills.lock().is_empty());
        // Arm the mock and reset the log before the first prompt settles. The
        // queued send runs on executor threads as soon as the reply lands,
        // so it can write its own session/new and session/prompt before this
        // thread wakes from `first.await`. The TypeScript ran both on one
        // thread and could do this after.
        let prompt = h.wire.last_outbound("session/prompt").unwrap();
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        h.wire.clear_sent();
        let stop = h.mock.prompt_stop.lock().clone();
        h.wire.reply(
            &prompt.session,
            &prompt.message["id"],
            json!({ "stopReason": stop }),
        );
        first.await.unwrap();
        assert!(contains(&events, json!({ "type": "message.completed" })));
        // Once it owns the turn, the queued send recycles onto its own cwd:
        // a moved cwd drops the resume binding and starts a fresh session.
        second.await.unwrap();
        assert_eq!(h.wire.spawn_count(), 2);
        assert!(h.wire.count("session/new") > 0);
        assert_eq!(h.wire.count("session/resume"), 0);
    });
}

#[test]
fn recycles_the_transport_when_a_cancel_lands_during_configuration() {
    smol::block_on(async {
        let h = harness();
        h.mock
            .silent
            .lock()
            .insert("session/set_config_option".into());
        let (_, sink) = recorder();
        let first = send(&h, input_with(json!({ "model": "antigravity:m2" })), &sink);
        wait_for(
            || h.wire.count("session/set_config_option") > 0,
            "set_config_option",
        )
        .await;
        h.sessions.cancel_antigravity_turn_now(THREAD);
        first.await.unwrap();
        // The config request was in flight: the next turn resumes on a fresh
        // process rather than trusting the old one's unknown state.
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        h.wire.clear_sent();
        send(&h, input_with(json!({ "text": "again" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 2);
        assert!(h.wire.count("session/resume") > 0);
    });
}

#[test]
fn fails_a_turn_whose_stdin_write_blocks_instead_of_hanging_forever() {
    smol::block_on(async {
        let h = harness_with(AntigravityOptions {
            rpc: JsonRpcClientOptions {
                write_timeout: Duration::from_millis(150),
                ..JsonRpcClientOptions::default()
            },
            ..test_options()
        });
        h.wire.block_writes.store(true, Ordering::SeqCst);
        let (_, sink) = recorder();
        let error = send(&h, input(), &sink).await.unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        // The wedged generation tears itself down so nothing reuses it.
        assert!(killed_generation(&h));
    });
}

#[test]
fn settles_the_turn_even_when_the_cancel_notification_write_blocks() {
    smol::block_on(async {
        let h = harness();
        let (_, sink) = recorder();
        let first = send(&h, input(), &sink);
        wait_prompt(&h).await;
        h.wire.block_writes.store(true, Ordering::SeqCst);
        let started = Instant::now();
        h.sessions.cancel_antigravity_turn_now(THREAD);
        first.await.unwrap();
        // rejectPending ran before the blocked session/cancel could hang us.
        assert!(started.elapsed() < Duration::from_secs(1));
        h.wire.block_writes.store(false, Ordering::SeqCst);
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        send(&h, input_with(json!({ "text": "again" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 2);
        assert!(h.wire.count("session/resume") > 0);
    });
}

#[test]
fn suppresses_a_queued_send_when_the_session_is_stopped() {
    smol::block_on(async {
        let h = harness();
        let (_, sink) = recorder();
        let first = send(&h, input(), &sink);
        wait_prompt(&h).await;
        let second = send(&h, input_with(json!({ "text": "queued" })), &sink);
        h.sessions.stop_antigravity_session(THREAD).await.unwrap();
        let _ = first.await;
        second.await.unwrap();
        assert_eq!(h.wire.count("session/prompt"), 1);
        // Stop keeps the resume binding: the next turn rebinds on a fresh process.
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        h.wire.clear_sent();
        send(&h, input_with(json!({ "text": "after" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 2);
        assert!(h.wire.count("session/resume") > 0);
    });
}

#[test]
fn never_spawns_when_forget_lands_while_binary_resolution_is_pending() {
    smol::block_on(async {
        let h = harness();
        let (release, gate) = async_channel::bounded::<()>(1);
        *h.wire.resolve_gate.lock() = Some(gate);
        let (_, sink) = recorder();
        let pending = send(&h, input(), &sink);
        wait_for(
            || h.wire.resolve_waiting.load(Ordering::SeqCst) == 1,
            "binary resolution",
        )
        .await;
        // Forget while resolution is suspended: the abandoned startup must
        // not reach spawn at all.
        h.sessions.forget_antigravity_session(THREAD).await.unwrap();
        *h.wire.resolve_gate.lock() = None;
        release.close();
        pending.await.unwrap();
        assert_eq!(h.wire.spawn_count(), 0);
        h.mock.auto_prompt.store(true, Ordering::SeqCst);
        send(&h, input_with(json!({ "text": "after" })), &sink)
            .await
            .unwrap();
        assert_eq!(h.wire.spawn_count(), 1);
        assert!(h.wire.count("session/new") > 0);
    });
}

#[test]
fn fails_the_turn_when_a_permission_reply_cannot_be_written() {
    smol::block_on(async {
        let h = harness_with(AntigravityOptions {
            rpc: JsonRpcClientOptions {
                write_timeout: Duration::from_millis(150),
                ..JsonRpcClientOptions::default()
            },
            ..test_options()
        });
        let (events, sink) = recorder();
        let turn = send(&h, input(), &sink);
        wait_prompt(&h).await;
        permission(&h, "execute", 100);
        wait_for(|| has_type(&events, "approval.requested"), "approval").await;
        h.wire.block_writes.store(true, Ordering::SeqCst);
        h.sessions
            .respond_antigravity_approval(THREAD, 100, ApprovalDecision::Allow);
        // The reply write wedges; the write bound must retire the whole
        // generation, since the provider waits for an answer that never left.
        let error = turn.await.unwrap_err().to_string();
        assert!(
            error.contains("timed out") || error.contains("not running"),
            "{error}"
        );
        assert!(killed_generation(&h));
    });
}
