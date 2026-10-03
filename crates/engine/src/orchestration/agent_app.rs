//! Port of src/features/agent-app/model/agentApp.ts: the `/operator` app API
//! (`models.list`, `sessions.*`, `worktrees.*`, `folders.*`, `notes.*`) that
//! the `app` CLI reaches through the control server.

use std::sync::LazyLock;

use crate::projects::backend::{Worktree, Worktrees};
use gpui::{App, AsyncApp, Task};
use monocode_core::models::model_effort_setting;
use monocode_core::paths::path_key;
use monocode_core::session::WorkspaceMode;
use monocode_core::{
    HARNESSES, HarnessId, ModelCatalog, ModelSettings, RUNTIME_MODES, RuntimeMode, Session, js,
};
use monocode_layout::SplitDir;
use monocode_settings::Kv;
use monocode_store::notes::{Note, NoteUpsert};
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::plan::js_integer;
use super::session_conversation::{SessionReadOptions, session_conversation_page};
use crate::history::notes::{normalize_note_tags, note_title};
use crate::history::session_folders::{
    SessionFolderTarget, load_session_folders, place_session_in_folder, save_session_folders,
};
use crate::projects::recents::looks_like_project;
use crate::submit::operator_command::consume_operator_command;

/// `AppSessionListing`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionListing {
    pub id: String,
    pub title: String,
    pub harness: HarnessId,
    pub model: String,
    pub busy: bool,
    pub has_draft: bool,
}

/// `AppSessionPlacement`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionPlacement {
    pub direction: SplitDir,
    pub beside_session_id: String,
}

/// The `QuickLaunch` an app call builds: a session the window starts.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLaunch {
    pub cwd: String,
    pub prompt: String,
    /// Create an unsent user draft instead of starting an agent turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    pub harness: HarnessId,
    pub model: String,
    pub model_settings: ModelSettings,
    pub runtime_mode: RuntimeMode,
    pub reveal: bool,
    pub workspace_mode: WorkspaceMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<String>,
}

/// `host.send`'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendResult {
    pub already_submitted: bool,
}

/// `host.draft`'s answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftResult {
    pub already_saved: bool,
    pub draft: bool,
}

/// `AgentAppHost`, plus the module state agentApp.ts read directly: the
/// model catalog, harness availability, the preferred model, and the settings
/// store that holds session folders.
pub trait AgentAppHost {
    fn start(
        &self,
        launch: AppLaunch,
        id: &str,
        placement: Option<AppSessionPlacement>,
        cx: &mut App,
    ) -> Task<Result<(), String>>;
    fn sessions(&self, cwd: &str, cx: &mut App) -> Task<Result<Vec<AppSessionListing>, String>>;
    fn session(&self, id: &str, cx: &mut App) -> Task<Result<Option<Session>, String>>;
    fn send(
        &self,
        id: &str,
        prompt: &str,
        request_id: &str,
        cx: &mut App,
    ) -> Task<Result<SendResult, String>>;
    fn draft(
        &self,
        id: &str,
        prompt: &str,
        request_id: &str,
        cx: &mut App,
    ) -> Task<Result<DraftResult, String>>;
    fn worktrees(&self, cwd: &str, cx: &mut App) -> Task<Result<Worktrees, String>>;
    fn create_worktree(
        &self,
        cwd: &str,
        branch: &str,
        base: &str,
        existing: bool,
        cx: &mut App,
    ) -> Task<Result<Worktree, String>>;
    fn notes(&self, cx: &mut App) -> Task<Result<Vec<Note>, String>>;
    fn note(&self, id: &str, cx: &mut App) -> Task<Result<Option<Note>, String>>;
    fn save_note(&self, note: NoteUpsert, cx: &mut App) -> Task<Result<Note, String>>;
    /// `modelsFor`, `resolveModel`, and `mergeModelSettings` read this.
    fn catalog(&self, cx: &App) -> ModelCatalog;
    /// `isHarnessAvailable`.
    fn is_harness_available(&self, harness: HarnessId, cx: &App) -> bool;
    /// `preferredModelId`.
    fn preferred_model_id(&self, harness: HarnessId, cx: &App) -> String;
    /// The settings store behind `loadSessionFolders` and `saveSessionFolders`.
    fn kv(&self) -> Kv;
}

