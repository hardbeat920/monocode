//! Port of src/integrations/harness/providers/claude/claudeProtocol.test.ts.

use monocode_core::attachment::{Attachment, AttachmentKind};
use monocode_core::block::{TaskListItem, TaskListItemStatus, TurnMetrics};
use monocode_core::harness::RuntimeMode;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::user_question::UserQuestionReply;
use serde_json::{Value, json};

use super::catalog::{
    claude_model_catalog, models_for_claude_version, models_from_claude_list_models,
};
use super::protocol::*;

fn rec(value: Value) -> Record {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

/// `expect(args).toEqual(expect.arrayContaining(items))`.
fn contains_all(args: &[String], items: &[&str]) -> bool {
    items.iter().all(|item| args.iter().any(|arg| arg == item))
}

fn settings_arg(args: &[String]) -> Value {
    let index = args.iter().position(|arg| arg == "--settings").unwrap();
    serde_json::from_str(&args[index + 1]).unwrap()
}

fn item(id: Option<&str>, text: &str, status: TaskListItemStatus) -> TaskListItem {
    TaskListItem {
        id: id.map(str::to_string),
        text: text.into(),
        status,
        extra: Default::default(),
    }
}

// describe("runtimeModeToPermission")

#[test]
fn maps_runtime_modes_onto_claude_permission_flags() {
    use ClaudePermissionMode::*;
    assert_eq!(runtime_mode_to_permission(RuntimeMode::Supervised), Default);
    assert_eq!(
        runtime_mode_to_permission(RuntimeMode::AutoAcceptEdits),
        AcceptEdits
    );
    assert_eq!(runtime_mode_to_permission(RuntimeMode::Auto), Auto);
    assert_eq!(
        runtime_mode_to_permission(RuntimeMode::FullAccess),
        BypassPermissions
    );
}

#[test]
fn sends_supervised_as_a_flag_so_settings_cannot_lower_it() {
    let args = build_claude_spawn_args(&ClaudeSpawnOptions {
        permission_mode: Some(runtime_mode_to_permission(RuntimeMode::Supervised)),
        session_id: Some("sess-supervised".into()),
        ..Default::default()
    });
    assert!(contains_all(&args, &["--permission-mode", "default"]));
}

// describe("normalizeClaudeCliEffort")

#[test]
fn drops_ultrathink_and_maps_ultracode_to_xhigh() {
    assert_eq!(
        normalize_claude_cli_effort(Some("ultrathink"), Some("claude-sonnet-5")),
        None
    );
    assert_eq!(
        normalize_claude_cli_effort(Some("ultracode"), Some("claude-opus-5")).as_deref(),
        Some("xhigh")
    );
}

#[test]
fn maps_xhigh_to_max_on_older_models() {
    assert_eq!(
        normalize_claude_cli_effort(Some("xhigh"), Some("claude-opus-4-6")).as_deref(),
        Some("max")
    );
    assert_eq!(
        normalize_claude_cli_effort(Some("xhigh"), Some("claude-opus-5")).as_deref(),
        Some("xhigh")
    );
    assert_eq!(
        normalize_claude_cli_effort(Some("xhigh"), Some("sonnet")).as_deref(),
        Some("xhigh")
    );
}

#[test]
fn maps_max_to_high_on_sonnet_4_6() {
    assert_eq!(
        normalize_claude_cli_effort(Some("max"), Some("claude-sonnet-4-6")).as_deref(),
        Some("high")
    );
}

// describe("applyClaudePromptEffortPrefix")

#[test]
fn prefixes_ultrathink_on_the_prompt() {
    assert_eq!(
        apply_claude_prompt_effort_prefix("Investigate the edge cases", Some("ultrathink")),
        "Ultrathink:\nInvestigate the edge cases"
    );
    assert_eq!(
        apply_claude_prompt_effort_prefix("hello", Some("high")),
        "hello"
    );
}

// describe("resolveClaudeApiModelId")

#[test]
fn appends_1m_for_the_1m_context_window() {
    assert_eq!(
        resolve_claude_api_model_id("claude-opus-5", Some("1m")),
        "claude-opus-5[1m]"
    );
    assert_eq!(
        resolve_claude_api_model_id("claude-sonnet-5", Some("200k")),
        "claude-sonnet-5"
    );
}

// describe("buildClaudeSpawnArgs")

#[test]
fn speaks_stream_json_with_stdio_permissions_like_the_agent_sdk() {
    let args = build_claude_spawn_args(&ClaudeSpawnOptions {
        model: Some("claude-sonnet-5".into()),
        effort: Some("high".into()),
        permission_mode: Some(ClaudePermissionMode::AcceptEdits),
        session_id: Some("sess-1".into()),
        ..Default::default()
    });
    for flag in [
        "--output-format",
        "stream-json",
        "--input-format",
        "--permission-prompt-tool",
        "stdio",
        "--include-partial-messages",
        "--setting-sources=user,project,local",
    ] {
        assert!(args.iter().any(|arg| arg == flag), "missing {flag}");
    }
    assert!(contains_all(
        &args,
        &["--model", "claude-sonnet-5", "--effort", "high"]
    ));
    assert!(contains_all(&args, &["--permission-mode", "acceptEdits"]));
    assert!(contains_all(&args, &["--session-id", "sess-1"]));
    assert_eq!(settings_arg(&args).get("disableAllHooks"), None);
}

#[test]
fn only_disables_hooks_for_interactive_sessions_when_asked() {
    let args = build_claude_spawn_args(&ClaudeSpawnOptions {
        settings: Some(ClaudeCliSettings {
            disable_all_hooks: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    });
    assert_eq!(settings_arg(&args)["disableAllHooks"], json!(true));
}

#[test]
fn skips_permissions_and_mcp_for_isolated_text_sessions() {
    let args = build_claude_spawn_args(&ClaudeSpawnOptions {
        isolated: true,
        max_turns: Some(1),
        model: Some("claude-haiku-4-5".into()),
        ..Default::default()
    });
    assert!(args.iter().any(|arg| arg == "--no-session-persistence"));
    assert!(args.iter().any(|arg| arg == "--strict-mcp-config"));
    assert!(contains_all(&args, &["--max-turns", "1"]));
    assert_eq!(settings_arg(&args)["disableAllHooks"], json!(true));
    assert!(!args.iter().any(|arg| arg == "--permission-prompt-tool"));
}

#[test]
fn locks_isolated_read_only_prompts_to_plan_mode() {
    let args = build_claude_spawn_args(&ClaudeSpawnOptions {
        isolated: true,
        permission_mode: Some(ClaudePermissionMode::Plan),
        max_turns: Some(1),
        model: Some("claude-haiku-4-5".into()),
        ..Default::default()
    });
    assert!(contains_all(
        &args,
        &["--permission-mode", "plan", "--max-turns", "1"]
    ));
}

#[test]
fn adds_bypass_flag_for_full_access() {
    let args = build_claude_spawn_args(&ClaudeSpawnOptions {
        permission_mode: Some(ClaudePermissionMode::BypassPermissions),
        ..Default::default()
    });
    assert!(
        args.iter()
            .any(|arg| arg == "--allow-dangerously-skip-permissions")
    );
}

// describe("buildClaudeUserMessage")

#[test]
fn embeds_vision_images_as_base64_source_blocks() {
    let message = build_claude_user_message(
        "look",
        &[Attachment {
            id: "a1".into(),
            name: "diagram.png".into(),
            mime_type: "image/png".into(),
            kind: AttachmentKind::Image,
            size: 4,
            data: Some("AQIDBA==".into()),
            ..Default::default()
        }],
        None,
    )
    .unwrap();
    let content = user_message_content(&message);
    assert_eq!(content[0], json!({ "type": "text", "text": "look" }));
    assert_eq!(
        content[1],
        json!({
            "type": "image",
            "source": { "type": "base64", "media_type": "image/png", "data": "AQIDBA==" },
        })
    );
}

// describe("control protocol")

#[test]
fn parses_can_use_tool_requests() {
    let parsed = parse_control_request(&rec(json!({
        "type": "control_request",
        "request_id": "req_1",
        "request": { "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "ls" } },
    })))
    .unwrap();
    assert_eq!(parsed.request_id, "req_1");
    assert_eq!(parsed.subtype, "can_use_tool");
    assert_eq!(parsed.tool_name.as_deref(), Some("Bash"));
    assert_eq!(parsed.input, rec(json!({ "command": "ls" })));
}

#[test]
fn maps_allow_deny_onto_sdk_permission_results() {
    assert_eq!(
        to_claude_permission_result(ApprovalDecision::Allow, &rec(json!({ "command": "ls" }))),
        json!({ "behavior": "allow", "updatedInput": { "command": "ls" } })
    );
    assert_eq!(
        to_claude_permission_result(ApprovalDecision::Deny, &Record::new())["behavior"],
        json!("deny")
    );
}

// describe("stream mapping")

#[test]
fn reads_text_and_thinking_deltas() {
    assert_eq!(
        stream_delta_from_event(&rec(json!({
            "type": "stream_event",
            "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": "Hi" } },
        }))),
        Some(ClaudeStreamDelta {
            kind: ClaudeDeltaKind::Assistant,
            text: "Hi".into()
        })
    );
    assert_eq!(
        stream_delta_from_event(&rec(json!({
            "type": "stream_event",
            "event": { "type": "content_block_delta", "delta": { "type": "thinking_delta", "thinking": "hmm" } },
        }))),
        Some(ClaudeStreamDelta {
            kind: ClaudeDeltaKind::Reasoning,
            text: "hmm".into()
        })
    );
}

#[test]
fn reads_tool_use_content_blocks() {
    assert_eq!(
        tool_start_from_event(&rec(json!({
            "type": "stream_event",
            "event": {
                "type": "content_block_start",
                "index": 1,
                "content_block": { "type": "tool_use", "id": "toolu_1", "name": "Read", "input": { "file_path": "a.ts" } },
            },
        }))),
        Some(ClaudeToolStart {
            index: 1,
            id: "toolu_1".into(),
            name: "Read".into(),
            input: rec(json!({ "file_path": "a.ts" })),
        })
    );
}

// describe("usage limits")

#[test]
fn reads_a_refused_window_and_when_it_resets() {
    assert_eq!(
        usage_limit_from_rate_limit_event(&rec(json!({
            "type": "rate_limit_event",
            "rate_limit_info": { "status": "rejected", "resetsAt": 1_790_000_000, "rateLimitType": "five_hour" },
        }))),
        Some(ClaudeUsageLimit {
            resets_at: Some(1_790_000_000_000)
        })
    );
}

#[test]
fn ignores_allowed_windows_and_extra_usage() {
    assert_eq!(
        usage_limit_from_rate_limit_event(&rec(json!({
            "rate_limit_info": { "status": "allowed_warning", "resetsAt": 1 },
        }))),
        None
    );
    assert_eq!(
        usage_limit_from_rate_limit_event(&rec(json!({
            "rate_limit_info": { "status": "rejected", "isUsingOverage": true },
        }))),
        None
    );
}

#[test]
fn recognizes_a_limit_in_an_errored_result() {
    assert!(is_usage_limit_result(&rec(json!({
        "type": "result",
        "subtype": "success",
        "is_error": true,
        "result": "You've hit your limit · resets 3am (Europe/Sofia)",
    }))));
    assert!(!is_usage_limit_result(&rec(json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "result": "You've hit your limit",
    }))));
}

