//! What the automations page reads and changes.
//!
//! The engine's automations package owns the list, the selection, the run
//! history, and the save, toggle, delete, and run-now actions in its
//! `Automations` entity; [`AutomationsData`] mirrors that API. The rest are
//! the model reads AutomationsView.tsx made directly: the draft defaults
//! (`defaultSessionChoice`, `firstEnabledHarness`, `modelsFor`), the
//! templates, Inbox provider connections, session folders, git branches,
//! the model catalog for the pickers, the skill catalog for the prompt
//! field, and the window's confirm dialog. The view keeps the editor state
//! (draft, picker, tabs, menus), as React did.

use std::rc::Rc;

use gpui::{App, AppContext as _, Entity, Subscription, Task, Window};
use monocode_core::models::{ModelCatalog, ModelPrefs};
use monocode_core::{HarnessId, ProjectProviders};
use monocode_view_composer::pickers::model_source::all_available;
use monocode_view_composer::pickers::{
    LocalModelSource, ModelSource, PickerSkill, SkillCompletions, SkillTextPart, SlashToken,
};

use super::model::{
    Automation, AutomationDraft, AutomationRun, AutomationTriggerKind, automation_triggers,
    automation_upsert,
};
use super::templates::AutomationTemplate;
use crate::data::{DataTask, Listener};
use crate::format::{looks_like_project, now_ms};

/// One read of the engine's `Automations` entity.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutomationsSnapshot {
    /// Every automation, newest change first.
    pub automations: Vec<Automation>,
    /// The first list load has not finished.
    pub loading: bool,
    /// The last load or action error.
    pub error: Option<String>,
    pub selected_id: Option<String>,
    /// The selected automation's runs, newest first.
    pub runs: Vec<AutomationRun>,
    pub saving: bool,
    /// The automation a Run now is starting.
    pub running: Option<String>,
}

impl AutomationsSnapshot {
    pub fn selected(&self) -> Option<&Automation> {
        let id = self.selected_id.as_deref()?;
        self.automations
            .iter()
            .find(|automation| automation.id == id)
    }
}

/// `defaultDraftTarget`: where a new draft runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftTarget {
    pub project: String,
    pub harness: HarnessId,
    pub model: String,
}

/// Which Inbox providers are signed in, for the event triggers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProviderConnections {
    pub github: bool,
    pub linear: bool,
    pub jira: bool,
    pub gitlab: bool,
    pub azuredevops: bool,
}

impl ProviderConnections {
    /// `triggerReady`: time triggers always work; event triggers need the
    /// provider.
    pub fn ready(&self, kind: AutomationTriggerKind) -> bool {
        match kind {
            AutomationTriggerKind::Time => true,
            AutomationTriggerKind::Github => self.github,
            AutomationTriggerKind::Linear => self.linear,
            AutomationTriggerKind::Jira => self.jira,
            AutomationTriggerKind::Gitlab => self.gitlab,
            AutomationTriggerKind::AzureDevops => self.azuredevops,
        }
    }
}

/// A sidebar session folder a run can land in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFolderOption {
    pub id: String,
    pub name: String,
}

