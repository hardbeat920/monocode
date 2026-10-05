//! Port of grokProtocol.test.ts and the Grok cases of core/fileAttachments.test.ts.

use super::*;
use monocode_core::attachment::{ATTACHMENT_ONLY_PROMPT, AttachmentKind};
use monocode_core::harness::harness_supports_attachments;
use monocode_core::user_question::QuestionAnswers;
use serde_json::json;

fn image(id: &str, name: &str, size: i64, data: &str) -> Attachment {
    Attachment {
        id: id.into(),
        name: name.into(),
        mime_type: "image/png".into(),
        kind: AttachmentKind::Image,
        size,
        data: Some(data.into()),
        ..Attachment::default()
    }
}

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

#[test]
fn supports_attachments_despite_groks_stale_advertised_capability() {
    assert!(harness_supports_attachments(HarnessId::Grok));
    assert!(harness_supports_attachments(HarnessId::Cursor));
}

#[test]
fn sends_text_and_image_prompt_blocks() {
    let blocks = grok_prompt_blocks(
        "  describe this  ",
        &[image("image-1", "screenshot.png", 3, "YWJj")],
    )
    .unwrap();
    assert_eq!(
        value(&blocks),
        json!([
            { "type": "text", "text": "describe this" },
            { "type": "image", "mimeType": "image/png", "data": "YWJj" },
        ])
    );
    assert!(grok_prompt_blocks("   ", &[]).unwrap().is_empty());
}

#[test]
fn puts_global_flags_before_agent_and_stdio_last() {
    assert_eq!(
        grok_spawn_args(GrokSpawnInput {
            model: "grok:grok-4.6",
            effort: Some("high"),
            full_access: true,
            plan: false,
        }),
        ids(&[
            "--no-auto-update",
            "agent",
            "--no-leader",
            "--model",
            "grok-4.6",
            "--reasoning-effort",
            "high",
            "--always-approve",
            "stdio",
        ])
    );
    let text = grok_text_spawn_args();
    assert_eq!(text[0], "--no-auto-update");
    assert_eq!(text.last().unwrap(), "stdio");
    assert!(text.iter().any(|arg| arg == "dontAsk"));
    assert_eq!(
        grok_spawn_args(GrokSpawnInput {
            model: "grok:grok-4.6",
            plan: true,
            ..GrokSpawnInput::default()
        }),
        ids(&[
            "--no-auto-update",
            "--permission-mode",
            "plan",
            "agent",
            "--no-leader",
            "--model",
            "grok-4.6",
            "stdio",
        ])
    );
}

#[test]
fn sets_yolo_mode_only_for_full_access() {
    assert_eq!(
        grok_session_new_params("/repo", RuntimeMode::Supervised),
        json!({ "cwd": "/repo", "mcpServers": [] })
    );
    assert_eq!(
        grok_session_new_params("/repo", RuntimeMode::FullAccess),
        json!({ "cwd": "/repo", "mcpServers": [], "_meta": { "yoloMode": true } })
    );
    assert_eq!(
        grok_session_new_params("/repo", RuntimeMode::Auto),
        json!({ "cwd": "/repo", "mcpServers": [], "_meta": { "autoMode": true } })
    );
}

#[test]
fn never_authenticates_with_the_browser_grok_com_method() {
    assert_eq!(
        grok_auth_method_id(&json!({
            "authMethods": [{ "id": "grok.com" }, { "id": "cached_token" }, { "id": "xai.api_key" }],
            "_meta": { "defaultAuthMethodId": "grok.com" },
        }))
        .as_deref(),
        Some("xai.api_key")
    );
    assert_eq!(
        grok_auth_method_id(&json!({
            "authMethods": [{ "id": "cached_token" }, { "id": "grok.com" }],
            "_meta": { "defaultAuthMethodId": "cached_token" },
        }))
        .as_deref(),
        Some("cached_token")
    );
    assert_eq!(
        grok_auth_method_id(&json!({ "authMethods": [{ "id": "grok.com" }] })),
        None
    );
}