// describe("turnStatusFromResult")

#[test]
fn treats_aborted_terminals_as_interrupted() {
    assert_eq!(
        turn_status_from_result(&rec(json!({
            "type": "result",
            "subtype": "error_during_execution",
            "terminal_reason": "aborted_streaming",
            "errors": ["interrupt"],
        })))
        .status,
        ClaudeTurnStatus::Interrupted
    );
    assert_eq!(
        turn_status_from_result(&rec(json!({ "type": "result", "subtype": "success" }))).status,
        ClaudeTurnStatus::Completed
    );
}

// describe("modelsForClaudeVersion")

fn native_ids(version: Option<&str>) -> Vec<String> {
    models_for_claude_version(version)
        .into_iter()
        .filter_map(|model| model.native_id)
        .collect()
}

fn has(ids: &[String], id: &str) -> bool {
    ids.iter().any(|item| item == id)
}

#[test]
fn hides_opus_5_until_2_1_219() {
    let old = native_ids(Some("2.1.100"));
    assert!(!has(&old, "claude-opus-5"));
    assert!(!has(&old, "claude-opus-4-8"));
    assert!(has(&old, "claude-sonnet-4-6"));

    let next = native_ids(Some("2.1.233"));
    assert!(has(&next, "claude-opus-5"));
    assert!(has(&next, "claude-fable-5"));
    assert!(has(&next, "claude-sonnet-5"));
    assert!(!has(&next, "claude-opus-5-5"));
}

