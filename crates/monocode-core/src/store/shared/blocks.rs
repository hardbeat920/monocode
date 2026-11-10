//! Lossless desktop block adaptation. Unknown fields and non-displayed blocks
//! stay in the source document when native rows are appended or edited.
use crate::{
    tool::{tasks, ToolCall, ToolState},
    transcript::{Row, Transcript},
};
use serde_json::{json, Value};

fn tool(block: &Value) -> ToolCall {
    let meta = &block["tool"];
    let preview = &meta["preview"];
    // Rich display metadata must supplement even a sparse nativeInput without
    // changing the recorded arguments. Older native envelopes remain readable.
    let native = meta["nativeInput"]
        .as_str()
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    let mut presentation = meta
        .get("nativePresentation")
        .filter(|value| value.is_object())
        .cloned()
        .or_else(|| {
            native
                .as_ref()
                .and_then(|value| value.get("__desktop"))
                .filter(|value| value.is_object())
                .cloned()
        })
        .unwrap_or_else(|| meta.clone());
    if let Some(fields) = presentation.as_object_mut() {
        // Output is separately bounded for display; the source block retains its
        // complete payload. Avoid recursively embedding prior native metadata.
        for key in [
            "detail",
            "rawOutput",
            "raw_output",
            "nativeInput",
            "nativePresentation",
        ] {
            fields.remove(key);
        }
    }
    if let Some(preview) = presentation
        .get_mut("preview")
        .and_then(Value::as_object_mut)
    {
        preview.remove("output");
    }
    let input = meta["nativeInput"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if let Some(raw) = meta
                .get("rawInput")
                .or_else(|| meta.get("raw_input"))
                .or_else(|| meta.get("input"))
            {
                return raw
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| raw.to_string());
            }
            let mut input = json!({});
            for key in ["path", "query", "startLine"] {
                if !preview[key].is_null() {
                    input[key] = preview[key].clone();
                }
            }
            if matches!(meta["kind"].as_str(), Some("execute" | "shell" | "bash")) {
                if let Some(command) = preview["title"]
                    .as_str()
                    .or(meta["title"].as_str())
                    .or(block["text"].as_str())
                {
                    input["command"] = json!(command);
                }
            }
            input.to_string()
        });
    let output = tasks::desktop_detail(meta);
    ToolCall {
        id: block["taskList"]["nativeCallId"]
            .as_str()
            .or(meta["callId"].as_str())
            .or(block["id"].as_str())
            .unwrap_or("")
            .into(),
        name: meta["nativeName"]
            .as_str()
            .unwrap_or_else(|| match meta["kind"].as_str().unwrap_or("") {
                "read" => "Read",
                "write" => "Write",
                "edit" => "Edit",
                "search" => "Grep",
                "execute" | "shell" | "bash" => "Bash",
                _ => meta["title"]
                    .as_str()
                    .or(block["text"].as_str())
                    .unwrap_or("Tool"),
            })
            .into(),
        input,
        output,
        panel: tasks::from_desktop(block),
        display: presentation.is_object().then_some(presentation),
        state: match meta["status"].as_str() {
            Some("failed" | "error") => ToolState::Failed,
            Some("cancelled" | "canceled" | "aborted") => ToolState::Cancelled,
            Some("in_progress" | "running" | "pending") => ToolState::Running,
            _ => ToolState::Completed,
        },
    }
}
fn project_content(block: &Value) -> Result<Vec<Row>, String> {
    if block["draft"] == true || (block["internal"] == true && block["role"] != "user") {
        return Ok(Vec::new());
    }
    if block["nativeRemotePlan"] == true && block["role"] == "plan" {
        return Ok(vec![metadata_row(
            "remote-plan",
            &json!({
                "id":block["id"],"text":block["text"],"streaming":block["streaming"].as_bool().unwrap_or(false),
                "status":block["plan"]["status"].as_str().unwrap_or("ready")
            }),
        )]);
    }
    let role = block["role"].as_str().unwrap_or("system");
    let text = block["text"].as_str().unwrap_or("");
    let mut t = Transcript::default();
    if block["interjection"].is_object() {
        let note = crate::transcript::Interjection {
            text: text.into(),
            custom_type: block["interjection"]["customType"]
                .as_str()
                .unwrap_or("custom")
                .into(),
            severity: block["interjection"]["severity"]
                .as_str()
                .map(str::to_owned),
        };
        t.begin("interjection");
        t.append(&super::encode(&note)?);
    } else if role == "tool" || (role == "tasks" && tasks::from_desktop(block).is_some()) {
        let mut call = tool(block);
        if role == "tasks" {
            call.name = "update_plan".into();
            call.input = block["taskList"].to_string();
        }
        if !matches!(call.panel(), Some(tasks::Panel::Agent(_))) {
            call.bound_output();
        }
        t.begin("tool");
        t.rows[0].text = super::encode(&call)?;
    } else {
        if text.is_empty()
            && (!matches!(role, "user" | "assistant" | "reasoning")
                || block["monoCard"].is_object())
        {
            return Ok(Vec::new());
        }
        t.begin(match role {
            "user" if block["internal"] == true => "mono-system",
            "user" | "assistant" | "reasoning" | "error" | "mono-card" | "visual" => role,
            "system" if block["notice"] == "error" => "error",
            "plan" | "tasks" => "assistant",
            _ => "system",
        });
        t.append(text);
    }
    // Desktop habit reports record only when their run happened.
    t.rows[0].sent_at = block["sentAt"]
        .as_u64()
        .or(block["startedAt"].as_u64())
        .or(block["monoHabit"]["at"].as_u64());
    Ok(t.rows)
}
fn metadata_row(role: &str, value: &Value) -> Row {
    Row {
        role: role.into(),
        text: value.to_string(),
        first: true,
        sent_at: None,
    }
}
fn project_block(block: &Value) -> Result<Vec<Row>, String> {
    if block["draft"] == true || (block["internal"] == true && block["role"] != "user") {
        return Ok(Vec::new());
    }
    let mut rows = project_content(block)?;
    if block["role"] == "user"
        && (block["nativeRemoteAttachments"] == true
            || block["attachments"]
                .as_array()
                .is_some_and(|files| !files.is_empty()))
    {
        rows.push(metadata_row(
            crate::transcript::attachments::ROLE,
            &block["attachments"],
        ));
    }
    for card in crate::mono::work::document_cards(block) {
        rows.push(metadata_row("mono-document", &card));
    }
    if let Some(id) = block["noteCard"]["id"].as_str() {
        rows.push(metadata_row("mono-note", &json!({"id":format!("note:{id}"),"type":"document","documentId":id,"title":block["noteCard"]["title"],"kind":"document","note":true})));
    }
    for launch in crate::mono::work::block_launches(std::slice::from_ref(block))
        .into_iter()
        .flatten()
    {
        let mut launch = launch;
        launch["sessionId"] = json!(crate::mono::work::session_handle(&launch["sessionId"]));
        rows.push(metadata_row("mono-launch", &launch));
    }
    if block["monoCard"].is_object() {
        let mut card = block["monoCard"].clone();
        card["id"] = block["id"].clone();
        if card["type"] == "session" {
            card["sessionId"] = json!(crate::mono::work::session_handle(&card["sessionId"]));
        }
        let mut row = metadata_row("mono-card", &card);
        row.sent_at = block["sentAt"].as_u64().or(block["startedAt"].as_u64());
        rows.push(row);
    }
    Ok(rows)
}
pub(crate) fn project(blocks: &[Value]) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for block in blocks {
        rows.extend(project_block(block)?);
    }
    Ok(rows)
}
fn update(old: Option<&Value>, rows: &[Row]) -> Result<Value, String> {
    let mut block = old
        .cloned()
        .unwrap_or_else(|| json!({"id":format!("gpui-{}",crate::mono::new_id())}));
    let role = rows[0].role.as_str();
    let text = rows.iter().map(|r| r.text.as_str()).collect::<String>();
    // Reuse the complete original object for unchanged display rows. In
    // particular, bounded tool previews must not truncate the stored output.
    if let Some(old) = old {
        if project_content(old)? == rows {
            return Ok(old.clone());
        }
    }
    if role == "interjection" {
        let note: crate::transcript::Interjection = super::parse(&text)?;
        block["role"] = json!("system");
        block["text"] = json!(note.text);
        block["interjection"] = json!({"customType":note.custom_type});
        if let Some(severity) = note.severity {
            block["interjection"]["severity"] = json!(severity);
        }
        if old.is_none() {
            block["sentAt"] = json!(rows[0].sent_at);
        }
    } else if role == "mono-card" {
        let mut card: Value = super::parse(&text)?;
        if old.is_none() && card["id"].is_string() {
            block["id"] = card["id"].clone();
        }
        if let Some(external) = old.and_then(|old| old["monoCard"]["sessionId"].as_str()) {
            // UI handles are a projection; never write one over the desktop UUID.
            card["sessionId"] = json!(external);
        }
        if let Some(object) = card.as_object_mut() {
            object.remove("id");
        }
        block["role"] = json!("assistant");
        block["text"] = json!("");
        block["monoCard"] = card;
    } else if role == "tool" {
        let call: ToolCall = super::parse(&text)?;
        let previous = old.map(tool);
        if let Some(tasks::Panel::Tasks(list)) = call.panel() {
            block["role"] = json!("tasks");
            block["text"] = json!(list
                .items
                .iter()
                .map(|item| format!(
                    "[{}] {}",
                    match item.status {
                        tasks::TaskStatus::Completed => "x",
                        tasks::TaskStatus::InProgress => "~",
                        tasks::TaskStatus::Cancelled => "-",
                        tasks::TaskStatus::Pending => " ",
                    },
                    item.text
                ))
                .collect::<Vec<_>>()
                .join("\n"));
            if !block["taskList"].is_object() {
                block["taskList"] = json!({});
            }
            let items = serde_json::to_value(&list.items).map_err(|e| e.to_string())?;
            let previous_items = block["taskList"]["items"].as_array().cloned();
            block["taskList"]["items"] = json!(items
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let old = previous_items.as_ref().and_then(|rows| {
                        if !item["id"].is_null() {
                            rows.iter().find(|row| row["id"] == item["id"])
                        } else {
                            rows.get(index).filter(|row| row["text"] == item["text"])
                        }
                    });
                    let mut merged = old.cloned().unwrap_or(json!({}));
                    for (key, value) in item.as_object().unwrap() {
                        merged[key] = value.clone();
                    }
                    merged
                })
                .collect::<Vec<_>>());
            block["taskList"]["nativeCallId"] = json!(call.id);
            block["taskList"]["explanation"] = json!(list.explanation);
            if old.is_none() {
                block["sentAt"] = json!(rows[0].sent_at);
            }
            return Ok(block);
        }
        block["role"] = json!("tool");
        if let Some(tasks::Panel::Agent(run)) = call.panel() {
            let updated = tasks::desktop_agent(&run);
            if !block["agentRun"].is_object() {
                block["agentRun"] = json!({});
            }
            for (key, value) in updated.as_object().unwrap() {
                if key == "steps" {
                    let mut steps = block["agentRun"]["steps"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    for step in value.as_array().unwrap() {
                        if let Some(old) = steps.iter_mut().find(|old| old["id"] == step["id"]) {
                            for (field, value) in step.as_object().unwrap() {
                                old[field] = value.clone();
                            }
                        } else {
                            steps.push(step.clone());
                        }
                    }
                    block["agentRun"]["steps"] = json!(steps);
                } else {
                    block["agentRun"][key] = value.clone();
                }
            }
        }
        if !block["tool"].is_object() {
            block["tool"] = json!({});
        }
        block["tool"]["callId"] = json!(call.id);
        block["tool"]["nativeName"] = json!(call.name);
        block["tool"]["nativeInput"] = json!(call.input);
        if let Some(display) = &call.display {
            block["tool"]["nativePresentation"] = display.clone();
        }
        if previous
            .as_ref()
            .is_none_or(|p| p.name != call.name || p.input != call.input)
        {
            let input: Value =
                serde_json::from_str(&call.input).unwrap_or(json!({"command":call.input}));
            let kind = match call.name.as_str() {
                "Bash" | "commandExecution" => "execute",
                "Read" => "read",
                "Write" => "write",
                "Edit" | "fileChange" => "edit",
                "Grep" | "Glob" | "webSearch" => "search",
                "Agent" | "Task" | "collabAgentToolCall" | "subAgentActivity" => "agent",
                _ => "other",
            };
            block["tool"]["kind"] = json!(kind);
            block["tool"]["title"] = json!(input["command"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or(input["path"].as_str())
                .or(input["file_path"].as_str())
                .unwrap_or(&call.name));
            block["text"] = block["tool"]["title"].clone();
            // Native tool input remains available without removing rich
            // previews, subagent activity or other desktop-only metadata.
            if !block["tool"]["preview"].is_object() {
                block["tool"]["preview"] = json!({});
            }
            for field in ["path", "query", "title"] {
                if !input[field].is_null() {
                    block["tool"]["preview"][field] = input[field].clone();
                }
            }
            if kind == "execute" {
                block["tool"]["preview"]["title"] = block["tool"]["title"].clone();
            }
        }
        if previous.as_ref().is_none_or(|p| {
            let mut p = p.clone();
            p.bound_output();
            p.output != call.output
        }) {
            block["tool"]["detail"] = json!(call.output);
        }
        if previous.as_ref().is_none_or(|p| p.state != call.state) {
            block["tool"]["status"] = json!(match call.state {
                ToolState::Running => "in_progress",
                ToolState::Completed => "completed",
                ToolState::Failed => "error",
                ToolState::Cancelled => "cancelled",
            });
        }
    } else {
        // Plans and task lists keep their desktop role and associated metadata.
        if old.is_none() {
            block["role"] = json!(match role {
                "mono-system" => "user",
                "error" => "system",
                _ => role,
            });
            if role == "error" {
                block["notice"] = json!("error");
            }
            if role == "mono-system" {
                block["internal"] = json!(true);
            }
        }
        block["text"] = json!(text);
    }
    if old.is_none() {
        if let Some(at) = rows[0].sent_at {
            block["sentAt"] = json!(at);
        }
    }
    Ok(block)
}
pub(super) fn merge(blocks: &[Value], start: usize, delta: &[Row]) -> Result<Vec<Value>, String> {
    let mut rows = project(blocks)?;
    let original_len = rows.len();
    if start > rows.len() {
        return Err(
            "Transcript changed; refusing to overwrite a different conversation revision".into(),
        );
    }
    for (ix, row) in delta.iter().enumerate() {
        if let Some(existing) = rows.get_mut(start + ix) {
            *existing = row.clone();
        } else {
            rows.push(row.clone());
        }
    }
    let mut groups = std::collections::BTreeMap::new();
    let mut from = 0;
    while from < rows.len() {
        let mut to = from + 1;
        while to < rows.len() && !rows[to].first {
            to += 1;
        }
        groups.insert(from, &rows[from..to]);
        from = to;
    }
    let mut offset = 0;
    let mut result = Vec::new();
    for block in blocks {
        let displayed = project_block(block)?;
        if displayed.is_empty() {
            result.push(block.clone());
            continue;
        }
        let mut value = block.clone();
        let mut local = 0;
        while local < displayed.len() {
            let end = (local + 1..displayed.len())
                .find(|&i| displayed[i].first)
                .unwrap_or(displayed.len());
            let group = groups.remove(&(offset + local)).ok_or(
                "A saved message boundary changed; the original conversation was preserved",
            )?;
            let original = &displayed[local..end];
            if group != original {
                if group[0].role == "mono-card" && value["monoCard"].is_object() {
                    let card: Value =
                        super::parse(&group.iter().map(|r| r.text.as_str()).collect::<String>())?;
                    let external = value["monoCard"]["sessionId"].clone();
                    value["monoCard"] = card;
                    if external.is_string() {
                        value["monoCard"]["sessionId"] = external;
                    }
                    if let Some(object) = value["monoCard"].as_object_mut() {
                        object.remove("id");
                    }
                } else if !matches!(
                    group[0].role.as_str(),
                    "mono-document"
                        | "mono-note"
                        | "mono-launch"
                        | crate::transcript::attachments::ROLE
                ) {
                    value = update(Some(&value), group)?;
                } else {
                    return Err(
                        "Card references changed; the original conversation was preserved".into(),
                    );
                }
            }
            local = end;
        }
        result.push(value);
        offset += displayed.len();
    }
    for (offset, rows) in groups {
        if offset < original_len {
            return Err("Invalid transcript update".into());
        }
        if rows[0].role == crate::transcript::attachments::ROLE {
            let prompt = result
                .last_mut()
                .filter(|block| block["role"] == "user")
                .ok_or("Attachment manifest has no user message; history was preserved")?;
            let files: Value =
                super::parse(&rows.iter().map(|r| r.text.as_str()).collect::<String>())?;
            if !files.is_array() {
                return Err("Invalid attachment manifest; history was preserved".into());
            }
            prompt["attachments"] = files;
        } else {
            result.push(update(None, rows)?);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod notice_tests {
    use super::*;

    #[test]
    fn errors_use_desktop_notices_and_retain_their_identity_after_reload() {
        let source = json!({"id":"failure","role":"system","notice":"error","text":"Stopped","sentAt":123,"future":{"keep":true}});
        let mut rows = project(std::slice::from_ref(&source)).unwrap();
        assert_eq!(rows[0].role, "error");
        assert_eq!(
            merge(std::slice::from_ref(&source), 0, &rows).unwrap(),
            [source]
        );

        rows[0].text = "Provider disconnected".into();
        let fresh = merge(&[], 0, &rows).unwrap();
        assert_eq!(fresh[0]["role"], "system");
        assert_eq!(fresh[0]["notice"], "error");
        assert_eq!(project(&fresh).unwrap(), rows);

        let legacy = json!({"id":"legacy","role":"error","text":"Stopped"});
        assert_eq!(project(&[legacy]).unwrap()[0].role, "error");
    }
}

#[cfg(test)]
mod panel_tests {
    use super::*;
    use crate::tool::tasks::{AgentRun, AgentStep, Panel, TaskList};
    #[test]
    fn native_visuals_survive_chunking_storage_and_later_messages() {
        let document = crate::visual::VisualDocument {
            id: "visual-plot".into(),
            title: "Results 🙂".into(),
            source: format!(
                "// {}\nreturn () => visual.text('Ready');",
                "sample ".repeat(800)
            ),
        };
        let mut transcript = Transcript::default();
        transcript.begin("visual");
        transcript.append(&serde_json::to_string(&document).unwrap());
        assert!(transcript.rows.len() > 1);
        let saved = merge(&[], 0, &transcript.rows).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0]["role"], "visual");
        let rows = project(&saved).unwrap();
        assert_eq!(rows, transcript.rows);
        let loaded: crate::visual::VisualDocument =
            serde_json::from_str(&rows.iter().map(|row| row.text.as_str()).collect::<String>())
                .unwrap();
        assert_eq!(loaded.source, document.source);
        assert_eq!(loaded.title, document.title);
        let start = transcript.rows.len();
        transcript.begin("assistant");
        transcript.append("The interactive results are ready.");
        let saved = merge(&saved, start, &transcript.rows[start..]).unwrap();
        assert_eq!(project(&saved).unwrap(), transcript.rows);
    }
    fn row(call: &ToolCall) -> Row {
        Row {
            role: "tool".into(),
            text: serde_json::to_string(call).unwrap(),
            first: true,
            sent_at: Some(123),
        }
    }
    #[test]
    fn structured_task_identity_explanation_and_source_metadata_survive_edits() {
        let source = json!({"id":"block","role":"tasks","text":"[ ] Inspect","taskList":{"sourceId":"provider-source","items":[{"id":"actual","text":"Inspect","status":"pending","future":true}],"futureField":"keep"},"unknown":7});
        let projected = project(std::slice::from_ref(&source)).unwrap();
        let mut call: ToolCall = serde_json::from_str(&projected[0].text).unwrap();
        let Some(Panel::Tasks(mut list)) = call.panel() else {
            panic!()
        };
        list.items[0].status = tasks::TaskStatus::Completed;
        call.panel = Some(Panel::Tasks(list));
        let merged = merge(&[source], 0, &[row(&call)]).unwrap();
        assert_eq!(merged[0]["unknown"], 7);
        assert_eq!(merged[0]["taskList"]["futureField"], "keep");
        assert_eq!(merged[0]["taskList"]["items"][0]["future"], true);
        let reloaded = project(&merged).unwrap();
        let loaded: ToolCall = serde_json::from_str(&reloaded[0].text).unwrap();
        assert_eq!(loaded.panel(), call.panel());
        assert_eq!(loaded.id, call.id);
        let empty = TaskList {
            items: Vec::new(),
            explanation: "Explicit clear".into(),
        };
        call.panel = Some(Panel::Tasks(empty));
        let cleared = merge(&merged, 0, &[row(&call)]).unwrap();
        assert_eq!(cleared[0]["taskList"]["items"], json!([]));
    }
    #[test]
    fn child_steps_and_raw_native_input_roundtrip_without_erasing_desktop_metadata() {
        let source = json!({"id":"block","role":"tool","text":"Review","tool":{"callId":"actual-call","kind":"agent","title":"Review","status":"in_progress","future":true},"agentRun":{"name":"Reviewer","steps":[{"id":"old-step","kind":"tool","text":"Read","preview":{"path":"keep.rs"},"future":9}],"unknownAgent":true}});
        let mut call = tool(&source);
        call.name = "Agent".into();
        call.input = json!({"description":"Review","model":"child-model"}).to_string();
        let run = call.agent_mut().unwrap();
        run.ids.push("actual-child-thread".into());
        run.model = Some("child-model".into());
        run.upsert(AgentStep {
            id: "old-step".into(),
            kind: "tool".into(),
            text: "Read source.rs".into(),
            state: Some(ToolState::Completed),
            detail: "actual result".into(),
            ..AgentStep::default()
        });
        let merged = merge(&[source], 0, &[row(&call)]).unwrap();
        assert_eq!(merged[0]["agentRun"]["unknownAgent"], true);
        assert_eq!(
            merged[0]["agentRun"]["steps"][0]["preview"]["path"],
            "keep.rs"
        );
        assert_eq!(merged[0]["agentRun"]["steps"][0]["future"], 9);
        assert_eq!(merged[0]["tool"]["future"], true);
        let loaded = tool(&merged[0]);
        assert_eq!(loaded.input, call.input);
        assert_eq!(loaded.panel(), call.panel());
        assert!(
            matches!(loaded.panel(), Some(Panel::Agent(AgentRun { ids, .. })) if ids == vec!["actual-child-thread"])
        );
    }
}

#[cfg(test)]
mod card_tests {
    use super::*;
    #[test]
    fn rich_desktop_cards_project_without_rewriting_source_when_later_reply_changes() {
        let source = vec![
            json!({"id":"prompt","role":"user","text":"Write a report","future":"keep","noteCard":{"id":"note","slug":"read-me","title":"Reference","sourceCwd":"/repo","future":true},"artifactCards":[{"id":"report","kind":"document","title":"Report","summary":"Summary","future":7}],"monoSpawnedSessions":[{"sessionId":"app-child","cwd":"/repo","title":"Review","harness":"codex","model":"gpt-6","future":8}]}),
            json!({"id":"reply","role":"assistant","text":"Written","future":{"keep":true}}),
            json!({"id":"choice","role":"assistant","text":"","monoCard":{"type":"session","sessionId":"app-child","note":"Review this"},"future":9}),
        ];
        let rows = project(&source).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.role.as_str()).collect::<Vec<_>>(),
            [
                "user",
                "mono-document",
                "mono-note",
                "mono-launch",
                "assistant",
                "mono-card"
            ]
        );
        assert_eq!(merge(&source, 0, &rows).unwrap(), source);
        let mut reply = rows[4].clone();
        reply.text = "Written and checked".into();
        let merged = merge(&source, 4, &[reply]).unwrap();
        assert_eq!(merged[0], source[0]);
        assert_eq!(merged[2], source[2]);
        assert_eq!(merged[1]["future"], source[1]["future"]);
        assert_eq!(project(&merged).unwrap().len(), rows.len());
        let mut card = rows[5].clone();
        let mut value: Value = serde_json::from_str(&card.text).unwrap();
        value["title"] = json!("Live title");
        card.text = value.to_string();
        let patched = merge(&merged, 5, &[card]).unwrap();
        assert_eq!(patched[2]["monoCard"]["sessionId"], "app-child");
    }
    #[test]
    fn internal_completion_prompts_keep_live_row_offsets_and_timing_after_reload() {
        let source = json!({"id":"completion","role":"user","internal":true,"text":"Monitor finished","sentAt":123,"future":true});
        let rows = project(std::slice::from_ref(&source)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].role, "mono-system");
        assert_eq!(rows[0].sent_at, Some(123));
        assert_eq!(
            merge(std::slice::from_ref(&source), 0, &rows).unwrap(),
            [source]
        );
        let native = merge(&[], 0, &rows).unwrap();
        assert_eq!(native[0]["role"], "user");
        assert_eq!(native[0]["internal"], true);
        assert_eq!(project(&native).unwrap(), rows);
    }
    #[test]
    fn new_native_cards_keep_their_identity_and_reload_as_cards() {
        let mut row = metadata_row(
            "mono-card",
            &json!({"id":"card-identity","type":"choices","options":["Yes","No"]}),
        );
        row.sent_at = Some(123);
        let saved = merge(&[], 0, std::slice::from_ref(&row)).unwrap();
        assert_eq!(saved[0]["id"], "card-identity");
        let reloaded = project(&saved).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].role, row.role);
        assert_eq!(reloaded[0].first, row.first);
        assert_eq!(reloaded[0].sent_at, row.sent_at);
        // The workspace enables serde_json's insertion-order feature. Card IDs
        // live on the outer desktop block and are restored into the card, so
        // compare the payload rather than its object-key serialization order.
        assert_eq!(
            serde_json::from_str::<Value>(&reloaded[0].text).unwrap(),
            serde_json::from_str::<Value>(&row.text).unwrap()
        );
    }
}