const FIELDS: [(&str, &[&str]); 13] = [
    ("models.list", &[]),
    ("sessions.list", &[]),
    (
        "sessions.read",
        &["sessionId", "before", "limit", "maxChars"],
    ),
    ("sessions.send", &["sessionId", "prompt"]),
    ("sessions.draft", &["sessionId", "prompt"]),
    (
        "sessions.start",
        &[
            "prompt",
            "draft",
            "harness",
            "model",
            "modelSettings",
            "effort",
            "runtimeMode",
            "reveal",
            "workspaceMode",
            "worktreeBase",
            "worktreeCwd",
            "placement",
            "besideSessionId",
        ],
    ),
    ("worktrees.list", &[]),
    ("worktrees.create", &["branch", "base", "existing"]),
    ("folders.list", &[]),
    ("folders.move", &["sessionId", "folderId", "newFolderName"]),
    ("notes.list", &["limit", "offset"]),
    ("notes.read", &["id"]),
    ("notes.write", &["id", "title", "body", "tags"]),
];

fn fields(action: &str, input: &Map<String, Value>) -> Result<(), String> {
    let Some((_, allowed)) = FIELDS.iter().find(|(name, _)| *name == action) else {
        return Err(format!("Unknown app action: {action}"));
    };
    let unknown: Vec<&str> = input
        .keys()
        .map(String::as_str)
        .filter(|key| !allowed.contains(key))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!("Unknown {action} fields: {}", unknown.join(", ")))
    }
}

/// `requiredString`.
fn required_string(value: Option<&Value>, name: &str, max: usize) -> Result<String, String> {
    match value.and_then(Value::as_str) {
        Some(text) if !js::trim(text).is_empty() && js::len(text) <= max => {
            Ok(js::trim(text).to_string())
        }
        _ => Err(format!(
            "{name} must be a non-empty string under {max} characters"
        )),
    }
}

/// `optionalString`: only a missing field is optional; `null` is invalid.
fn optional_string(
    value: Option<&Value>,
    name: &str,
    max: usize,
) -> Result<Option<String>, String> {
    match value {
        None => Ok(None),
        Some(value) => required_string(Some(value), name, max).map(Some),
    }
}

/// `value ?? fallback`.
fn or_default<'a>(value: Option<&'a Value>, fallback: &'a Value) -> &'a Value {
    match value {
        None | Some(Value::Null) => fallback,
        Some(value) => value,
    }
}

/// `agentPrompt`: an app call never enables `/operator` in another session.
fn agent_prompt(value: Option<&Value>) -> Result<String, String> {
    let prompt = required_string(value, "prompt", 240_000)?;
    if consume_operator_command(&prompt).matched {
        return Err("App calls cannot enable /operator in another session".into());
    }
    Ok(prompt)
}

/// `noteBody`.
fn note_body(value: &Value) -> Result<String, String> {
    match value.as_str() {
        Some(body) if js::len(body) <= 240_000 => {
            Ok(body.replace("\r\n", "\n").replace('\r', "\n"))
        }
        _ => Err("body must be a string under 240000 characters".into()),
    }
}

/// `noteTags`.
fn note_tags(value: &Value) -> Result<Vec<String>, String> {
    let error =
        || "tags must be an array of at most 20 strings under 48 characters each".to_string();
    let items = value
        .as_array()
        .filter(|items| items.len() <= 20)
        .ok_or_else(error)?;
    let mut tags = Vec::new();
    for item in items {
        match item.as_str() {
            Some(tag) if js::len(tag) <= 48 => tags.push(tag.to_string()),
            _ => return Err(error()),
        }
    }
    Ok(normalize_note_tags(&tags))
}

/// `requireProject`.
fn require_project(source: &Session) -> Result<String, String> {
    if !looks_like_project(&source.cwd) {
        return Err("Choose a project folder in this session first".into());
    }
    Ok(source.cwd.clone())
}

static REQUEST_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]{1,128}$").expect("request id pattern"));
static NOTE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]+$").expect("note id pattern"));
static PARAGRAPH_BREAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n\s*\n").expect("paragraph pattern"));

