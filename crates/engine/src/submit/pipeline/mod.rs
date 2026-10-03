//! The `Submit` entity: the composer's actions on a session. Port of the
//! App.tsx callbacks `submitSession` and `onSubmit` (lines 5854-7013),
//! `onModelChange`, `onModelSettingsChange`, `onRuntimeModeChange`,
//! `onSaveDraft`, and `onRemoveDraft` (5692-5852), `onUpdatePlan` and
//! `onBuildPlan` (7320-7389), and `onCompactContext` and `onStop`
//! (8386-8518), with the helpers above `App` (683-876).
//!
//! The queue dispatch effect (7391-7447) belongs to the attention package,
//! which calls [`Submit::submit`] through its `AttentionSubmit` hook.

mod actions;
mod options;
mod session_edits;
mod submit;
mod turn;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use futures::future::Shared;
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global, Task};
use monocode_core::HarnessId;
use monocode_harness::core::catalog::SharedCatalog;
use monocode_harness::core::registry::HarnessRegistry;
use monocode_harness::core::task::SharedSpawner;
use monocode_settings::Kv;

pub use options::{OnResendRejected, SubmitOptions};
pub use session_edits::{
    last_assistant_text_in_turn, named_worktree_branch, shell_path, temporary_worktree_branch_name,
    with_harness_choice, with_plan_build_target, with_plan_status,
};

use super::acceptance::{ControlOutcome, ProjectLocationSync};
use super::attachments::{AttachmentIo, local_io};
use super::chat_context::ChatContextItem;
use super::draft_cache::ComposerDrafts;
use super::edit_last_turn::EditedResendCoordinator;
use super::hooks::SubmitPeers;
use super::link_preview::LinkPreviews;
use super::mcp_settings_cache::McpSettingsCache;
use super::prefs::KvStore;
use super::skills::{ProcessSkillSources, SkillCatalog, SkillSources};

/// What `Submit` needs from the app.
#[derive(Clone)]
pub struct SubmitConfig {
    pub registry: HarnessRegistry,
    pub catalog: SharedCatalog,
    pub kv: Kv,
    /// Runs detached work such as skill catalog loads.
    pub spawner: SharedSpawner,
    /// `isHarnessAvailable`, for picking the harness that names branches.
    pub is_harness_available: Arc<dyn Fn(HarnessId) -> bool + Send + Sync>,
    pub attachment_io: Arc<dyn AttachmentIo>,
    pub skill_sources: Arc<dyn SkillSources>,
    /// `app_cli_path`: the executable the `/operator` prompt tells the agent
    /// to run.
    pub app_cli_path: Arc<dyn Fn() -> Result<String, String> + Send + Sync>,
    /// The MCP settings cache, when the app has a harness host for
    /// `claude mcp list`.
    pub mcp_settings: Option<McpSettingsCache>,
}

impl SubmitConfig {
    /// The real IO for everything except the MCP cache.
    pub fn new(
        registry: HarnessRegistry,
        catalog: SharedCatalog,
        kv: Kv,
        spawner: SharedSpawner,
    ) -> Self {
        Self {
            skill_sources: Arc::new(ProcessSkillSources {
                registry: registry.clone(),
            }),
            registry,
            catalog,
            kv,
            spawner,
            is_harness_available: Arc::new(|_| true),
            attachment_io: local_io(),
            app_cli_path: Arc::new(monocode_process::control::app_cli_path),
            mcp_settings: None,
        }
    }
}

/// What the entity reports besides its own changes.
#[derive(Debug, Clone, PartialEq)]
pub enum SubmitEvent {
    /// `requestAddToChat`: send a context chip to the focused session, or a
    /// new one.
    AddToChat(ChatContextItem),
}

type LocationSync = Shared<Task<Result<Option<ProjectLocationSync>, String>>>;

/// The composer's actions on sessions, and the composer state that outlives
/// a pane.
pub struct Submit {
    pub(crate) config: SubmitConfig,
    pub(crate) peers: SubmitPeers,
    skills: SkillCatalog,
    drafts: Rc<ComposerDrafts>,
    link_previews: LinkPreviews,
    pub(crate) edited_resends: EditedResendCoordinator,
    project_location_syncs: HashMap<String, LocationSync>,
}

impl EventEmitter<SubmitEvent> for Submit {}

/// The app's `Submit` entity.
pub struct SubmitGlobal(pub Entity<Submit>);

impl Global for SubmitGlobal {}

impl Submit {
    pub fn new(config: SubmitConfig) -> Self {
        let skills = SkillCatalog::new(
            config.skill_sources.clone(),
            Arc::new(KvStore(config.kv.clone())),
            config.spawner.clone(),
        );
        Self {
            config,
            peers: SubmitPeers::default(),
            skills,
            drafts: Rc::new(ComposerDrafts::new()),
            link_previews: LinkPreviews::default(),
            edited_resends: EditedResendCoordinator::new(),
            project_location_syncs: HashMap::new(),
        }
    }

    /// Create the entity and install it as the app's.
    pub fn init(config: SubmitConfig, cx: &mut App) -> Entity<Submit> {
        let entity = cx.new(|_| Submit::new(config));
        cx.set_global(SubmitGlobal(entity.clone()));
        entity
    }

    pub fn global(cx: &App) -> Entity<Submit> {
        cx.global::<SubmitGlobal>().0.clone()
    }

    pub fn try_global(cx: &App) -> Option<Entity<Submit>> {
        cx.try_global::<SubmitGlobal>()
            .map(|global| global.0.clone())
    }

    /// Fill in hooks. Each package replaces only its own field.
    pub fn set_peers(&mut self, update: impl FnOnce(&mut SubmitPeers)) {
        update(&mut self.peers);
    }

    pub fn config(&self) -> &SubmitConfig {
        &self.config
    }

    /// The composer skill catalog.
    pub fn skills(&self) -> &SkillCatalog {
        &self.skills
    }

    /// Composer drafts and MCP tags that outlive a pane.
    pub fn drafts(&self) -> Rc<ComposerDrafts> {
        self.drafts.clone()
    }

    /// Link preview metadata, fetched once per URL.
    pub fn link_previews(&self) -> &LinkPreviews {
        &self.link_previews
    }

    /// The MCP settings cache, when configured.
    pub fn mcp_settings(&self) -> Option<&McpSettingsCache> {
        self.config.mcp_settings.as_ref()
    }

    /// `requestAddToChat`.
    pub fn request_add_to_chat(&mut self, item: ChatContextItem, cx: &mut Context<Self>) {
        cx.emit(SubmitEvent::AddToChat(item));
    }
}

/// Report a managed caller's outcome.
pub(crate) fn settle(options: &SubmitOptions, outcome: ControlOutcome, cx: &mut App) {
    if let Some(on_settled) = &options.on_settled {
        on_settled(outcome, cx);
    }
}
