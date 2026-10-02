//! Port of src/integrations/harness/providers/codex/codexProtocol.test.ts.

use serde_json::{Value, json};

use monocode_core::attachment::{Attachment, AttachmentKind};
use monocode_core::block::TurnIntent;
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent};

use super::super::catalog::parse_codex_model_list;
use super::super::protocol::*;
use super::support::{assert_match, assert_no_key, js};

const SANDBOXED: [RuntimeMode; 3] = [
    RuntimeMode::Supervised,
    RuntimeMode::AutoAcceptEdits,
    RuntimeMode::Auto,
];

fn config(mode: RuntimeMode, controls_agents: bool) -> Value {
    js(&runtime_mode_to_codex_config(mode, controls_agents))
}

fn turn(input: TurnStartInput<'_>) -> Value {
    Value::Object(build_turn_start_params(&input).unwrap())
}

fn events(method: &str, params: Value) -> Value {
    js(&map_codex_notification(method, &params).events)
}

fn first_event(method: &str, params: Value) -> Value {
    events(method, params)[0].clone()
}

// runtimeModeToCodexConfig

#[test]
fn maps_supervised_to_untrusted_read_only() {
    assert_eq!(
        config(RuntimeMode::Supervised, false),
        json!({
            "approvalPolicy": "untrusted",
            "sandbox": "read-only",
            "approvalsReviewer": "user",
            "sandboxPolicy": { "type": "readOnly" },
        })
    );
}

#[test]
fn opens_loopback_for_a_lead_since_every_sandbox_denies_network_by_default() {
    // Without this an orchestration lead cannot reach its own control CLI.
    for mode in SANDBOXED {
        assert_match(
            &config(mode, true)["sandboxPolicy"],
            &json!({ "networkAccess": true }),
        );
    }
    // Ordinary sessions keep the default, and full access needs no flag.
    assert_no_key(
        &config(RuntimeMode::Auto, false)["sandboxPolicy"],
        "networkAccess",
    );
    assert_eq!(
        config(RuntimeMode::FullAccess, true)["sandboxPolicy"],
        json!({ "type": "dangerFullAccess" })
    );
}

#[test]
fn maps_auto_accept_edits_to_workspace_write_with_user_reviewer() {
    assert_match(
        &config(RuntimeMode::AutoAcceptEdits, false),
        &json!({
            "approvalPolicy": "on-request",
            "sandbox": "workspace-write",
            "approvalsReviewer": "user",
            "sandboxPolicy": { "type": "workspaceWrite" },
        }),
    );
}

#[test]
fn maps_auto_to_workspace_write_with_auto_review() {
    assert_match(
        &config(RuntimeMode::Auto, false),
        &json!({
            "approvalPolicy": "on-request",
            "approvalsReviewer": "auto_review",
            "sandboxPolicy": { "type": "workspaceWrite" },
        }),
    );
}

#[test]
fn opens_the_sandbox_network_only_for_a_lead_which_needs_the_control_socket() {
    // Both sandboxed policies default networkAccess to false, which denies
    // loopback too, so the control CLI cannot reach MonoCode without this.
    for mode in SANDBOXED {
        assert_no_key(&config(mode, false)["sandboxPolicy"], "networkAccess");
        assert_match(
            &config(mode, true)["sandboxPolicy"],
            &json!({ "networkAccess": true }),
        );
    }
    // Full access already permits it, and its policy takes no such field.
    assert_eq!(
        config(RuntimeMode::FullAccess, true)["sandboxPolicy"],
        json!({ "type": "dangerFullAccess" })
    );
}