#[test]
fn parks_supervised_permissions_and_auto_allows_full_access() {
    let options = ids(&["allow-once", "reject-once"]);
    assert_eq!(
        pick_auto_option(RuntimeMode::Supervised, Some("execute"), &options),
        None
    );
    assert_eq!(
        pick_auto_option(RuntimeMode::AutoAcceptEdits, Some("edit"), &options).as_deref(),
        Some("allow-once")
    );
    assert_eq!(
        pick_auto_option(RuntimeMode::AutoAcceptEdits, Some("execute"), &options),
        None
    );
    assert_eq!(
        pick_auto_option(RuntimeMode::FullAccess, Some("execute"), &options).as_deref(),
        Some("allow-once")
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
fn reads_grok_tool_metadata_from_permission_payloads() {
    let request = permission_request_from_acp(&json!({
        "toolCall": {
            "toolCallId": "call-1",
            "kind": "execute",
            "title": "Execute `git status`",
            "rawInput": { "variant": "Bash", "command": "git status" },
        },
        "options": [{ "optionId": "allow-once" }, { "optionId": "reject-once" }],
    }));
    assert_eq!(request.call_id.as_deref(), Some("call-1"));
    assert!(request.title.contains("git status"), "{}", request.title);
    assert_eq!(request.option_ids, ids(&["allow-once", "reject-once"]));
}

#[test]
fn maps_agent_message_thought_and_grok_tool_updates() {
    assert_eq!(
        events_from_acp_update(&json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "Hi" },
        })),
        vec![HarnessEvent::MessageDelta { text: "Hi".into() }]
    );
    assert_eq!(
        events_from_acp_update(&json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": { "type": "text", "text": "Hmm" },
        })),
        vec![HarnessEvent::ReasoningDelta { text: "Hmm".into() }]
    );

    let early = events_from_acp_update(&json!({
        "sessionUpdate": "tool_call_delta_chunk",
        "tool_call_id": "call-0",
        "name": "read_file",
    }));
    assert_match(
        &value(&early[0]),
        &json!({ "type": "tool.updated", "callId": "call-0", "kind": "read", "status": "pending" }),
    );

    let tools = events_from_acp_update(&json!({
        "sessionUpdate": "tool_call",
        "toolCallId": "call-1",
        "kind": "read",
        "title": "Read README.md",
        "status": "in_progress",
        "_meta": {
            "x.ai/tool": {
                "name": "read_file",
                "kind": "read",
                "label": "Read",
                "input": { "path": "README.md" },
            },
        },
    }));
    assert_match(
        &value(&tools[0]),
        &json!({
            "type": "tool.updated",
            "callId": "call-1",
            "kind": "read",
            "preview": { "kind": "read", "path": "README.md", "fileName": "README.md" },
        }),
    );
}

#[test]
fn maps_turn_completed_usage_onto_the_context_meter() {
    assert_eq!(
        value(&events_from_acp_update(&json!({
            "sessionUpdate": "turn_completed",
            "usage": { "inputTokens": 19762, "outputTokens": 36, "totalTokens": 19798 },
        }))),
        json!([
            { "type": "context", "used": 19798 },
            { "type": "turn.metrics", "inputTokens": 19762, "outputTokens": 36 },
        ])
    );
}

#[test]
fn accepts_hermes_style_acp_usage_size_as_the_context_window() {
    assert_eq!(
        value(&events_from_acp_update(&json!({
            "sessionUpdate": "usage_update",
            "size": 131072,
            "used": 32768,
        }))),
        json!([{ "type": "context", "used": 32768, "window": 131072 }])
    );
}

#[test]
fn maps_plan_entries() {
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
}

#[test]
fn parses_initialize_and_session_new_model_catalogs() {
    let models = models_from_initialize(&json!({
        "_meta": {
            "modelState": {
                "currentModelId": "grok-4.6",
                "availableModels": [{
                    "modelId": "grok-4.6",
                    "name": "Grok 4.6",
                    "_meta": {
                        "totalContextTokens": 500000,
                        "supportsReasoningEffort": true,
                        "reasoningEffort": "high",
                        "reasoningEfforts": [
                            { "id": "xhigh", "value": "xhigh", "label": "Extra High Effort" },
                            { "id": "high", "value": "high", "label": "High Effort", "default": true },
                            { "id": "low", "value": "low", "label": "Low Effort" },
                        ],
                    },
                }],
            },
        },
    }));
    assert_match(
        &value(&models[0]),
        &json!({
            "id": "grok:grok-4.6",
            "harness": "grok",
            "name": "Grok 4.6",
            "nativeId": "grok-4.6",
            "contextWindow": 500000,
        }),
    );
    assert_match(
        &value(&models[0].settings.as_ref().unwrap()[0]),
        &json!({ "id": "effort", "value": "high" }),
    );
    let natives: Vec<_> = models_from_session_new(&json!({
        "models": {
            "currentModelId": "grok-4.6",
            "availableModels": [{ "modelId": "grok-4.5", "name": "Grok 4.5" }],
        },
    }))
    .into_iter()
    .map(|model| model.native_id.unwrap())
    .collect();
    assert_eq!(natives, ids(&["grok-4.5"]));
}

