//! Port of droidProtocol.test.ts.

use super::*;
use serde_json::json;

/// Trimmed from a real `droid exec --output-format acp` session/new (0.225.1).
fn session_new() -> Value {
    json!({
        "sessionId": "06480ace-4dc2-480f-9b83-950224e10c22",
        "models": {
            "availableModels": [
                { "modelId": "auto", "name": "Auto Model", "description": "1x Factory token rate" },
                { "modelId": "claude-opus-5-5", "name": "Opus 5.5" },
                { "modelId": "gpt-6-luna", "name": "GPT-6 Luna" },
            ],
            "currentModelId": "gpt-6-luna",
        },
        "modes": {
            "availableModes": [
                { "id": "normal", "name": "Auto (Off)" },
                { "id": "spec", "name": "Spec" },
            ],
            "currentModeId": "normal",
        },
        "configOptions": [
            {
                "id": "autonomy_level",
                "category": "mode",
                "type": "select",
                "currentValue": "normal",
                "options": [
                    { "value": "normal", "name": "Auto (Off)" },
                    { "value": "auto-high", "name": "Auto (High)" },
                ],
            },
            {
                "id": "model",
                "category": "model",
                "type": "select",
                "currentValue": "gpt-6-luna",
                "options": [
                    { "value": "auto", "name": "Auto Model" },
                    { "value": "claude-opus-5-5", "name": "Opus 5.5" },
                    { "value": "gpt-6-luna", "name": "GPT-6 Luna" },
                ],
            },
            {
                "id": "reasoning_effort",
                "category": "thought_level",
                "type": "select",
                "currentValue": "medium",
                "options": [
                    { "value": "none", "name": "None" },
                    { "value": "low", "name": "Low" },
                    { "value": "medium", "name": "Medium" },
                    { "value": "xhigh", "name": "Extra High" },
                ],
            },
        ],
    })
}

fn choice(value: &str, name: &str) -> DroidConfigChoice {
    DroidConfigChoice {
        value: value.into(),
        name: name.into(),
    }
}

fn settings(effort: &str) -> ModelSettings {
    [("effort".to_string(), effort.to_string())].into()
}

#[test]
fn maps_access_modes_onto_droid_autonomy_levels() {
    assert_eq!(
        droid_mode_id(RuntimeMode::Supervised, false).as_str(),
        "normal"
    );
    assert_eq!(
        droid_mode_id(RuntimeMode::AutoAcceptEdits, false).as_str(),
        "auto-low"
    );
    assert_eq!(
        droid_mode_id(RuntimeMode::Auto, false).as_str(),
        "auto-medium"
    );
    assert_eq!(
        droid_mode_id(RuntimeMode::FullAccess, false).as_str(),
        "auto-high"
    );
    assert_eq!(
        droid_mode_id(RuntimeMode::FullAccess, true).as_str(),
        "spec"
    );
}

#[test]
fn reads_the_catalog_with_droids_current_model_first() {
    let models = models_from_droid_session(&session_new(), &HashMap::new());
    let ids: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(
        ids,
        ["droid:gpt-6-luna", "droid:auto", "droid:claude-opus-5-5"]
    );
    assert_eq!(models[0].harness, HarnessId::Droid);
    assert_eq!(models[0].name, "GPT-6 Luna");
    assert_eq!(models[0].native_id.as_deref(), Some("gpt-6-luna"));
    assert_eq!(
        droid_current_model_id(&session_new()).as_deref(),
        Some("gpt-6-luna")
    );
}

#[test]
fn attaches_per_model_reasoning_levels_as_the_effort_setting() {
    let opus = DroidConfigOption {
        id: "reasoning_effort".into(),
        category: Some("thought_level".into()),
        current_value: Some("high".into()),
        options: vec![
            choice("low", "Low"),
            choice("high", "High"),
            choice("max", "Maximum"),
        ],
    };
    let auto = DroidConfigOption {
        id: "reasoning_effort".into(),
        category: None,
        current_value: Some("none".into()),
        options: vec![choice("none", "None")],
    };
    let models = models_from_droid_session(
        &session_new(),
        &HashMap::from([
            ("claude-opus-5-5".to_string(), opus),
            ("auto".to_string(), auto),
        ]),
    );
    let by_id: HashMap<_, _> = models
        .iter()
        .map(|model| (model.native_id.clone().unwrap(), model))
        .collect();
    assert_eq!(
        serde_json::to_value(&by_id["claude-opus-5-5"].settings).unwrap(),
        json!([{
            "id": "effort",
            "label": "Reasoning",
            "kind": "select",
            "value": "high",
            "options": [
                { "value": "low", "label": "Low" },
                { "value": "high", "label": "High" },
                { "value": "max", "label": "Maximum" },
            ],
        }])
    );
    // A single fixed level is not a choice.
    assert_eq!(by_id["auto"].settings, None);
    assert_eq!(droid_effort_setting(None), None);
}