#[test]
fn carries_the_leads_network_grant_onto_the_turn_including_a_plan_turn() {
    let base = || TurnStartInput {
        thread_id: "t",
        runtime_mode: RuntimeMode::Auto,
        ..Default::default()
    };
    assert_match(
        &turn(TurnStartInput {
            controls_agents: true,
            ..base()
        })["sandboxPolicy"],
        &json!({ "type": "workspaceWrite", "networkAccess": true }),
    );
    assert_match(
        &turn(TurnStartInput {
            controls_agents: true,
            intent: Some(TurnIntent::Plan),
            ..base()
        })["sandboxPolicy"],
        &json!({ "type": "readOnly", "networkAccess": true }),
    );
    assert_no_key(&turn(base())["sandboxPolicy"], "networkAccess");
}

#[test]
fn allows_explicit_escalation_requests_in_full_access() {
    assert_match(
        &config(RuntimeMode::FullAccess, false),
        &json!({
            "approvalPolicy": "on-request",
            "approvalsReviewer": "user",
            "sandbox": "danger-full-access",
            "sandboxPolicy": { "type": "dangerFullAccess" },
        }),
    );
}

// buildThreadStartParams / buildTurnStartParams

#[test]
fn includes_model_and_omits_default_service_tier() {
    let thread = Value::Object(build_thread_start_params(&ThreadStartInput {
        cwd: "/tmp/proj",
        runtime_mode: RuntimeMode::Supervised,
        model: Some("gpt-5.4"),
        service_tier: Some("default"),
        ..Default::default()
    }));
    assert_match(
        &thread,
        &json!({ "cwd": "/tmp/proj", "model": "gpt-5.4", "approvalPolicy": "untrusted" }),
    );
    assert_no_key(&thread, "serviceTier");
}

#[test]
fn builds_turn_input_with_text_and_image_attachments() {
    let attachments = [Attachment {
        id: "img".into(),
        name: "shot.png".into(),
        kind: AttachmentKind::Image,
        mime_type: "image/png".into(),
        size: 3,
        data: Some("abc".into()),
        ..Default::default()
    }];
    let params = turn(TurnStartInput {
        thread_id: "thr_1",
        runtime_mode: RuntimeMode::AutoAcceptEdits,
        prompt: Some("hello"),
        attachments: &attachments,
        model: Some("gpt-5.4"),
        effort: Some("high"),
        service_tier: Some("fast"),
        ..Default::default()
    });
    assert_eq!(params["threadId"], "thr_1");
    assert_eq!(params["effort"], "high");
    assert_eq!(params["serviceTier"], "fast");
    assert_eq!(
        params["input"],
        json!([
            { "type": "text", "text": "hello" },
            { "type": "image", "url": "data:image/png;base64,abc" },
        ])
    );
    assert_eq!(params["sandboxPolicy"], json!({ "type": "workspaceWrite" }));
    assert_eq!(
        params["collaborationMode"],
        json!({
            "mode": "default",
            "settings": { "model": "gpt-5.4", "reasoning_effort": "high", "developer_instructions": null },
        })
    );
}

#[test]
fn uses_native_plan_mode_with_a_non_escalating_read_only_sandbox() {
    let params = turn(TurnStartInput {
        thread_id: "thr_1",
        runtime_mode: RuntimeMode::FullAccess,
        prompt: Some("plan this"),
        model: Some("gpt-5.4"),
        intent: Some(TurnIntent::Plan),
        ..Default::default()
    });
    assert_match(
        &params,
        &json!({
            "approvalPolicy": "never",
            "approvalsReviewer": "user",
            "sandboxPolicy": { "type": "readOnly" },
            "collaborationMode": { "mode": "plan", "settings": { "developer_instructions": null } },
        }),
    );
}

#[test]
fn preserves_the_selected_reviewer_in_plan_turns() {
    for mode in [
        RuntimeMode::Supervised,
        RuntimeMode::AutoAcceptEdits,
        RuntimeMode::Auto,
        RuntimeMode::FullAccess,
    ] {
        let params = turn(TurnStartInput {
            thread_id: "thr_1",
            runtime_mode: mode,
            intent: Some(TurnIntent::Plan),
            prompt: Some("inspect"),
            ..Default::default()
        });
        assert_match(
            &params,
            &json!({
                "approvalPolicy": "never",
                "approvalsReviewer": if mode == RuntimeMode::Auto { "auto_review" } else { "user" },
                "sandboxPolicy": { "type": "readOnly" },
            }),
        );
    }
}

