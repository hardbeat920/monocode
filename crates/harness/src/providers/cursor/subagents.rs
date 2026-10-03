//! Port of src/integrations/harness/providers/cursor/cursorSubagents.ts:
//! subagent names, the events that replay a stored child run onto its parent
//! row, and the tool-name to kind map for Cursor's stored calls.

use std::collections::HashMap;
use std::sync::LazyLock;

use monocode_core::block::{AgentStepKind, ToolPreview, ToolPreviewKind};
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::HarnessEvent;
use monocode_core::js;
use monocode_core::session::Session;
use monocode_core::task_list::is_task_list_tool_name;
use regex::Regex;
use serde_json::Value;

use monocode_core::reducer::{extract_tool_preview, format_agent_type, title_from_tool_input};

use super::json::Rec;
use super::store::{StoreReader, StoredCursorSubagentRun};

static LEADING_PUNCTUATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[:\s·-]+").unwrap());
static AGENT_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:agent|task)\s*[:·-]\s*").unwrap());
static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:subagent(?:\s+task)?|task|agent|unspecified)$").unwrap());
static REVIEW_OF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:perform|conduct|do)\b.*?\b(?:code\s+)?review\s+of\s+").unwrap()
});
static IN_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+in\s+(?:/|[A-Za-z]:[\\/]).*$").unwrap());
static TRAILING_DOTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.\s]+$").unwrap());

/// `cursorAgentLabel`: a subagent name, or `None` for Cursor's placeholders
/// such as "Task: Subagent task".
pub fn cursor_agent_label(value: Option<&str>) -> Option<String> {
    let value = value?;
    let label = LEADING_PUNCTUATION.replace(js::trim(value), "");
    let label = AGENT_PREFIX.replace(&label, "");
    let label = js::trim(&label);
    (!label.is_empty() && !PLACEHOLDER.is_match(label)).then(|| label.to_string())
}