#[test]
fn reads_config_options_from_results_and_config_option_update() {
    let from_result = droid_config_options_from(&session_new()).unwrap();
    assert_eq!(
        droid_effort_config(&from_result)
            .unwrap()
            .current_value
            .as_deref(),
        Some("medium")
    );
    let from_update = droid_config_options_from(&json!({
        "sessionId": "s",
        "update": {
            "sessionUpdate": "config_option_update",
            "configOptions": session_new()["configOptions"],
        },
    }))
    .unwrap();
    let ids: Vec<_> = from_update
        .iter()
        .map(|option| option.id.as_str())
        .collect();
    assert_eq!(ids, ["autonomy_level", "model", "reasoning_effort"]);
    assert_eq!(
        droid_config_options_from(&json!({ "update": { "sessionUpdate": "x" } })),
        None
    );
}

#[test]
fn maps_monocode_effort_values_onto_the_models_levels() {
    let options = droid_config_options_from(&session_new()).unwrap();
    let config = droid_effort_config(&options);
    assert_eq!(
        droid_effort_value(config, Some(&settings("low"))).as_deref(),
        Some("low")
    );
    assert_eq!(
        droid_effort_value(config, Some(&settings("extra-high"))).as_deref(),
        Some("xhigh")
    );
    assert_eq!(
        droid_effort_value(config, Some(&settings("off"))).as_deref(),
        Some("none")
    );
    assert_eq!(droid_effort_value(config, Some(&settings("max"))), None);
    assert_eq!(
        droid_effort_value(config, Some(&ModelSettings::new())),
        None
    );
}

#[test]
fn surfaces_the_detail_droid_hides_in_json_rpc_error_data() {
    let error = anyhow::Error::new(RpcError {
        message: "Internal error: Agent error".into(),
        code: Some(-32603),
        data: Some(Value::String(
            "402 {\"detail\":\"You've reached your 5-hour Droid Core usage limit (resets in 2h 15min).\",\"status\":402}"
                .into(),
        )),
    });
    assert_eq!(
        droid_error_message(&error),
        "You've reached your 5-hour Droid Core usage limit (resets in 2h 15min)."
    );
    assert_eq!(droid_error_message(&anyhow::anyhow!("plain")), "plain");
    assert!(
        droid_startup_error(&anyhow::anyhow!("Authentication required"))
            .to_string()
            .contains("FACTORY_API_KEY")
    );
}

#[test]
fn drops_the_streamed_echo_of_a_failed_request() {
    assert!(is_droid_error_echo(&json!({
        "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "Error: 402 {\"detail\":\"limit\"}" },
        },
    })));
    assert!(!is_droid_error_echo(&json!({
        "update": {
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": "Error: handled gracefully" },
        },
    })));
}

#[test]
fn extracts_the_spec_from_an_exit_spec_permission_request() {
    assert_eq!(
        droid_spec_plan(&json!({
            "toolCall": {
                "toolCallId": "t1",
                "kind": "switch_mode",
                "title": "Exit spec mode",
                "rawInput": { "plan": "## Plan\n1. Do it" },
            },
        }))
        .as_deref(),
        Some("## Plan\n1. Do it")
    );
    assert_eq!(
        droid_spec_plan(&json!({
            "toolCall": {
                "toolCallId": "t2",
                "kind": "switch_mode",
                "content": [{ "type": "content", "content": { "type": "text", "text": "Spec body" } }],
            },
        }))
        .as_deref(),
        Some("Spec body")
    );
    assert_eq!(
        droid_spec_plan(&json!({ "toolCall": { "toolCallId": "t3", "kind": "edit" } })),
        None
    );
}
