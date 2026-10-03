//! The engine behind one session's composer: `ComposerHost` over `Submit`
//! (turns, stop, drafts, compaction, skills, attachments), side threads for
//! `/btw`, the workspace file index for `@` mentions, and the pickers'
//! `ModelSource` over the live model catalog. Port of the props
//! SessionPane.tsx passed to Composer.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{App, AppContext as _, Task, WeakEntity, Window};
use monocode_core::models::{AgentModel, ModelPrefs};
use monocode_core::project_providers::{PROJECT_PROVIDER_SETTINGS_KEY, ProjectProviders};
use monocode_core::{Attachment, HarnessId, ModelSettings};
use monocode_engine::runtime::Engine;
use monocode_engine::side_threads::BtwSubmit;
use monocode_engine::side_threads::SideThreads;
use monocode_engine::side_threads::btw::{btw_open_target_turn_id, group_block_turns};
use monocode_engine::submit::attachments::{self, AttachmentIo, PastedFile};
use monocode_engine::submit::skills::{self as engine_skills, SkillCatalog, SkillCatalogContext};
use monocode_engine::submit::{Submit, SubmitOptions};
use monocode_engine::workspace::Files;
use monocode_engine::workspace::files::file_index::rank_project_files;
use monocode_harness::core::catalog::SharedCatalog;
use monocode_harness::{HarnessAvailabilityStore, HarnessRegistry, harness_unavailable_hint};
use monocode_settings::Kv;
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::composer::model::mentions::{ProjectFile, RankedFile};
use monocode_view_composer::composer::model::skills::Skill;
use monocode_view_composer::composer::{Composer, ComposerHost, ComposerSubmission, SkillContext};
use monocode_view_composer::pickers::ModelSource;

use monocode_app::boot::AppServices;

/// The model picker's source: the live catalog and the installer probe.
pub struct CatalogModelSource {
    catalog: SharedCatalog,
    availability: HarnessAvailabilityStore,
    registry: HarnessRegistry,
}

impl ModelSource for CatalogModelSource {
    fn models_for(&self, harness: HarnessId) -> Vec<AgentModel> {
        self.catalog.read().models_for(harness).to_vec()
    }

    fn resolve(&self, harness: HarnessId, id: Option<&str>) -> AgentModel {
        self.catalog.read().resolve_model(harness, id)
    }

    fn find(&self, id: &str) -> Option<AgentModel> {
        self.catalog.read().find_model(id).cloned()
    }

    fn available(&self, harness: HarnessId) -> bool {
        self.availability.is_harness_available(harness)
    }

    fn probed(&self) -> bool {
        self.availability.has_probed_harness_availability()
    }

    /// `refreshHarnessCatalogs` for the tabs the picker shows.
    fn refresh(&self, harnesses: &[HarnessId]) {
        let registry = self.registry.clone();
        let catalog = self.catalog.clone();
        let harnesses = harnesses.to_vec();
        registry.clone().spawner().spawn(Box::pin(async move {
            registry
                .refresh_harness_catalogs(harnesses, false, |id| catalog.has_live_catalog(id))
                .await;
        }));
    }

    fn unavailable_hint(&self, harness: HarnessId) -> String {
        harness_unavailable_hint(harness)
    }
}

/// `SkillCatalogContext` from the composer's key.
fn catalog_context(context: &SkillContext) -> SkillCatalogContext {
    let mut catalog = SkillCatalogContext::new(context.harness, context.cwd.clone());
    if let Some(id) = &context.session_id {
        catalog = catalog.with_session(id.clone());
    }
    catalog
}

/// The composer's slash row for an engine skill.
fn picker_skill(skill: &engine_skills::Skill) -> Skill {
    match skill {
        engine_skills::Skill::File(file) => {
            let scope = match file.scope {
                engine_skills::FileSkillScope::Project => "project",
                engine_skills::FileSkillScope::User => "user",
            };
            let mut row = Skill::file(
                &file.name,
                &file.description,
                &file.path,
                scope,
                &file.source,
            );
            row.invocation = file.invocation.clone();
            row
        }
        engine_skills::Skill::Builtin(builtin) => {
            Skill::builtin(builtin.name, builtin.invocation, builtin.description)
        }
        engine_skills::Skill::Native(native) => {
            let mut row = Skill::native(
                &native.name,
                &native.invocation,
                &native.description,
                native.source.as_str(),
            );
            row.aliases = native.aliases.clone().unwrap_or_default();
            row
        }
    }
}

fn mention_file(file: &monocode_engine::workspace::files::ProjectFile) -> ProjectFile {
    ProjectFile {
        name: file.name.clone(),
        path: file.path.clone(),
        relative: file.relative.clone(),
        is_dir: file.is_dir == Some(true),
    }
}

