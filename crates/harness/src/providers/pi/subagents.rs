//! Port of src/integrations/harness/providers/pi/piSubagents.ts: map Pi
//! subagent extension results and omp `TaskToolDetails` onto agent rows.

use monocode_core::block::AgentStepKind;
use monocode_core::harness_event::HarnessEvent;
use monocode_core::js;
use serde_json::Value;

use super::deps::Rec;
use super::protocol::{
    as_record, preview_from_tool, string_field, text_from_content, tool_kind_from_name, tool_title,
};

/// JavaScript `String(value)` for the values these payloads carry.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => js::number_to_string(number.as_f64().unwrap_or(0.0)),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| {
                if item.is_null() {
                    String::new()
                } else {
                    js_string(item)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// JavaScript `===` for JSON values: numbers compare by value.
fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(Value::Number(a)), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
        (a, b) => a == b,
    }
}

/// JavaScript truthiness.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

fn is_nullish(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

fn records(value: Option<&Value>) -> Vec<&Rec> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_object).collect(),
        _ => Vec::new(),
    }
}

fn is_str(value: Option<&Value>, expected: &str) -> bool {
    value.and_then(Value::as_str) == Some(expected)
}

/// One `agent.step` without its `callId`, `agentName`, and `agentType`.
struct Step {
    step_id: String,
    kind: AgentStepKind,
    text: String,
    tool_kind: Option<String>,
    status: Option<String>,
    detail: Option<String>,
    preview: Option<monocode_core::block::ToolPreview>,
}

impl Step {
    fn new(step_id: String, kind: AgentStepKind, text: String) -> Self {
        Self {
            step_id,
            kind,
            text,
            tool_kind: None,
            status: None,
            detail: None,
            preview: None,
        }
    }
}

