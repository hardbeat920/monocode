//! Port of src/features/quick-composer/model: quickComposer.ts (the launch
//! shape and its parser, project and model lists, the catalog the panel
//! borrows), quickAttachments.ts, quickWorkspace.ts, and the quickGitPopup.ts
//! message types. The shortcut helpers (quickComposerShortcut.ts) live in
//! `monocode_core::shortcut`.

use std::future::Future;

use monocode_core::attachment::{MAX_ATTACHMENTS, persistable_attachment};
use monocode_core::block::ModelSettings;
use monocode_core::harness::harness_supports_attachments;
use monocode_core::js;
use monocode_core::models::{AgentModel, LastModelChoice, ModelCatalog, ModelEnv};
use monocode_core::paths::path_key;
use monocode_core::session::WorkspaceMode;
use monocode_core::{
    Attachment, AttachmentKind, HARNESSES, HarnessId, Platform, RuntimeMode, Session,
};
use monocode_layout::paths::{pretty_cwd, project_name};
use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::projects::backend::{Worktree, Worktrees};
use crate::projects::recents::{
    load_archived_projects, load_pinned_projects, load_project_rail_order, load_recents,
    looks_like_project,
};
use crate::runtime::util::project_path::normalize_project_path;
use crate::submit::attachments::AttachmentIo;
use crate::submit::paths::parent_path;

/// `QUICK_COMPOSER_LAUNCH_EVENT`: a window has a launch to take.
pub const QUICK_COMPOSER_LAUNCH_EVENT: &str = "quick_composer_launch";
/// `QUICK_COMPOSER_SHOWN_EVENT`: the panel came up.
pub const QUICK_COMPOSER_SHOWN_EVENT: &str = "quick_composer_shown";
/// `QUICK_COMPOSER_CATALOG_REQUEST_EVENT`.
pub const QUICK_COMPOSER_CATALOG_REQUEST_EVENT: &str = "quick_composer_catalog_request";
/// `QUICK_COMPOSER_CATALOG_EVENT`.
pub const QUICK_COMPOSER_CATALOG_EVENT: &str = "quick_composer_catalog";

/// `LAST_PROJECT_KEY`.
pub const LAST_PROJECT_KEY: &str = "monocode.quickComposerProject";

/// `intent`: the turn mode a leading composer command picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuickIntent {
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "orchestrate")]
    Orchestrate,
}

/// `QuickLaunch`: a session the floating composer hands to a window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickLaunchRequest {
    pub prompt: String,
    /// Create an unsent user draft instead of starting an agent turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<QuickIntent>,
    pub cwd: String,
    pub harness: HarnessId,
    /// Missing means the harness default, resolved by the workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_settings: Option<ModelSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<RuntimeMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_mode: Option<WorkspaceMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_cwd: Option<String>,
    /// Bring the new session forward instead of starting it quietly.
    pub reveal: bool,
}

impl QuickLaunchRequest {
    /// A launch with only the required fields.
    pub fn new(prompt: &str, cwd: &str, harness: HarnessId, reveal: bool) -> Self {
        Self {
            prompt: prompt.to_string(),
            draft: None,
            intent: None,
            cwd: cwd.to_string(),
            harness,
            model: None,
            model_settings: None,
            runtime_mode: None,
            attachments: None,
            workspace_mode: None,
            worktree_base: None,
            worktree_cwd: None,
            reveal,
        }
    }

    /// `launch.draft` is truthy.
    pub fn is_draft(&self) -> bool {
        self.draft == Some(true)
    }
}

/// `quickComposerSupported`: the panel exists on macOS only; elsewhere the
/// shortcut is never claimed.
pub fn quick_composer_supported(platform: Platform) -> bool {
    platform == Platform::Mac
}

/// `isHarnessId`.
pub fn is_harness_id(value: &Value) -> Option<HarnessId> {
    value.as_str().and_then(HarnessId::parse)
}