/// One session's `ComposerHost`.
pub struct SessionComposerHost {
    session_id: String,
    kv: Kv,
    skills: Option<SkillCatalog>,
    attachment_io: Option<std::sync::Arc<dyn AttachmentIo>>,
    model_source: Option<Rc<dyn ModelSource>>,
    /// The composer this host serves, for refreshing suggestions when a
    /// skill catalog or the file index finishes loading.
    composer: RefCell<Option<WeakEntity<Composer>>>,
}

impl SessionComposerHost {
    pub fn new(session_id: String, cx: &App) -> Self {
        let services = AppServices::try_global(cx);
        let submit = Submit::try_global(cx);
        let skills = submit
            .as_ref()
            .map(|submit| submit.read(cx).skills().clone());
        let attachment_io = submit
            .as_ref()
            .map(|submit| submit.read(cx).config().attachment_io.clone());
        let model_source = services.map(|services| {
            Rc::new(CatalogModelSource {
                catalog: services.catalog.clone(),
                availability: services.availability.clone(),
                registry: services.registry.clone(),
            }) as Rc<dyn ModelSource>
        });
        Self {
            session_id,
            kv: services
                .map(|services| services.kv.clone())
                .unwrap_or_else(Kv::in_memory),
            skills,
            attachment_io,
            model_source,
            composer: RefCell::new(None),
        }
    }

    pub fn set_composer(&self, composer: WeakEntity<Composer>) {
        *self.composer.borrow_mut() = Some(composer);
    }

    /// Ask the composer to re-read skills and mentions once `load` is done.
    fn refresh_when<T: 'static>(&self, load: Task<T>, cx: &mut App) {
        let Some(composer) = self.composer.borrow().clone() else {
            load.detach();
            return;
        };
        cx.spawn(async move |cx| {
            load.await;
            composer
                .update(cx, |composer, cx| composer.refresh_suggestions(cx))
                .ok();
        })
        .detach();
    }

    fn submit_entity(cx: &App) -> Option<gpui::Entity<Submit>> {
        Submit::try_global(cx)
    }
}

impl ComposerHost for SessionComposerHost {
    fn submit(&self, submission: ComposerSubmission, _: &mut Window, cx: &mut App) -> bool {
        let Some(submit) = Self::submit_entity(cx) else {
            return false;
        };
        let options = SubmitOptions {
            intent: submission.options.intent,
            resend_edited: submission.options.resend_edited == Some(true),
            draft_block_id: submission.options.draft_block_id,
            ..SubmitOptions::default()
        };
        let id = self.session_id.clone();
        submit.update(cx, |submit, cx| {
            submit.on_submit(&id, &submission.text, submission.attachments, options, cx)
        })
    }

    fn stop(&self, _: &mut Window, cx: &mut App) {
        if let Some(submit) = Self::submit_entity(cx) {
            let id = self.session_id.clone();
            submit.update(cx, |submit, cx| submit.stop(&id, false, cx));
        }
    }

    fn save_draft(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        _: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(submit) = Self::submit_entity(cx) else {
            return false;
        };
        let id = self.session_id.clone();
        submit.update(cx, |submit, cx| {
            submit.save_draft(&id, &text, attachments, None, cx)
        })
    }

    /// `/btw <question>`: ask about the last finished turn on the side.
    /// The side sheet is not drawn yet, so a bare `/btw ` that only opens
    /// the sheet is rejected.
    fn btw(&self, text: String, draft: bool, _: &mut Window, cx: &mut App) -> bool {
        if draft || text.trim().is_empty() {
            return false;
        }
        let Some(side_threads) = SideThreads::try_global(cx) else {
            return false;
        };
        let Some(session) = Engine::sessions(cx).read(cx).get(&self.session_id).cloned() else {
            return false;
        };
        let managed = session.orchestration_lead_id.is_some();
        let turns = group_block_turns(&session.blocks, managed);
        let Some(turn_id) =
            btw_open_target_turn_id(&turns, &session.blocks, session.harness, managed)
        else {
            return false;
        };
        let Some(turn) = turns
            .iter()
            .find(|turn| turn.first().is_some_and(|block| block.id == turn_id))
        else {
            return false;
        };
        let thread_id = uuid::Uuid::new_v4().to_string();
        let message_id = uuid::Uuid::new_v4().to_string();
        side_threads.btw_submit(
            BtwSubmit {
                session_id: &self.session_id,
                turn,
                thread_id: &thread_id,
                message_id: &message_id,
                text: &text,
                model: None,
                model_settings: None::<&ModelSettings>,
            },
            cx,
        )
    }

    fn compact_context(&self, _: &mut Window, cx: &mut App) -> bool {
        let Some(submit) = Self::submit_entity(cx) else {
            return false;
        };
        let id = self.session_id.clone();
        submit.update(cx, |submit, cx| submit.compact(&id, cx))
    }

    fn draft_changed(&self, text: &str, cx: &mut App) {
        if let Some(submit) = Self::submit_entity(cx) {
            submit
                .read(cx)
                .drafts()
                .set_composer_draft(&self.session_id, text);
        }
    }