#[test]
fn hides_opus_5_5_until_2_1_280_and_rejects_a_missing_version() {
    let before = native_ids(Some("2.1.279"));
    assert!(!has(&before, "claude-opus-5-5"));
    assert!(has(&before, "claude-opus-5"));

    let at = native_ids(Some("2.1.280"));
    assert!(has(&at, "claude-opus-5-5"));
    assert!(has(&at, "claude-opus-5"));

    assert!(!has(&native_ids(None), "claude-opus-5-5"));
}

#[test]
fn hides_sonnet_5_until_2_1_197_and_rejects_a_missing_version() {
    assert!(!has(&native_ids(Some("2.1.196")), "claude-sonnet-5"));
    assert!(has(&native_ids(Some("2.1.197")), "claude-sonnet-5"));
    assert!(!has(&native_ids(None), "claude-sonnet-5"));
}

// describe("list_models catalog")

fn listed() -> Record {
    rec(json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "list_1",
            "response": {
                "models": [
                    {
                        "value": "default",
                        "resolvedModel": "claude-sonnet-5",
                        "displayName": "Default (recommended)",
                        "description": "Sonnet 5 · Efficient for routine tasks",
                        "supportsEffort": true,
                        "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"],
                    },
                    {
                        "value": "sonnet",
                        "resolvedModel": "claude-sonnet-5",
                        "displayName": "Sonnet",
                        "description": "Sonnet 5 · Efficient for routine tasks",
                        "supportsEffort": true,
                        "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"],
                        "supportsAdaptiveThinking": true,
                    },
                    {
                        "value": "claude-fable-5[1m]",
                        "resolvedModel": "claude-fable-5",
                        "displayName": "Fable",
                        "description": "Fable 5 · Most capable for your hardest tasks",
                        "supportsEffort": true,
                        "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"],
                    },
                    {
                        "value": "opus",
                        "resolvedModel": "claude-opus-5",
                        "displayName": "Opus",
                        "description": "Opus 5 · Best for everyday, complex tasks",
                        "supportsEffort": true,
                        "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"],
                        "supportsFastMode": true,
                    },
                    {
                        "value": "haiku",
                        "resolvedModel": "claude-haiku-4-5-20251001",
                        "displayName": "Haiku",
                        "description": "Haiku 4.5 · Fastest for quick answers",
                    },
                    {
                        "value": "cc-update-required-1",
                        "resolvedModel": "cc-update-required-1",
                        "displayName": "Fable 5.1 (disabled)",
                        "description": "Update to 2.1.255+ to use Fable 5.1",
                        "disabled": true,
                    },
                ],
            },
        },
    }))
}