/// `notePreview`: two short body paragraphs, with a hard cap independent of
/// Markdown length.
pub fn note_preview(body: &str) -> String {
    let paragraphs: Vec<String> = PARAGRAPH_BREAK
        .split(js::trim(body))
        .map(|paragraph| {
            paragraph
                .split(js::is_space)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|paragraph| !paragraph.is_empty())
        .take(2)
        .collect();
    js::slice_prefix(&paragraphs.join("\n\n"), 400).to_string()
}

async fn project_session(
    source: &Session,
    id: &str,
    host: &dyn AgentAppHost,
    cx: &mut AsyncApp,
) -> Result<Session, String> {
    let cwd = require_project(source)?;
    let listed = cx.update(|cx| host.sessions(&cwd, cx)).await?;
    if !listed.iter().any(|session| session.id == id) {
        return Err("Session was not found in this project".into());
    }
    cx.update(|cx| host.session(id, cx))
        .await?
        .ok_or_else(|| "Session was not found in this project".into())
}

/// `startLaunch`: the session `sessions.start` asks the window for.
fn start_launch(
    source: &Session,
    input: &Map<String, Value>,
    host: &dyn AgentAppHost,
    cx: &App,
) -> Result<AppLaunch, String> {
    let cwd = require_project(source)?;
    let prompt = agent_prompt(input.get("prompt"))?;
    let draft = or_default(input.get("draft"), &Value::Bool(false))
        .as_bool()
        .ok_or_else(|| "draft must be a boolean".to_string())?;
    let source_harness = json!(source.harness);
    let harness = or_default(input.get("harness"), &source_harness)
        .as_str()
        .and_then(HarnessId::parse)
        .filter(|id| HARNESSES.contains(id))
        .ok_or_else(|| "Unknown harness; run models.list for available providers".to_string())?;
    if !host.is_harness_available(harness, cx) {
        return Err(format!("{harness} is not available in MonoCode"));
    }
    let catalog = host.catalog(cx);
    let requested_model = optional_string(input.get("model"), "model", 512)?;
    let model = match &requested_model {
        Some(requested) => catalog
            .models_for(harness)
            .iter()
            .find(|entry| &entry.id == requested)
            .cloned(),
        None => {
            let id = if harness == source.harness {
                source.model.clone()
            } else {
                host.preferred_model_id(harness, cx)
            };
            Some(catalog.resolve_model(harness, Some(&id)))
        }
    };
    let Some(model) = model.filter(|model| model.harness == harness) else {
        return Err("Unknown model; run models.list for exact model IDs".into());
    };
    let mut requested_settings: Map<String, Value> = match input.get("modelSettings") {
        None => Map::new(),
        Some(Value::Object(settings)) => settings.clone(),
        Some(_) => {
            return Err("modelSettings must be an object of setting IDs and values".into());
        }
    };
    if let Some(effort) = input.get("effort") {
        let Some(setting) = model_effort_setting(&model) else {
            return Err(format!("{} does not expose an effort setting", model.id));
        };
        requested_settings.insert(
            setting.id.clone(),
            Value::String(required_string(Some(effort), "effort", 128)?),
        );
    }
    let mut requested = ModelSettings::new();
    for (key, value) in &requested_settings {
        let valid = model
            .settings
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|entry| &entry.id == key)
            .zip(value.as_str())
            .is_some_and(|(setting, value)| {
                setting.options.iter().any(|option| option.value == value)
            });
        if !valid {
            return Err(format!(
                "Invalid model setting {key}; run models.list for allowed values"
            ));
        }
        requested.insert(key.clone(), value.as_str().unwrap_or_default().to_string());
    }
    let source_mode = json!(source.runtime_mode);
    let runtime_mode = or_default(input.get("runtimeMode"), &source_mode)
        .as_str()
        .and_then(RuntimeMode::parse)
        .ok_or_else(|| {
            format!(
                "runtimeMode must be one of: {}",
                RUNTIME_MODES.map(RuntimeMode::as_str).join(", ")
            )
        })?;
    let reveal = or_default(input.get("reveal"), &Value::Bool(false))
        .as_bool()
        .ok_or_else(|| "reveal must be a boolean".to_string())?;
    let workspace_mode = match or_default(input.get("workspaceMode"), &json!("current")).as_str() {
        Some("current") => WorkspaceMode::Current,
        Some("worktree") => WorkspaceMode::Worktree,
        _ => return Err("workspaceMode must be current or worktree".into()),
    };
    let worktree_base = optional_string(input.get("worktreeBase"), "worktreeBase", 512)?;
    if worktree_base.is_some() && workspace_mode != WorkspaceMode::Worktree {
        return Err("worktreeBase requires workspaceMode worktree".into());
    }
    let worktree_cwd = optional_string(input.get("worktreeCwd"), "worktreeCwd", 512)?;
    if worktree_cwd.is_some() && workspace_mode != WorkspaceMode::Current {
        return Err("worktreeCwd requires workspaceMode current".into());
    }
    let mut settings = if harness == source.harness && model.id == source.model {
        source.model_settings.clone()
    } else {
        ModelSettings::new()
    };
    settings.extend(requested);
    let model_settings = catalog.merge_model_settings(&model, Some(&settings));
    let worktree_cwd = if workspace_mode == WorkspaceMode::Current {
        worktree_cwd
            .filter(|cwd| !cwd.is_empty())
            .or_else(|| source.worktree_cwd.clone().filter(|cwd| !cwd.is_empty()))
    } else {
        None
    };
    Ok(AppLaunch {
        cwd,
        prompt,
        draft: draft.then_some(true),
        harness,
        model: model.id.clone(),
        model_settings,
        runtime_mode,
        reveal,
        workspace_mode,
        worktree_cwd,
        worktree_base,
    })
}