#[test]
fn builds_steer_input_with_expected_turn_id() {
    let steer = build_turn_steer_params(&TurnSteerInput {
        thread_id: "thr_1",
        expected_turn_id: "turn_9",
        prompt: Some("focus on tests"),
        attachments: &[],
    })
    .unwrap();
    assert_eq!(
        Value::Object(steer),
        json!({
            "threadId": "thr_1",
            "expectedTurnId": "turn_9",
            "input": [{ "type": "text", "text": "focus on tests" }],
        })
    );
}

// isRecoverableThreadResumeError

#[test]
fn detects_missing_thread_errors() {
    assert!(is_recoverable_thread_resume_error("Thread thr_x not found"));
    assert!(is_recoverable_thread_resume_error("unknown thread id"));
}

#[test]
fn rejects_unrelated_errors() {
    assert!(!is_recoverable_thread_resume_error("rate limited"));
    assert!(!is_recoverable_thread_resume_error("network down"));
}

// mapCodexNotification

#[test]
fn maps_agent_message_deltas() {
    assert_eq!(
        events("item/agentMessage/delta", json!({ "delta": "Hello" })),
        json!([{ "type": "message.delta", "text": "Hello" }])
    );
}

#[test]
fn keeps_task_progress_distinct_from_authored_plan_documents() {
    assert_eq!(
        events(
            "turn/plan/updated",
            json!({
                "turnId": "turn_1",
                "explanation": "The inspection is done.",
                "plan": [
                    { "step": "Inspect", "status": "completed" },
                    { "step": "Implement", "status": "inProgress" },
                ],
            })
        ),
        json!([{
            "type": "tasks.updated",
            "key": "turn_1",
            "explanation": "The inspection is done.",
            "items": [
                { "text": "Inspect", "status": "completed" },
                { "text": "Implement", "status": "in_progress" },
            ],
        }])
    );
    assert_eq!(
        events(
            "item/plan/delta",
            json!({ "itemId": "plan_1", "delta": "# Approach" })
        ),
        json!([{ "type": "plan", "text": "# Approach", "key": "plan_1", "append": true, "streaming": true }])
    );
}

#[test]
fn keeps_whitespace_only_agent_message_deltas() {
    assert_eq!(
        events("item/agentMessage/delta", json!({ "delta": "\n\n" })),
        json!([{ "type": "message.delta", "text": "\n\n" }])
    );
}

#[test]
fn maps_completed_image_generation_items_as_image_events() {
    let item = json!({
        "id": "image_1",
        "type": "imageGeneration",
        "result": "aW1hZ2U=",
        "revisedPrompt": "A clean product photo",
        "savedPath": "/tmp/image_1.png",
    });
    assert_eq!(events("item/started", json!({ "item": item })), json!([]));
    assert_eq!(
        events("item/completed", json!({ "item": item })),
        json!([{
            "type": "image.generated",
            "itemId": "image_1",
            "data": "aW1hZ2U=",
            "name": "generated-image",
            "alt": "A clean product photo",
        }])
    );
}

#[test]
fn does_not_map_an_empty_image_generation_result() {
    assert_eq!(
        events(
            "item/completed",
            json!({ "item": { "id": "image_2", "type": "imageGeneration", "result": "" } })
        ),
        json!([])
    );
}

#[test]
fn maps_reasoning_summary_deltas() {
    assert_eq!(
        events(
            "item/reasoning/summaryTextDelta",
            json!({ "delta": "thinking\u{2026}" })
        ),
        json!([{ "type": "reasoning.delta", "text": "thinking\u{2026}" }])
    );
}