pub trait AutomationsData: 'static {
    // The engine's `Automations` entity.

    fn snapshot(&self, cx: &App) -> AutomationsSnapshot;
    /// Runs after every change (`AutomationsEvent::Changed` and notify).
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription;
    /// List again (the page opened).
    fn refresh(&self, cx: &mut App);
    /// Select an automation and load its runs.
    fn select(&self, id: Option<String>, cx: &mut App);
    /// `onSave`: save, select it, and reload.
    fn save(&self, draft: AutomationDraft, cx: &mut App) -> DataTask<Automation>;
    /// `setAutomationEnabled`.
    fn set_enabled(
        &self,
        automation: &Automation,
        enabled: bool,
        cx: &mut App,
    ) -> DataTask<Automation>;
    /// `deleteAutomation`, with its runs. The view confirms first.
    fn delete(&self, id: &str, cx: &mut App) -> DataTask<()>;
    /// `onRun`: record a manual run and launch it.
    fn run_now(&self, id: &str, cx: &mut App) -> DataTask<()>;
    fn clear_error(&self, cx: &mut App);

    // App calls.

    /// Open a run's session in the workspace.
    fn open_session(&self, session_id: &str, window: &mut Window, cx: &mut App) -> DataTask<()>;
    /// `window.confirm`.
    fn confirm(&self, message: &str, window: &mut Window, cx: &mut App) -> Task<bool>;

    // Model reads.

    /// `defaultDraftTarget`: the active project (or the newest recent), and
    /// the selected automation's or the project's default provider and
    /// model.
    fn default_target(
        &self,
        cwd: Option<&str>,
        selected: Option<&Automation>,
        cx: &App,
    ) -> DraftTarget;
    /// `AUTOMATION_TEMPLATES`.
    fn templates(&self, cx: &App) -> Vec<AutomationTemplate>;
    /// `githubStatus`, `linearConnected`, and the rest.
    fn provider_connections(&self, cx: &mut App) -> Task<ProviderConnections>;
    /// The provider change events (`LINEAR_CHANGE_EVENT` and the rest).
    fn subscribe_provider_changes(&self, _listener: Listener, _cx: &mut App) -> Subscription {
        Subscription::new(|| {})
    }
    /// `loadSessionFolders(cwd)`.
    fn session_folders(&self, cwd: &str, cx: &App) -> Vec<SessionFolderOption>;
    /// `subscribeSessionFolders(cwd)`.
    fn subscribe_session_folders(
        &self,
        _cwd: &str,
        _listener: Listener,
        _cx: &mut App,
    ) -> Subscription {
        Subscription::new(|| {})
    }
    /// `gitBranches(cwd)`, by name.
    fn git_branches(&self, cwd: &str, cx: &mut App) -> DataTask<Vec<String>>;
    /// `resolveModel(harness, model).name`.
    fn model_name(&self, harness: HarnessId, model: &str, cx: &App) -> String;
    /// The model picker's catalog and availability.
    fn model_source(&self, cx: &App) -> Rc<dyn ModelSource>;
    fn model_prefs(&self, _cx: &App) -> ModelPrefs {
        ModelPrefs::default()
    }
    fn project_providers(&self, _cx: &App) -> ProjectProviders {
        ProjectProviders::default()
    }
    /// `loadModelControls() === "beside"`: model options show as pills.
    fn model_controls_beside(&self, _cx: &App) -> bool {
        false
    }
    /// The skill catalog for `/skill` completions in the prompt.
    fn skill_completions(
        &self,
        harness: HarnessId,
        cwd: &str,
        cx: &App,
    ) -> Rc<dyn SkillCompletions>;
}

/// Prompt completions with no skills.
pub struct NoSkills;

impl SkillCompletions for NoSkills {
    fn slash_token_at(&self, _: &str, _: usize) -> Option<SlashToken> {
        None
    }

    fn rank(&self, _: &str) -> Vec<PickerSkill> {
        Vec::new()
    }

    fn replace_slash_token(&self, text: &str, _: &SlashToken, _: &str) -> String {
        text.to_string()
    }

    fn text_parts(&self, text: &str) -> Vec<SkillTextPart> {
        vec![SkillTextPart {
            text: text.to_string(),
            skill: false,
        }]
    }
}

/// The state behind [`LocalAutomations`].
pub struct LocalAutomationsState {
    pub automations: Vec<Automation>,
    pub runs: Vec<AutomationRun>,
    pub templates: Vec<AutomationTemplate>,
    pub connections: ProviderConnections,
    pub folders: Vec<SessionFolderOption>,
    pub branches: Vec<String>,
    pub recents: Vec<String>,
    pub selected_id: Option<String>,
    pub error: Option<String>,
    pub confirm: bool,
    pub opened_sessions: Vec<String>,
    pub ran: Vec<String>,
    pub deleted: Vec<String>,
    pub saved: Vec<AutomationDraft>,
    catalog: ModelCatalog,
    next_id: u64,
}

/// In-memory automations for the gallery and tests. Saves and runs land at
/// once.
#[derive(Clone)]
pub struct LocalAutomations {
    state: Entity<LocalAutomationsState>,
}