/// `piSubagentEvents`. Pi extension results and omp `TaskToolDetails` are
/// cumulative snapshots.
pub fn pi_subagent_events(
    call_id: &str,
    input: &Rec,
    result: Option<&Value>,
    completed: bool,
    is_error: bool,
) -> Vec<HarnessEvent> {
    let Some(details) = as_record(as_record(result).and_then(|result| result.get("details")))
    else {
        return Vec::new();
    };
    let results = records(details.get("results"));
    let progress = records(details.get("progress"));
    let entries = if progress.is_empty() {
        &results
    } else {
        &progress
    };
    if entries.is_empty() {
        return Vec::new();
    }
    let batch = entries.len() > 1
        || matches!(input.get("tasks"), Some(Value::Array(_)))
        || matches!(input.get("chain"), Some(Value::Array(_)));
    let mut events: Vec<HarnessEvent> = Vec::new();
    if batch {
        events.push(HarnessEvent::ToolUpdated {
            agent_model: None,
            call_id: call_id.to_string(),
            title: Some("Delegate subagents".into()),
            kind: Some("other".into()),
            status: None,
            detail: None,
            preview: None,
            paths: None,
        });
    }
    let empty = Rec::new();
    for (index, entry) in entries.iter().enumerate() {
        let entry: &Rec = entry;
        let key = match entry.get("index") {
            Some(value) if !value.is_null() => value.clone(),
            _ => Value::from(index),
        };
        let row_id = if batch {
            format!("{call_id}:agent:{}", js_string(&key))
        } else {
            call_id.to_string()
        };
        let final_rec: &Rec = if progress.is_empty() {
            entry
        } else {
            results
                .iter()
                .copied()
                .find(|item| {
                    (strict_eq(item.get("id"), entry.get("id")) && !is_nullish(item.get("id")))
                        || strict_eq(item.get("index"), Some(&key))
                })
                .unwrap_or(&empty)
        };
        let entry_status = entry.get("status");
        let failed = is_str(entry_status, "failed")
            || is_str(entry_status, "aborted")
            || final_rec.get("aborted") == Some(&Value::Bool(true))
            || is_str(final_rec.get("stopReason"), "error")
            || is_str(final_rec.get("stopReason"), "aborted")
            || final_rec
                .get("exitCode")
                .and_then(Value::as_f64)
                .is_some_and(|code| code > 0.0)
            || (completed
                && is_error
                && is_nullish(final_rec.get("exitCode"))
                && is_nullish(entry_status));
        let background = is_str(
            as_record(details.get("async")).and_then(|state| state.get("state")),
            "running",
        );
        let status = if failed {
            "failed"
        } else if is_str(entry_status, "completed") || (completed && !background) {
            "completed"
        } else {
            "in_progress"
        };
        let entry_opt = Some(entry);
        let final_opt = Some(final_rec);
        let name = string_field(entry_opt, "description")
            .or_else(|| string_field(entry_opt, "task"))
            .or_else(|| string_field(entry_opt, "agent"))
            .unwrap_or("Subagent")
            .to_string();
        let agent_type = string_field(entry_opt, "agent").map(str::to_string);
        let model = string_field(entry_opt, "model")
            .or_else(|| string_field(final_opt, "model"))
            .or_else(|| {
                records(final_rec.get("messages"))
                    .into_iter()
                    .filter(|message| is_str(message.get("role"), "assistant"))
                    .filter_map(|message| string_field(Some(message), "model"))
                    .next_back()
            })
            .map(str::to_string);
        let report = string_field(final_opt, "output")
            .or_else(|| string_field(final_opt, "errorMessage"))
            .or_else(|| string_field(final_opt, "error"))
            .map(str::to_string)
            .or_else(|| {
                failed.then(|| {
                    string_field(final_opt, "stderr")
                        .unwrap_or("Subagent failed.")
                        .to_string()
                })
            });
        events.push(HarnessEvent::ToolUpdated {
            agent_model: model,
            call_id: row_id.clone(),
            title: Some(name.clone()),
            kind: Some("agent".into()),
            status: Some(status.into()),
            detail: report,
            preview: None,
            paths: None,
        });
        let mut emit = |step: Step| {
            events.push(HarnessEvent::AgentStep {
                call_id: row_id.clone(),
                step_id: step.step_id,
                kind: step.kind,
                text: step.text,
                tool_kind: step.tool_kind,
                status: step.status,
                detail: step.detail,
                preview: step.preview,
                agent_name: Some(name.clone()),
                agent_type: agent_type.clone(),
            });
        };
        let messages = records(final_rec.get("messages"));
        let tool_results: Vec<(Option<&Value>, &Rec)> = messages
            .iter()
            .filter(|message| is_str(message.get("role"), "toolResult"))
            .map(|message| (message.get("toolCallId"), *message))
            .collect();
        for (message_index, message) in messages.iter().enumerate() {
            if !is_str(message.get("role"), "assistant") {
                continue;
            }
            for (part_index, part) in records(message.get("content")).into_iter().enumerate() {
                let step_id = format!("{row_id}:message:{message_index}:{part_index}");
                let part_type = part.get("type").and_then(Value::as_str);
                if matches!(part_type, Some("text" | "thinking")) {
                    let is_text = part_type == Some("text");
                    let text = string_field(Some(part), if is_text { "text" } else { "thinking" });
                    if let Some(text) = text {
                        let kind = if is_text {
                            AgentStepKind::Message
                        } else {
                            AgentStepKind::Reasoning
                        };
                        emit(Step::new(step_id, kind, text.to_string()));
                    }
                } else if part_type == Some("toolCall") {
                    let id = string_field(Some(part), "id");
                    let tool = string_field(Some(part), "name").unwrap_or("tool");
                    let args = as_record(part.get("arguments"))
                        .cloned()
                        .unwrap_or_default();
                    // A JavaScript Map keeps the last entry for a repeated key.
                    let lookup = id.map(|id| Value::String(id.to_string()));
                    let outcome = tool_results
                        .iter()
                        .rev()
                        .find(|(key, _)| strict_eq(*key, lookup.as_ref()))
                        .map(|(_, message)| *message);
                    let output =
                        text_from_content(outcome.and_then(|outcome| outcome.get("content")));
                    let outcome_failed =
                        outcome.is_some_and(|outcome| truthy(outcome.get("isError")));
                    let step_status = match outcome {
                        Some(_) if outcome_failed => "failed",
                        Some(_) => "completed",
                        None => status,
                    };
                    let step_key = match id {
                        Some(id) => id.to_string(),
                        None => format!("{message_index}:{part_index}"),
                    };
                    emit(Step {
                        tool_kind: Some(tool_kind_from_name(tool)),
                        status: Some(step_status.into()),
                        // Only a failure earns detail: a preview's output never
                        // shows on the row, so this is the one place the
                        // error can be read.
                        detail: (outcome_failed && !output.is_empty()).then(|| output.clone()),
                        preview: preview_from_tool(tool, &args, Some(&output)),
                        ..Step::new(
                            format!("{row_id}:tool:{step_key}"),
                            AgentStepKind::Tool,
                            tool_title(tool, &args),
                        )
                    });
                }
            }
        }
        // omp reports newest-first bounded tails and an increasing toolCount.
        let count = match entry.get("toolCount") {
            Some(Value::Number(count)) => count.as_f64().unwrap_or(0.0),
            _ => 0.0,
        };
        let current = string_field(entry_opt, "currentTool");
        let recent: Vec<&Rec> = records(entry.get("recentTools"))
            .into_iter()
            .rev()
            .collect();
        let all = recent.len() as f64;
        for (tool_index, tool) in recent.iter().enumerate() {
            let Some(tool_name) = string_field(Some(tool), "tool") else {
                continue;
            };
            let number =
                count - if current.is_some() { 1.0 } else { 0.0 } - all + tool_index as f64 + 1.0;
            let text = [Some(tool_name), string_field(Some(tool), "args")]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            emit(Step {
                tool_kind: Some(tool_kind_from_name(tool_name)),
                status: Some("completed".into()),
                ..Step::new(
                    format!("{row_id}:tool:{}", js::number_to_string(number)),
                    AgentStepKind::Tool,
                    text,
                )
            });
        }
        if let Some(current) = current {
            let text = [Some(current), string_field(entry_opt, "currentToolArgs")]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            emit(Step {
                tool_kind: Some(tool_kind_from_name(current)),
                status: Some(status.into()),
                ..Step::new(
                    format!("{row_id}:tool:{}", js::number_to_string(count)),
                    AgentStepKind::Tool,
                    text,
                )
            });
        }
        let output = match entry.get("recentOutput") {
            Some(Value::Array(lines)) => lines
                .iter()
                .filter_map(Value::as_str)
                .rev()
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        if !output.is_empty() {
            let requests = match entry.get("requests") {
                Some(value) if !value.is_null() => js_string(value),
                _ => js::number_to_string(count),
            };
            emit(Step::new(
                format!("{row_id}:output:{requests}"),
                AgentStepKind::Message,
                output,
            ));
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;
    use monocode_core::block::{AgentStep, Block};
    use monocode_core::reducer::apply_harness_events;
    use monocode_core::session::Session;
    use serde_json::json;

    fn rec(value: Value) -> Rec {
        value.as_object().cloned().unwrap()
    }

    /// `events.reduce(applyHarnessEvent, newSession("pi", "/repo"))`.
    fn apply(events: &[HarnessEvent]) -> Session {
        apply_harness_events(
            &Session::blank("s", HarnessId::Pi, "pi:default", "/repo"),
            events,
        )
    }

    fn steps(block: &Block) -> &[AgentStep] {
        &block.agent_run.as_ref().unwrap().steps
    }

    mod pi_subagent_snapshots {
        use super::*;

        #[test]
        fn merges_reasoning_prose_and_tool_results_without_duplicating_repeated_snapshots() {
            let result = json!({
                "details": { "results": [{
                    "agent": "scout",
                    "task": "Check auth",
                    "model": "claude-haiku-4-5",
                    "exitCode": 0,
                    "messages": [
                        { "role": "user", "content": [{ "type": "text", "text": "Private task input" }] },
                        { "role": "assistant", "content": [
                            { "type": "thinking", "thinking": "Follow the imports" },
                            { "type": "text", "text": "Reading auth" },
                            { "type": "toolCall", "id": "read-1", "name": "read", "arguments": { "path": "auth.ts" } },
                        ] },
                    ],
                }] },
            });
            let started = pi_subagent_events("spawn", &Rec::new(), Some(&result), false, false);
            let mut completed = result.clone();
            completed["details"]["results"][0]["messages"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "role": "toolResult",
                    "toolCallId": "read-1",
                    "isError": true,
                    "content": [{ "type": "text", "text": "Missing file" }],
                }));
            let mut events = started.clone();
            events.extend(started);
            events.extend(pi_subagent_events(
                "spawn",
                &Rec::new(),
                Some(&completed),
                true,
                false,
            ));
            let session = apply(&events);
            assert_eq!(session.blocks.len(), 1);
            let run = session.blocks[0].agent_run.as_ref().unwrap();
            assert_eq!(run.model.as_deref(), Some("claude-haiku-4-5"));
            let steps = steps(&session.blocks[0]);
            assert_eq!(steps.len(), 3);
            assert_eq!(
                (steps[0].kind, steps[0].text.as_str()),
                (AgentStepKind::Reasoning, "Follow the imports")
            );
            assert_eq!(
                (steps[1].kind, steps[1].text.as_str()),
                (AgentStepKind::Message, "Reading auth")
            );
            assert_eq!(steps[2].kind, AgentStepKind::Tool);
            assert_eq!(steps[2].status.as_deref(), Some("failed"));
            assert_eq!(steps[2].text, "Read auth.ts");
            // The step's only readable copy of why it failed.
            assert_eq!(steps[2].detail.as_deref(), Some("Missing file"));
        }

        #[test]
        fn gives_parallel_agents_distinct_rows_even_when_their_local_tool_ids_match() {
            let results: Vec<Value> = ["first", "second"]
                .iter()
                .enumerate()
                .map(|(index, task)| {
                    json!({
                        "agent": "scout",
                        "task": task,
                        "exitCode": index,
                        "stderr": if index == 1 { "Provider failed" } else { "" },
                        "messages": [{ "role": "assistant", "content": [{
                            "type": "toolCall", "id": "same", "name": "read", "arguments": { "path": format!("{task}.ts") }
                        }] }],
                    })
                })
                .collect();
            let result = json!({ "details": { "results": results } });
            let session = apply(&pi_subagent_events(
                "batch",
                &rec(json!({ "tasks": [{}, {}] })),
                Some(&result),
                true,
                true,
            ));
            let agents: Vec<&Block> = session
                .blocks
                .iter()
                .filter(|block| {
                    block.tool.as_ref().and_then(|tool| tool.kind.as_deref()) == Some("agent")
                })
                .collect();
            assert_eq!(agents.len(), 2);
            assert_eq!(
                agents
                    .iter()
                    .map(|block| block.text.as_str())
                    .collect::<Vec<_>>(),
                ["first", "second"]
            );
            assert_eq!(steps(agents[0])[0].text, "Read first.ts");
            assert_eq!(
                agents[0].tool.as_ref().unwrap().status.as_deref(),
                Some("completed")
            );
            assert_eq!(
                agents[1].tool.as_ref().unwrap().status.as_deref(),
                Some("failed")
            );
            assert_eq!(
                agents[1].tool.as_ref().unwrap().detail.as_deref(),
                Some("Provider failed")
            );
            assert_eq!(
                session.blocks[0].tool.as_ref().unwrap().kind.as_deref(),
                Some("other")
            );
        }
    }

    #[test]
    fn settles_the_current_tool_in_place_as_it_moves_into_the_recent_tools_tail() {
        let progress = json!({
            "index": 0,
            "id": "scout-1",
            "agent": "scout",
            "task": "Inspect auth",
            "status": "running",
            "toolCount": 1,
            "currentTool": "read",
            "currentToolArgs": "auth.ts",
            "recentTools": [],
            "recentOutput": ["Second line", "First line"],
        });
        let start = pi_subagent_events(
            "task",
            &Rec::new(),
            Some(&json!({ "details": { "progress": [progress.clone()], "results": [] } })),
            false,
            false,
        );
        let mut finished = progress.clone();
        finished["status"] = json!("completed");
        finished.as_object_mut().unwrap().remove("currentTool");
        finished["recentTools"] = json!([{ "tool": "read", "args": "auth.ts", "endMs": 123 }]);
        let end = pi_subagent_events(
            "task",
            &Rec::new(),
            Some(&json!({ "details": {
                "progress": [finished],
                "results": [{ "index": 0, "id": "scout-1", "exitCode": 0, "output": "Found the handler" }],
            } })),
            true,
            false,
        );
        let mut events = start;
        events.extend(end);
        let session = apply(&events);
        let row = &session.blocks[0];
        assert_eq!(
            row.tool.as_ref().unwrap().status.as_deref(),
            Some("completed")
        );
        assert_eq!(
            row.tool.as_ref().unwrap().detail.as_deref(),
            Some("Found the handler")
        );
        let tools: Vec<&AgentStep> = steps(row)
            .iter()
            .filter(|step| step.kind == AgentStepKind::Tool)
            .collect();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].status.as_deref(), Some("completed"));
        assert_eq!(tools[0].text, "read auth.ts");
        let message = steps(row)
            .iter()
            .find(|step| step.kind == AgentStepKind::Message)
            .unwrap();
        assert_eq!(message.text, "First line\nSecond line");
    }

    #[test]
    fn leaves_unknown_extension_detail_formats_alone() {
        assert!(
            pi_subagent_events(
                "call",
                &Rec::new(),
                Some(&json!({ "details": { "arbitrary": [] } })),
                false,
                false
            )
            .is_empty()
        );
    }

    #[test]
    fn keeps_background_runs_active_after_their_launch_tool_returns() {
        let events = pi_subagent_events(
            "task",
            &Rec::new(),
            Some(&json!({
                "details": {
                    "async": { "state": "running" },
                    "results": [],
                    "progress": [{
                        "index": 0, "agent": "scout", "task": "Explore", "status": "running", "toolCount": 0
                    }],
                },
            })),
            true,
            false,
        );
        match &events[0] {
            HarnessEvent::ToolUpdated { status, .. } => {
                assert_eq!(status.as_deref(), Some("in_progress"))
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn names_batch_rows_by_their_index_and_reads_failed_exit_codes() {
        let results: Vec<Value> = ["first", "second"]
            .iter()
            .enumerate()
            .map(|(index, task)| {
                json!({
                    "agent": "scout",
                    "task": task,
                    "exitCode": index,
                    "stderr": if index == 1 { "Provider failed" } else { "" },
                    "messages": [],
                })
            })
            .collect();
        let result = json!({ "details": { "results": results } });
        let events = pi_subagent_events(
            "batch",
            &rec(json!({ "tasks": [{}, {}] })),
            Some(&result),
            true,
            true,
        );
        let rows: Vec<(String, Option<String>, Option<String>)> = events
            .iter()
            .filter_map(|event| match event {
                HarnessEvent::ToolUpdated {
                    call_id,
                    status,
                    detail,
                    ..
                } => Some((call_id.clone(), status.clone(), detail.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("batch".to_string(), None, None),
                (
                    "batch:agent:0".to_string(),
                    Some("completed".to_string()),
                    None
                ),
                (
                    "batch:agent:1".to_string(),
                    Some("failed".to_string()),
                    Some("Provider failed".to_string())
                ),
            ]
        );
    }

    #[test]
    fn numbers_recent_omp_tools_oldest_first() {
        let events = pi_subagent_events(
            "task",
            &Rec::new(),
            Some(&json!({
                "details": {
                    "progress": [{
                        "index": 0,
                        "agent": "scout",
                        "status": "running",
                        "toolCount": 3,
                        "currentTool": "grep",
                        "recentTools": [{ "tool": "read", "args": "b.ts" }, { "tool": "read", "args": "a.ts" }],
                        "recentOutput": ["two", "one"],
                    }],
                    "results": [],
                },
            })),
            false,
            false,
        );
        let steps: Vec<(String, String)> = events
            .iter()
            .filter_map(|event| match event {
                HarnessEvent::AgentStep { step_id, text, .. } => {
                    Some((step_id.clone(), text.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            steps,
            [
                ("task:tool:1".to_string(), "read a.ts".to_string()),
                ("task:tool:2".to_string(), "read b.ts".to_string()),
                ("task:tool:3".to_string(), "grep".to_string()),
                ("task:output:3".to_string(), "one\ntwo".to_string()),
            ]
        );
    }
}
