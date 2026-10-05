//! Port of fxProtocol.test.ts.

use super::*;
use monocode_core::harness::harness_supports_attachments;
use serde_json::json;

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn value<T: serde::Serialize>(item: &T) -> Value {
    serde_json::to_value(item).unwrap()
}

/// `expect(actual).toMatchObject(expected)` for JSON objects.
fn assert_match(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Object(actual_map), Value::Object(expected_map)) => {
            for (key, expected_value) in expected_map {
                let actual_value = actual_map
                    .get(key)
                    .unwrap_or_else(|| panic!("missing {key} in {actual}"));
                assert_match(actual_value, expected_value);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

fn first_event(update: Value) -> Value {
    value(&events_from_acp_update(&json!({ "update": update }))[0])
}

// fx's "ask" mode stops for every read and command, and MonoCode shows none
// of those prompts, so a turn would park forever. Always use fx's auto mode.
#[test]
fn always_runs_fx_in_its_own_code_auto_mode() {
    for mode in [
        RuntimeMode::Supervised,
        RuntimeMode::AutoAcceptEdits,
        RuntimeMode::Auto,
        RuntimeMode::FullAccess,
    ] {
        assert_eq!(fx_mode_id(mode), "code");
    }
}

#[test]
fn sends_text_only_prompt_blocks() {
    assert_eq!(
        value(&fx_prompt_blocks("  hello  ")),
        json!([{ "type": "text", "text": "hello" }])
    );
    assert!(fx_prompt_blocks("   ").is_empty());
}

#[test]
fn does_not_support_attachments() {
    assert!(!harness_supports_attachments(HarnessId::Fx));
    assert!(harness_supports_attachments(HarnessId::Grok));
    assert!(harness_supports_attachments(HarnessId::Cursor));
}

#[test]
fn auto_allows_in_every_runtime_mode_so_a_turn_never_parks_on_approval() {
    let options = ids(&["allow-once", "reject-once"]);
    assert_eq!(
        auto_permission_option(RuntimeMode::Supervised, &options).as_deref(),
        Some("allow-once")
    );
    assert_eq!(
        auto_permission_option(RuntimeMode::Auto, &options).as_deref(),
        Some("allow-once")
    );
    assert_eq!(
        auto_permission_option(RuntimeMode::FullAccess, &options).as_deref(),
        Some("allow-once")
    );
    assert_eq!(auto_permission_option(RuntimeMode::Supervised, &[]), None);
}

#[test]
fn prefers_allow_always_so_fx_stops_re_asking() {
    assert_eq!(
        auto_permission_option(
            RuntimeMode::Supervised,
            &ids(&["allow_once", "allow_always", "reject_once"])
        )
        .as_deref(),
        Some("allow_always")
    );
}

#[test]
fn picks_allow_reject_option_ids_from_acp_permission_options() {
    assert_eq!(
        permission_option_id(
            ApprovalDecision::Allow,
            &ids(&["allow_once", "reject_once"])
        ),
        "allow_once"
    );
    assert_eq!(
        permission_option_id(ApprovalDecision::Deny, &ids(&["allow-once", "reject-once"])),
        "reject-once"
    );
}

#[test]
fn reads_a_permission_prompt_from_an_acp_request() {
    let request = permission_request_from_acp(&json!({
        "toolCall": { "toolCallId": "call-1", "kind": "edit", "title": "Edit src/lib/fx.ts" },
        "options": [{ "optionId": "allow-once" }, { "optionId": "reject-once" }],
    }));
    assert_eq!(request.call_id.as_deref(), Some("call-1"));
    assert_eq!(request.kind.as_deref(), Some("edit"));
    assert_eq!(request.option_ids, ids(&["allow-once", "reject-once"]));
    assert!(request.title.contains("fx.ts"), "{}", request.title);
}

#[test]
fn extracts_nested_shell_args_from_permission_payloads() {
    let request = permission_request_from_acp(&json!({
        "toolCall": {
            "toolCallId": "call_a",
            "title": "terminal.exec git status -s",
            "kind": "execute",
            "rawInput": { "action": "exec", "command": "git status -s", "cwd": "/repo" },
        },
        "options": [{ "optionId": "allow_once" }],
    }));
    assert_eq!(request.call_id.as_deref(), Some("call_a"));
    assert!(request.title.contains("git status -s"), "{}", request.title);
}

#[test]
fn maps_agent_message_and_tool_updates_to_harness_events() {
    assert_eq!(
        events_from_acp_update(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "Hi" },
        })),
        vec![HarnessEvent::MessageDelta { text: "Hi".into() }]
    );
    let tools = events_from_acp_update(&json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "t1",
        "kind": "read",
        "title": "Read README.md",
        "status": "in_progress",
    }));
    assert_match(
        &value(&tools[0]),
        &json!({ "type": "tool.updated", "callId": "t1", "kind": "read", "status": "in_progress" }),
    );
}