impl LocalAutomations {
    pub fn new(
        automations: Vec<Automation>,
        templates: Vec<AutomationTemplate>,
        recents: Vec<String>,
        cx: &mut App,
    ) -> Self {
        let selected_id = automations.first().map(|automation| automation.id.clone());
        Self {
            state: cx.new(|_| LocalAutomationsState {
                automations,
                runs: Vec::new(),
                templates,
                connections: ProviderConnections {
                    github: true,
                    linear: true,
                    ..Default::default()
                },
                folders: Vec::new(),
                branches: vec!["main".into()],
                recents,
                selected_id,
                error: None,
                confirm: true,
                opened_sessions: Vec::new(),
                ran: Vec::new(),
                deleted: Vec::new(),
                saved: Vec::new(),
                catalog: ModelCatalog::new(),
                next_id: 1,
            }),
        }
    }

    pub fn state(&self) -> &Entity<LocalAutomationsState> {
        &self.state
    }

    pub fn set_runs(&self, runs: Vec<AutomationRun>, cx: &mut App) {
        self.update(cx, |state| state.runs = runs);
    }

    pub fn set_folders(&self, folders: Vec<SessionFolderOption>, cx: &mut App) {
        self.update(cx, |state| state.folders = folders);
    }

    pub fn set_error(&self, error: Option<String>, cx: &mut App) {
        self.update(cx, |state| state.error = error);
    }

    fn update(&self, cx: &mut App, f: impl FnOnce(&mut LocalAutomationsState)) {
        self.state.update(cx, |state, cx| {
            f(state);
            cx.notify();
        });
    }
}