#[test]
fn reads_rows_from_the_matching_control_response() {
    assert_eq!(
        list_models_from_control_response(&listed(), "list_1").map(|rows| rows.len()),
        Some(6)
    );
    assert_eq!(list_models_from_control_response(&listed(), "other"), None);
    assert_eq!(
        list_models_from_control_response(
            &rec(json!({
                "type": "control_response",
                "response": {
                    "subtype": "success",
                    "request_id": "init_1",
                    "response": { "commands": [], "models": [] },
                },
            })),
            "list_1",
        ),
        None
    );
}

fn setting_ids(model: &monocode_core::AgentModel) -> Vec<&str> {
    model
        .settings
        .iter()
        .flatten()
        .map(|setting| setting.id.as_str())
        .collect()
}

#[test]
fn maps_the_picker_catalog_and_drops_default_disabled_rows() {
    let rows = list_models_from_control_response(&listed(), "list_1").unwrap();
    let models = models_from_claude_list_models(&Value::Array(rows));
    let native: Vec<_> = models
        .iter()
        .map(|m| m.native_id.clone().unwrap())
        .collect();
    assert_eq!(native, ["sonnet", "claude-fable-5", "opus", "haiku"]);
    let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "claude:sonnet",
            "claude:fable-5",
            "claude:opus",
            "claude:haiku"
        ]
    );
    let names: Vec<_> = models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Sonnet 5", "Fable 5", "Opus 5", "Haiku 4.5"]);

    let sonnet = &models[0];
    let effort = sonnet
        .settings
        .iter()
        .flatten()
        .find(|setting| setting.id == "effort")
        .unwrap();
    let values: Vec<_> = effort.options.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(
        values,
        [
            "low",
            "medium",
            "high",
            "xhigh",
            "max",
            "ultracode",
            "ultrathink"
        ]
    );
    assert!(!setting_ids(sonnet).contains(&"fast"));

    // Fable 5 runs at 1M from its bare id, so its `[1m]` row adds no choice.
    assert!(!setting_ids(&models[1]).contains(&"context"));
    assert!(setting_ids(&models[2]).contains(&"fast"));
    assert_eq!(models[3].settings, None);
}

#[test]
fn offers_context_only_where_the_model_id_changes_the_window() {
    let models = models_from_claude_list_models(&json!([
        // Native 1M models run at 1M from the bare id; `[1m]` changes nothing.
        { "value": "opus", "resolvedModel": "claude-opus-5-5", "displayName": "Opus 5.5", "supportsEffort": true },
        { "value": "claude-fable-5-1[1m]", "resolvedModel": "claude-fable-5-1", "displayName": "Fable 5.1", "supportsEffort": true },
        { "value": "sonnet[1m]", "resolvedModel": "claude-sonnet-5-5-20260601", "displayName": "Sonnet 5.5", "supportsEffort": true },
        // Opus 4.6 runs at 200k and reaches 1M only through a listed variant.
        { "value": "claude-opus-4-6", "resolvedModel": "claude-opus-4-6", "displayName": "Opus 4.6", "supportsEffort": true },
        { "value": "claude-sonnet-4-6[1m]", "resolvedModel": "claude-sonnet-4-6", "displayName": "Sonnet 4.6 (1M)", "supportsEffort": true },
    ]));
    let context = |id: &str| {
        models
            .iter()
            .find(|model| model.native_id.as_deref() == Some(id))
            .and_then(|model| model.settings.as_ref())
            .and_then(|settings| settings.iter().find(|setting| setting.id == "context"))
            .cloned()
    };
    assert_eq!(context("opus"), None);
    assert_eq!(context("claude-fable-5-1"), None);
    assert_eq!(context("sonnet"), None);
    assert_eq!(context("claude-opus-4-6"), None);
    let sonnet = context("claude-sonnet-4-6").unwrap();
    assert_eq!(sonnet.value, "1m");
    let values: Vec<_> = sonnet.options.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, ["200k", "1m"]);
    // Each choice offered launches the window it names.
    assert_eq!(
        resolve_claude_api_model_id("claude-sonnet-4-6", Some("1m")),
        "claude-sonnet-4-6[1m]"
    );
    assert_eq!(
        resolve_claude_api_model_id("claude-sonnet-4-6", Some("200k")),
        "claude-sonnet-4-6"
    );
}

#[test]
fn offers_no_context_choice_in_the_built_in_catalog() {
    // Native 1M models have nothing to choose, and without a listed `[1m]`
    // variant nothing shows that the account can use 1M on the others.
    for model in claude_model_catalog() {
        assert!(!setting_ids(model).contains(&"context"), "{}", model.id);
    }
}