fn json_value(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// `handleAgentApp`: run one app call for the session that made it.
pub async fn handle_agent_app(
    source: &Session,
    request_id: &str,
    action: &str,
    input: &Map<String, Value>,
    host: &dyn AgentAppHost,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    fields(action, input)?;
    match action {
        "models.list" => Ok(cx.update(|cx| {
            let catalog = host.catalog(cx);
            json!({
                "runtimeModes": RUNTIME_MODES
                    .iter()
                    .map(|mode| json!({ "id": mode, "label": mode.label(), "description": mode.hint() }))
                    .collect::<Vec<_>>(),
                "harnesses": HARNESSES
                    .iter()
                    .map(|harness| json!({
                        "id": harness,
                        "available": host.is_harness_available(*harness, cx),
                        "models": catalog
                            .models_for(*harness)
                            .iter()
                            .map(|model| json!({
                                "id": model.id,
                                "name": model.name,
                                "settings": model.settings.clone().unwrap_or_default(),
                            }))
                            .collect::<Vec<_>>(),
                    }))
                    .collect::<Vec<_>>(),
            })
        })),
        "sessions.list" => {
            let cwd = require_project(source)?;
            let sessions = cx.update(|cx| host.sessions(&source.cwd, cx)).await?;
            Ok(json!({ "cwd": cwd, "sessions": sessions }))
        }
        "sessions.read" => {
            let id = required_string(input.get("sessionId"), "sessionId", 256)?;
            let target = project_session(source, &id, host, cx).await?;
            let before = optional_string(input.get("before"), "before", 256)?;
            let page = session_conversation_page(
                &target,
                SessionReadOptions {
                    before: before.as_deref(),
                    limit: input.get("limit"),
                    max_chars: input.get("maxChars"),
                },
            )?;
            Ok(json_value(page))
        }
        "sessions.send" | "sessions.draft" => {
            let id = required_string(input.get("sessionId"), "sessionId", 256)?;
            let prompt = agent_prompt(input.get("prompt"))?;
            let send = action == "sessions.send";
            if id == source.id {
                return Err(if send {
                    "Use the current conversation to continue this session".into()
                } else {
                    "Use the composer to save a draft in this session".into()
                });
            }
            if !REQUEST_ID.is_match(request_id) {
                return Err("Invalid request ID".into());
            }
            project_session(source, &id, host, cx).await?;
            let key = format!("app-{}-{request_id}", source.id);
            if send {
                let result = cx.update(|cx| host.send(&id, &prompt, &key, cx)).await?;
                Ok(json!({
                    "sessionId": id,
                    "submitted": true,
                    "alreadySubmitted": result.already_submitted,
                }))
            } else {
                let result = cx.update(|cx| host.draft(&id, &prompt, &key, cx)).await?;
                Ok(json!({
                    "sessionId": id,
                    "saved": true,
                    "alreadySaved": result.already_saved,
                    "draft": result.draft,
                }))
            }
        }
        "sessions.start" => {
            if !REQUEST_ID.is_match(request_id) {
                return Err(
                    "request ID must use letters, digits, underscores or hyphens".into(),
                );
            }
            let mut launch = cx.update(|cx| start_launch(source, input, host, cx))?;
            if input.get("worktreeCwd").is_some() {
                let listed = cx.update(|cx| host.worktrees(&launch.cwd, cx)).await?;
                let wanted = launch.worktree_cwd.clone().unwrap_or_default();
                let Some(chosen) = listed
                    .worktrees
                    .iter()
                    .find(|tree| !tree.missing && path_key(&tree.path) == path_key(&wanted))
                else {
                    return Err(
                        "Worktree is unavailable in this project; run worktrees.list".into(),
                    );
                };
                launch.worktree_cwd = if path_key(&chosen.path) == path_key(&launch.cwd) {
                    None
                } else {
                    Some(chosen.path.clone())
                };
            }
            let direction = match or_default(input.get("placement"), &json!("tab")).as_str() {
                Some("tab") => None,
                Some("right") => Some(SplitDir::Right),
                Some("down") => Some(SplitDir::Down),
                _ => return Err("placement must be tab, right or down".into()),
            };
            if input.get("besideSessionId").is_some() && direction.is_none() {
                return Err("besideSessionId requires placement right or down".into());
            }
            let placement = match direction {
                Some(direction) => Some(AppSessionPlacement {
                    direction,
                    beside_session_id: optional_string(
                        input.get("besideSessionId"),
                        "besideSessionId",
                        256,
                    )?
                    .unwrap_or_else(|| source.id.clone()),
                }),
                None => None,
            };
            let id = format!("app-{}-{request_id}", source.id);
            let draft = launch.draft == Some(true);
            let result = json!({
                "id": id,
                "cwd": launch.cwd,
                "harness": launch.harness,
                "model": launch.model,
                "submitted": !draft,
                "draft": draft,
            });
            cx.update(|cx| host.start(launch, &id, placement, cx)).await?;
            Ok(result)
        }
        "worktrees.list" => {
            let cwd = require_project(source)?;
            Ok(json_value(cx.update(|cx| host.worktrees(&cwd, cx)).await?))
        }
        "worktrees.create" => {
            let cwd = require_project(source)?;
            let branch = required_string(input.get("branch"), "branch", 400)?;
            let existing = or_default(input.get("existing"), &Value::Bool(false))
                .as_bool()
                .ok_or_else(|| "existing must be a boolean".to_string())?;
            let base = optional_string(input.get("base"), "base", 400)?;
            if existing && base.is_some() {
                return Err("base cannot be set for an existing branch".into());
            }
            let base = base.unwrap_or_else(|| "HEAD".into());
            Ok(json_value(
                cx.update(|cx| host.create_worktree(&cwd, &branch, &base, existing, cx))
                    .await?,
            ))
        }
        "folders.list" => {
            let cwd = require_project(source)?;
            let folders: Vec<Value> = load_session_folders(&host.kv(), &cwd)
                .into_iter()
                .map(|folder| {
                    json!({ "id": folder.id, "name": folder.name, "sessionIds": folder.session_ids })
                })
                .collect();
            Ok(json!({ "cwd": cwd, "folders": folders }))
        }
        "folders.move" => {
            let cwd = require_project(source)?;
            let session_id = required_string(input.get("sessionId"), "sessionId", 256)?;
            let folder_id = optional_string(input.get("folderId"), "folderId", 256)?;
            let new_folder_name =
                optional_string(input.get("newFolderName"), "newFolderName", 100)?;
            if folder_id.is_some() == new_folder_name.is_some() {
                return Err("Supply exactly one of folderId or newFolderName".into());
            }
            let listed = cx.update(|cx| host.sessions(&cwd, cx)).await?;
            if !listed.iter().any(|session| session.id == session_id) {
                return Err("Session was not found in this project".into());
            }
            let kv = host.kv();
            let folders = load_session_folders(&kv, &cwd);
            if let Some(folder_id) = &folder_id
                && !folders.iter().any(|folder| &folder.id == folder_id)
            {
                return Err("Folder was not found in this project".into());
            }
            let target = match (folder_id, new_folder_name) {
                (Some(folder_id), _) => SessionFolderTarget::Existing { folder_id },
                (None, Some(name)) => SessionFolderTarget::New { name },
                (None, None) => unreachable!("checked above"),
            };
            let next = place_session_in_folder(&folders, &session_id, &target);
            save_session_folders(&kv, &cwd, &next);
            let folder = next
                .iter()
                .find(|folder| folder.session_ids.contains(&session_id));
            let mut result = json!({ "sessionId": session_id });
            if let Some(folder) = folder {
                result["folderId"] = json!(folder.id);
                result["folderName"] = json!(folder.name);
            }
            Ok(result)
        }
        "notes.list" => {
            let limit = js_integer(Some(or_default(input.get("limit"), &json!(30))))
                .filter(|limit| (1..=100).contains(limit))
                .ok_or_else(|| "limit must be an integer from 1 to 100".to_string())?;
            let offset = js_integer(Some(or_default(input.get("offset"), &json!(0))))
                .filter(|offset| *offset >= 0)
                .ok_or_else(|| "offset must be a non-negative integer".to_string())?;
            let notes = cx.update(|cx| host.notes(cx)).await?;
            let listed: Vec<Value> = notes
                .iter()
                .skip(offset as usize)
                .take(limit as usize)
                .map(|note| {
                    let mut entry = json!({
                        "id": note.id,
                        "title": note.title,
                        "preview": note_preview(&note.body),
                        "tags": note.tags,
                    });
                    if let Some(cwd) = &note.source_cwd {
                        entry["sourceCwd"] = json!(cwd);
                    }
                    entry
                })
                .collect();
            Ok(json!({ "total": notes.len(), "offset": offset, "notes": listed }))
        }
        "notes.read" => {
            let id = required_string(input.get("id"), "id", 256)?;
            let note = cx.update(|cx| host.note(&id, cx)).await?;
            note.map(json_value).ok_or_else(|| "Note was not found".into())
        }
        "notes.write" => {
            let id = optional_string(input.get("id"), "id", 256)?;
            if id.as_deref().is_some_and(|id| !NOTE_ID.is_match(id)) {
                return Err("Invalid note ID".into());
            }
            let title = match input.get("title") {
                None => None,
                Some(value) => Some(required_string(Some(value), "title", 200)?),
            };
            let body = input.get("body").map(note_body).transpose()?;
            let tags = input.get("tags").map(note_tags).transpose()?;
            if let Some(id) = id {
                if title.is_none() && body.is_none() && tags.is_none() {
                    return Err("Supply title, body or tags to update a note".into());
                }
                let Some(current) = cx.update(|cx| host.note(&id, cx)).await? else {
                    return Err("Note was not found".into());
                };
                let upsert = NoteUpsert {
                    id,
                    title: title.unwrap_or(current.title),
                    body: body.unwrap_or(current.body),
                    tags: tags.unwrap_or(current.tags),
                    source_session_id: None,
                    source_cwd: None,
                };
                return Ok(json_value(cx.update(|cx| host.save_note(upsert, cx)).await?));
            }
            let Some(body) = body else {
                return Err("body is required to create a note".into());
            };
            if !REQUEST_ID.is_match(request_id) {
                return Err("Invalid request ID".into());
            }
            let created_id = format!("app-{}-{request_id}", source.id);
            let title = title.unwrap_or_else(|| note_title(&body));
            let tags = tags.unwrap_or_default();
            if let Some(existing) = cx.update(|cx| host.note(&created_id, cx)).await? {
                if existing.title != title || existing.body != body || existing.tags != tags {
                    return Err("Request ID was already used for another note".into());
                }
                return Ok(json_value(existing));
            }
            let upsert = NoteUpsert {
                id: created_id,
                title,
                body,
                tags,
                source_session_id: Some(source.id.clone()),
                source_cwd: looks_like_project(&source.cwd).then(|| source.cwd.clone()),
            };
            Ok(json_value(cx.update(|cx| host.save_note(upsert, cx)).await?))
        }
        _ => Err(format!("Unknown app action: {action}")),
    }
}
