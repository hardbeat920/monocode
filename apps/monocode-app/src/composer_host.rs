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
use monocode_core::{Attachment, HarnessId};
use monocode_engine::history::HistoryPackage;
use monocode_engine::runtime::Engine;
use monocode_engine::submit::attachments::{self, AttachmentIo, PastedFile};
use monocode_engine::submit::mcp_settings_cache::McpSettingsCache;
use monocode_engine::submit::skills::{self as engine_skills, SkillCatalog, SkillCatalogContext};
use monocode_engine::submit::{Submit, SubmitOptions};
use monocode_engine::workspace::Files;
use monocode_engine::workspace::files::file_index::rank_project_files;
use monocode_harness::core::catalog::SharedCatalog;
use monocode_harness::{HarnessAvailabilityStore, HarnessRegistry, harness_unavailable_hint};
use monocode_settings::Kv;
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::composer::model::mcp::{McpConnection, McpTag};
use monocode_view_composer::composer::model::mentions::{ProjectFile, RankedFile};
use monocode_view_composer::composer::model::skills::Skill;
use monocode_view_composer::composer::{
    Composer, ComposerHost, ComposerSubmission, FolderTarget, McpServers, NewSkillScope,
    SessionFolder, SkillContext,
};
use monocode_view_composer::pickers::ModelSource;
use monocode_view_transcript::threads::BtwSheet;

use monocode_app::boot::AppServices;

/// The model picker's source: the live catalog and the installer probe.
pub struct CatalogModelSource {
    catalog: SharedCatalog,
    availability: HarnessAvailabilityStore,
    registry: HarnessRegistry,
}

impl CatalogModelSource {
    pub fn from_services(services: &AppServices) -> Self {
        Self {
            catalog: services.catalog.clone(),
            availability: services.availability.clone(),
            registry: services.registry.clone(),
        }
    }
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
    btw_sheet: RefCell<Option<WeakEntity<BtwSheet>>>,
    registry: Option<HarnessRegistry>,
    mcp_cache: Option<McpSettingsCache>,
    mcp_watches: RefCell<std::collections::HashMap<String, McpWatch>>,
}

struct McpWatch {
    cache: McpSettingsCache,
    cwd: String,
    id: u64,
    claude_health: bool,
    _task: Task<()>,
}
impl Drop for McpWatch {
    fn drop(&mut self) {
        self.cache.unsubscribe(&self.cwd, self.id);
    }
}

fn mcp_connection(server: &monocode_engine::submit::mcp::McpConnection) -> McpConnection {
    McpConnection {
        provider: server.provider.as_str().into(),
        name: server.name.clone(),
        scope: server.scope.as_str().into(),
        config_path: server.config_path.clone(),
        transport: server.transport.clone(),
        enabled: server.enabled,
    }
}

fn engine_mcp_connection(
    server: &McpConnection,
) -> Option<monocode_engine::submit::mcp::McpConnection> {
    serde_json::from_value(serde_json::json!({"provider":server.provider,"name":server.name,"scope":server.scope,"configPath":server.config_path,"transport":server.transport,"enabled":server.enabled})).ok()
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
        let mcp_cache = submit
            .as_ref()
            .and_then(|submit| submit.read(cx).mcp_settings().cloned());
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
            btw_sheet: RefCell::new(None),
            registry: services.map(|services| services.registry.clone()),
            mcp_cache,
            mcp_watches: RefCell::new(std::collections::HashMap::new()),
        }
    }

    pub fn set_composer(&self, composer: WeakEntity<Composer>) {
        *self.composer.borrow_mut() = Some(composer);
    }

    pub fn set_btw_sheet(&self, sheet: WeakEntity<BtwSheet>) {
        *self.btw_sheet.borrow_mut() = Some(sheet);
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

    fn notes(&self, cx: &mut App) -> Vec<monocode_engine::history::notes::Note> {
        if !monocode_settings::settings_store::load_notes_enabled(&self.kv) {
            return Vec::new();
        }
        let Some(package) = HistoryPackage::try_global(cx) else {
            return Vec::new();
        };
        let notes = package.notes.clone();
        if let Some(cached) = notes.read(cx).peek_notes() {
            return cached.to_vec();
        }
        let load = notes.update(cx, |notes, cx| notes.load_notes(false, cx));
        let task = cx.background_spawn(async move {
            load.await;
        });
        self.refresh_when(task, cx);
        Vec::new()
    }
}

