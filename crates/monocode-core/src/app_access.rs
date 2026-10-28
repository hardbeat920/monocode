//! Shared project actions for ordinary Operator threads and resident Monos.
//! Visibility and Mono-only capabilities belong to the caller, not launch JSON.
use crate::{
    agent::{AccessMode, Settings},
    catalog::{Model, ModelControl},
    provider::Kind,
    store::Store,
};
use serde_json::{json, Value};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

pub fn validate_operator(action: &str, input: &Value) -> Result<(), String> {
    if !matches!(
        action,
        "models.list"
            | "sessions.list"
            | "sessions.read"
            | "sessions.start"
            | "sessions.send"
            | "sessions.draft"
            | "sessions.stop"
            | "sessions.archive"
            | "sessions.delete"
            | "folders.list"
            | "folders.move"
            | "notes.list"
            | "notes.read"
            | "notes.write"
            | "worktrees.list"
            | "worktrees.create"
    ) {
        return Err("Unknown Operator action. Run app help for project actions; memory, habits and artifacts require a Mono.".into());
    }
    crate::mono::api::validate(action, input)?;
    if input["notifyOnComplete"] == true {
        return Err("Completion notifications are only available in a Mono's chat".into());
    }
    Ok(())
}

pub fn text(input: &Value, key: &str, max: usize) -> Result<String, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= max)
        .map(|s| s.trim().to_owned())
        .ok_or_else(|| format!("Provide {key} (up to {max} bytes)."))
}
pub fn optional_text(input: &Value, key: &str, max: usize) -> Result<Option<String>, String> {
    input.get(key).map(|_| text(input, key, max)).transpose()
}
pub fn prompt(input: &Value) -> Result<String, String> {
    let prompt = text(input, "prompt", 240_000)?;
    if crate::composer::consume_command(&prompt, "operator").is_some() {
        return Err("App calls cannot enable /operator in another session".into());
    }
    Ok(prompt)
}
pub fn fingerprint(action: &str, input: &Value) -> String {
    crate::mono::files::content_hash(&format!("{action}:{input}"))
}
pub fn receipt_key(caller: i64, request: &str) -> String {
    format!(
        "operator-app-receipt:{caller}:{}",
        crate::mono::files::content_hash(request)
    )
}