/// `parseQuickLaunch`: the launch as a workspace window receives it.
/// Anything malformed is dropped.
///
/// TODO(port): the TypeScript parser drops `draft` and `intent`, so a draft
/// or `/plan` launch from the panel arrives as a plain turn. Kept as written.
pub fn parse_quick_launch(value: &Value) -> Option<QuickLaunchRequest> {
    let raw = value.as_object()?;
    let attachments = parse_quick_attachments(raw.get("attachments"))?;
    let prompt = raw.get("prompt")?.as_str()?;
    if js::trim(prompt).is_empty() && attachments.is_empty() {
        return None;
    }
    let cwd = raw.get("cwd")?.as_str()?;
    if js::trim(cwd).is_empty() {
        return None;
    }
    let harness = is_harness_id(raw.get("harness")?)?;
    if !attachments.is_empty() && !harness_supports_attachments(harness) {
        return None;
    }
    let workspace_mode = match raw.get("workspaceMode") {
        None => None,
        Some(Value::String(mode)) if mode == "current" => Some(WorkspaceMode::Current),
        Some(Value::String(mode)) if mode == "worktree" => Some(WorkspaceMode::Worktree),
        Some(_) => return None,
    };
    let non_blank = |key: &str| -> Option<Option<String>> {
        match raw.get(key) {
            None => Some(None),
            Some(Value::String(value)) if !js::trim(value).is_empty() => Some(Some(value.clone())),
            Some(_) => None,
        }
    };
    let worktree_base = non_blank("worktreeBase")?;
    let worktree_cwd = non_blank("worktreeCwd")?;
    if workspace_mode == Some(WorkspaceMode::Worktree) && worktree_cwd.is_some() {
        return None;
    }
    if worktree_base.is_some() && workspace_mode != Some(WorkspaceMode::Worktree) {
        return None;
    }
    let model = raw
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    let model_settings = raw
        .get("modelSettings")
        .and_then(Value::as_object)
        .map(|settings| {
            settings
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect::<ModelSettings>()
        });
    let runtime_mode = raw
        .get("runtimeMode")
        .and_then(Value::as_str)
        .and_then(RuntimeMode::parse);
    Some(QuickLaunchRequest {
        prompt: prompt.to_string(),
        draft: None,
        intent: None,
        cwd: cwd.to_string(),
        harness,
        model,
        model_settings,
        runtime_mode,
        attachments: (!attachments.is_empty()).then_some(attachments),
        workspace_mode,
        worktree_base,
        worktree_cwd,
        reveal: raw.get("reveal") == Some(&Value::Bool(true)),
    })
}

/// `QuickCatalog`: live model lists by harness, as a workspace knows them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCatalog {
    pub models: Map<String, Value>,
    pub available_harnesses: Vec<HarnessId>,
}

/// `liveQuickCatalog`: only live catalogs travel, because both sides bundle
/// the fallback lists. One process holds every window here, so the panel
/// can read the catalog directly; this keeps the TypeScript message.
pub fn live_quick_catalog(
    catalog: &ModelCatalog,
    is_available: impl Fn(HarnessId) -> bool,
) -> QuickCatalog {
    let mut models = Map::new();
    for harness in HARNESSES {
        if catalog.has_live_catalog(harness) {
            models.insert(
                harness.as_str().to_string(),
                serde_json::to_value(catalog.models_for(harness)).unwrap_or(Value::Null),
            );
        }
    }
    QuickCatalog {
        models,
        available_harnesses: HARNESSES
            .into_iter()
            .filter(|harness| is_available(*harness))
            .collect(),
    }
}