    fn attachments_from_paths(&self, paths: Vec<String>, cx: &mut App) -> Task<Vec<Attachment>> {
        let Some(io) = self.attachment_io.clone() else {
            return Task::ready(Vec::new());
        };
        cx.background_spawn(async move {
            attachments::attachments_from_paths(io.as_ref(), &paths)
                .await
                .unwrap_or_default()
        })
    }

    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        cx: &mut App,
    ) -> Task<Vec<Attachment>> {
        let Some(io) = self.attachment_io.clone() else {
            return Task::ready(Vec::new());
        };
        let pasted: Vec<PastedFile> = files
            .into_iter()
            .map(|file| PastedFile {
                name: file.name,
                mime_type: file.mime_type,
                bytes: file.bytes,
            })
            .collect();
        cx.background_spawn(async move {
            attachments::attachments_from_files(io.as_ref(), &[], &pasted)
                .await
                .unwrap_or_default()
        })
    }

    /// `pickAttachments`: the open panel, files and folders.
    fn pick_attachments(&self, _: &mut Window, cx: &mut App) -> Task<Vec<Attachment>> {
        let Some(io) = self.attachment_io.clone() else {
            return Task::ready(Vec::new());
        };
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: None,
        });
        cx.background_spawn(async move {
            let paths: Vec<PathBuf> = match picked.await {
                Ok(Ok(Some(paths))) => paths,
                _ => return Vec::new(),
            };
            let paths: Vec<String> = paths
                .into_iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect();
            attachments::attachments_from_paths(io.as_ref(), &paths)
                .await
                .unwrap_or_default()
        })
    }

    /// `useComposerSkills().skills`: the cached catalog. A catalog that is
    /// not loaded yet starts loading and refreshes the composer when done.
    fn skills(&self, context: &SkillContext, cx: &mut App) -> Vec<Skill> {
        let Some(catalog) = &self.skills else {
            return Vec::new();
        };
        let context = catalog_context(context);
        if let Some(skills) = catalog.peek_skills(&context) {
            return skills.iter().map(picker_skill).collect();
        }
        let load = catalog.load_skills(&context, false);
        let task = cx.background_spawn(async move {
            load.await;
        });
        self.refresh_when(task, cx);
        Vec::new()
    }

    fn reload_skills(&self, context: &SkillContext, refresh: bool, cx: &mut App) {
        let Some(catalog) = &self.skills else {
            return;
        };
        let load = catalog.load_skills(&catalog_context(context), refresh);
        let task = cx.background_spawn(async move {
            load.await;
        });
        self.refresh_when(task, cx);
    }

    fn has_native_commands(&self, harness: HarnessId) -> bool {
        self.skills
            .as_ref()
            .is_some_and(|catalog| catalog.has_native_commands(harness))
    }

    /// The project index for `@`. A project not listed yet starts its scan
    /// and refreshes the composer when it lands.
    fn mention_files(&self, cwd: &str, cx: &mut App) -> Vec<ProjectFile> {
        let Some(files) = Files::try_global(cx).map(|files| files.index.clone()) else {
            return Vec::new();
        };
        if let Some(listed) = files.read(cx).peek_project_files(cwd) {
            return listed.iter().map(mention_file).collect();
        }
        let load = files.update(cx, |index, cx| index.load_project_files(cwd, false, cx));
        let task = cx.background_spawn(async move {
            let _ = load.await;
        });
        self.refresh_when(task, cx);
        Vec::new()
    }

    fn rank_mentions(&self, cwd: &str, query: &str, cx: &mut App) -> Vec<RankedFile> {
        let Some(files) = Files::try_global(cx).map(|files| files.index.clone()) else {
            return Vec::new();
        };
        let index = files.read(cx);
        let Some(listed) = index.peek_project_files(cwd) else {
            return Vec::new();
        };
        let recents = index.recent_opened_files(cwd);
        rank_project_files(&listed, query, &recents)
            .into_iter()
            .map(|ranked| RankedFile {
                file: mention_file(&ranked.file),
                score: ranked.score,
                positions: ranked.positions,
            })
            .collect()
    }

    fn mentions_loading(&self, cwd: &str, cx: &mut App) -> bool {
        Files::try_global(cx)
            .is_some_and(|files| files.index.read(cx).peek_project_files(cwd).is_none())
    }

    fn model_source(&self, _: &mut App) -> Option<Rc<dyn ModelSource>> {
        self.model_source.clone()
    }

    fn model_prefs(&self, _: &mut App) -> ModelPrefs {
        ModelPrefs::from_local_storage(|key| self.kv.get_item(key))
    }

    fn project_providers(&self, _: &mut App) -> ProjectProviders {
        ProjectProviders::parse(self.kv.get_item(PROJECT_PROVIDER_SETTINGS_KEY).as_deref())
    }
}