#[test]
fn adds_resolved_versions_to_generic_live_catalog_alias_labels() {
    let models = models_from_claude_list_models(&json!([
        { "value": "opus[1m]", "resolvedModel": "claude-opus-5-5", "displayName": "Opus (1M context)", "description": "Most capable for complex tasks" },
        { "value": "fable", "resolvedModel": "claude-fable-5-1", "displayName": "Fable", "description": "Fast and capable" },
        { "value": "haiku", "resolvedModel": "claude-haiku-4-5-20251001", "displayName": "Haiku 4.5", "description": "Fastest for quick answers" },
        { "value": "next-family", "resolvedModel": "claude-next-family-7.2-20300101", "displayName": "Next Family — recommended", "description": "A future model family" },
    ]));
    let names: Vec<_> = models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Opus 5.5 (1M context)",
            "Fable 5.1",
            "Haiku 4.5",
            "Next Family 7.2 — recommended",
        ]
    );
    assert_eq!(models[0].id, "claude:opus");
    assert_eq!(models[0].native_id.as_deref(), Some("opus"));
    // Opus 5.5 runs at 1M from its bare id, so its `[1m]` alias adds no choice.
    assert!(!setting_ids(&models[0]).contains(&"context"));
}

#[test]
fn launches_a_versioned_short_value_with_the_claude_prefix() {
    let models = models_from_claude_list_models(&json!([
        { "value": "opus-5-5", "resolvedModel": "claude-opus-5-5", "displayName": "Opus 5.5" },
        { "value": "opus", "resolvedModel": "claude-opus-5-5", "displayName": "Opus" },
    ]));
    let native: Vec<_> = models
        .iter()
        .map(|m| m.native_id.clone().unwrap())
        .collect();
    assert_eq!(native, ["claude-opus-5-5", "opus"]);
    assert_eq!(models[0].id, "claude:opus-5-5");
    assert_eq!(models[1].id, "claude:opus");
}

#[test]
fn parses_success_and_error_control_responses() {
    assert_eq!(
        parse_control_response(&rec(json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": "init_1", "response": { "pid": 12 } },
        }))),
        Some(ClaudeControlResponse {
            request_id: "init_1".into(),
            ok: true,
            payload: Some(rec(json!({ "pid": 12 }))),
            error: None,
        })
    );
    assert_eq!(
        parse_control_response(&rec(json!({
            "type": "control_response",
            "response": { "subtype": "error", "request_id": "list_1", "error": "nope" },
        }))),
        Some(ClaudeControlResponse {
            request_id: "list_1".into(),
            ok: false,
            payload: None,
            error: Some("nope".into()),
        })
    );
    assert!(is_claude_init_message(&rec(
        json!({ "type": "system", "subtype": "init" })
    )));
    assert!(!is_claude_init_message(&rec(
        json!({ "type": "assistant", "subtype": "init" })
    )));
}

// describe("helpers")

#[test]
fn parses_cli_version_strings() {
    assert_eq!(
        parse_claude_version("2.1.233 (Claude Code)").as_deref(),
        Some("2.1.233")
    );
}

#[test]
fn classifies_tools_and_todo_plans() {
    assert_eq!(tool_kind_from_name("Bash"), "execute");
    assert_eq!(tool_kind_from_name("Skill"), "skill");
    assert_eq!(tool_kind_from_name("Agent"), "agent");
    assert_eq!(tool_kind_from_name("Task"), "agent");
    assert_eq!(
        tool_title(
            "Agent",
            &rec(json!({ "description": "Explore the auth module" }))
        ),
        "Explore the auth module"
    );
    assert_eq!(
        tool_title("Task", &rec(json!({ "subagent_type": "explore" }))),
        "Explore subagent"
    );
    assert_eq!(
        tool_title("Bash", &rec(json!({ "command": "ls -la src" }))),
        "List src"
    );
    assert_eq!(
        tool_title("Skill", &rec(json!({ "skill": "code-review" }))),
        "Skill /code-review"
    );
    assert!(is_todo_tool("TodoWrite"));
    assert_eq!(tool_kind_from_name("TodoWrite"), "tasks");
    assert_eq!(tool_kind_from_name("TaskCreate"), "tasks");
    assert_eq!(tool_kind_from_name("TaskUpdate"), "tasks");
    assert_ne!(tool_kind_from_name("TaskOutput"), "agent");
    assert_eq!(
        task_list_from_todos(&rec(json!({
            "todos": [
                { "content": "One", "status": "completed" },
                { "content": "Two", "status": "pending" },
            ],
        }))),
        Some(vec![
            item(None, "One", TaskListItemStatus::Completed),
            item(None, "Two", TaskListItemStatus::Pending),
        ])
    );
    assert_eq!(
        extract_exit_plan_mode_plan(&json!({ "plan": "# Plan" })).as_deref(),
        Some("# Plan")
    );
}

#[test]
fn answers_ask_user_question_with_the_selected_options() {
    let input = rec(json!({
        "questions": [{ "question": "Which file?", "options": [{ "label": "a.ts" }, { "label": "b.ts" }] }],
    }));
    let reply: UserQuestionReply = serde_json::from_value(json!({
        "kind": "answered",
        "answers": { "Which file?": ["b.ts"] },
    }))
    .unwrap();
    assert_eq!(
        ask_user_question_allow_input(&input, Some(&reply))["answers"],
        json!({ "Which file?": "b.ts" })
    );
}