impl AutomationsData for LocalAutomations {
    fn snapshot(&self, cx: &App) -> AutomationsSnapshot {
        let state = self.state.read(cx);
        let runs = match state.selected_id.as_deref() {
            Some(id) => state
                .runs
                .iter()
                .filter(|run| run.automation_id == id)
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        AutomationsSnapshot {
            automations: state.automations.clone(),
            loading: false,
            error: state.error.clone(),
            selected_id: state.selected_id.clone(),
            runs,
            saving: false,
            running: None,
        }
    }

    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.state, move |_, cx| listener(cx))
    }

    fn refresh(&self, cx: &mut App) {
        self.update(cx, |state| {
            let keep = state.selected_id.as_ref().is_some_and(|id| {
                state
                    .automations
                    .iter()
                    .any(|automation| automation.id == *id)
            });
            if !keep {
                state.selected_id = state.automations.first().map(|entry| entry.id.clone());
            }
        });
    }

    fn select(&self, id: Option<String>, cx: &mut App) {
        self.update(cx, |state| state.selected_id = id);
    }

    fn save(&self, draft: AutomationDraft, cx: &mut App) -> DataTask<Automation> {
        let saved = self.state.update(cx, |state, cx| {
            let now = now_ms();
            let mut draft = draft;
            if draft.id.is_none() {
                draft.id = Some(format!("local-automation-{}", state.next_id));
                state.next_id += 1;
            }
            state.saved.push(draft.clone());
            let upsert = automation_upsert(&draft, now);
            let previous = state
                .automations
                .iter()
                .find(|automation| automation.id == upsert.id)
                .cloned();
            let mut value = serde_json::to_value(&upsert).unwrap_or_default();
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "createdAt".into(),
                    previous
                        .as_ref()
                        .map_or(now, |entry| entry.created_at)
                        .into(),
                );
                object.insert("updatedAt".into(), now.into());
                if let Some(previous) = &previous
                    && let Some(at) = previous.last_run_at
                {
                    object.insert("lastRunAt".into(), at.into());
                }
            }
            let automation: Automation =
                serde_json::from_value(value).expect("an upsert reads as an automation");
            state.automations.retain(|entry| entry.id != automation.id);
            state.automations.insert(0, automation.clone());
            state.selected_id = Some(automation.id.clone());
            cx.notify();
            automation
        });
        Task::ready(Ok(saved))
    }

    fn set_enabled(
        &self,
        automation: &Automation,
        enabled: bool,
        cx: &mut App,
    ) -> DataTask<Automation> {
        let id = automation.id.clone();
        let updated = self.state.update(cx, |state, cx| {
            let entry = state.automations.iter_mut().find(|entry| entry.id == id)?;
            entry.enabled = enabled;
            cx.notify();
            Some(entry.clone())
        });
        Task::ready(updated.ok_or_else(|| "Automation not found.".to_string()))
    }

    fn delete(&self, id: &str, cx: &mut App) -> DataTask<()> {
        self.update(cx, |state| {
            state.automations.retain(|entry| entry.id != id);
            state.deleted.push(id.to_string());
            state.selected_id = state.automations.first().map(|entry| entry.id.clone());
        });
        Task::ready(Ok(()))
    }

    fn run_now(&self, id: &str, cx: &mut App) -> DataTask<()> {
        self.update(cx, |state| {
            state.ran.push(id.to_string());
            let now = now_ms();
            state.runs.insert(
                0,
                serde_json::from_value(serde_json::json!({
                    "id": format!("run-{}", state.ran.len()),
                    "automationId": id,
                    "trigger": "manual",
                    "scheduledFor": now,
                    "createdAt": now,
                    "startedAt": now,
                    "status": "running",
                }))
                .expect("a manual run"),
            );
        });
        Task::ready(Ok(()))
    }

    fn clear_error(&self, cx: &mut App) {
        self.update(cx, |state| state.error = None);
    }

    fn open_session(&self, session_id: &str, _: &mut Window, cx: &mut App) -> DataTask<()> {
        self.update(cx, |state| {
            state.opened_sessions.push(session_id.to_string())
        });
        Task::ready(Ok(()))
    }

    fn confirm(&self, _: &str, _: &mut Window, cx: &mut App) -> Task<bool> {
        Task::ready(self.state.read(cx).confirm)
    }

    fn default_target(
        &self,
        cwd: Option<&str>,
        selected: Option<&Automation>,
        cx: &App,
    ) -> DraftTarget {
        let state = self.state.read(cx);
        let project = cwd
            .filter(|cwd| looks_like_project(cwd))
            .map(str::to_string)
            .or_else(|| state.recents.first().cloned())
            .unwrap_or_else(|| "~".into());
        let harness = selected.map_or(HarnessId::Claude, |automation| automation.harness);
        let model = selected
            .filter(|automation| automation.harness == harness)
            .map(|automation| automation.model.clone())
            .unwrap_or_else(|| state.catalog.default_model_id(harness));
        DraftTarget {
            project,
            harness,
            model,
        }
    }

    fn templates(&self, cx: &App) -> Vec<AutomationTemplate> {
        self.state.read(cx).templates.clone()
    }

    fn provider_connections(&self, cx: &mut App) -> Task<ProviderConnections> {
        Task::ready(self.state.read(cx).connections)
    }

    fn session_folders(&self, _cwd: &str, cx: &App) -> Vec<SessionFolderOption> {
        self.state.read(cx).folders.clone()
    }

    fn git_branches(&self, _cwd: &str, cx: &mut App) -> DataTask<Vec<String>> {
        Task::ready(Ok(self.state.read(cx).branches.clone()))
    }

    fn model_name(&self, harness: HarnessId, model: &str, cx: &App) -> String {
        self.state
            .read(cx)
            .catalog
            .resolve_model(harness, Some(model))
            .name
    }

    fn model_source(&self, cx: &App) -> Rc<dyn ModelSource> {
        Rc::new(LocalModelSource::new(
            self.state.read(cx).catalog.clone(),
            all_available(),
        ))
    }

    fn skill_completions(&self, _: HarnessId, _: &str, _: &App) -> Rc<dyn SkillCompletions> {
        Rc::new(NoSkills)
    }
}

/// The first trigger of an automation decides its card's mark.
pub fn card_trigger_kind(automation: &Automation) -> AutomationTriggerKind {
    automation_triggers(automation)
        .first()
        .map_or(automation.trigger_kind, |trigger| trigger.kind)
}
