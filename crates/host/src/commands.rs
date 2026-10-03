//! `parseCommand` from host/engine.ts: validates an untrusted
//! `commands.dispatch` payload into a [`HostCommand`].

use std::collections::BTreeMap;
use std::sync::LazyLock;

use monocode_core::RuntimeMode;
use monocode_core::user_question::UserQuestionReply;
use monocode_remote::host::attachments::parse_remote_attachments;
use monocode_remote::host::js;
use monocode_remote::host::protocol::{ApprovalDecision, HostCommand, SendIntent, parse_provider};
use regex::Regex;
use serde_json::{Map, Value};

use crate::git_worktrees::is_auto_worktree_branch;

static SETTING_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9]{0,63}$").unwrap());

/// `text`: a non-blank string of at most `max` UTF-16 units, without NUL.
fn text(value: Option<&Value>, label: &str, max: usize) -> Result<String, String> {
    match value {
        Some(Value::String(text))
            if !monocode_core::js::trim(text).is_empty()
                && monocode_core::js::len(text) <= max
                && !text.contains('\0') =>
        {
            Ok(text.clone())
        }
        _ => Err(format!("Invalid {label}")),
    }
}

/// `modelSettings`.
fn model_settings(value: Option<&Value>) -> Result<BTreeMap<String, String>, String> {
    let invalid = || "Invalid model settings".to_string();
    let entries = match value {
        None | Some(Value::Null) => return Ok(BTreeMap::new()),
        Some(Value::Object(entries)) => entries,
        Some(_) => return Err(invalid()),
    };
    if entries.len() > 20 {
        return Err(invalid());
    }
    entries
        .iter()
        .map(|(key, setting)| match setting {
            Value::String(setting)
                if SETTING_KEY.is_match(key)
                    && monocode_core::js::len(setting) <= 128
                    && !setting.contains('\0') =>
            {
                Ok((key.clone(), setting.clone()))
            }
            _ => Err(invalid()),
        })
        .collect()
}

fn runtime_mode(value: Option<&Value>) -> Option<RuntimeMode> {
    value.and_then(Value::as_str).and_then(RuntimeMode::parse)
}

/// `parseCommand`.
pub fn parse_command(input: &Value) -> Result<HostCommand, String> {
    let v = input.as_object().ok_or("Invalid command")?;
    parse_fields(v)
}

