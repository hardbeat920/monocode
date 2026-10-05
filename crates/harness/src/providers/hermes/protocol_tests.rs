//! Port of hermesProtocol.test.ts.

use super::*;
use monocode_core::attachment::AttachmentKind;
use serde_json::json;

#[test]
fn maps_access_modes_onto_hermes_edit_approval_modes() {
    assert_eq!(hermes_mode_id(RuntimeMode::Supervised, false), "default");
    assert_eq!(
        hermes_mode_id(RuntimeMode::AutoAcceptEdits, false),
        "accept_edits"
    );
    assert_eq!(hermes_mode_id(RuntimeMode::Auto, false), "dont_ask");
    assert_eq!(hermes_mode_id(RuntimeMode::FullAccess, false), "dont_ask");
    assert_eq!(hermes_mode_id(RuntimeMode::FullAccess, true), "default");
}

#[test]
fn uses_standard_acp_text_and_image_prompt_blocks() {
    let attachment = Attachment {
        id: "image-1".into(),
        name: "screen.png".into(),
        mime_type: "image/png".into(),
        kind: AttachmentKind::Image,
        size: 4,
        data: Some("AAAA".into()),
        ..Attachment::default()
    };
    assert_eq!(
        serde_json::to_value(hermes_prompt_blocks("  inspect this  ", &[attachment]).unwrap())
            .unwrap(),
        json!([
            { "type": "text", "text": "inspect this" },
            { "type": "image", "mimeType": "image/png", "data": "AAAA" },
        ])
    );
}

#[test]
fn recovers_hermes_background_delegation_handles_from_acp_tool_content() {
    let handle = json!({
        "status": "dispatched",
        "mode": "background",
        "delegation_id": "deleg_1234",
        "live_transcripts": ["/tmp/deleg_1234/task-0.log", "/tmp/deleg_1234/task-1.log"],
    });
    assert_eq!(
        hermes_background_dispatch(&json!({
            "sessionId": "hermes-session-1",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "tool-delegate",
                "status": "completed",
                "content": [{
                    "type": "content",
                    "content": { "type": "text", "text": handle.to_string() },
                }],
            },
        })),
        Some(HermesBackgroundDispatch {
            call_id: "tool-delegate".into(),
            delegation_id: "deleg_1234".into(),
            transcripts: vec![
                "/tmp/deleg_1234/task-0.log".into(),
                "/tmp/deleg_1234/task-1.log".into(),
            ],
        })
    );
}

#[test]
fn reads_hermes_model_state_and_puts_its_current_model_first() {
    let setup = json!({
        "sessionId": "hermes-session-1",
        "models": {
            "currentModelId": "nous:hermes-4",
            "availableModels": [
                { "modelId": "openrouter:gpt-5", "name": "OpenRouter · GPT-5" },
                { "modelId": "nous:hermes-4", "name": "Nous · Hermes 4" },
                { "modelId": "nous:hermes-4", "name": "duplicate" },
            ],
        },
    });
    assert_eq!(
        hermes_session_id(&setup).as_deref(),
        Some("hermes-session-1")
    );
    assert_eq!(
        hermes_current_model_id(&setup).as_deref(),
        Some("nous:hermes-4")
    );
    assert_eq!(
        serde_json::to_value(models_from_hermes_session(&setup)).unwrap(),
        json!([
            {
                "id": "hermes:nous:hermes-4",
                "harness": "hermes",
                "name": "Nous · Hermes 4",
                "nativeId": "nous:hermes-4",
            },
            {
                "id": "hermes:openrouter:gpt-5",
                "harness": "hermes",
                "name": "OpenRouter · GPT-5",
                "nativeId": "openrouter:gpt-5",
            },
        ])
    );
}

#[test]
fn accepts_snake_case_acp_response_fields() {
    let setup = json!({
        "session_id": "hermes-session-2",
        "models": {
            "current_model_id": "local:model",
            "available_models": [{ "model_id": "local:model", "name": "Local" }],
        },
    });
    assert_eq!(
        hermes_session_id(&setup).as_deref(),
        Some("hermes-session-2")
    );
    assert_eq!(
        models_from_hermes_session(&setup)[0].native_id.as_deref(),
        Some("local:model")
    );
}

#[test]
fn adds_actionable_setup_help_to_credential_errors() {
    let message = hermes_startup_error("provider is not configured").to_string();
    assert!(message.contains("provider is not configured"));
    assert!(message.contains("hermes model"));
    assert!(message.contains("hermes acp --check"));
}

#[test]
fn does_not_turn_hermes_provider_health_warnings_into_chat_errors() {
    assert_eq!(
        hermes_stderr_auth_error(
            "2026-09-17 10:24:42 [WARNING] agent.credential_pool: Copilot token exchange degraded to RAW token (exchange unavailable); enterprise-only models may 400 with model_not_available_for_integrator until exchange recovers."
        ),
        None
    );
}

#[test]
fn surfaces_explicit_hermes_authentication_failures_with_setup_help() {
    let message = hermes_stderr_auth_error(
        "2026-09-17 10:24:42 [ERROR] agent.provider: No LLM provider configured",
    )
    .unwrap();
    assert!(message.contains("No LLM provider configured"));
    assert!(message.contains("hermes model"));
}