/// `applyQuickCatalog`: take the live lists a window sent and return the
/// harnesses it reported available.
pub fn apply_quick_catalog(catalog: &mut ModelCatalog, value: &Value) -> Option<Vec<HarnessId>> {
    let raw = value.as_object()?;
    let models = raw.get("models")?.as_object()?;
    let available = raw.get("availableHarnesses")?.as_array()?;
    for (harness, list) in models {
        let (Some(harness), Some(list)) = (HarnessId::parse(harness), list.as_array()) else {
            continue;
        };
        let valid: Vec<AgentModel> = list
            .iter()
            .filter_map(|model| serde_json::from_value::<AgentModel>(model.clone()).ok())
            .filter(|model| model.harness == harness)
            .collect();
        catalog.set_harness_models(harness, valid);
    }
    Some(available.iter().filter_map(is_harness_id).collect())
}

/// `orderQuickProjects`: most recently opened first, then whatever else the
/// rail remembers, without archived projects.
pub fn order_quick_projects(
    recents: &[String],
    pinned: &[String],
    rail_order: &[String],
    archived: &[String],
) -> Vec<String> {
    let hidden: Vec<String> = archived.iter().map(|path| path_key(path)).collect();
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for path in recents.iter().chain(pinned).chain(rail_order) {
        if !looks_like_project(path) {
            continue;
        }
        let key = path_key(path);
        if hidden.contains(&key) || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(normalize_project_path(path));
    }
    out
}

/// `loadQuickProjects`.
pub fn load_quick_projects(kv: &Kv) -> Vec<String> {
    let recents: Vec<String> = load_recents(kv).into_iter().map(|item| item.path).collect();
    let archived: Vec<String> = load_archived_projects(kv)
        .into_iter()
        .map(|item| item.path)
        .collect();
    order_quick_projects(
        &recents,
        &load_pinned_projects(kv),
        &load_project_rail_order(kv),
        &archived,
    )
}

/// `prettyParent`.
fn pretty_parent(path: &str) -> String {
    pretty_cwd(&parent_path(path))
}

/// `filterQuickProjects`: the project name or its parent folder.
pub fn filter_quick_projects(projects: &[String], query: &str) -> Vec<String> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return projects.to_vec();
    }
    projects
        .iter()
        .filter(|path| {
            project_name(path).to_lowercase().contains(&needle)
                || pretty_parent(path).to_lowercase().contains(&needle)
        })
        .cloned()
        .collect()
}

/// `quickModelOptions`: every model the picker offers, by harness.
pub fn quick_model_options(catalog: &ModelCatalog, harnesses: &[HarnessId]) -> Vec<AgentModel> {
    harnesses
        .iter()
        .flat_map(|harness| catalog.models_for(*harness).iter().cloned())
        .collect()
}

/// `filterQuickModels`: the model, its upstream provider, or its harness.
pub fn filter_quick_models(models: &[AgentModel], query: &str) -> Vec<AgentModel> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        return models.to_vec();
    }
    models
        .iter()
        .filter(|model| {
            [
                model.name.as_str(),
                model.id.as_str(),
                model
                    .provider
                    .as_ref()
                    .map_or("", |provider| provider.name.as_str()),
                model.harness.title(),
            ]
            .join(" ")
            .to_lowercase()
            .contains(&needle)
        })
        .cloned()
        .collect()
}

/// `initialQuickProject`: the last quick session's project while it is
/// still offered, else the first.
pub fn initial_quick_project(kv: &Kv, projects: &[String]) -> Option<String> {
    if let Some(last) = kv
        .get_item(LAST_PROJECT_KEY)
        .filter(|last| !last.is_empty())
        && let Some(found) = projects
            .iter()
            .find(|path| path_key(path) == path_key(&last))
    {
        return Some(found.clone());
    }
    projects.first().cloned()
}

/// `initialQuickChoice`: the Providers default, not a separate last-used
/// quick-composer model.
pub fn initial_quick_choice(env: &ModelEnv<'_>) -> LastModelChoice {
    env.default_session_choice(None)
}

/// `resolveQuickModel`: keep a live-only model while its catalog loads.
pub fn resolve_quick_model(catalog: &ModelCatalog, choice: &LastModelChoice) -> Option<AgentModel> {
    if !catalog.has_live_catalog(choice.harness)
        && !catalog.models_for(choice.harness).iter().any(|model| {
            model.id == choice.model || model.native_id.as_deref() == Some(choice.model.as_str())
        })
    {
        return None;
    }
    let resolved = catalog.resolve_model(choice.harness, Some(&choice.model));
    (resolved.harness == choice.harness).then_some(resolved)
}