/// Advertise precisely the IDs and values accepted by launch_settings.
pub fn model_controls(kind: Kind, model: &Model) -> Vec<ModelControl> {
    let mut controls = model.controls.clone();
    if !model.efforts.is_empty() && !controls.iter().any(|c| is_effort(&c.id)) {
        controls.push(ModelControl {
            id: if kind == Kind::Codex {
                "reasoningEffort"
            } else {
                "effort"
            }
            .into(),
            name: "Effort".into(),
            current: model.default_effort.clone().unwrap_or_default(),
            choices: model
                .efforts
                .iter()
                .map(|e| (e.clone(), e.clone()))
                .collect(),
        });
    }
    for (enabled, id, name) in [
        (model.fast, "fast", "Fast mode"),
        (model.thinking, "thinking", "Thinking"),
    ] {
        if enabled && !controls.iter().any(|c| c.id == id) {
            controls.push(ModelControl {
                id: id.into(),
                name: name.into(),
                current: "false".into(),
                choices: vec![("false".into(), "Off".into()), ("true".into(), "On".into())],
            });
        }
    }
    if !model.tiers.is_empty() && !controls.iter().any(|c| c.id == "serviceTier") {
        controls.push(ModelControl {
            id: "serviceTier".into(),
            name: "Service tier".into(),
            current: "default".into(),
            choices: std::iter::once(("default".into(), "Default".into()))
                .chain(model.tiers.clone())
                .collect(),
        });
    }
    controls
}
fn is_effort(key: &str) -> bool {
    matches!(
        key,
        "effort" | "reasoning" | "reasoningEffort" | "reasoning_effort" | "variant"
    )
}
pub fn model_description(kind: Kind, model: &Model) -> Value {
    json!({"id":model.id,"name":model.name,"settings":model_controls(kind,model).iter().map(|c| json!({"id":c.id,"label":c.name,"defaultValue":c.current,"options":c.choices.iter().map(|(value,label)|json!({"value":value,"label":label})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}

/// Discovery runs on workers, independently of which provider views are open.
pub fn models_catalog(profiles: Vec<(Kind, crate::accounts::Profile)>) -> Result<Value, String> {
    let jobs:Vec<_>=profiles.into_iter().map(|(kind,profile)|std::thread::spawn(move|| {
        let available=crate::provider::resolve(kind).is_some();
        let result=if available {crate::catalog::models_for(kind,&profile)}else{Ok(Vec::new())};
        let (models,error)=match result {Ok(models)=>(models,None),Err(error)=>(Vec::new(),Some(error))};
        json!({"id":kind.id(),"available":available,"models":models.iter().map(|m|model_description(kind,m)).collect::<Vec<_>>(),"error":error})
    })).collect();
    let harnesses = jobs
        .into_iter()
        .map(|j| j.join().map_err(|_| "Model discovery failed".to_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"harnesses":harnesses,"runtimeModes":[
        {"id":"read-only","label":"Read-only","description":"Inspect without changing files"},
        {"id":"supervised","label":"Supervised","description":"Ask before changes requiring approval"},
        {"id":"auto-accept-edits","label":"Auto-accept edits","description":"Allow file edits with approval for other actions"},
        {"id":"auto","label":"Auto","description":"Review approvals automatically"},
        {"id":"full-access","label":"Full access","description":"Allow actions with full access"}
    ]}))
}

/// Account choice is supplied by the caller; transports and credentials are never
/// inherited from a Mono or an Operator grant into its child.
pub fn launch_settings(
    input: &Value,
    source_provider: &str,
    mut settings: Settings,
    kind: Kind,
    models: &[Model],
) -> Result<Settings, String> {
    crate::mono::api::validate("sessions.start", input)?;
    let requested =
        optional_text(input, "model", 512)?.and_then(|m| crate::store::native_model(&m, kind.id()));
    let changing = source_provider != kind.id()
        || requested
            .as_ref()
            .is_some_and(|m| settings.model.as_ref() != Some(m));
    if changing {
        settings.model = None;
        settings.effort = None;
        settings.model_settings.clear();
        settings.fast = false;
        settings.thinking = None;
        settings.service_tier = None;
    }
    if let Some(model) = requested {
        settings.model = Some(model);
    }
    let selected = settings
        .model
        .as_deref()
        .and_then(|id| models.iter().find(|m| m.id == id))
        .or_else(|| models.iter().find(|m| m.default))
        .or_else(|| models.first());
    if input.get("model").is_some()
        && !models
            .iter()
            .any(|m| Some(m.id.as_str()) == settings.model.as_deref())
    {
        return Err("Unknown model; run models.list for exact model IDs".into());
    }
    if settings.model.is_none() {
        settings.model = selected.map(|m| m.id.clone());
    }
    let mut values: HashMap<String, String> = input
        .get("modelSettings")
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or_default();
    if let Some(effort) = optional_text(input, "effort", 128)? {
        let model = selected.ok_or("No model settings available; run models.list first")?;
        let control = model_controls(kind, model)
            .into_iter()
            .find(|c| is_effort(&c.id))
            .ok_or("This model does not expose an effort setting")?;
        values.insert(control.id, effort);
    }
    if !values.is_empty() {
        let controls = model_controls(
            kind,
            selected.ok_or("No model settings available; run models.list first")?,
        );
        for (key, value) in &values {
            if !controls
                .iter()
                .any(|c| c.id == *key && c.choices.iter().any(|(id, _)| id == value))
            {
                return Err(format!(
                    "Invalid model setting {key}; run models.list for allowed values"
                ));
            }
            if is_effort(key) {
                settings.effort = Some(value.clone());
            }
            match key.as_str() {
                "fast" | "fastMode" => settings.fast = value == "true",
                "thinking" => settings.thinking = Some(value == "true"),
                "serviceTier" => {
                    settings.service_tier = (value != "default").then(|| value.clone())
                }
                _ => (),
            }
        }
        settings.model_settings.extend(values);
    }
    if let Some(mode) = input["runtimeMode"].as_str() {
        settings.access = AccessMode::from_id(mode);
    }
    let workspace = input["workspaceMode"].as_str().unwrap_or("current");
    if input.get("worktreeBase").is_some() && workspace != "worktree" {
        return Err("worktreeBase requires workspaceMode worktree".into());
    }
    if input.get("worktreeCwd").is_some() && workspace == "worktree" {
        return Err("worktreeCwd requires workspaceMode current".into());
    }
    settings.mono = None;
    settings.mono_message_id = None;
    settings.mono_sent_at = None;
    settings.operator = false;
    settings.operator_enabled = false;
    settings.orchestrator = false;
    settings.plan = false;
    settings.environment.clear();
    settings.attachments.clear();
    settings.turns.clear();
    settings.internal_message = false;
    settings.usage_limit = None;
    settings.handoff = None;
    settings.pending_provider_switch = None;
    settings.remote_action = None;
    settings.prepared_prompt = None;
    settings.turn_model = None;
    settings.text_only = false;
    settings.worktree_removed = false;
    settings.new_worktree = false;
    if workspace == "worktree" {
        settings.worktree_cwd = None;
    }
    Ok(settings)
}

/// Runs on a worker. Only a listed, existing checkout may be selected.
pub fn launch_checkout(
    input: &Value,
    project: &str,
    session: i64,
    settings: &mut Settings,
) -> Result<(), String> {
    if let Some(cwd) = optional_text(input, "worktreeCwd", 4096)? {
        let snapshot = crate::git::snapshot(std::path::Path::new(project))?;
        let selected = snapshot
            .worktrees
            .into_iter()
            .find(|w| !w.missing && crate::files::path_key(&w.path) == crate::files::path_key(&cwd))
            .ok_or("Worktree is unavailable in this project; run worktrees.list")?;
        settings.worktree_cwd = (crate::files::path_key(&selected.path)
            != crate::files::path_key(project))
        .then_some(selected.path);
    } else if input["workspaceMode"] == "worktree" {
        settings.worktree_cwd = Some(
            crate::git::create_worktree(
                std::path::Path::new(project),
                input["worktreeBase"].as_str().unwrap_or("HEAD"),
                session,
            )?
            .to_string_lossy()
            .into_owned(),
        );
    }
    Ok(())
}

fn bounded(
    input: &Value,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, String> {
    let value = input
        .get(key)
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| format!("{key} must be an integer"))
        })
        .transpose()?
        .unwrap_or(default as u64);
    if value < min as u64 || value > max as u64 {
        return Err(format!("{key} must be from {min} to {max}"));
    }
    Ok(value as usize)
}
fn capped(text: &str, max: usize) -> Value {
    let text = text.trim();
    json!({"text":text.chars().take(max).collect::<String>(),"truncated":text.chars().count()>max})
}
/// Tauri's prose-only exchanges, with canonical source block IDs as cursors.
pub fn conversation_page(blocks: &[Value], input: &Value) -> Result<Value, String> {
    let limit = bounded(input, "limit", 3, 1, 3)?;
    let max = bounded(input, "maxChars", 1200, 200, 6000)?;
    let mut turns: Vec<(&Value, Vec<&Value>)> = Vec::new();
    for block in blocks {
        if block["role"] == "user"
            && (block["internal"] != true || block["monoSessionCompletion"].is_object())
            && block["draft"] != true
        {
            turns.push((block, Vec::new()));
        } else if block["role"] == "assistant"
            && block["internal"] != true
            && block["text"].as_str().is_some_and(|s| !s.trim().is_empty())
        {
            if let Some((_, answers)) = turns.last_mut() {
                answers.push(block);
            }
        }
    }
    let end = match input.get("before") {
        Some(Value::String(before)) => turns
            .iter()
            .position(|(b, _)| b["id"] == *before)
            .ok_or("before is not a turn ID in this session")?,
        Some(value) => value
            .as_u64()
            .ok_or("before must be a turn ID")?
            .min(turns.len() as u64) as usize,
        None => turns.len(),
    };
    let start = end.saturating_sub(limit);
    let selected = &turns[start..end];
    Ok(json!({"turns":selected.iter().map(|(user,answers)|{
        let text=user["text"].as_str().unwrap_or("");
        let text=crate::composer::consume_command(text,"operator").unwrap_or(text);
        json!({"turnId":user["id"],"user":capped(text,max),"assistant":answers.last().map(|b|capped(b["text"].as_str().unwrap_or(""),max)),"earlierAssistantMessages":answers.len().saturating_sub(1)})
    }).collect::<Vec<_>>(),"nextBefore":if start>0 {selected.first().map(|(b,_)|b["id"].clone())}else{None}}))
}

/// Notes use their actual persisted string IDs, including desktop-created notes.
pub fn notes_action(
    store: &Store,
    caller: i64,
    request: &str,
    action: &str,
    input: &Value,
    projects: &[String],
) -> Result<Value, String> {
    let notes = store.list_notes()?;
    let allowed = |note: &crate::notes::Note| {
        note.source_cwd.as_deref().is_none_or(|cwd| {
            projects
                .iter()
                .any(|p| crate::files::path_key(p) == crate::files::path_key(cwd))
        })
    };
    let id = input
        .get("id")
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_u64().map(|id| id.to_string()))
                .ok_or("id must be a note ID")
        })
        .transpose()?;
    if action == "notes.list" {
        let limit = bounded(input, "limit", 30, 1, 100)?;
        let offset = bounded(input, "offset", 0, 0, usize::MAX)?;
        let notes: Vec<_> = notes.iter().filter(|n| allowed(n)).collect();
        return Ok(
            json!({"total":notes.len(),"offset":offset,"notes":notes.iter().skip(offset).take(limit).map(|n|json!({"id":n.id,"title":n.title,"preview":n.preview(),"tags":n.tags,"sourceCwd":n.source_cwd})).collect::<Vec<_>>() }),
        );
    }
    let current = id
        .as_deref()
        .map(|id| {
            notes
                .iter()
                .find(|n| {
                    (n.id == id
                        || n.id == format!("gpui-note-{id}")
                        || id.parse::<u64>().is_ok_and(|number| {
                            crate::store::desktop_session_handle(&n.id) as u64 == number
                        }))
                        && allowed(n)
                })
                .ok_or("Note not found in this project")
        })
        .transpose()?;
    if action == "notes.read" {
        return current.map(|note| json!(note)).ok_or("Provide id".into());
    }
    let title = optional_text(input, "title", crate::notes::MAX_TITLE)?;
    let body = input
        .get("body")
        .map(|v| {
            v.as_str()
                .filter(|s| s.len() <= 240_000)
                .map(|s| s.replace("\r\n", "\n").replace('\r', "\n"))
                .ok_or("body must be text under 240000 bytes")
        })
        .transpose()?;
    let tags = input
        .get("tags")
        .map(|v| serde_json::from_value::<Vec<String>>(v.clone()).map_err(|e| e.to_string()))
        .transpose()?;
    if current.is_some() && title.is_none() && body.is_none() && tags.is_none() {
        return Err("Supply title, body or tags to update a note".into());
    }
    if current.is_none() && body.is_none() {
        return Err("body is required to create a note".into());
    }
    let body = body.unwrap_or_else(|| current.unwrap().body.clone());
    let id = current.map(|n| n.id.clone()).or(id).unwrap_or_else(|| {
        format!(
            "app-note-{caller}-{}",
            crate::mono::files::content_hash(request)
        )
    });
    let note = store.upsert_note(&crate::notes::NoteUpsert {
        id,
        title: title
            .or_else(|| current.map(|n| n.title.clone()))
            .unwrap_or_else(|| crate::notes::title(&body)),
        body,
        tags: tags
            .or_else(|| current.map(|n| n.tags.clone()))
            .unwrap_or_default(),
        source_session_id: match current {
            Some(note) => note.source_session_id.clone(),
            None => Some(store.persisted_session_id(caller)?),
        },
        source_cwd: current
            .map(|n| n.source_cwd.clone())
            .unwrap_or_else(|| projects.first().cloned()),
        finalize_slug: true,
    })?;
    Ok(json!(note))
}