#[test]
fn ignores_user_message_items_echoed_by_codex() {
    let item = json!({
        "id": "msg_1",
        "type": "userMessage",
        "content": [{ "type": "text", "text": "hey are you there" }],
    });
    assert_eq!(events("item/started", json!({ "item": item })), json!([]));
    assert_eq!(events("item/completed", json!({ "item": item })), json!([]));
}

#[test]
fn maps_command_execution_item_lifecycle() {
    assert_match(
        &first_event(
            "item/started",
            json!({ "item": { "id": "cmd_1", "type": "commandExecution", "command": "ls -la", "status": "inProgress" } }),
        ),
        &json!({ "type": "tool.started", "callId": "cmd_1", "title": "ls -la", "kind": "execute" }),
    );
    assert_match(
        &first_event(
            "item/completed",
            json!({ "item": {
                "id": "cmd_1", "type": "commandExecution", "command": "ls -la",
                "status": "completed", "aggregatedOutput": "ok",
            } }),
        ),
        &json!({ "type": "tool.updated", "callId": "cmd_1", "status": "completed", "detail": "ok" }),
    );
}

#[test]
fn uses_codex_command_actions_for_readable_command_rows() {
    assert_match(
        &first_event(
            "item/started",
            json!({ "item": {
                "id": "cmd_read",
                "type": "commandExecution",
                "command": "/bin/zsh -lc \"nl -ba src/lib/orchestration.ts | sed -n '1,260p'\"",
                "cwd": "/Users/me/project",
                "status": "inProgress",
                "commandActions": [
                    {
                        "type": "read",
                        "command": "nl -ba src/lib/orchestration.ts",
                        "name": "orchestration.ts",
                        "path": "/Users/me/project/src/lib/orchestration.ts",
                    },
                    { "type": "unknown", "command": "sed -n '1,260p'" },
                ],
            } }),
        ),
        &json!({
            "type": "tool.started",
            "callId": "cmd_read",
            "title": "Read src/lib/orchestration.ts",
            "kind": "execute",
            "preview": {
                "kind": "shell",
                "path": "/Users/me/project/src/lib/orchestration.ts",
                "fileName": "orchestration.ts",
            },
        }),
    );
}

#[test]
fn falls_back_to_unwrapping_codex_shell_launchers() {
    assert_match(
        &first_event(
            "item/started",
            json!({ "item": {
                "id": "cmd_find",
                "type": "commandExecution",
                "command": "/bin/zsh -lc \"rg -n 'submissionError|hydrate' src/lib\"",
                "status": "inProgress",
            } }),
        ),
        &json!({
            "title": "Find submissionError|hydrate",
            "preview": { "kind": "shell", "path": "src/lib", "query": "submissionError|hydrate" },
        }),
    );
}

#[test]
fn maps_file_change_items() {
    assert_match(
        &first_event(
            "item/started",
            json!({ "item": {
                "id": "fc_1",
                "type": "fileChange",
                "status": "inProgress",
                "changes": [
                    { "path": "src/App.tsx", "kind": "update", "diff": "@@ -1 +1 @@\n-old\n+new\n" },
                    { "path": "src/lib/checkpoint.ts", "kind": "update", "diff": "@@ -1 +1 @@\n-old\n+new\n" },
                ],
            } }),
        ),
        &json!({
            "type": "tool.started",
            "callId": "fc_1",
            "kind": "edit",
            "paths": ["src/App.tsx", "src/lib/checkpoint.ts"],
        }),
    );
}

#[test]
fn maps_sub_agent_activity_as_a_live_agent_tool_not_silence() {
    assert_match(
        &first_event(
            "item/completed",
            json!({ "item": {
                "id": "sa_1", "type": "subAgentActivity", "kind": "started",
                "agentPath": "/root/explore-auth", "agentThreadId": "thr_child",
            } }),
        ),
        &json!({
            "type": "tool.started",
            "callId": "sa_1",
            "kind": "agent",
            "status": "in_progress",
            "title": "Explore Auth subagent",
        }),
    );
    assert_match(
        &first_event(
            "item/completed",
            json!({ "item": {
                "id": "sa_1", "type": "subAgentActivity", "kind": "interrupted",
                "agentPath": "/root/explore-auth",
            } }),
        ),
        &json!({
            "type": "tool.updated",
            "callId": "sa_1",
            "kind": "agent",
            "status": "failed",
            "detail": "Subagent interrupted.",
        }),
    );
}

