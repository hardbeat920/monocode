//! Automations use the scheduler's persisted list, run history, and launch hooks.
use gpui::{App, AppContext as _, Entity, Subscription, Task, Window};
use monocode_app::boot::AppServices;
use monocode_engine::automations::{automations::Automations, templates as engine_templates};
use monocode_engine::{
    history::HistoryPackage,
    projects::ProjectsGlobal,
    submit::{Submit, skills as engine_skills},
    workspace::Workspace,
};
use monocode_view_composer::pickers::skill_picker::SkillSubcommand;
use monocode_view_composer::pickers::{
    ModelSource, PickerSkill, SkillCompletions, SkillKind, SkillScope, SkillTextPart, SlashToken,
};
use monocode_view_pages::{
    DataTask, Listener,
    automations::{
        Automation, AutomationDraft, AutomationTemplate, AutomationsData, AutomationsSnapshot,
        DraftTarget, ProviderConnections, SessionFolderOption, TemplateCategory, TemplateIcon,
        model as view,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashSet;
use std::rc::Rc;

pub struct AppAutomationsData {
    pub automations: Entity<Automations>,
    pub workspace: Entity<Workspace>,
}
fn convert<S: Serialize, T: DeserializeOwned>(value: S) -> Result<T, String> {
    serde_json::to_value(value)
        .and_then(serde_json::from_value)
        .map_err(|error| error.to_string())
}
fn engine_draft(
    draft: AutomationDraft,
) -> Result<monocode_engine::automations::model::AutomationDraft, String> {
    Ok(monocode_engine::automations::model::AutomationDraft {
        id: draft.id,
        name: draft.name,
        prompt: draft.prompt,
        harness: draft.harness,
        model: draft.model,
        model_settings: draft.model_settings,
        cwd: draft.cwd,
        workspace_mode: convert(draft.workspace_mode)?,
        worktree_cwd: draft.worktree_cwd,
        session_folder_id: draft.session_folder_id,
        reuse_session: draft.reuse_session,
        runtime_mode: draft.runtime_mode,
        trigger_kind: convert(draft.trigger_kind)?,
        trigger_event: draft.trigger_event,
        schedule_kind: convert(draft.schedule_kind)?,
        minute: draft.minute,
        time: draft.time,
        day_of_week: draft.day_of_week,
        triggers: convert(draft.triggers)?,
        missed_run_grace_minutes: draft.missed_run_grace_minutes,
        enabled: draft.enabled,
    })
}
impl AutomationsData for AppAutomationsData {
    fn snapshot(&self, cx: &App) -> AutomationsSnapshot {
        let model = self.automations.read(cx);
        AutomationsSnapshot {
            automations: model
                .automations()
                .iter()
                .map(|row| convert(row).expect("automation types share their wire format"))
                .collect(),
            runs: model
                .runs()
                .iter()
                .map(|row| convert(row).expect("automation run types share their wire format"))
                .collect(),
            loading: model.is_loading(),
            error: model.error().map(str::to_owned),
            selected_id: model.selected_id().map(str::to_owned),
            saving: model.is_saving(),
            running: model.running().map(str::to_owned),
        }
    }
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.automations, move |_, cx| listener(cx))
    }
    fn refresh(&self, cx: &mut App) {
        self.automations
            .update(cx, |model, cx| model.refresh(cx))
            .detach();
    }
    fn select(&self, id: Option<String>, cx: &mut App) {
        self.automations
            .update(cx, |model, cx| model.select(id, cx));
    }
    fn save(&self, draft: AutomationDraft, cx: &mut App) -> DataTask<Automation> {
        let draft = match engine_draft(draft) {
            Ok(draft) => draft,
            Err(error) => return Task::ready(Err(error)),
        };
        let task = self
            .automations
            .update(cx, |model, cx| model.save(draft, cx));
        cx.spawn(async move |_| task.await.and_then(convert))
    }
    fn set_enabled(
        &self,
        automation: &Automation,
        enabled: bool,
        cx: &mut App,
    ) -> DataTask<Automation> {
        let automation = match convert(automation) {
            Ok(automation) => automation,
            Err(error) => return Task::ready(Err(error)),
        };
        let task = self
            .automations
            .update(cx, |model, cx| model.set_enabled(&automation, enabled, cx));
        cx.spawn(async move |_| task.await.and_then(convert))
    }
    fn delete(&self, id: &str, cx: &mut App) -> DataTask<()> {
        self.automations
            .update(cx, |model, cx| model.delete(id, cx))
    }
    fn run_now(&self, id: &str, cx: &mut App) -> DataTask<()> {
        self.automations
            .update(cx, |model, cx| model.run_now(id, cx))
    }
    fn clear_error(&self, cx: &mut App) {
        self.automations
            .update(cx, |model, cx| model.clear_error(cx));
    }
    fn open_session(&self, id: &str, _window: &mut Window, cx: &mut App) -> DataTask<()> {
        let task = self
            .workspace
            .update(cx, |workspace, cx| workspace.open_session(id, cx));
        monocode_app::bridge::shell::ShellRequests::send(
            monocode_app::bridge::shell::ShellRequest::ClosePages,
            cx,
        );
        cx.spawn(async move |_| {
            task.await;
            Ok(())
        })
    }
    fn confirm(&self, message: &str, _window: &mut Window, cx: &mut App) -> Task<bool> {
        monocode_app::bridge::dialogs::confirm(message, "Delete", cx)
    }
    fn default_target(
        &self,
        cwd: Option<&str>,
        selected: Option<&Automation>,
        cx: &App,
    ) -> DraftTarget {
        if let Some(selected) = selected {
            return DraftTarget {
                project: selected.cwd.clone(),
                harness: selected.harness,
                model: selected.model.clone(),
            };
        }
        let project = cwd
            .filter(|cwd| monocode_view_pages::format::looks_like_project(cwd))
            .map(str::to_owned)
            .or_else(|| {
                ProjectsGlobal::global(cx)
                    .projects
                    .read(cx)
                    .recents()
                    .first()
                    .map(|project| project.path.clone())
            })
            .unwrap_or_default();
        let inputs = ProjectsGlobal::model_inputs(cx);
        let choice = inputs.env().default_session_choice(Some(&project));
        DraftTarget {
            project,
            harness: choice.harness,
            model: choice.model,
        }
    }
    fn templates(&self, _cx: &App) -> Vec<AutomationTemplate> {
        engine_templates::AUTOMATION_TEMPLATES
            .iter()
            .map(template)
            .collect()
    }
    fn provider_connections(&self, cx: &mut App) -> Task<ProviderConnections> {
        let Some(inbox) = monocode_engine::inbox::inbox::Inbox::try_global(cx) else {
            return Task::ready(ProviderConnections::default());
        };
        let client = inbox.read(cx).client().clone();
        cx.spawn(async move |_| {
            let (github, linear, jira, gitlab, azure) = futures::join!(
                client.github_status(),
                client.linear_connected(),
                client.jira_connected(),
                client.gitlab_connected(),
                client.azure_dev_ops_connected()
            );
            ProviderConnections {
                github: github.is_ok_and(|status| status.connected),
                linear: linear.is_ok_and(|status| status.connected),
                jira: jira.is_ok_and(|status| status.connected),
                gitlab: gitlab.is_ok_and(|status| status.connected),
                azuredevops: azure.is_ok_and(|status| status.connected),
            }
        })
    }
    fn subscribe_provider_changes(&self, listener: Listener, cx: &mut App) -> Subscription {
        match monocode_engine::inbox::inbox::Inbox::try_global(cx) {
            Some(inbox) => cx.observe(&inbox, move |_, cx| listener(cx)),
            None => Subscription::new(|| {}),
        }
    }
    fn session_folders(&self, cwd: &str, cx: &App) -> Vec<SessionFolderOption> {
        monocode_engine::history::session_folders::load_session_folders(
            &AppServices::global(cx).kv,
            cwd,
        )
        .into_iter()
        .map(|folder| SessionFolderOption {
            id: folder.id,
            name: folder.name,
        })
        .collect()
    }
    fn subscribe_session_folders(
        &self,
        _cwd: &str,
        listener: Listener,
        cx: &mut App,
    ) -> Subscription {
        cx.observe(&HistoryPackage::history(cx), move |_, cx| listener(cx))
    }
    fn git_branches(&self, cwd: &str, cx: &mut App) -> DataTask<Vec<String>> {
        let backend = ProjectsGlobal::global(cx).backend.clone();
        let cwd = cwd.to_owned();
        cx.background_spawn(async move {
            backend.git_branches(&cwd).map(|branches| {
                branches
                    .branches
                    .into_iter()
                    .map(|branch| branch.name)
                    .collect()
            })
        })
    }
    fn model_name(&self, harness: monocode_core::HarnessId, model: &str, cx: &App) -> String {
        AppServices::global(cx)
            .catalog
            .read()
            .resolve_model(harness, Some(model))
            .name
    }
    fn model_source(&self, cx: &App) -> Rc<dyn ModelSource> {
        Rc::new(crate::composer_host::CatalogModelSource::from_services(
            AppServices::global(cx),
        ))
    }
    fn model_prefs(&self, cx: &App) -> monocode_core::models::ModelPrefs {
        ProjectsGlobal::model_inputs(cx).prefs
    }
    fn project_providers(&self, cx: &App) -> monocode_core::ProjectProviders {
        ProjectsGlobal::model_inputs(cx).projects
    }
    fn model_controls_beside(&self, cx: &App) -> bool {
        monocode_settings::settings_store::load_model_controls(&AppServices::global(cx).kv).as_str()
            == "beside"
    }
    fn skill_completions(
        &self,
        harness: monocode_core::HarnessId,
        cwd: &str,
        cx: &App,
    ) -> Rc<dyn SkillCompletions> {
        let catalog = Submit::global(cx).read(cx).skills().clone();
        let context = engine_skills::SkillCatalogContext::new(harness, cwd);
        let load = catalog.load_skills(&context, false);
        AppServices::global(cx)
            .registry
            .spawner()
            .spawn(Box::pin(async move {
                let _ = load.await;
            }));
        Rc::new(AppSkillCompletions { catalog, context })
    }
}
fn template(source: &engine_templates::AutomationTemplate) -> AutomationTemplate {
    let category = match source.category {
        engine_templates::TemplateCategory::Popular => TemplateCategory::Popular,
        engine_templates::TemplateCategory::Review => TemplateCategory::Review,
        engine_templates::TemplateCategory::Security => TemplateCategory::Security,
        engine_templates::TemplateCategory::Incidents => TemplateCategory::Incidents,
        engine_templates::TemplateCategory::Research => TemplateCategory::Research,
        engine_templates::TemplateCategory::Environment => TemplateCategory::Environment,
    };
    let icon = match source.icon {
        engine_templates::TemplateIcon::Search => TemplateIcon::Search,
        engine_templates::TemplateIcon::Alert => TemplateIcon::Alert,
        engine_templates::TemplateIcon::File => TemplateIcon::File,
        engine_templates::TemplateIcon::Check => TemplateIcon::Check,
        engine_templates::TemplateIcon::Lock => TemplateIcon::Lock,
        engine_templates::TemplateIcon::Pr => TemplateIcon::Pr,
        engine_templates::TemplateIcon::Inbox => TemplateIcon::Inbox,
        engine_templates::TemplateIcon::Gauge => TemplateIcon::Gauge,
        engine_templates::TemplateIcon::Terminal => TemplateIcon::Terminal,
        engine_templates::TemplateIcon::Note => TemplateIcon::Note,
    };
    AutomationTemplate {
        id: source.id.to_owned(),
        category,
        popular: source.popular,
        icon,
        name: source.name.to_owned(),
        description: source.description.to_owned(),
        prompt: source.prompt.to_owned(),
        trigger_label: source.trigger_label.to_owned(),
        trigger: view::TemplateTrigger {
            kind: convert(source.trigger.kind).unwrap(),
            event: source.trigger.event.to_owned(),
            schedule_kind: source
                .trigger
                .schedule_kind
                .map(|value| convert(value).unwrap()),
            time: source.trigger.time.map(str::to_owned),
            day_of_week: source.trigger.day_of_week,
            minute: source.trigger.minute,
        },
    }
}
struct AppSkillCompletions {
    catalog: engine_skills::SkillCatalog,
    context: engine_skills::SkillCatalogContext,
}
impl AppSkillCompletions {
    fn skills(&self) -> Vec<engine_skills::Skill> {
        self.catalog.peek_skills(&self.context).unwrap_or_default()
    }
}
impl SkillCompletions for AppSkillCompletions {
    fn slash_token_at(&self, text: &str, cursor: usize) -> Option<SlashToken> {
        engine_skills::slash_token_at(
            text,
            cursor,
            self.catalog.has_native_commands(self.context.harness),
        )
        .map(|token| SlashToken {
            start: token.start,
            end: token.end,
            query: token.query,
        })
    }
    fn rank(&self, query: &str) -> Vec<PickerSkill> {
        engine_skills::rank_skills(&self.skills(), query, 8)
            .into_iter()
            .map(|skill| {
                let mut row = PickerSkill::builtin(skill.name(), skill.description());
                row.invocation = skill.invocation().to_owned().into();
                match skill {
                    engine_skills::Skill::File(skill) => {
                        row.kind = SkillKind::File;
                        row.scope = if skill.scope == engine_skills::FileSkillScope::User {
                            SkillScope::User
                        } else {
                            SkillScope::Project
                        };
                        row.source = skill.source.into();
                    }
                    engine_skills::Skill::Native(skill) => {
                        row.kind = SkillKind::Native;
                        row.source = skill.source.as_str().into();
                        row.origin = skill.origin.map(Into::into);
                        row.input_hint = skill.input_hint.map(Into::into);
                        row.subcommands = skill
                            .subcommands
                            .unwrap_or_default()
                            .into_iter()
                            .map(|command| SkillSubcommand {
                                name: command.name.into(),
                                usage: command.usage.map(Into::into),
                            })
                            .collect();
                    }
                    engine_skills::Skill::Builtin(_) => {}
                }
                row
            })
            .collect()
    }
    fn replace_slash_token(&self, text: &str, token: &SlashToken, invocation: &str) -> String {
        engine_skills::replace_slash_token(
            text,
            &engine_skills::SlashToken {
                start: token.start,
                end: token.end,
                query: token.query.clone(),
            },
            invocation,
        )
    }
    fn text_parts(&self, text: &str) -> Vec<SkillTextPart> {
        let names: HashSet<String> = self
            .skills()
            .iter()
            .map(|skill| skill.invocation().to_owned())
            .collect();
        engine_skills::skill_text_parts(text, &names)
            .into_iter()
            .map(|part| SkillTextPart {
                text: part.text,
                skill: part.skill,
            })
            .collect()
    }
}