#[test]
fn parses_grok_models_text_output() {
    let models = models_from_grok_models_output(
        "You are not authenticated.\n\nDefault model: grok-4.6\n\nAvailable models:\n  * grok-4.6 (default)\n  - grok-4.5\n",
    );
    let natives: Vec<_> = models
        .into_iter()
        .map(|model| model.native_id.unwrap())
        .collect();
    assert_eq!(natives, ids(&["grok-4.6", "grok-4.5"]));
}

#[test]
fn ships_a_grok_4_6_fallback_catalog() {
    assert_eq!(
        fallback_grok_models()[0].native_id.as_deref(),
        Some("grok-4.6")
    );
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
fn reads_effort_from_model_settings() {
    let settings: ModelSettings = [("effort".to_string(), "xhigh".to_string())].into();
    assert_eq!(grok_effort(Some(&settings)).as_deref(), Some("xhigh"));
    assert_eq!(grok_effort(Some(&ModelSettings::new())), None);
}

#[test]
fn answers_ask_user_questions_from_the_form_reply() {
    let questions = ask_questions_from_acp(&json!({
        "questions": [{
            "question": "Which colour?",
            "options": [{ "label": "Red" }, { "label": "Blue" }],
        }],
    }));
    assert_eq!(questions[0].prompt, "Which colour?");
    let mut answers = QuestionAnswers::new();
    answers.insert("Which colour?".into(), vec!["Blue".into()]);
    assert_eq!(
        ask_question_response(
            &UserQuestionReply::Answered {
                answers,
                custom: None
            },
            &questions
        ),
        json!({ "outcome": "accepted", "answers": { "Which colour?": "Blue" } })
    );
    assert_eq!(
        ask_question_response(&UserQuestionReply::Skipped, &questions),
        json!({ "outcome": "skip_interview" })
    );
}

#[test]
fn extracts_plan_text_from_exit_plan_mode() {
    assert_eq!(
        plan_from_exit_plan(&json!({ "planContent": "Ship it" })),
        "Ship it"
    );
}

// core/fileAttachments.test.ts, the Grok ACP cases.

fn document() -> Attachment {
    Attachment {
        id: "document".into(),
        name: "report.pdf".into(),
        mime_type: "application/pdf".into(),
        kind: AttachmentKind::File,
        size: 100,
        path: Some("/tmp/report.pdf".into()),
        ..Attachment::default()
    }
}

fn folder() -> Attachment {
    Attachment {
        id: "folder".into(),
        name: "reports".into(),
        mime_type: "inode/directory".into(),
        kind: AttachmentKind::File,
        size: 4096,
        path: Some("/tmp/reports".into()),
        ..Attachment::default()
    }
}

#[test]
fn keeps_grok_acp_documents_as_resource_links() {
    assert_eq!(
        value(&grok_prompt_blocks("", &[document()]).unwrap()),
        json!([
            { "type": "text", "text": ATTACHMENT_ONLY_PROMPT },
            {
                "type": "resource_link",
                "uri": "file:///tmp/report.pdf",
                "name": "report.pdf",
                "mimeType": "application/pdf",
                "size": 100,
            },
        ])
    );
    let mut screenshot = image("image", "screenshot.png", 3, "YWJj");
    screenshot.path = Some("/tmp/screenshot.png".into());
    assert_match(
        &value(&grok_prompt_blocks("look", &[screenshot]).unwrap()[1]),
        &json!({ "type": "image", "mimeType": "image/png", "data": "YWJj" }),
    );
}

#[test]
fn sends_a_grok_folder_as_its_path_never_as_a_resource_link() {
    assert_eq!(
        value(&grok_prompt_blocks("", &[folder()]).unwrap()),
        json!([
            { "type": "text", "text": ATTACHMENT_ONLY_PROMPT },
            {
                "type": "text",
                "text": "Attached folder (list or read the files inside from this path): \"/tmp/reports\"",
            },
        ])
    );
}

#[test]
fn caps_long_tool_detail() {
    let long = "x".repeat(8_010);
    let capped = cap(&long);
    assert!(capped.ends_with("\n…"));
    assert_eq!(js::len(&capped), 8_002);
}