#[test]
fn retains_the_explicitly_selected_spawn_model() {
    assert_match(
        &first_event(
            "item/completed",
            json!({ "item": {
                "id": "spawn", "type": "collabAgentToolCall", "tool": "spawnAgent",
                "model": "gpt-5.6-sol", "status": "completed",
            } }),
        ),
        &json!({ "kind": "agent", "agentModel": "gpt-5.6-sol" }),
    );
}

#[test]
fn maps_current_collab_agent_failures_with_their_provider_detail() {
    // Waiting is bookkeeping against rows that already exist, not a third
    // subagent of its own.
    assert_match(
        &first_event(
            "item/started",
            json!({ "item": {
                "id": "collab_1", "type": "collabAgentToolCall", "tool": "wait",
                "status": "inProgress", "receiverThreadIds": ["thr_a", "thr_b"], "agentsStates": {},
            } }),
        ),
        &json!({
            "type": "tool.started",
            "callId": "collab_1",
            "title": "Wait for 2 subagents",
            "kind": "other",
            "status": "in_progress",
        }),
    );
    assert_match(
        &first_event(
            "item/completed",
            json!({ "item": {
                "id": "collab_1", "type": "collabAgentToolCall", "tool": "wait",
                "status": "completed", "receiverThreadIds": ["thr_a", "thr_b"],
                "agentsStates": {
                    "thr_a": { "status": "completed", "message": "done" },
                    "thr_b": { "status": "errored", "message": "worker disconnected" },
                },
            } }),
        ),
        &json!({
            "type": "tool.updated",
            "callId": "collab_1",
            "kind": "other",
            "status": "failed",
            "detail": "worker disconnected",
        }),
    );
}

#[test]
fn does_not_treat_a_completed_agent_message_as_the_end_of_the_turn() {
    let mapped = map_codex_notification(
        "item/completed",
        &json!({ "item": { "id": "msg_2", "type": "agentMessage", "text": "I'll inspect the changelog first." } }),
    );
    assert_eq!(mapped.turn_completed, None);
    assert_eq!(mapped.active_turn_id, None);
    assert_eq!(
        js(&mapped.events),
        json!([
            { "type": "message.delta", "text": "I'll inspect the changelog first." },
            { "type": "message.completed" },
        ])
    );
}

#[test]
fn maps_turn_completion_and_clears_active_turn() {
    let mapped = map_codex_notification(
        "turn/completed",
        &json!({ "turn": { "id": "turn_1", "status": "completed" } }),
    );
    assert_eq!(mapped.turn_completed.unwrap().status, TurnStatus::Completed);
    assert_eq!(mapped.active_turn_id, Some(None));
    assert!(mapped.events.contains(&HarnessEvent::MessageCompleted));
    assert!(mapped.events.contains(&HarnessEvent::ReasoningCompleted));
}

#[test]
fn maps_aborted_turns_as_interrupted_completion() {
    let mapped = map_codex_notification("turn/aborted", &json!({ "turn": { "id": "turn_1" } }));
    assert_eq!(
        mapped.turn_completed.unwrap().status,
        TurnStatus::Interrupted
    );
    assert_eq!(mapped.active_turn_id, Some(None));
}

#[test]
fn keeps_retry_notifications_diagnostic_only() {
    for (params, message) in [
        (
            json!({ "error": { "message": "Reconnecting... 1/5" }, "willRetry": true }),
            "Reconnecting... 1/5",
        ),
        (
            json!({ "message": "Temporary service interruption", "willRetry": true }),
            "Temporary service interruption",
        ),
    ] {
        assert_eq!(
            map_codex_notification("error", &params),
            MappedCodexNotification {
                diagnostic: Some(message.into()),
                ..Default::default()
            }
        );
    }
}