#[test]
fn drops_request_lifecycle_status_pings() {
    assert_eq!(
        status_text_from_system(&rec(
            json!({ "type": "system", "subtype": "status", "status": "requesting" })
        )),
        None
    );
    assert_eq!(
        status_text_from_system(&rec(
            json!({ "type": "system", "subtype": "status", "message": "Responding" })
        )),
        None
    );
    assert_eq!(
        status_text_from_system(&rec(json!({ "type": "system", "subtype": "status" }))),
        None
    );
}

#[test]
fn keeps_status_messages_that_carry_real_prose() {
    assert_eq!(
        status_text_from_system(&rec(json!({
            "type": "system", "subtype": "status", "message": "Retrying in 3s (rate limited)",
        })))
        .as_deref(),
        Some("Retrying in 3s (rate limited)")
    );
    assert_eq!(
        status_text_from_system(&rec(json!({
            "type": "system", "subtype": "compact", "message": "Compacted context to 40k tokens",
        })))
        .as_deref(),
        Some("Compacted context to 40k tokens")
    );
}

#[test]
fn still_marks_a_compact_boundary_that_carries_no_prose() {
    assert_eq!(
        status_text_from_system(&rec(json!({
            "type": "system", "subtype": "compact_boundary", "compact_metadata": { "trigger": "auto" },
        })))
        .as_deref(),
        Some("Compacted context")
    );
}

#[test]
fn ignores_system_messages_that_are_not_status_or_compact() {
    assert_eq!(
        status_text_from_system(&rec(
            json!({ "type": "system", "subtype": "init", "message": "ready" })
        )),
        None
    );
    assert_eq!(
        status_text_from_system(&rec(json!({ "type": "assistant", "message": "hello" }))),
        None
    );
}

// describe("contextUsedFromAssistant")

#[test]
fn counts_cached_reads_and_writes_as_window_occupancy() {
    // Shape captured from `claude --output-format stream-json --verbose`.
    let message = rec(json!({
        "type": "assistant",
        "message": {
            "usage": {
                "input_tokens": 2,
                "cache_creation_input_tokens": 12941,
                "cache_read_input_tokens": 16652,
                "output_tokens": 3,
            },
        },
    }));
    assert_eq!(context_used_from_assistant(&message), Some(29598));
}

#[test]
fn ignores_a_message_with_no_usage() {
    assert_eq!(
        context_used_from_assistant(&rec(json!({ "type": "assistant", "message": {} }))),
        None
    );
}

// describe("contextFromResult")

#[test]
fn reads_the_window_the_cli_reports_rather_than_a_model_table() {
    let result = rec(json!({
        "type": "result",
        "usage": {
            "input_tokens": 2,
            "cache_creation_input_tokens": 12941,
            "cache_read_input_tokens": 16652,
            "output_tokens": 13,
        },
        "modelUsage": { "claude-sonnet-5": { "contextWindow": 1_000_000, "maxOutputTokens": 64000 } },
    }));
    assert_eq!(
        context_from_result(&result),
        Some(ClaudeContextReading {
            used: Some(29608),
            window: Some(1_000_000)
        })
    );
}

#[test]
fn uses_the_last_iteration_since_top_level_usage_sums_the_whole_turn() {
    let result = rec(json!({
        "type": "result",
        "usage": {
            "input_tokens": 10,
            "cache_read_input_tokens": 90_000,
            "output_tokens": 500,
            "iterations": [
                { "input_tokens": 5, "cache_read_input_tokens": 20_000, "output_tokens": 200 },
                { "input_tokens": 5, "cache_read_input_tokens": 70_000, "output_tokens": 300 },
            ],
        },
        "modelUsage": { "claude-opus-5": { "contextWindow": 200_000 } },
    }));
    assert_eq!(
        context_from_result(&result),
        Some(ClaudeContextReading {
            used: Some(70_305),
            window: Some(200_000)
        })
    );
}

#[test]
fn has_nothing_to_report_for_a_turn_that_never_called_the_api() {
    assert_eq!(
        context_from_result(&rec(json!({ "type": "result", "usage": {} }))),
        None
    );
}

// describe("turnMetricsFromResult")

#[test]
fn normalizes_aggregate_input_output_and_cache_usage() {
    assert_eq!(
        turn_metrics_from_result(&rec(json!({
            "usage": {
                "input_tokens": 2,
                "cache_creation_input_tokens": 12_941,
                "cache_read_input_tokens": 16_652,
                "output_tokens": 13,
            },
        }))),
        Some(TurnMetrics {
            input_tokens: Some(2),
            cache_write_tokens: Some(12_941),
            cache_read_tokens: Some(16_652),
            output_tokens: Some(13),
            cache_hit_percent: Some((16_652.0 / (2.0 + 12_941.0 + 16_652.0)) * 100.0),
            extra: Default::default(),
        })
    );
}

// describe("subagent messages")

