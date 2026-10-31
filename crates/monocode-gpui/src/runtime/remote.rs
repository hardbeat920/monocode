//! The host owns agent execution. Desktop disconnection never cancels it.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use monocode_core::{
    agent::{Decision, Question, QuestionItem, QuestionOption},
    store::{RemoteSession, RemoteUpload},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
};

fn update(status: impl Into<String>, finished: bool) -> Update {
    Update {
        worktree_cwd: None,
        commands: None,
        provider_turn_id: None,
        turn_ready: false,
        follow_up_delivered: vec![],
        follow_up_errors: vec![],
        delivered: None,
        start: 0,
        rows: vec![],
        provider_id: None,
        model: None,
        config_model: None,
        config_settings: None,
        context: None,
        metrics: None,
        usage_limit: None,
        background: None,
        approvals: vec![],
        approvals_resolved: vec![],
        approvals_save_failed: vec![],
        questions: vec![],
        questions_resolved: vec![],
        handoff: None,
        status: status.into(),
        finished,
    }
}
fn command(kind: &str, session_id: Option<&str>) -> Value {
    let mut value = json!({"type":kind,"commandId":monocode_core::mono::new_id()});
    if let Some(id) = session_id {
        value["sessionId"] = json!(id);
    }
    value
}
pub(crate) fn host_model(provider: &str, model: Option<&str>) -> Result<String, String> {
    let model = model
        .filter(|value| !value.trim().is_empty())
        .ok_or("Choose a model on the remote machine before sending")?;
    Ok(if model.starts_with(&format!("{provider}:")) {
        model.into()
    } else {
        format!("{provider}:{model}")
    })
}
fn model_settings(provider: &str, settings: &Settings) -> HashMap<String, String> {
    monocode_core::remote::protocol::wire_model_settings(provider, settings)
}
/// Serializes command preparation with idle setting updates for this desktop
/// session, while observation itself never holds an operation lock.
fn operation_lock(store: &Database, id: i64) -> Arc<Mutex<()>> {
    type Locks = Mutex<HashMap<(usize, i64), std::sync::Weak<Mutex<()>>>>;
    static LOCKS: std::sync::OnceLock<Locks> = std::sync::OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks.retain(|_, lock| lock.strong_count() > 0);
    let key = (Arc::as_ptr(store) as usize, id);
    if let Some(lock) = locks.get(&key).and_then(std::sync::Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}
fn configure_command(
    provider: &str,
    settings: &Settings,
    session_id: &str,
) -> Result<Value, String> {
    let mut value = command("configure", Some(session_id));
    value["model"] = json!(host_model(provider, settings.model.as_deref())?);
    value["modelSettings"] = json!(model_settings(provider, settings));
    value["runtimeMode"] = json!(settings.access.id());
    Ok(value)
}
/// Called off-thread after saving an idle remote conversation's settings.
/// Read the latest settings after acquiring the operation lock so an older
/// background callback cannot overwrite a more recent model/access choice.
pub(crate) fn configure_saved(store: Database, id: i64) -> Result<(), String> {
    let lock = operation_lock(&store, id);
    let _guard = lock.lock().map_err(|_| "Remote session is unavailable")?;
    let (mut binding, settings) = {
        let store = locked(&store)?;
        let Some(binding) = store.remote_session(id)? else {
            return Ok(());
        };
        if binding.session_id.is_none() || !binding.outbox.is_empty() {
            return Ok(());
        }
        (binding, store.agent_settings(id)?.unwrap_or_default())
    };
    let client = monocode_core::remote::default_connections()?;
    let machine = binding.machine(&client)?;
    let snapshot =
        client.load_session(&machine.id, binding.session_id.as_deref().unwrap(), None)?;
    if snapshot["status"] == "running" {
        return Ok(());
    }
    let next = configure_command(
        &binding.summary.provider,
        &settings,
        binding.session_id.as_deref().unwrap(),
    )?;
    if next["model"] == snapshot["session"]["model"]
        && next["modelSettings"] == snapshot["session"]["modelSettings"]
        && next["runtimeMode"] == snapshot["session"]["runtimeMode"]
    {
        return Ok(());
    }
    binding.outbox.push(next);
    journal(&store, &binding)?;
    dispatch_outbox(&client, &machine.id, &store, &mut binding)
}

fn journal(store: &Database, binding: &RemoteSession) -> Result<(), String> {
    locked(store)?.save_remote_session(binding)
}
/// Commit durable state before changing the worker's view of pending commands.
fn commit(
    store: &Database,
    binding: &mut RemoteSession,
    next: RemoteSession,
) -> Result<(), String> {
    journal(store, &next)?;
    *binding = next;
    Ok(())
}
/// Commands are written before I/O. A lost response leaves the original ID for
/// Retry or restart recovery; the host's receipt is the only commit boundary.
fn dispatch_outbox(
    client: &monocode_core::remote::Connections,
    machine: &str,
    store: &Database,
    binding: &mut RemoteSession,
) -> Result<(), String> {
    while let Some(mut next) = binding.outbox.first().cloned() {
        if next["type"] != "create" && next["sessionId"].is_null() {
            next["sessionId"] = json!(
                binding
                    .session_id
                    .as_deref()
                    .ok_or("Remote session creation has not been confirmed")?
            );
            let mut durable = binding.clone();
            durable.outbox[0] = next.clone();
            commit(store, binding, durable)?;
        }
        if binding
            .uploads
            .iter()
            .any(|upload| Some(upload.command_id.as_str()) == next["commandId"].as_str())
        {
            return Err("Remote attachments have not finished uploading".into());
        }
        let receipt = match client.request(machine, "commands.dispatch", next.clone()) {
            Ok(receipt) => receipt,
            Err(error) => {
                if error.starts_with("Host rejected request:") {
                    let mut durable = binding.clone();
                    durable.rejected = Some(
                        json!({"error":error,"commands":binding.outbox,"uploads":binding.uploads}),
                    );
                    durable.outbox.clear();
                    durable.uploads.clear();
                    commit(store, binding, durable)?;
                }
                return Err(error);
            }
        };
        let id = receipt["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or("Host command receipt has no session identity")?;
        if receipt["commandId"] != next["commandId"] {
            return Err("Host returned a receipt for another command".into());
        }
        if next["type"] != "create" && binding.session_id.as_deref() != Some(id) {
            return Err("Host returned a receipt for another session".into());
        }
        if receipt["revision"].as_u64().is_none() {
            return Err("Host command receipt has no revision".into());
        }
        let mut durable = binding.clone();
        if next["type"] == "create" {
            durable.session_id = Some(id.into());
        }
        durable.outbox.remove(0);
        commit(store, binding, durable)?;
    }
    Ok(())
}
const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
const ATTACHMENT_CHUNK_BYTES: usize = 512 * 1024;

fn attachment_type(path: &std::path::Path) -> (&'static str, &'static str) {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => ("image/png", "image"),
        "jpg" | "jpeg" => ("image/jpeg", "image"),
        "webp" => ("image/webp", "image"),
        "gif" => ("image/gif", "image"),
        "wav" => ("audio/wav", "audio"),
        "mp3" => ("audio/mpeg", "audio"),
        "m4a" => ("audio/mp4", "audio"),
        "pdf" => ("application/pdf", "file"),
        _ => ("application/octet-stream", "file"),
    }
}
/// Reserves host references locally so the whole send can be journaled before
/// any attachment bytes leave this desktop. The local paths never go on wire.
fn prepare_attachments(
    files: &[std::path::PathBuf],
    command_id: &str,
) -> Result<(Vec<Value>, Vec<RemoteUpload>), String> {
    if files.len() > 20 {
        return Err("Too many attachments (20 maximum)".into());
    }
    let mut references = Vec::new();
    let mut uploads = Vec::new();
    for path in files {
        let metadata =
            std::fs::metadata(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        let size = metadata.len();
        if size > MAX_ATTACHMENT_BYTES {
            return Err(format!(
                "{} is too large to send to a remote machine (20 MB maximum)",
                path.display()
            ));
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|name| !name.trim().is_empty() && name.encode_utf16().count() <= 255)
            .ok_or("Invalid attachment file name")?;
        let id = monocode_core::remote::protocol::new_attachment_id();
        let (mime, kind) = attachment_type(path);
        references.push(json!({"id":id,"name":name,"size":size,"mimeType":mime,"kind":kind}));
        uploads.push(RemoteUpload {
            command_id: command_id.into(),
            id,
            path: path.clone(),
            size,
        });
    }
    Ok((references, uploads))
}
fn prepare_for_session(
    client: &monocode_core::remote::Connections,
    machine: &str,
    environment: &str,
    files: &[std::path::PathBuf],
    command_id: &str,
) -> Result<(Vec<Value>, Vec<RemoteUpload>), String> {
    if files.len() > 20 {
        return Err("Too many attachments (20 maximum)".into());
    }
    let mut references = Vec::new();
    let mut uploads = Vec::new();
    for path in files {
        let source = path.to_string_lossy();
        if monocode_core::remote::attachments::is_attachment_source(&source) {
            let source = monocode_core::remote::attachments::parse_source(&source)
                .ok_or("Invalid remote attachment")?;
            if source.environment != environment {
                return Err("Attachment belongs to another machine".into());
            }
            let snapshot = client.load_session(machine, &source.session, None)?;
            let file = snapshot["session"]["blocks"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|block| block["attachments"].as_array().into_iter().flatten())
                .find(|file| {
                    file["id"].as_str() == Some(source.id.as_str())
                        && file["size"].as_u64() == Some(source.size as u64)
                })
                .ok_or("Original remote attachment is no longer available")?;
            let mut reference = json!({});
            for key in ["id", "name", "mimeType", "kind", "size"] {
                reference[key] = file[key].clone();
            }
            references.push(reference);
        } else {
            let (files, pending) = prepare_attachments(std::slice::from_ref(path), command_id)?;
            references.extend(files);
            uploads.extend(pending);
        }
    }
    Ok((references, uploads))
}

fn upload_pending(
    client: &monocode_core::remote::Connections,
    machine: &str,
    store: &Database,
    binding: &mut RemoteSession,
) -> Result<(), String> {
    while let Some(upload) = binding.uploads.first().cloned() {
        let mut file = std::fs::File::open(&upload.path)
            .map_err(|e| format!("Cannot read {}: {e}", upload.path.display()))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.len() != upload.size
            || upload.size > MAX_ATTACHMENT_BYTES
        {
            return Err(
                "Attachment changed before upload. Restore the original file before retrying."
                    .into(),
            );
        }
        let mut offset = 0u64;
        loop {
            let mut data = vec![0; ATTACHMENT_CHUNK_BYTES];
            let read = file.read(&mut data).map_err(|e| e.to_string())?;
            data.truncate(read);
            if read == 0 && offset > 0 {
                break;
            }
            let expected = offset + read as u64;
            if expected > upload.size {
                return Err("Attachment changed during upload".into());
            }
            let reply = client.request(machine,"attachments.upload",json!({"id":upload.id,"offset":offset,"size":upload.size,"data":STANDARD.encode(&data)}))?;
            if reply["offset"].as_u64() != Some(expected) {
                return Err("Host returned an invalid attachment offset".into());
            }
            offset = expected;
            if read == 0 {
                break;
            }
        }
        if offset != upload.size {
            return Err("Attachment changed during upload".into());
        }
        let mut durable = binding.clone();
        durable.uploads.remove(0);
        commit(store, binding, durable)?;
    }
    Ok(())
}
fn accounting(
    snapshot: &Value,
) -> (
    Option<monocode_core::context::Usage>,
    Option<monocode_core::agent::TurnMetrics>,
) {
    let context = snapshot["session"]
        .get("context")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let metrics = snapshot["session"]["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find(|block| block["role"] == "user" && block["draft"] != true)
        .and_then(|block| block.get("turnMetrics"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    (context, metrics)
}

pub(crate) fn question(snapshot: &Value) -> Option<Question> {
    let prompt = &snapshot["session"]["pendingQuestion"];
    let id = prompt["requestId"].as_i64()?;
    let items = prompt["questions"]
        .as_array()?
        .iter()
        .map(|item| QuestionItem {
            id: item["id"].as_str().unwrap_or("").into(),
            prompt: item["prompt"].as_str().unwrap_or("").into(),
            multiple: item["multiSelect"] == true,
            allow_custom: item["allowCustom"] == true,
            options: item["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|option| QuestionOption {
                    id: option["id"].as_str().unwrap_or("").into(),
                    label: option["label"].as_str().unwrap_or("").into(),
                    description: option["description"].as_str().unwrap_or("").into(),
                })
                .collect(),
        })
        .collect();
    Some(Question {
        id: id.to_string(),
        title: prompt["title"].as_str().unwrap_or("Question").into(),
        items,
        auto_resolve_at: prompt["autoResolveAt"].as_u64(),
    })
}
pub(crate) fn approvals(snapshot: &Value) -> Vec<Approval> {
    snapshot["session"]["blocks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| {
            let approval = &block["approval"];
            if !approval.is_object()
                || approval
                    .get("decided")
                    .is_some_and(|value| !value.is_null())
            {
                return None;
            }
            Some(Approval {
                id: approval["requestId"].as_i64()?.to_string(),
                title: block["tool"]["title"]
                    .as_str()
                    .or(block["text"].as_str())
                    .unwrap_or("Approval required")
                    .into(),
                detail: block["tool"]["preview"]["title"]
                    .as_str()
                    .or(block["tool"]["detail"].as_str())
                    .unwrap_or("")
                    .into(),
                can_remember: false,
                remember_scope: None,
            })
        })
        .collect()
}
fn reply(question: &Question, answers: Vec<Vec<String>>) -> Value {
    let mut selected = serde_json::Map::new();
    let mut custom = serde_json::Map::new();
    for (item, values) in question.items.iter().zip(answers) {
        let mut ids = vec![];
        for value in values {
            if let Some(option) = item
                .options
                .iter()
                .find(|option| option.id == value)
                .or_else(|| item.options.iter().find(|option| option.label == value))
            {
                ids.push(option.id.clone());
            } else if item.allow_custom {
                custom.insert(item.id.clone(), json!(value));
            }
        }
        if !ids.is_empty() {
            selected.insert(item.id.clone(), json!(ids));
        }
    }
    json!({"kind":"answered","answers":selected,"custom":custom})
}
pub(crate) fn run(
    summary: Session,
    prompt: String,
    settings: Settings,
    controls: std::sync::mpsc::Receiver<Control>,
    store: Database,
    cancel: Arc<AtomicBool>,
    tx: async_channel::Sender<Update>,
) {
    match monocode_core::remote::default_connections() {
        Ok(client) => run_with_client(
            client, summary, prompt, settings, controls, store, cancel, tx,
        ),
        Err(error) => {
            let _ = tx.send_blocking(update(error, true));
        }
    }
}
#[allow(
    clippy::too_many_arguments,
    reason = "Mirrors the turn worker inputs while injecting the remote transport for isolation"
)]
pub(crate) fn run_with_client(
    client: Arc<monocode_core::remote::Connections>,
    summary: Session,
    prompt: String,
    mut settings: Settings,
    controls: std::sync::mpsc::Receiver<Control>,
    store: Database,
    cancel: Arc<AtomicBool>,
    tx: async_channel::Sender<Update>,
) {
    let mut latest_rows = Vec::new();
    let result = (|| -> Result<(), String> {
        if tx.is_closed() {
            return Ok(());
        }
        let operation = operation_lock(&store, summary.id);
        let guard = operation
            .lock()
            .map_err(|_| "Remote session is unavailable")?;
        let mut binding = locked(&store)?
            .remote_session(summary.id)?
            .ok_or("Remote session no longer exists")?;
        let action = settings
            .remote_action
            .take()
            .unwrap_or_else(|| json!({"type":"send"}));
        let attaching = action["type"] == "attach";
        let retrying = action["type"] == "retry";
        if !attaching
            && let Some(recovery) = binding
                .rejected
                .as_ref()
                .filter(|value| value["legacyRecovery"] == true)
        {
            return Err(recovery["error"]
                .as_str()
                .unwrap_or("Older pending requests need recovery in the original desktop app.")
                .into());
        }
        let machine = binding.machine(&client)?;
        if !attaching && !retrying {
            if !binding.outbox.is_empty() {
                return Err("A remote command needs confirmation. Choose Retry before sending another message.".into());
            }
            if settings.operator || settings.orchestrator {
                return Err(
                    "Operator and orchestration are not available on remote projects".into(),
                );
            }
            if settings
                .model
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                let models =
                    monocode_core::catalog::remote_models(&summary.cwd, &summary.provider)?;
                if let Some(model) = models
                    .iter()
                    .find(|model| model.default)
                    .or_else(|| models.first())
                {
                    settings.model = Some(model.id.clone());
                    if settings.effort.is_none() {
                        settings.effort = model.default_effort.clone();
                    }
                    for control in &model.controls {
                        settings
                            .model_settings
                            .entry(control.id.clone())
                            .or_insert_with(|| control.current.clone());
                    }
                }
            }
            if binding.session_id.is_none() {
                let mut create = command("create", None);
                create["projectId"] = json!(binding.project.project_id);
                create["harness"] = json!(summary.provider);
                create["model"] = json!(host_model(&summary.provider, settings.model.as_deref())?);
                create["modelSettings"] = json!(model_settings(&summary.provider, &settings));
                create["runtimeMode"] = json!(settings.access.id());
                if let Some(path) = &settings.worktree_cwd {
                    let remote = monocode_core::remote::parse_remote_path(path)
                        .ok_or("Invalid remote worktree")?;
                    if remote.environment_id != binding.project.environment_id {
                        return Err("Worktree belongs to another machine".into());
                    }
                    create["worktreeCwd"] = json!(remote.host_path);
                }
                binding.outbox.push(create);
            } else {
                let mut configure = command("configure", binding.session_id.as_deref());
                configure["model"] =
                    json!(host_model(&summary.provider, settings.model.as_deref())?);
                configure["modelSettings"] = json!(model_settings(&summary.provider, &settings));
                configure["runtimeMode"] = json!(settings.access.id());
                binding.outbox.push(configure);
            }
            let operation = action["type"].as_str().unwrap_or("send");
            let operation = if prompt.trim() == "/compact" {
                "compact"
            } else {
                operation
            };
            let mut send = command(operation, binding.session_id.as_deref());
            if matches!(operation, "send" | "draft") {
                send["text"] = json!(prompt);
                let (references, uploads) = prepare_for_session(
                    &client,
                    &machine.id,
                    &binding.project.environment_id,
                    &settings.attachments,
                    send["commandId"]
                        .as_str()
                        .ok_or("Remote command identity is missing")?,
                )?;
                send["attachments"] = json!(references);
                binding.uploads.extend(uploads);
                if settings.plan {
                    send["intent"] = json!("plan");
                }
                for key in ["draftBlockId", "planBlockId", "intent"] {
                    if let Some(value) = action.get(key) {
                        send[key] = value.clone();
                    }
                }
            } else if operation == "removeDraft" {
                send["draftBlockId"] = action["draftBlockId"].clone();
            }
            binding.outbox.push(send);
            binding.rejected = None;
            binding.summary.has_messages = true;
            binding.summary.updated_at = monocode_core::time::now_ms();
            journal(&store, &binding)?;
            let descriptor = client.request(&machine.id, "environment.describe", json!({}))?;
            if !descriptor["providers"].as_array().is_some_and(|providers| {
                providers
                    .iter()
                    .any(|provider| provider.as_str() == Some(&summary.provider))
            }) {
                return Err(format!(
                    "{} is not installed on this machine",
                    summary.provider
                ));
            }
        }
        if !attaching {
            upload_pending(&client, &machine.id, &store, &mut binding)?;
            dispatch_outbox(&client, &machine.id, &store, &mut binding)?;
        }
        drop(guard);
        let id = binding
            .session_id
            .clone()
            .ok_or("No remote session has been started")?;
        let mut known = None;
        let mut seen_approvals = HashSet::new();
        let mut seen_questions = HashSet::new();
        let mut last_revision = None;
        let mut control_commands = Vec::<Value>::new();
        let mut stopping_run = None::<String>;
        loop {
            // Cancel is explicit Stop. Dropping the desktop receiver simply
            // ends observation, leaving the host process and run untouched.
            if tx.is_closed() {
                return Ok(());
            }
            let snapshot = client.load_session(&machine.id, &id, known.as_ref())?;
            let revision = snapshot["revision"].as_u64();
            let running = snapshot["status"] == "running";
            let current_approvals = approvals(&snapshot);
            let current_question = question(&snapshot);
            let mut next = update(if running { "Working…" } else { "Ready" }, !running);
            next.start = latest_rows.len();
            if revision != last_revision {
                let mut projected = snapshot.clone();
                monocode_core::remote::attachments::qualify_snapshot(
                    &mut projected,
                    &binding.project.environment_id,
                    &id,
                );
                let (history, _) = monocode_core::store::Store::project_remote_history(&projected)?;
                let prefix = latest_rows
                    .iter()
                    .zip(&history.rows)
                    .take_while(|(known, next)| known == next)
                    .count();
                next.start = prefix;
                next.rows = history.rows[prefix..].to_vec();
                latest_rows = history.rows;
            }
            next.provider_id = snapshot["session"]["providerSessionId"]
                .as_str()
                .map(str::to_owned);
            next.model = snapshot["session"]["model"].as_str().map(str::to_owned);
            (next.context, next.metrics) = accounting(&snapshot);
            let current_ids: HashSet<_> = current_approvals.iter().map(|a| a.id.clone()).collect();
            next.approvals_resolved = seen_approvals.difference(&current_ids).cloned().collect();
            next.approvals = current_approvals
                .into_iter()
                .filter(|a| !seen_approvals.contains(&a.id))
                .collect();
            seen_approvals = current_ids;
            let question_ids: HashSet<_> = current_question.iter().map(|q| q.id.clone()).collect();
            next.questions_resolved = seen_questions.difference(&question_ids).cloned().collect();
            next.questions = current_question
                .iter()
                .filter(|q| !seen_questions.contains(&q.id))
                .cloned()
                .collect();
            seen_questions = question_ids;
            // Keep the selected desktop handle when creation's receipt binds a
            // new host session. Metadata refreshes find this binding by host ID.
            binding.running = running;
            binding.needs_input = !seen_approvals.is_empty() || !seen_questions.is_empty();
            binding.summary.title = snapshot["session"]["title"]
                .as_str()
                .unwrap_or(&binding.summary.title)
                .into();
            binding.summary.provider_id = next.provider_id.clone();
            binding.summary.updated_at = snapshot["updatedAt"]
                .as_u64()
                .unwrap_or(binding.summary.updated_at);
            journal(&store, &binding)?;
            tx.send_blocking(next).map_err(|_| "Window closed")?;
            last_revision = revision;
            if !running {
                break;
            }
            if cancel.load(Ordering::Relaxed) && stopping_run.is_none() {
                let run_id = snapshot["runId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or("Running host session has no run identity")?;
                let mut stop = command("cancel", Some(&id));
                stop["runId"] = json!(run_id);
                control_commands.push(stop);
                stopping_run = Some(run_id.into());
            }
            while let Ok(control) = controls.try_recv() {
                let mut next = command("approve", Some(&id));
                next["runId"] = snapshot["runId"].clone();
                next["requestId"] = json!(
                    control
                        .id
                        .parse::<i64>()
                        .map_err(|_| "Invalid host request ID")?
                );
                match control.decision {
                    Decision::Allow | Decision::Session => next["decision"] = json!("allow"),
                    Decision::Deny
                        if current_question
                            .as_ref()
                            .is_some_and(|q| q.id == control.id) =>
                    {
                        next["type"] = json!("answer");
                        next["reply"] = json!({"kind":"skipped"});
                    }
                    Decision::Deny => next["decision"] = json!("deny"),
                    Decision::Answer(answers) => {
                        let question = current_question
                            .as_ref()
                            .filter(|q| q.id == control.id)
                            .ok_or("Host question is no longer pending")?;
                        next["type"] = json!("answer");
                        next["reply"] = reply(question, answers);
                    }
                    Decision::KeepOpen => continue,
                    Decision::Always => {
                        return Err("This remote host cannot remember permissions".into());
                    }
                    Decision::Steer(_) => {
                        return Err("Steering is not supported by this remote host".into());
                    }
                }
                control_commands.push(next);
            }
            for command in control_commands.drain(..) {
                binding.outbox.push(command);
                journal(&store, &binding)?;
                dispatch_outbox(&client, &machine.id, &store, &mut binding)?;
            }
            known = Some(snapshot);
            for _ in 0..15 {
                if tx.is_closed() {
                    return Ok(());
                }
                if cancel.load(Ordering::Relaxed) && stopping_run.is_none() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut next = update(error, true);
        // Every valid snapshot was already published. An empty error patch
        // preserves any optimistic prompt awaiting a host receipt.
        next.start = latest_rows.len();
        let _ = tx.send_blocking(next);
    }
}

#[cfg(test)]
mod tests;