#[test]
fn keeps_errors_visible_unless_will_retry_is_explicitly_true() {
    for will_retry in [json!(false), Value::Null, json!("true")] {
        for message in [
            "Reconnecting... 5/5",
            "Falling back from WebSockets to HTTPS transport. Connection failed",
            "Unauthorized",
            "quota exceeded",
        ] {
            let mut params = json!({ "error": { "message": message } });
            if !will_retry.is_null() {
                params["willRetry"] = will_retry.clone();
            }
            assert_eq!(
                map_codex_notification("error", &params),
                MappedCodexNotification {
                    events: vec![HarnessEvent::SessionError {
                        message: message.into()
                    }],
                    ..Default::default()
                }
            );
        }
    }
}

#[test]
fn keeps_the_known_runtime_fallback_warning_diagnostic_only() {
    for message in [
        "Falling back from WebSockets to HTTPS transport",
        "Falling back from WebSockets to HTTPS transport. unexpected status 404 Not Found",
        "Falling back from WebSockets to HTTPS transport: connection closed",
    ] {
        assert_eq!(
            map_codex_notification("warning", &json!({ "message": message })),
            MappedCodexNotification {
                diagnostic: Some(message.into()),
                ..Default::default()
            }
        );
    }
}

#[test]
fn preserves_other_runtime_warnings() {
    for message in [
        "Reconnecting to the MCP server failed",
        "Proxy error: Falling back from WebSockets to HTTPS transport failed",
        "Falling back from WebSockets to HTTPS transport is disabled",
        "An unrelated runtime warning",
    ] {
        assert_eq!(
            map_codex_notification("warning", &json!({ "message": message })),
            MappedCodexNotification {
                events: vec![HarnessEvent::Status {
                    text: message.into()
                }],
                ..Default::default()
            }
        );
    }
}

#[test]
fn preserves_configuration_warnings_even_with_fallback_wording() {
    let message = "Falling back from WebSockets to HTTPS transport.";
    for field in ["summary", "message", "details"] {
        let mut params = json!({});
        params[field] = json!(message);
        assert_eq!(
            map_codex_notification("configWarning", &params),
            MappedCodexNotification {
                events: vec![HarnessEvent::Status {
                    text: message.into()
                }],
                ..Default::default()
            }
        );
    }
}

#[test]
fn maps_failed_turns_to_session_error() {
    let mapped = map_codex_notification(
        "turn/completed",
        &json!({ "turn": { "id": "turn_1", "status": "failed", "error": { "message": "quota exceeded" } } }),
    );
    assert_eq!(mapped.turn_completed.unwrap().status, TurnStatus::Failed);
    assert!(mapped.events.contains(&HarnessEvent::SessionError {
        message: "quota exceeded".into()
    }));
}

#[test]
fn flags_turns_that_failed_on_a_spent_usage_limit() {
    let mapped = map_codex_notification(
        "turn/completed",
        &json!({ "turn": {
            "id": "turn_1", "status": "failed",
            "error": { "message": "You've hit your usage limit.", "codexErrorInfo": "usageLimitExceeded" },
        } }),
    );
    assert!(mapped.usage_limited);
    let other = map_codex_notification(
        "turn/completed",
        &json!({ "turn": {
            "id": "turn_1", "status": "failed",
            "error": { "message": "overloaded", "codexErrorInfo": "serverOverloaded" },
        } }),
    );
    assert!(!other.usage_limited);
}