impl ComposerHost for SessionComposerHost {
    fn submit(&self, submission: ComposerSubmission, window: &mut Window, cx: &mut App) -> bool {
        // A workspace switch is moving this session. Reject before async
        // preparation can make the composer clear its draft.
        if monocode_app::bridge::ActiveWorkspace::get(cx)
            .and_then(|workspace| workspace.upgrade())
            .is_some_and(|workspace| workspace.read(cx).is_switching(&self.session_id, cx))
        {
            return false;
        }
        let Some(submit) = Self::submit_entity(cx) else {
            return false;
        };
        let window = window.window_handle();
        let on_resend_rejected = submission.resend.and_then(|ticket| {
            self.composer.borrow().clone().map(|composer| {
                Rc::new(move |rejection, cx: &mut App| {
                    let composer = composer.clone();
                    cx.defer(move |cx| {
                        let _ = window.update(cx, |_, window, cx| {
                            let _ = composer.update(cx, |composer, cx| {
                                composer.resend_rejected(ticket, rejection, window, cx)
                            });
                        });
                    });
                }) as monocode_engine::submit::pipeline::OnResendRejected
            })
        });
        let options = SubmitOptions {
            intent: submission.options.intent,
            resend_edited: submission.options.resend_edited == Some(true),
            draft_block_id: submission.options.draft_block_id,
            on_resend_rejected,
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

    /// Open the side sheet, with a draft or a submitted question.
    fn btw(&self, text: String, draft: bool, _: &mut Window, cx: &mut App) -> bool {
        self.btw_sheet
            .borrow()
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .is_some_and(|sheet| sheet.update(cx, |sheet, cx| sheet.open_with(&text, draft, cx)))
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

    fn place_in_folder(&self, target: FolderTarget, _: &mut Window, cx: &mut App) {
        let sessions = Engine::sessions(cx);
        let Some(cwd) = sessions
            .read(cx)
            .get(&self.session_id)
            .map(|session| session.cwd.clone())
        else {
            return;
        };
        let target = match target {
            FolderTarget::Existing { folder_id } => {
                monocode_engine::history::session_folders::SessionFolderTarget::Existing {
                    folder_id,
                }
            }
            FolderTarget::New { name } => {
                monocode_engine::history::session_folders::SessionFolderTarget::New { name }
            }
        };
        monocode_engine::history::sidebar::place_session_in_project_folder(
            &self.kv,
            &cwd,
            &self.session_id,
            &target,
        );
    }

    fn session_folders(&self, cwd: &str, _: &mut App) -> Vec<SessionFolder> {
        monocode_engine::history::session_folders::load_session_folders(&self.kv, cwd)
            .into_iter()
            .map(|folder| SessionFolder {
                id: folder.id,
                name: folder.name,
                session_count: folder.session_ids.len(),
            })
            .collect()
    }

    fn load_mcp_tags(&self, session_id: &str, cx: &mut App) -> Vec<McpTag> {
        Submit::try_global(cx)
            .map(|submit| {
                submit
                    .read(cx)
                    .drafts()
                    .get_composer_mcp_tags(session_id)
                    .into_iter()
                    .map(|tag| McpTag {
                        server: mcp_connection(&tag.server),
                        token: tag.token,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn save_mcp_tags(&self, session_id: &str, tags: &[McpTag], cx: &mut App) {
        if let Some(submit) = Submit::try_global(cx) {
            let tags = tags
                .iter()
                .filter_map(|tag| {
                    Some(monocode_engine::submit::mcp_picker::McpTag {
                        server: engine_mcp_connection(&tag.server)?,
                        token: tag.token.clone(),
                    })
                })
                .collect();
            submit
                .read(cx)
                .drafts()
                .set_composer_mcp_tags(session_id, tags);
        }
    }

    fn raw_slash_commands(&self, harness: HarnessId) -> bool {
        self.registry
            .as_ref()
            .and_then(|registry| registry.get_harness(harness))
            .and_then(|provider| provider.commands())
            .is_some_and(|commands| commands.raw_slash_commands())
    }

    fn create_skill(
        &self,
        cwd: &str,
        name: &str,
        scope: NewSkillScope,
        cx: &mut App,
    ) -> Task<Result<String, String>> {
        let Some(catalog) = self.skills.clone() else {
            return Task::ready(Err("The skill catalog is unavailable.".into()));
        };
        let (cwd, name) = (cwd.to_owned(), name.to_owned());
        let scope = match scope {
            NewSkillScope::Project => engine_skills::FileSkillScope::Project,
            NewSkillScope::User => engine_skills::FileSkillScope::User,
        };
        cx.background_spawn(async move { catalog.create_blank_skill(&cwd, &name, scope).await })
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
        let mut result: Vec<ProjectFile> =
            monocode_engine::history::notes::notes_as_project_files(&self.notes(cx))
                .into_iter()
                .map(|note| ProjectFile {
                    name: note.name,
                    path: note.path,
                    relative: note.relative,
                    is_dir: false,
                })
                .collect();
        let Some(files) = Files::try_global(cx).map(|files| files.index.clone()) else {
            return result;
        };
        if let Some(listed) = files.read(cx).peek_project_files(cwd) {
            result.extend(listed.iter().map(mention_file));
            return result;
        }
        let load = files.update(cx, |index, cx| index.load_project_files(cwd, false, cx));
        let task = cx.background_spawn(async move {
            let _ = load.await;
        });
        self.refresh_when(task, cx);
        result
    }

    fn rank_mentions(&self, cwd: &str, query: &str, cx: &mut App) -> Vec<RankedFile> {
        let mut result: Vec<RankedFile> =
            monocode_engine::history::notes::rank_note_files(&self.notes(cx), query)
                .into_iter()
                .map(|note| RankedFile {
                    file: ProjectFile {
                        name: note.name,
                        path: note.path,
                        relative: note.relative,
                        is_dir: false,
                    },
                    score: note.score,
                    positions: note.positions,
                })
                .collect();
        let Some(files) = Files::try_global(cx).map(|files| files.index.clone()) else {
            return result;
        };
        let index = files.read(cx);
        let Some(listed) = index.peek_project_files(cwd) else {
            return result;
        };
        let recents = index.recent_opened_files(cwd);
        result.extend(
            rank_project_files(&listed, query, &recents)
                .into_iter()
                .map(|ranked| RankedFile {
                    file: mention_file(&ranked.file),
                    score: ranked.score,
                    positions: ranked.positions,
                }),
        );
        result
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

    fn mcp_servers(&self, cwd: &str, harness: HarnessId, cx: &mut App) -> McpServers {
        let Some(cache) = &self.mcp_cache else {
            return McpServers::default();
        };
        if !self.mcp_watches.borrow().contains_key(cwd)
            && let Some(composer) = self.composer.borrow().clone()
        {
            let (tx, rx) = async_channel::bounded(1);
            let id = cache.subscribe_mcp_settings(cwd, move |_| {
                let _ = tx.try_send(());
            });
            let task = cx.spawn(async move |cx| {
                while rx.recv().await.is_ok() {
                    if composer
                        .update(cx, |composer, cx| composer.refresh_suggestions(cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            self.mcp_watches.borrow_mut().insert(
                cwd.to_owned(),
                McpWatch {
                    cache: cache.clone(),
                    cwd: cwd.to_owned(),
                    id,
                    claude_health: false,
                    _task: task,
                },
            );
        }
        let snapshot = cache.get_cached_mcp_settings(cwd);
        let request_health = harness == HarnessId::Claude
            && self
                .mcp_watches
                .borrow_mut()
                .get_mut(cwd)
                .is_some_and(|watch| {
                    if watch.claude_health {
                        return false;
                    }
                    watch.claude_health = true;
                    true
                });
        if snapshot.is_none() || request_health {
            let load = cache.load_mcp_settings(cwd, false, harness == HarnessId::Claude);
            let task = cx.background_spawn(async move {
                load.await;
            });
            self.refresh_when(task, cx);
        }
        let loading = snapshot.is_none();
        let snapshot = snapshot.unwrap_or_default();
        McpServers {
            servers: snapshot
                .servers
                .iter()
                .map(|row| mcp_connection(&row.connection))
                .collect(),
            claude_status: snapshot
                .servers
                .iter()
                .filter(|row| {
                    row.connection.provider == monocode_engine::submit::mcp::McpProvider::Claude
                })
                .map(|row| (row.connection.name.clone(), row.status.clone()))
                .collect(),
            loading,
            error: snapshot.error,
        }
    }
}