#[test]
fn detects_nested_agent_traffic_by_parent_tool_use_id() {
    assert!(is_subagent_message(&rec(
        json!({ "parent_tool_use_id": "toolu_agent" })
    )));
    assert!(!is_subagent_message(&rec(
        json!({ "parent_tool_use_id": null })
    )));
    assert!(!is_subagent_message(&rec(json!({ "type": "assistant" }))));
}

#[test]
fn does_not_rebind_the_parent_session_to_a_subagent_session_id() {
    assert_eq!(
        session_id_from_message(&rec(json!({
            "type": "assistant", "session_id": "sub_1", "parent_tool_use_id": "toolu_agent",
        }))),
        None
    );
    assert_eq!(
        session_id_from_message(&rec(json!({
            "type": "assistant", "session_id": "sess_1", "parent_tool_use_id": null,
        })))
        .as_deref(),
        Some("sess_1")
    );
}

#[test]
fn parses_task_lifecycle_frames_for_local_agents() {
    assert_eq!(
        parse_task_started(&rec(json!({
            "type": "system",
            "subtype": "task_started",
            "task_id": "t1",
            "tool_use_id": "toolu_agent",
            "description": "Explore the auth module",
            "task_type": "local_agent",
            "is_backgrounded": true,
        }))),
        Some(ClaudeAgentTaskStarted {
            task_id: "t1".into(),
            tool_use_id: Some("toolu_agent".into()),
            description: "Explore the auth module".into(),
            task_type: "local_agent".into(),
            backgrounded: true,
            ambient: false,
        })
    );
    let progress = parse_task_progress(&rec(json!({
        "type": "system",
        "subtype": "task_progress",
        "task_id": "t1",
        "last_tool_name": "Read",
        "description": "Explore the auth module",
    })))
    .unwrap();
    assert_eq!(progress.task_id, "t1");
    assert_eq!(progress.last_tool_name.as_deref(), Some("Read"));
    let updated = parse_task_updated(&rec(json!({
        "type": "system", "subtype": "task_updated", "task_id": "t1", "patch": { "status": "completed" },
    })))
    .unwrap();
    assert_eq!(updated.task_id, "t1");
    assert_eq!(updated.status.as_deref(), Some("completed"));
    let notice = parse_task_notification(&rec(json!({
        "type": "system",
        "subtype": "task_notification",
        "task_id": "t1",
        "tool_use_id": "toolu_agent",
        "status": "completed",
        "summary": "Found the tokens",
    })))
    .unwrap();
    assert_eq!(notice.task_id, "t1");
    assert_eq!(notice.status, "completed");
    assert_eq!(notice.summary, "Found the tokens");
    assert_eq!(
        parse_background_tasks(&rec(json!({
            "type": "system",
            "subtype": "background_tasks_changed",
            "tasks": [
                { "task_id": "t1", "task_type": "local_agent", "description": "Explore" },
                { "task_id": "bash_1", "task_type": "local_bash", "description": "sleep 10" },
                { "task_id": "watch", "task_type": "local_agent", "description": "watcher", "ambient": true },
            ],
        }))),
        Some(vec![
            ClaudeBackgroundTask {
                task_id: "t1".into(),
                task_type: "local_agent".into(),
                description: "Explore".into(),
            },
            ClaudeBackgroundTask {
                task_id: "bash_1".into(),
                task_type: "local_bash".into(),
                description: "sleep 10".into(),
            },
        ])
    );
    let progress = parse_tool_progress(&rec(json!({
        "type": "tool_progress", "tool_use_id": "toolu_agent", "subagent_type": "explore",
    })))
    .unwrap();
    assert_eq!(progress.tool_use_id, "toolu_agent");
    assert_eq!(progress.subagent_type.as_deref(), Some("explore"));
}

// describe("applyClaudeTaskTool")

#[test]
fn creates_from_the_result_id_updates_renames_and_deletes() {
    let mut tasks = ClaudeTaskMap::new();
    assert!(apply_claude_task_tool(
        &mut tasks,
        "TaskCreate",
        &rec(json!({ "subject": "One" })),
        "Task #7 created successfully: One",
    ));
    assert_eq!(
        tasks.values().cloned().collect::<Vec<_>>(),
        [item(Some("7"), "One", TaskListItemStatus::Pending)]
    );
    apply_claude_task_tool(
        &mut tasks,
        "TaskUpdate",
        &rec(json!({ "taskId": 7, "status": "in_progress" })),
        "",
    );
    apply_claude_task_tool(
        &mut tasks,
        "TaskUpdate",
        &rec(json!({ "taskId": "7", "subject": "Uno" })),
        "",
    );
    assert_eq!(
        tasks.get("7"),
        Some(&item(Some("7"), "Uno", TaskListItemStatus::InProgress))
    );
    apply_claude_task_tool(
        &mut tasks,
        "TaskUpdate",
        &rec(json!({ "taskId": "7", "status": "deleted" })),
        "",
    );
    assert_eq!(tasks.len(), 0);
}