#[test]
fn passes_rate_limit_snapshots_through() {
    let rate_limits = json!({
        "limitId": "codex",
        "primary": { "usedPercent": 100, "windowDurationMins": 300, "resetsAt": 1_900 },
        "secondary": null,
    });
    assert_eq!(
        map_codex_notification(
            "account/rateLimits/updated",
            &json!({ "rateLimits": rate_limits })
        ),
        MappedCodexNotification {
            rate_limits: rate_limits.as_object().cloned(),
            ..Default::default()
        }
    );
}

#[test]
fn does_not_silently_complete_a_failed_turn_with_no_error_payload() {
    let mapped = map_codex_notification(
        "turn/completed",
        &json!({ "turn": { "id": "turn_1", "status": "failed" } }),
    );
    assert!(mapped.events.contains(&HarnessEvent::SessionError {
        message: "Codex turn failed.".into()
    }));
}

#[test]
fn ignores_unknown_methods() {
    assert_eq!(events("future/unknown", json!({ "x": 1 })), json!([]));
}

// approvals

#[test]
fn maps_command_approval_requests() {
    let mapped = map_approval_request(
        "item/commandExecution/requestApproval",
        &json!({ "itemId": "cmd_1", "command": "rm -rf /", "reason": "cleanup" }),
        7,
    )
    .unwrap();
    assert_eq!(mapped.kind, CodexApprovalKind::Command);
    assert_match(
        &js(&mapped.event),
        &json!({ "type": "approval.requested", "requestId": 7, "callId": "cmd_1", "kind": "execute" }),
    );
}

#[test]
fn keeps_readable_codex_actions_on_command_approvals() {
    let mapped = map_approval_request(
        "item/commandExecution/requestApproval",
        &json!({
            "itemId": "cmd_read",
            "command": "/bin/zsh -lc \"cat src/App.tsx\"",
            "cwd": "/Users/me/project",
            "reason": "Inspect the app",
            "commandActions": [{
                "type": "read",
                "command": "cat src/App.tsx",
                "name": "App.tsx",
                "path": "/Users/me/project/src/App.tsx",
            }],
        }),
        8,
    )
    .unwrap();
    assert_match(
        &js(&mapped.event),
        &json!({
            "title": "Read src/App.tsx",
            "kind": "execute",
            "preview": { "kind": "shell", "path": "/Users/me/project/src/App.tsx", "fileName": "App.tsx" },
        }),
    );
}

#[test]
fn maps_file_change_approval_requests() {
    let mapped = map_approval_request(
        "item/fileChange/requestApproval",
        &json!({ "itemId": "fc_1", "reason": "Write config" }),
        3,
    )
    .unwrap();
    assert_eq!(mapped.kind, CodexApprovalKind::FileChange);
    assert_eq!(js(&mapped.event)["title"], "Write config");
}

#[test]
fn translates_ui_decisions_to_codex_wire_decisions() {
    assert_eq!(
        to_codex_approval_decision(ApprovalDecision::Allow, CodexApprovalKind::Command),
        "accept"
    );
    assert_eq!(
        to_codex_approval_decision(ApprovalDecision::Deny, CodexApprovalKind::FileChange),
        "decline"
    );
}

// parseCodexModelList

#[test]
fn builds_models_with_reasoning_and_service_tier_settings() {
    let models = parse_codex_model_list(&[
        json!({
            "model": "gpt-5.6-luna",
            "displayName": "gpt-5.6-luna",
            "defaultReasoningEffort": "medium",
            "supportedReasoningEfforts": [
                { "reasoningEffort": "low" },
                { "reasoningEffort": "medium" },
                { "reasoningEffort": "high" },
            ],
            "serviceTiers": [{ "id": "fast", "name": "Fast" }],
            "defaultServiceTier": "default",
        }),
        json!({
            "model": "gpt-5.6-terra",
            "displayName": "gpt-5.6-terra",
            "isDefault": true,
            "supportedReasoningEfforts": [],
        }),
    ]);
    let native: Vec<_> = models.iter().map(|m| m.native_id.clone()).collect();
    assert_eq!(
        native,
        vec![
            Some("gpt-5.6-terra".to_string()),
            Some("gpt-5.6-luna".to_string())
        ]
    );
    let luna = models
        .iter()
        .find(|m| m.native_id.as_deref() == Some("gpt-5.6-luna"))
        .unwrap();
    let settings = luna.settings.as_deref().unwrap_or_default();
    assert!(settings.iter().any(|s| s.id == "reasoningEffort"));
    assert!(settings.iter().any(|s| s.id == "serviceTier"));
    assert_eq!(
        settings
            .iter()
            .find(|s| s.id == "reasoningEffort")
            .map(|s| s.value.as_str()),
        Some("medium")
    );
}