#[cfg(test)]
mod tool_presentation_tests {
    use super::*;
    #[test]
    fn restored_desktop_read_preserves_path_line_and_rich_source() {
        let source = json!({"id":"read","role":"tool","text":"Read","tool":{"kind":"read","title":"Read","status":"completed","rawInput":{"file_path":"src/nested.rs","offset":18},"preview":{"kind":"read","path":"src/nested.rs","fileName":"nested.rs","startLine":18,"lines":[{"number":18,"text":"fn main() {}"}]},"future":true}});
        let projected = project(std::slice::from_ref(&source)).unwrap();
        let call: ToolCall = serde_json::from_str(&projected[0].text).unwrap();
        let shown = call.presentation();
        assert_eq!(
            (shown.action.as_str(), shown.target.as_str()),
            ("Read", "src/nested.rs")
        );
        assert_eq!(shown.start_line, Some(18));
        assert_eq!(call.output, "18  fn main() {}");
        assert_eq!(
            merge(std::slice::from_ref(&source), 0, &projected).unwrap(),
            vec![source]
        );
    }
    #[test]
    fn restored_semantic_shell_caption_keeps_real_command_and_output() {
        let source = json!({"id":"read","role":"tool","text":"Read src/lib.rs","tool":{"kind":"execute","title":"Read src/lib.rs","status":"error","preview":{"kind":"shell","title":"/bin/zsh -lc \"cat src/lib.rs\"","path":"src/lib.rs"},"detail":"permission denied","rawInput":{"command":"cat src/lib.rs"},"future":{"keep":true}}});
        let call = tool(&source);
        assert_eq!(call.summary(), ("Read".into(), "src/lib.rs".into()));
        assert_eq!(
            call.presentation().command.as_deref(),
            Some("cat src/lib.rs")
        );
        assert_eq!(call.state, ToolState::Failed);
        assert_eq!(call.output, "permission denied");
        let projected = project(std::slice::from_ref(&source)).unwrap();
        assert_eq!(
            merge(std::slice::from_ref(&source), 0, &projected).unwrap(),
            vec![source]
        );
    }
    #[test]
    fn native_input_is_exact_and_execution_path_is_not_a_read() {
        let raw = "{ \"command\": \"npm run build\", \"cwd\": \"/repo\" }";
        let source = json!({"id":"run","role":"tool","tool":{"kind":"execute","nativeName":"Bash","nativeInput":raw,"preview":{"path":"/repo"}}});
        let call = tool(&source);
        assert_eq!(call.input, raw);
        assert_eq!(call.summary(), ("Run".into(), "npm run build".into()));
        assert_eq!(call.presentation().file_path, None);
    }
    #[test]
    fn sparse_native_input_uses_rich_preview_after_status_save_and_reload() {
        let raw = "{  }";
        let source = json!({"id":"old-native","role":"tool","tool":{"nativeName":"Read","nativeInput":raw,"kind":"read","title":"Read","status":"in_progress","preview":{"path":"/repo/feature/src/lib.rs","startLine":12},"future":true}});
        let mut call = tool(&source);
        assert_eq!(call.input, raw);
        assert_eq!(
            call.presentation_in(Some("/repo/feature")).target,
            "src/lib.rs"
        );
        call.state = ToolState::Completed;
        let row = Row {
            role: "tool".into(),
            text: serde_json::to_string(&call).unwrap(),
            first: true,
            sent_at: None,
        };
        let saved = merge(&[source], 0, &[row]).unwrap();
        assert_eq!(saved[0]["tool"]["nativeInput"], raw);
        assert_eq!(saved[0]["tool"]["future"], true);
        let reloaded = tool(&saved[0]);
        assert_eq!(reloaded.input, raw);
        assert_eq!(reloaded.presentation().start_line, Some(12));
        assert_eq!(
            reloaded.presentation_in(Some("/repo/feature")).target,
            "src/lib.rs"
        );
    }
}