#[test]
fn ignores_unknown_ids_missing_result_ids_and_other_tools() {
    let mut tasks = ClaudeTaskMap::new();
    assert!(!apply_claude_task_tool(
        &mut tasks,
        "TaskCreate",
        &rec(json!({ "subject": "One" })),
        "error",
    ));
    assert!(!apply_claude_task_tool(
        &mut tasks,
        "TaskUpdate",
        &rec(json!({ "taskId": "9", "status": "completed" })),
        "",
    ));
    assert!(!apply_claude_task_tool(
        &mut tasks,
        "TaskList",
        &Record::new(),
        "#1 [pending] One"
    ));
    assert_eq!(tasks.len(), 0);
}

// Claude cases of src/integrations/harness/core/fileAttachments.test.ts.

mod file_attachments {
    use monocode_core::attachment::{ATTACHMENT_ONLY_PROMPT, attachment_path_text};

    use super::*;

    fn document() -> Attachment {
        Attachment {
            id: "document".into(),
            name: "report.pdf".into(),
            mime_type: "application/pdf".into(),
            kind: AttachmentKind::File,
            size: 100,
            path: Some("/tmp/report.pdf".into()),
            ..Default::default()
        }
    }

    fn image() -> Attachment {
        Attachment {
            id: "image".into(),
            name: "screenshot.png".into(),
            mime_type: "image/png".into(),
            kind: AttachmentKind::Image,
            size: 3,
            data: Some("YWJj".into()),
            path: Some("/tmp/screenshot.png".into()),
            ..Default::default()
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
            ..Default::default()
        }
    }

    fn claude_content(text: &str, attachments: &[Attachment]) -> Vec<Value> {
        let message = build_claude_user_message(text, attachments, None).unwrap();
        user_message_content(&message).to_vec()
    }

    fn text(value: &str) -> Value {
        json!({ "type": "text", "text": value })
    }

    #[test]
    fn gives_a_folder_to_claude_as_a_path_it_can_read() {
        let content = claude_content("look", &[folder()]);
        assert_eq!(content.len(), 2);
        assert_eq!(content[0], text("look"));
        assert!(
            content[1]["text"]
                .as_str()
                .unwrap()
                .contains("Attached folder")
        );
    }

    #[test]
    fn tells_the_model_to_read_the_attachments_in_the_light_of_the_conversation() {
        assert_eq!(
            claude_content("", &[document()])[0],
            text(ATTACHMENT_ONLY_PROMPT)
        );
    }

    #[test]
    fn treats_a_whitespace_only_draft_as_no_text_at_all() {
        assert_eq!(
            claude_content("  ", &[document()])[0],
            text(ATTACHMENT_ONLY_PROMPT)
        );
    }

    #[test]
    fn leaves_a_real_message_and_a_turn_with_nothing_attached_untouched() {
        assert_eq!(claude_content("Review", &[document()])[0], text("Review"));
        assert_eq!(claude_content("", &[]), Vec::<Value>::new());
    }

    #[test]
    fn delivers_documents_in_claude_prompts() {
        for (name, mime_type, kind) in [
            ("report.pdf", "application/pdf", AttachmentKind::File),
            ("transcript.md", "text/markdown", AttachmentKind::File),
            ("server.log", "text/plain", AttachmentKind::File),
            ("recording.wav", "audio/wav", AttachmentKind::Audio),
            ("archive.zip", "application/zip", AttachmentKind::File),
        ] {
            let path = format!("/tmp/{name}");
            let file = Attachment {
                name: name.into(),
                mime_type: mime_type.into(),
                kind,
                path: Some(path.clone()),
                ..document()
            };
            let expected = format!(
                "Attached file (read from disk): {}",
                serde_json::to_string(&path).unwrap()
            );
            assert_eq!(
                claude_content("", &[file]),
                [text(ATTACHMENT_ONLY_PROMPT), text(&expected)]
            );
        }
    }

    #[test]
    fn preserves_native_images_alongside_documents_without_duplicate_path_inputs() {
        assert_eq!(
            claude_content("Review", &[image(), document()]),
            [
                text("Review"),
                json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": "image/png", "data": "YWJj" },
                }),
                text(r#"Attached file (read from disk): "/tmp/report.pdf""#),
            ]
        );
    }

    #[test]
    fn falls_back_for_an_image_that_cannot_be_embedded() {
        let too_big = Attachment {
            data: None,
            size: 21 * 1024 * 1024,
            ..image()
        };
        let svg = Attachment {
            name: "drawing.svg".into(),
            mime_type: "image/svg+xml".into(),
            path: Some("/tmp/drawing.svg".into()),
            ..image()
        };
        for file in [too_big, svg] {
            assert_eq!(
                claude_content("", std::slice::from_ref(&file)),
                [
                    text(ATTACHMENT_ONLY_PROMPT),
                    text(&attachment_path_text(&file).unwrap())
                ]
            );
        }
    }

    #[test]
    fn reports_a_missing_source_instead_of_silently_dropping_an_attachment() {
        let files = [Attachment {
            path: None,
            ..document()
        }];
        let error = build_claude_user_message("Review", &files, None).unwrap_err();
        assert!(error.contains("report.pdf"), "{error}");
        assert!(error.contains("no local file path"), "{error}");
    }
}