// mapCodexNotification thread/tokenUsage/updated

#[test]
fn reports_the_last_request_and_the_window_the_app_server_supplies() {
    assert_eq!(
        events(
            "thread/tokenUsage/updated",
            json!({
                "threadId": "t1",
                "turnId": "turn1",
                "tokenUsage": {
                    "last": {
                        "totalTokens": 42_000, "inputTokens": 40_000, "cachedInputTokens": 30_000,
                        "cacheWriteInputTokens": 0, "outputTokens": 2_000, "reasoningOutputTokens": 500,
                    },
                    "total": {
                        "totalTokens": 900_000, "inputTokens": 880_000, "cachedInputTokens": 800_000,
                        "cacheWriteInputTokens": 0, "outputTokens": 20_000, "reasoningOutputTokens": 4_000,
                    },
                    "modelContextWindow": 272_000,
                },
            })
        ),
        json!([
            { "type": "context", "used": 42_000, "window": 272_000 },
            {
                "type": "turn.metrics",
                "inputTokens": 40_000,
                "cacheReadTokens": 30_000,
                "outputTokens": 2_000,
                "cacheHitPercent": 75.0,
            },
        ])
    );
}

#[test]
fn never_uses_total_which_keeps_climbing_past_the_window() {
    assert_eq!(
        events(
            "thread/tokenUsage/updated",
            json!({ "tokenUsage": {
                "last": { "totalTokens": 10_000 },
                "total": { "totalTokens": 5_000_000 },
                "modelContextWindow": 272_000,
            } })
        ),
        json!([{ "type": "context", "used": 10_000, "window": 272_000 }])
    );
}

#[test]
fn omits_the_window_when_the_app_server_does_not_know_it() {
    assert_eq!(
        events(
            "thread/tokenUsage/updated",
            json!({ "tokenUsage": {
                "last": { "totalTokens": 10_000 },
                "total": { "totalTokens": 10_000 },
                "modelContextWindow": null,
            } })
        ),
        json!([{ "type": "context", "used": 10_000 }])
    );
}

#[test]
fn stays_quiet_on_an_empty_reading() {
    assert_eq!(
        events(
            "thread/tokenUsage/updated",
            json!({ "tokenUsage": { "last": {}, "total": {} } })
        ),
        json!([])
    );
}

// mapCodexSubagentSteps

fn subagent_bash(status: &str, output: Option<&str>) -> Value {
    let mut item = json!({ "id": "cmd_1", "type": "commandExecution", "command": "npm test", "status": status });
    if let Some(output) = output {
        item["aggregatedOutput"] = json!(output);
    }
    js(&map_codex_subagent_steps(
        "agent-1",
        "item/completed",
        &json!({ "threadId": "thr_1", "item": item }),
    ))
}

#[test]
fn keeps_a_failed_child_tools_output_on_its_step_where_it_can_be_read() {
    assert_match(
        &subagent_bash("failed", Some("Tests failed: assertion error")),
        &json!([{
            "type": "agent.step",
            "stepId": "cmd_1",
            "status": "failed",
            "detail": "Tests failed: assertion error",
        }]),
    );
}

#[test]
fn leaves_a_settled_childs_result_off_its_step() {
    let steps = subagent_bash("completed", Some("12 passed"));
    assert_no_key(&steps[0], "detail");
}