#[test]
fn maps_plan_entries_and_usage() {
    assert_eq!(
        value(&events_from_acp_update(&json!({
            "sessionUpdate": "plan",
            "entries": [
                { "content": "Inspect router", "status": "completed" },
                { "content": "Add test", "status": "pending" },
            ],
        }))),
        json!([{
            "type": "tasks.updated",
            "items": [
                { "text": "Inspect router", "status": "completed" },
                { "text": "Add test", "status": "pending" },
            ],
        }])
    );
    assert_eq!(
        value(&events_from_acp_update(
            &json!({ "sessionUpdate": "plan", "text": "# Approach" })
        )),
        json!([{ "type": "plan", "text": "# Approach" }])
    );
    assert_eq!(
        value(&events_from_acp_update(
            &json!({ "usage": { "used": 1200, "window": 200000 } })
        )),
        json!([{ "type": "context", "used": 1200, "window": 200000 }])
    );
}

#[test]
fn parses_fx_models_json() {
    let models = models_from_fx_output(
        &json!({ "kind": "models", "count": 2, "ids": ["zai/glm-5.2-fast", "openai/gpt-5.2"] })
            .to_string(),
    );
    assert_eq!(
        value(&models),
        json!([
            {
                "id": "fx:zai/glm-5.2-fast",
                "harness": "fx",
                "name": "zai/glm-5.2-fast",
                "nativeId": "zai/glm-5.2-fast",
            },
            {
                "id": "fx:openai/gpt-5.2",
                "harness": "fx",
                "name": "openai/gpt-5.2",
                "nativeId": "openai/gpt-5.2",
            },
        ])
    );
}

#[test]
fn parses_object_shaped_model_entries_when_present() {
    let models = models_from_fx_output(
        &json!({
            "models": [{ "id": "zai/glm-5.2-fast", "name": "GLM 5.2 Fast", "contextWindow": 202752 }],
        })
        .to_string(),
    );
    assert_eq!(
        value(&models),
        json!([{
            "id": "fx:zai/glm-5.2-fast",
            "harness": "fx",
            "name": "GLM 5.2 Fast",
            "nativeId": "zai/glm-5.2-fast",
            "contextWindow": 202752,
        }])
    );
}

#[test]
fn parses_a_text_model_list_when_json_is_missing() {
    let models = models_from_fx_output(
        "zai/glm-5.2-fast - GLM 5.2 Fast (default)\nopenai/gpt-5.4 - GPT-5.4\n",
    );
    let natives: Vec<_> = models
        .iter()
        .map(|model| model.native_id.as_deref().unwrap())
        .collect();
    assert_eq!(natives, ["zai/glm-5.2-fast", "openai/gpt-5.4"]);
    assert_eq!(models[0].name, "GLM 5.2 Fast");
}

#[test]
fn adds_the_tui_selected_status_model_when_the_list_command_omits_it() {
    let listed =
        models_from_fx_output(&json!({ "ids": ["zai/glm-4.7", "openai/gpt-5.2"] }).to_string());
    let active = model_from_fx_status_output(
        &json!({ "kind": "status", "model": "zai/glm-5.2" }).to_string(),
    );
    let natives: Vec<_> = merge_fx_catalog_models(listed, active)
        .into_iter()
        .map(|model| model.native_id.unwrap())
        .collect();
    assert_eq!(natives, ["zai/glm-4.7", "openai/gpt-5.2", "zai/glm-5.2"]);
}