/// `promptLabel`: a name from the first line of the child's prompt.
fn prompt_label(prompt: Option<&str>) -> Option<String> {
    let first = js::trim(prompt?).split('\n').next()?;
    let line = js::trim(first);
    if line.is_empty() {
        return None;
    }
    let line = REVIEW_OF.replace(line, "Review ");
    let line = IN_PATH.replace(&line, "");
    Some(TRAILING_DOTS.replace(&line, "").into_owned())
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

/// `cursorSubagentEvents`: replay a stored child run onto its parent row.
pub fn cursor_subagent_events(
    run: &StoredCursorSubagentRun,
    title: Option<&str>,
) -> Vec<HarnessEvent> {
    let agent_type = non_empty(run.agent_type.as_deref());
    let type_label = match agent_type {
        Some(kind) => format!("{} subagent", format_agent_type(kind)),
        None => "Subagent".to_string(),
    };
    let label = cursor_agent_label(title);
    let name = label
        .filter(|label| *label != type_label)
        .or_else(|| prompt_label(run.prompt.as_deref()))
        .unwrap_or_else(|| type_label.clone());
    let mut events = vec![HarnessEvent::ToolUpdated {
        agent_model: non_empty(run.model.as_deref()).map(str::to_string),
        call_id: run.tool_call_id.clone(),
        title: Some(name.clone()),
        kind: Some("agent".into()),
        status: None,
        detail: None,
        preview: None,
        paths: None,
    }];
    for step in &run.steps {
        let args: Rec = match &step.args {
            Some(Value::Object(args)) => args.clone(),
            _ => Rec::new(),
        };
        let kind = kind_from_cursor_tool_name(step.tool_name.as_deref(), None);
        let tool_name = step.tool_name.clone().unwrap_or_else(|| "Tool".into());
        let mut tool = Rec::new();
        tool.insert("name".into(), Value::from(tool_name.clone()));
        if let Some(kind) = &kind {
            tool.insert("kind".into(), Value::from(kind.clone()));
        }
        tool.insert("rawInput".into(), Value::Object(args.clone()));
        if let Some(output) = &step.output {
            tool.insert("content".into(), Value::from(output.clone()));
        }
        let is_tool = step.kind == AgentStepKind::Tool;
        let preview = extract_tool_preview(&tool, &tool);
        let output = non_empty(step.output.as_deref());
        events.push(HarnessEvent::AgentStep {
            call_id: run.tool_call_id.clone(),
            step_id: step.id.clone(),
            kind: step.kind,
            text: if is_tool {
                title_from_tool_input(&tool_name, kind.as_deref().unwrap_or("other"), &args)
            } else {
                step.text.clone()
            },
            tool_kind: if is_tool { kind.clone() } else { None },
            status: if is_tool { step.status.clone() } else { None },
            // A preview's output is not shown on the row, so a failure has to
            // carry its own text to be readable at all.
            detail: match (is_tool, step.status.as_deref(), output) {
                (true, Some("failed"), Some(output)) => Some(output.to_string()),
                _ => None,
            },
            preview: if !is_tool {
                None
            } else if let Some(output) = output {
                let mut preview = preview.unwrap_or_else(|| {
                    let mut stub = ToolPreview::new(ToolPreviewKind::Read);
                    stub.content_only = Some(true);
                    stub
                });
                preview.output = Some(output.to_string());
                Some(preview)
            } else {
                preview
            },
            agent_name: Some(name.clone()),
            agent_type: agent_type.map(str::to_string),
        });
    }
    events
}

/// `recoverCursorSubagents`: reopen older Cursor rows with the work already
/// saved in their child stores. `apply` is the transcript reducer
/// (`applyHarnessEvent`), which lives in `monocode_core::reducer`.
pub async fn recover_cursor_subagents(
    session: Session,
    reader: &StoreReader,
    apply: impl Fn(Session, &HarnessEvent) -> Session,
) -> Session {
    if session.harness != HarnessId::Cursor {
        return session;
    }
    let Some(provider_session_id) = session
        .provider_session_id
        .clone()
        .filter(|id| !id.is_empty())
    else {
        return session;
    };
    let rows: Vec<(String, String)> = session
        .blocks
        .iter()
        .filter_map(|block| {
            let tool = block.tool.as_ref()?;
            if tool.kind.as_deref() != Some("agent") {
                return None;
            }
            let call_id = tool.call_id.clone().filter(|id| !id.is_empty())?;
            Some((
                call_id,
                tool.title.clone().unwrap_or_else(|| block.text.clone()),
            ))
        })
        .collect();
    if rows.is_empty() {
        return session;
    }
    let ids: Vec<String> = rows.iter().map(|(id, _)| id.clone()).collect();
    let ids = ids[ids.len().saturating_sub(256)..].to_vec();
    let runs = reader
        .read_stored_cursor_subagent_runs(&provider_session_id, ids, HashMap::new())
        .await
        .unwrap_or_default();
    // `new Map(rows...)` keeps the last title for a repeated call id.
    let titles: HashMap<String, String> = rows.into_iter().collect();
    let mut session = session;
    for run in runs {
        let Some(title) = titles.get(&run.tool_call_id) else {
            continue;
        };
        for event in cursor_subagent_events(&run, Some(title)) {
            session = apply(session, &event);
        }
    }
    session
}

/// `kindFromCursorToolName`: the transcript tool kind for a Cursor tool name.
pub fn kind_from_cursor_tool_name(name: Option<&str>, fallback: Option<&str>) -> Option<String> {
    let key = name.unwrap_or("").to_lowercase();
    let kind = if key == "grep" || key == "glob" || key == "rg" || key.contains("search") {
        "search"
    } else if key == "read" {
        "read"
    } else if ["edit", "write", "strreplace", "applypatch"].contains(&key.as_str()) {
        "edit"
    } else if key == "shell" || key == "bash" {
        "execute"
    } else if key == "skill" || key == "skills" {
        "skill"
    } else if key == "agent" || key == "task" || key == "subagent" {
        "agent"
    } else if is_task_list_tool_name(&key) {
        "tasks"
    } else {
        return fallback.map(str::to_string);
    };
    Some(kind.to_string())
}

/// The test fixture `run` in cursorSubagents.test.ts.
#[cfg(test)]
pub(crate) fn sample_run() -> StoredCursorSubagentRun {
    serde_json::from_value(serde_json::json!({
        "agentId": "child",
        "toolCallId": "spawn",
        "revision": "7",
        "agentType": "generalPurpose",
        "prompt": "Perform a read-only, defect-first code review of OpenCode subagent work in /repo.\nInspect the diff.",
        "steps": [
            { "id": "child:blob:0", "kind": "message", "text": "Inspecting the diff." },
            {
                "id": "child:tool:shell", "kind": "tool", "text": "", "toolName": "Shell",
                "args": { "command": "git diff" }, "status": "completed",
                "output": "diff --git a/a.ts b/a.ts"
            },
            { "id": "child:last:0", "kind": "message", "text": "One finding." }
        ]
    }))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::task::BoxFuture;
    use crate::providers::cursor::store::{CursorStore, NoCursorStore, StoredCursorToolCall};
    use monocode_core::reducer::apply_harness_event;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[test]
    fn rejects_placeholders_as_names() {
        for title in [
            "Task: Subagent task",
            ": Subagent task",
            "Subagent",
            "Task",
            "Agent",
        ] {
            assert_eq!(cursor_agent_label(Some(title)), None, "{title}");
        }
        assert_eq!(
            cursor_agent_label(Some("Task: Review OpenCode")).as_deref(),
            Some("Review OpenCode")
        );
        assert_eq!(cursor_agent_label(None), None);
    }

    #[test]
    fn names_a_run_from_its_prompt_and_maps_steps() {
        let events = cursor_subagent_events(&sample_run(), Some("Task: Subagent task"));
        assert_eq!(events.len(), 4);
        match &events[0] {
            HarnessEvent::ToolUpdated { title, kind, .. } => {
                assert_eq!(title.as_deref(), Some("Review OpenCode subagent work"));
                assert_eq!(kind.as_deref(), Some("agent"));
            }
            other => panic!("unexpected {other:?}"),
        }
        match &events[2] {
            HarnessEvent::AgentStep {
                text,
                tool_kind,
                status,
                preview,
                agent_name,
                agent_type,
                detail,
                ..
            } => {
                assert_eq!(text, "git diff");
                assert_eq!(tool_kind.as_deref(), Some("execute"));
                assert_eq!(status.as_deref(), Some("completed"));
                assert_eq!(detail, &None);
                assert_eq!(agent_name.as_deref(), Some("Review OpenCode subagent work"));
                assert_eq!(agent_type.as_deref(), Some("generalPurpose"));
                let preview = preview.as_ref().unwrap();
                assert_eq!(preview.output.as_deref(), Some("diff --git a/a.ts b/a.ts"));
            }
            other => panic!("unexpected {other:?}"),
        }
        match &events[1] {
            HarnessEvent::AgentStep {
                text,
                tool_kind,
                preview,
                ..
            } => {
                assert_eq!(text, "Inspecting the diff.");
                assert_eq!(tool_kind, &None);
                assert_eq!(preview, &None);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn keeps_a_useful_task_description() {
        let events = cursor_subagent_events(&sample_run(), Some("Task: Review OpenCode"));
        assert!(
            matches!(&events[0], HarnessEvent::ToolUpdated { title: Some(title), .. } if title == "Review OpenCode")
        );
        // A title equal to the type label falls through to the prompt.
        let events = cursor_subagent_events(&sample_run(), Some("GeneralPurpose subagent"));
        assert!(
            matches!(&events[0], HarnessEvent::ToolUpdated { title: Some(title), .. } if title == "Review OpenCode subagent work")
        );
    }

    #[test]
    fn carries_a_failed_steps_output_as_detail() {
        let mut run = sample_run();
        run.steps = vec![
            serde_json::from_value(json!({
                "id": "child:tool:shell", "kind": "tool", "text": "", "toolName": "Shell",
                "args": { "command": "npm test" }, "status": "failed",
                "output": "Tests failed: assertion error"
            }))
            .unwrap(),
        ];
        let events = cursor_subagent_events(&run, Some("Task: Subagent task"));
        match &events[1] {
            HarnessEvent::AgentStep {
                text,
                status,
                detail,
                ..
            } => {
                assert_eq!(text, "npm test");
                assert_eq!(status.as_deref(), Some("failed"));
                assert_eq!(detail.as_deref(), Some("Tests failed: assertion error"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn maps_cursor_tool_names_to_kinds() {
        let kind = |name| kind_from_cursor_tool_name(Some(name), None);
        assert_eq!(kind("Grep").as_deref(), Some("search"));
        assert_eq!(kind("semanticSearch").as_deref(), Some("search"));
        assert_eq!(kind("StrReplace").as_deref(), Some("edit"));
        assert_eq!(kind("Shell").as_deref(), Some("execute"));
        assert_eq!(kind("Task").as_deref(), Some("agent"));
        assert_eq!(kind("TodoWrite").as_deref(), Some("tasks"));
        assert_eq!(kind("mcp").as_deref(), None);
        assert_eq!(
            kind_from_cursor_tool_name(None, Some("read")).as_deref(),
            Some("read")
        );
    }

    struct Fixed {
        runs: Vec<StoredCursorSubagentRun>,
        fail: bool,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl CursorStore for Fixed {
        fn subagent_runs(
            &self,
            _session_id: String,
            tool_call_ids: Vec<String>,
            _known: HashMap<String, String>,
        ) -> BoxFuture<'static, anyhow::Result<Vec<StoredCursorSubagentRun>>> {
            self.calls.lock().unwrap().push(tool_call_ids);
            let result = if self.fail {
                Err(anyhow::anyhow!("store unavailable"))
            } else {
                Ok(self.runs.clone())
            };
            Box::pin(async move { result })
        }
        fn tool_calls(
            &self,
            _: String,
            _: Vec<String>,
        ) -> BoxFuture<'static, anyhow::Result<Vec<StoredCursorToolCall>>> {
            Box::pin(async { Ok(Vec::new()) })
        }
    }

    /// `savedSession` in cursorSubagents.test.ts: a finished Cursor row with
    /// a placeholder title.
    fn saved_session() -> Session {
        let mut session = Session::blank("s", HarnessId::Cursor, "cursor:composer-2.5", "/repo");
        session.provider_session_id = Some("parent".into());
        apply_harness_event(
            &session,
            &HarnessEvent::ToolUpdated {
                agent_model: None,
                call_id: "spawn".into(),
                title: Some("Task: Subagent task".into()),
                kind: Some("agent".into()),
                status: Some("completed".into()),
                detail: None,
                preview: None,
                paths: None,
            },
        )
    }

    fn reducer(session: Session, event: &HarnessEvent) -> Session {
        apply_harness_event(&session, event)
    }

    fn fixed(runs: Vec<StoredCursorSubagentRun>, fail: bool) -> Arc<Fixed> {
        Arc::new(Fixed {
            runs,
            fail,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn recover(session: Session, store: Arc<Fixed>) -> Session {
        let reader = StoreReader::new(store, false);
        smol::block_on(recover_cursor_subagents(session, &reader, reducer))
    }

    #[test]
    fn restores_a_saved_rows_name_steps_previews_and_completed_status() {
        let store = fixed(vec![sample_run()], false);
        let recovered = recover(saved_session(), store.clone());
        assert_eq!(
            store.calls.lock().unwrap().as_slice(),
            &[vec!["spawn".to_string()]]
        );
        let row = &recovered.blocks[0];
        assert_eq!(row.text, "Review OpenCode subagent work");
        assert_eq!(
            row.tool.as_ref().unwrap().status.as_deref(),
            Some("completed")
        );
        let steps = &row.agent_run.as_ref().unwrap().steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[1].text, "git diff");
        assert_eq!(steps[1].tool_kind.as_deref(), Some("execute"));
        assert_eq!(steps[1].status.as_deref(), Some("completed"));
        assert_ne!(recovered.busy, Some(true));
        let again = recover(recovered.clone(), fixed(vec![sample_run()], false));
        assert_eq!(again.blocks, recovered.blocks);
    }

    #[test]
    fn preserves_a_useful_task_description_and_failed_parent_status() {
        let session = apply_harness_event(
            &saved_session(),
            &HarnessEvent::ToolUpdated {
                agent_model: None,
                call_id: "spawn".into(),
                title: Some("Task: Review OpenCode".into()),
                kind: None,
                status: Some("failed".into()),
                detail: None,
                preview: None,
                paths: None,
            },
        );
        let recovered = recover(session, fixed(vec![sample_run()], false));
        assert_eq!(
            recovered.blocks[0].agent_run.as_ref().unwrap().name,
            "Review OpenCode"
        );
        assert_eq!(
            recovered.blocks[0].tool.as_ref().unwrap().status.as_deref(),
            Some("failed")
        );
    }

    #[test]
    fn carries_a_failed_steps_output_onto_the_row_as_error_detail() {
        let mut run = sample_run();
        run.steps = vec![
            serde_json::from_value(json!({
                "id": "child:tool:shell", "kind": "tool", "text": "", "toolName": "Shell",
                "args": { "command": "npm test" }, "status": "failed",
                "output": "Tests failed: assertion error"
            }))
            .unwrap(),
        ];
        let recovered = recover(saved_session(), fixed(vec![run], false));
        let step = &recovered.blocks[0].agent_run.as_ref().unwrap().steps[0];
        assert_eq!(step.text, "npm test");
        assert_eq!(step.status.as_deref(), Some("failed"));
        assert_eq!(
            step.detail.as_deref(),
            Some("Tests failed: assertion error")
        );
    }

    #[test]
    fn ignores_unrelated_calls_and_tolerates_an_unavailable_store() {
        let session = saved_session();
        let mut unrelated = sample_run();
        unrelated.tool_call_id = "unrelated".into();
        assert_eq!(
            recover(session.clone(), fixed(vec![unrelated], false)),
            session
        );
        assert_eq!(recover(session.clone(), fixed(vec![], true)), session);
        let mut codex = session.clone();
        codex.harness = HarnessId::Codex;
        let watched = fixed(vec![sample_run()], false);
        assert_eq!(recover(codex.clone(), watched.clone()), codex);
        assert!(watched.calls.lock().unwrap().is_empty());
        let reader = StoreReader::new(Arc::new(NoCursorStore), false);
        assert_eq!(
            smol::block_on(recover_cursor_subagents(session.clone(), &reader, reducer)),
            session
        );
    }
}