/// `parseCommand` for a payload that is already an object.
pub fn parse_fields(v: &Map<String, Value>) -> Result<HostCommand, String> {
    let get = |key: &str| v.get(key);
    let kind = get("type").and_then(Value::as_str);
    let command_id = text(get("commandId"), "command ID", 128)?;
    if kind == Some("create") {
        let harness = get("harness")
            .and_then(Value::as_str)
            .and_then(parse_provider);
        let (Some(harness), Some(runtime_mode)) = (harness, runtime_mode(get("runtimeMode")))
        else {
            return Err("Invalid provider or permission mode".into());
        };
        let auto_worktree_branch = match get("autoWorktreeBranch") {
            None => None,
            Some(Value::String(branch))
                if get("worktreeCwd").is_some() && is_auto_worktree_branch(branch) =>
            {
                Some(branch.clone())
            }
            Some(_) => return Err("Invalid automatically created worktree branch".into()),
        };
        let project_id = text(get("projectId"), "project ID", 128)?;
        let worktree_cwd = match get("worktreeCwd") {
            None => None,
            value => Some(text(value, "working copy", 4096)?),
        };
        let model = text(get("model"), "model", 200)?;
        let model_settings = match get("modelSettings") {
            None => None,
            value => Some(model_settings(value)?),
        };
        return Ok(HostCommand::Create {
            command_id,
            project_id,
            worktree_cwd,
            auto_worktree_branch,
            harness,
            model,
            model_settings,
            runtime_mode,
        });
    }
    let session_id = text(get("sessionId"), "session ID", 128)?;
    if kind == Some("configure") {
        let Some(runtime_mode) = runtime_mode(get("runtimeMode")) else {
            return Err("Invalid permission mode".into());
        };
        return Ok(HostCommand::Configure {
            command_id,
            session_id,
            model: text(get("model"), "model", 200)?,
            model_settings: model_settings(get("modelSettings"))?,
            runtime_mode,
        });
    }
    if kind == Some("compact") {
        return Ok(HostCommand::Compact {
            command_id,
            session_id,
        });
    }
    if kind == Some("send") || kind == Some("draft") {
        let send = kind == Some("send");
        let attachments = parse_remote_attachments(get("attachments"))?;
        let prompt = match get("text") {
            Some(Value::String(prompt))
                if monocode_core::js::len(prompt) <= 256_000
                    && !prompt.contains('\0')
                    && (!monocode_core::js::trim(prompt).is_empty()
                        || !attachments.is_empty()
                        || (send && get("draftBlockId").is_some())) =>
            {
                prompt.clone()
            }
            _ => return Err("Invalid prompt".into()),
        };
        let intent = get("intent");
        if send
            && intent.is_some()
            && !["default", "plan", "build"].contains(&js::string(intent).as_str())
        {
            return Err("Invalid turn intent".into());
        }
        if get("planBlockId").is_some()
            && (!send || intent.and_then(Value::as_str) != Some("build"))
        {
            return Err("Invalid plan build".into());
        }
        let attachments = (!attachments.is_empty()).then_some(attachments);
        if !send {
            return Ok(HostCommand::Draft {
                command_id,
                session_id,
                text: prompt,
                attachments,
            });
        }
        let intent = match intent.and_then(Value::as_str) {
            Some("default") => Some(SendIntent::Default),
            Some("plan") => Some(SendIntent::Plan),
            Some("build") => Some(SendIntent::Build),
            _ => None,
        };
        let draft_block_id = match get("draftBlockId") {
            None => None,
            value => Some(text(value, "draft block ID", 128)?),
        };
        let plan_block_id = match get("planBlockId") {
            None => None,
            value => Some(text(value, "plan block ID", 128)?),
        };
        return Ok(HostCommand::Send {
            command_id,
            session_id,
            text: prompt,
            attachments,
            intent,
            draft_block_id,
            plan_block_id,
        });
    }
    if kind == Some("removeDraft") {
        return Ok(HostCommand::RemoveDraft {
            command_id,
            session_id,
            draft_block_id: text(get("draftBlockId"), "draft block ID", 128)?,
        });
    }
    let run_id = text(get("runId"), "run ID", 128)?;
    if kind == Some("cancel") {
        return Ok(HostCommand::Cancel {
            command_id,
            session_id,
            run_id,
        });
    }
    let request_id = js::safe_integer(get("requestId"))
        .filter(|id| *id >= 0)
        .ok_or("Invalid request ID")?;
    let decision = match get("decision").and_then(Value::as_str) {
        Some("allow") => Some(ApprovalDecision::Allow),
        Some("deny") => Some(ApprovalDecision::Deny),
        _ => None,
    };
    if kind == Some("approve")
        && let Some(decision) = decision
    {
        return Ok(HostCommand::Approve {
            command_id,
            session_id,
            run_id,
            request_id,
            decision,
        });
    }
    if kind == Some("answer") {
        let reply = get("reply").and_then(Value::as_object);
        let reply_kind = reply
            .and_then(|reply| reply.get("kind"))
            .and_then(Value::as_str);
        if reply_kind == Some("skipped") {
            return Ok(HostCommand::Answer {
                command_id,
                session_id,
                run_id,
                request_id,
                reply: UserQuestionReply::Skipped,
            });
        }
        let answers = reply
            .and_then(|reply| reply.get("answers"))
            .and_then(Value::as_object);
        if let (Some("answered"), Some(answers)) = (reply_kind, answers) {
            let parsed: Option<BTreeMap<String, Vec<String>>> = (answers.len() <= 50)
                .then(|| {
                    answers
                        .iter()
                        .map(|(key, value)| {
                            let values = value.as_array()?;
                            if monocode_core::js::len(key) > 200 || values.len() > 50 {
                                return None;
                            }
                            let values = values
                                .iter()
                                .map(|item| {
                                    item.as_str()
                                        .filter(|item| monocode_core::js::len(item) <= 10_000)
                                        .map(str::to_string)
                                })
                                .collect::<Option<Vec<_>>>()?;
                            Some((key.clone(), values))
                        })
                        .collect()
                })
                .flatten();
            let answers = parsed.ok_or("Invalid question answers")?;
            let custom = match reply.and_then(|reply| reply.get("custom")) {
                None | Some(Value::Null) => None,
                Some(Value::Object(custom)) => Some(
                    custom
                        .iter()
                        .map(|(key, value)| {
                            value
                                .as_str()
                                .filter(|value| monocode_core::js::len(value) <= 10_000)
                                .map(|value| (key.clone(), value.to_string()))
                        })
                        .collect::<Option<BTreeMap<_, _>>>()
                        .ok_or("Invalid custom answers")?,
                ),
                Some(_) => return Err("Invalid custom answers".into()),
            };
            return Ok(HostCommand::Answer {
                command_id,
                session_id,
                run_id,
                request_id,
                reply: UserQuestionReply::Answered { answers, custom },
            });
        }
    }
    Err("Unsupported command".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// engine.test.ts: "validates untrusted commands before execution".
    #[test]
    fn validates_untrusted_commands_before_execution() {
        assert!(
            parse_command(
                &json!({ "type": "send", "commandId": "x", "sessionId": "y", "text": "" })
            )
            .is_err()
        );
        assert!(
            parse_command(&json!({
                "type": "create", "commandId": "x", "projectId": "y", "harness": "shell",
                "model": "x", "runtimeMode": "auto",
            }))
            .is_err()
        );
        assert!(
            parse_command(&json!({
                "type": "answer", "commandId": "x", "sessionId": "y", "runId": "z", "requestId": 1,
                "reply": { "kind": "answered", "answers": { "a": [42] } },
            }))
            .is_err()
        );
    }

    #[test]
    fn keeps_the_typescript_field_order_for_receipt_signatures() {
        let command = parse_command(&json!({
            "runtimeMode": "supervised", "model": "codex:test", "harness": "codex",
            "projectId": "p", "commandId": "c", "type": "create",
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_string(&command).unwrap(),
            r#"{"type":"create","commandId":"c","projectId":"p","harness":"codex","model":"codex:test","runtimeMode":"supervised"}"#
        );
        let send = parse_command(&json!({
            "type": "send", "commandId": "c", "sessionId": "s", "text": "Go", "intent": "plan",
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_string(&send).unwrap(),
            r#"{"type":"send","commandId":"c","sessionId":"s","text":"Go","intent":"plan"}"#
        );
    }

    #[test]
    fn checks_each_field_with_the_typescript_messages() {
        let error = |value: Value| parse_command(&value).unwrap_err();
        assert_eq!(error(json!([])), "Invalid command");
        assert_eq!(error(json!({ "type": "send" })), "Invalid command ID");
        assert_eq!(
            error(
                json!({ "type": "create", "commandId": "c", "harness": "codex", "runtimeMode": "supervised",
                "projectId": "p", "model": "m", "autoWorktreeBranch": "mc/12345678" })
            ),
            "Invalid automatically created worktree branch"
        );
        assert!(
            parse_command(
                &json!({ "type": "create", "commandId": "c", "harness": "codex",
                "runtimeMode": "supervised", "projectId": "p", "model": "m",
                "worktreeCwd": "/w", "autoWorktreeBranch": "mc/12345678" })
            )
            .is_ok()
        );
        assert_eq!(
            error(
                json!({ "type": "configure", "commandId": "c", "sessionId": "s", "runtimeMode": "x" })
            ),
            "Invalid permission mode"
        );
        assert_eq!(
            error(
                json!({ "type": "configure", "commandId": "c", "sessionId": "s", "runtimeMode": "auto",
                "model": "m", "modelSettings": { "1bad": "x" } })
            ),
            "Invalid model settings"
        );
        assert_eq!(
            error(
                json!({ "type": "send", "commandId": "c", "sessionId": "s", "text": "x", "intent": "fast" })
            ),
            "Invalid turn intent"
        );
        assert_eq!(
            error(
                json!({ "type": "send", "commandId": "c", "sessionId": "s", "text": "x", "planBlockId": "p" })
            ),
            "Invalid plan build"
        );
        assert!(
            parse_command(
                &json!({ "type": "send", "commandId": "c", "sessionId": "s", "text": "",
                "draftBlockId": "d" })
            )
            .is_ok()
        );
        assert_eq!(
            error(
                json!({ "type": "approve", "commandId": "c", "sessionId": "s", "runId": "r", "requestId": -1 })
            ),
            "Invalid request ID"
        );
        assert_eq!(
            error(
                json!({ "type": "approve", "commandId": "c", "sessionId": "s", "runId": "r",
                "requestId": 1, "decision": "maybe" })
            ),
            "Unsupported command"
        );
        assert_eq!(
            error(
                json!({ "type": "answer", "commandId": "c", "sessionId": "s", "runId": "r",
                "requestId": 1, "reply": { "kind": "answered", "answers": {}, "custom": [] } })
            ),
            "Invalid custom answers"
        );
        let answered = parse_command(
            &json!({ "type": "answer", "commandId": "c", "sessionId": "s",
            "runId": "r", "requestId": 1, "reply": { "kind": "answered", "answers": { "q": ["a"] },
            "custom": { "q": "free" } } }),
        )
        .unwrap();
        assert!(matches!(
            answered,
            HostCommand::Answer {
                reply: UserQuestionReply::Answered {
                    custom: Some(_),
                    ..
                },
                ..
            }
        ));
    }
}