/// `rememberQuickProject`.
pub fn remember_quick_project(kv: &Kv, cwd: &str) {
    kv.set_item(LAST_PROJECT_KEY, cwd);
}

// quickAttachments.ts

/// `storeQuickAttachments`: paths survive the handoff to another window;
/// pasted bytes are written to a file first.
pub async fn store_quick_attachments(
    io: &dyn AttachmentIo,
    files: Vec<Attachment>,
) -> Result<Vec<Attachment>, String> {
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        if file.path.as_deref().is_some_and(|path| !path.is_empty()) {
            out.push(file);
            continue;
        }
        let Some(data) = file.data.clone().filter(|data| !data.is_empty()) else {
            return Err(format!("Could not attach {}.", file.name));
        };
        let path = io.write_attachment(file.name.clone(), data).await?;
        out.push(Attachment {
            path: Some(path),
            ..file
        });
    }
    Ok(out)
}

/// `quickLaunchAttachments`: the portable fields of attachments on disk.
pub fn quick_launch_attachments(files: &[Attachment]) -> Result<Vec<Attachment>, String> {
    files
        .iter()
        .map(|file| {
            if file.path.as_deref().is_none_or(str::is_empty) {
                return Err(format!("Could not attach {}.", file.name));
            }
            Ok(persistable_attachment(file))
        })
        .collect()
}

/// `parseQuickAttachments`: `None` for anything malformed.
pub fn parse_quick_attachments(value: Option<&Value>) -> Option<Vec<Attachment>> {
    let Some(value) = value else {
        return Some(Vec::new());
    };
    let items = value.as_array()?;
    if items.len() > MAX_ATTACHMENTS {
        return None;
    }
    let mut files = Vec::new();
    for item in items {
        let item = item.as_object()?;
        let text = |key: &str| {
            item.get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        };
        let (Some(id), Some(name), Some(mime_type)) = (text("id"), text("name"), text("mimeType"))
        else {
            return None;
        };
        let kind = match item.get("kind").and_then(Value::as_str) {
            Some("image") => AttachmentKind::Image,
            Some("audio") => AttachmentKind::Audio,
            Some("file") => AttachmentKind::File,
            _ => return None,
        };
        let size = item
            .get("size")
            .and_then(safe_integer)
            .filter(|size| *size >= 0)?;
        let path = item
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !js::trim(path).is_empty())?;
        files.push(persistable_attachment(&Attachment {
            id: id.to_string(),
            name: name.to_string(),
            mime_type: mime_type.to_string(),
            kind,
            size,
            path: Some(path.to_string()),
            ..Attachment::default()
        }));
    }
    Some(files)
}

/// `Number.isSafeInteger`.
fn safe_integer(value: &Value) -> Option<i64> {
    if let Some(int) = value.as_i64() {
        return (int.abs() <= 9_007_199_254_740_991).then_some(int);
    }
    let float = value.as_f64()?;
    (float.is_finite() && float.fract() == 0.0 && float.abs() <= 9_007_199_254_740_991.0)
        .then_some(float as i64)
}

// quickWorkspace.ts

/// `QuickWorkspace`: the panel's working copy choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickWorkspace {
    pub cwd: Option<String>,
    pub mode: WorkspaceMode,
    pub base: Option<String>,
    pub tree: Option<Worktree>,
}

/// The workspace fields of a launch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuickWorkspaceFields {
    pub workspace_mode: Option<WorkspaceMode>,
    pub worktree_base: Option<String>,
    pub worktree_cwd: Option<String>,
}