#[test]
fn reads_a_session_id_from_acp_setup_results() {
    assert_eq!(
        session_id_from_result(&json!({ "sessionId": "  abc  " })).as_deref(),
        Some("abc")
    );
    assert_eq!(
        session_id_from_result(&json!({ "session_id": "xyz" })).as_deref(),
        Some("xyz")
    );
    assert_eq!(session_id_from_result(&json!({})), None);
}

#[test]
fn picks_the_model_config_option_not_the_provider_listed_first() {
    let options = read_config_options(&json!([
        { "id": "provider", "name": "Provider", "category": "model", "type": "select", "currentValue": "gateway" },
        { "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "zai/glm-5.2" },
        { "id": "mode", "name": "Session Mode", "category": "mode", "type": "select", "currentValue": "ask" },
    ]));
    assert_eq!(extract_model_config_id(&options), "model");
}

// Payloads below are verbatim from an `fx acp` 0.0.5 wire capture.

#[test]
fn recovers_a_read_target_from_the_fx_result_blob() {
    let event = first_event(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_1",
        "status": "completed",
        "content": [{
            "type": "content",
            "content": {
                "type": "text",
                "text": "<path>notes.txt</path>\n<content>\n1\thello\n2\tworld\n</content>",
            },
        }],
    }));
    assert_match(
        &event,
        &json!({
            "type": "tool.updated",
            "title": "Read notes.txt",
            "preview": { "kind": "read", "path": "notes.txt", "fileName": "notes.txt" },
        }),
    );
}

#[test]
fn never_slices_a_gerund_title_into_read_ing() {
    let event = first_event(json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "call_2",
        "title": "Reading",
        "kind": "read",
        "status": "pending",
    }));
    assert_match(&event, &json!({ "type": "tool.updated", "title": "Read" }));
}

#[test]
fn recovers_the_grep_query() {
    let event = first_event(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_3",
        "status": "completed",
        "content": [{
            "type": "content",
            "content": {
                "type": "text",
                "text": "[grep] 2 matches for export\n - app.ts:1: export const a = 1;\n",
            },
        }],
    }));
    assert_match(
        &event,
        &json!({ "title": "Find export", "preview": { "kind": "search", "query": "export" } }),
    );
}

#[test]
fn labels_a_shell_call_with_the_command_from_fxs_command_result() {
    let event = first_event(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_4",
        "kind": "execute",
        "status": "completed",
        "content": [{
            "type": "content",
            "content": { "type": "text", "text": "exit_code=0\n<stdout>\nhi\n</stdout>\n" },
        }],
        "command_result": { "kind": "foreground", "command": "echo hi", "cwd": "/tmp/x", "exit_code": 0 },
    }));
    assert_match(&event, &json!({ "title": "echo hi", "detail": "hi" }));
}

#[test]
fn keeps_stderr_and_the_exit_code_for_a_failed_shell_call() {
    let event = first_event(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_failed",
        "kind": "execute",
        "status": "failed",
        "content": [{
            "type": "content",
            "content": {
                "type": "text",
                "text": "exit_code=1\n<stdout>\n</stdout>\n<stderr>\ncommand failed\n</stderr>\n",
            },
        }],
        "command_result": { "kind": "foreground", "command": "false", "cwd": "/tmp/x", "exit_code": 1 },
    }));
    assert_match(
        &event,
        &json!({ "title": "false", "detail": "command failed\nexit 1" }),
    );
}

#[test]
fn recovers_the_edited_path_from_fxs_write_confirmation() {
    let event = first_event(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_5",
        "kind": "edit",
        "status": "completed",
        "content": [{ "type": "content", "content": { "type": "text", "text": "edited app.ts (41 bytes)" } }],
    }));
    assert_match(
        &event,
        &json!({
            "title": "Edit app.ts",
            "preview": { "kind": "write", "path": "app.ts", "fileName": "app.ts" },
        }),
    );
}

#[test]
fn reports_a_directory_listing_as_a_plain_list_call() {
    let event = first_event(json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": "call_6",
        "kind": "read",
        "status": "completed",
        "content": [{ "type": "content", "content": { "type": "text", "text": ".:\n- notes.txt\n" } }],
    }));
    assert_match(&event, &json!({ "title": "List .", "kind": "other" }));
    assert!(event.get("preview").is_none());
}