impl QuickWorkspaceFields {
    /// Copy the fields onto a launch.
    pub fn apply(self, launch: &mut QuickLaunchRequest) {
        launch.workspace_mode = self.workspace_mode;
        launch.worktree_base = self.worktree_base;
        launch.worktree_cwd = self.worktree_cwd;
    }
}

/// `workspaceForProject`: switching projects never carries another
/// repository's working copy or base.
pub fn workspace_for_project(choice: &QuickWorkspace, cwd: Option<&str>) -> QuickWorkspace {
    if choice.cwd.as_deref() == cwd {
        return choice.clone();
    }
    QuickWorkspace {
        cwd: cwd.map(str::to_string),
        mode: WorkspaceMode::Current,
        base: None,
        tree: None,
    }
}

/// `quickWorkspaceLaunch`: a new worktree is created later, on the first
/// turn; an existing one is checked again now.
pub async fn quick_workspace_launch<F>(
    choice: &QuickWorkspace,
    list_worktrees: impl FnOnce(String) -> F,
) -> Result<QuickWorkspaceFields, String>
where
    F: Future<Output = Result<Worktrees, String>>,
{
    if choice.mode == WorkspaceMode::Worktree {
        return Ok(QuickWorkspaceFields {
            workspace_mode: Some(WorkspaceMode::Worktree),
            worktree_base: Some(
                choice
                    .base
                    .clone()
                    .filter(|base| !base.is_empty())
                    .unwrap_or_else(|| "HEAD".into()),
            ),
            worktree_cwd: None,
        });
    }
    if let (Some(chosen), Some(cwd)) = (&choice.tree, &choice.cwd) {
        let listed = list_worktrees(cwd.clone()).await?;
        let tree = listed
            .worktrees
            .iter()
            .find(|tree| !tree.missing && path_key(&tree.path) == path_key(&chosen.path))
            .ok_or_else(|| {
                "This worktree is no longer available. Select another working copy.".to_string()
            })?;
        return Ok(if path_key(&tree.path) == path_key(cwd) {
            QuickWorkspaceFields::default()
        } else {
            QuickWorkspaceFields {
                worktree_cwd: Some(tree.path.clone()),
                ..QuickWorkspaceFields::default()
            }
        });
    }
    Ok(QuickWorkspaceFields::default())
}

/// `applyQuickWorkspace`: feed the same deferred worktree creation the
/// main composer uses.
pub fn apply_quick_workspace(session: Session, launch: &QuickLaunchRequest) -> Session {
    if launch.workspace_mode == Some(WorkspaceMode::Worktree) {
        return Session {
            workspace_mode: Some(WorkspaceMode::Worktree),
            worktree_base: Some(
                launch
                    .worktree_base
                    .clone()
                    .filter(|base| !base.is_empty())
                    .unwrap_or_else(|| "HEAD".into()),
            ),
            ..session
        };
    }
    match launch.worktree_cwd.as_ref().filter(|cwd| !cwd.is_empty()) {
        Some(worktree) => Session {
            worktree_cwd: Some(worktree.clone()),
            ..session
        },
        None => session,
    }
}

// quickGitPopup.ts

/// `QUICK_GIT_REQUEST`.
pub const QUICK_GIT_REQUEST: &str = "quick_git_request";
/// `QUICK_GIT_RESULT`.
pub const QUICK_GIT_RESULT: &str = "quick_git_result";

/// `QuickGitKind`: which picker the git popup shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuickGitKind {
    #[serde(rename = "workspace")]
    Workspace,
    #[serde(rename = "base")]
    Base,
    #[serde(rename = "branch")]
    Branch,
}

/// The anchor rectangle of the control that opened the popup.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QuickGitAnchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `QuickGitRequest`. `branches` is the `GitBranches` JSON the panel had.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickGitRequest {
    pub id: String,
    pub kind: QuickGitKind,
    pub choice: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branches: Option<Value>,
    pub anchor: QuickGitAnchor,
}

/// `QuickGitResult`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickGitResult {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<Value>,
    pub restore_focus: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_kind: Option<QuickGitKind>,
}
