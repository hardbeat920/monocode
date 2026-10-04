//! Session side questions and orchestration cards over the native engine.

use std::rc::Rc;

use gpui::{App, Subscription, Task};
use monocode_core::block::{BtwThread, ModelTarget};
use monocode_core::btw::BtwSessionThread;
use monocode_core::orchestration::OrchestrationProposal;
use monocode_core::{Block, HarnessId, ModelSettings};
use monocode_engine::orchestration::{Orchestration, orchestrator, package};
use monocode_engine::side_threads::btw;
use monocode_engine::side_threads::{BtwSubmit, SideThreads};
use monocode_view_composer::composer::ComposerHost;
use monocode_view_transcript::threads::{
    BtwHost, BtwRequest, BtwThreadBlocksInput, OrchestrationActions, OrchestrationRunView,
    OrchestrationRuns, OrchestrationWorkerDetail, ResumeBlocker,
};

use crate::composer_host::SessionComposerHost;
use monocode_app::boot::AppServices;
use monocode_app::bridge::ActiveWorkspace;

pub struct SessionBtwHost {
    pub session_id: String,
    pub composer: Rc<SessionComposerHost>,
    pub catalog: monocode_harness::core::catalog::SharedCatalog,
}

impl BtwHost for SessionBtwHost {
    fn session_threads(&self, blocks: &[Block], managed: bool) -> Vec<BtwSessionThread> {
        btw::session_btw_threads(blocks, managed)
    }
    fn open_target_turn_id(
        &self,
        turns: &[Vec<Block>],
        blocks: &[Block],
        harness: HarnessId,
        managed: bool,
    ) -> Option<String> {
        btw::btw_open_target_turn_id(turns, blocks, harness, managed)
    }
    fn surface_harness(
        &self,
        blocks: &[Block],
        turn: &[Block],
        harness: HarnessId,
        threads: Option<&[BtwThread]>,
    ) -> Option<HarnessId> {
        btw::btw_surface_harness(blocks, turn, harness, threads)
    }
    fn thread_blocks(&self, input: BtwThreadBlocksInput<'_>) -> Vec<Block> {
        btw::btw_thread_blocks(
            btw::BtwThreadBlocksInput {
                messages: input.messages,
                pending_blocks: input.pending_blocks,
                running: input.running,
                updated_at: input.updated_at,
                harness: input.harness,
                model: input.model,
            },
            &self.catalog.read(),
        )
    }
    fn preferred_model_settings(
        &self,
        harness: HarnessId,
        model: &str,
        current: &ModelSettings,
    ) -> ModelSettings {
        let catalog = self.catalog.read();
        let model = catalog.resolve_model(harness, Some(model));
        catalog.merge_model_settings(&model, Some(current))
    }
    fn submit(&self, request: BtwRequest<'_>, cx: &mut App) -> bool {
        let Some(threads) = SideThreads::try_global(cx) else {
            return false;
        };
        threads.btw_submit(
            BtwSubmit {
                session_id: &self.session_id,
                turn: request.turn,
                thread_id: request.thread_id,
                message_id: request.message_id,
                text: request.text,
                model: request.model,
                model_settings: Some(request.model_settings),
            },
            cx,
        )
    }
    fn retry(&self, turn: &[Block], thread_id: &str, cx: &mut App) {
        if let Some(threads) = SideThreads::try_global(cx) {
            threads.btw_retry(&self.session_id, turn, thread_id, cx);
        }
    }
    fn delete(&self, turn: &[Block], thread_id: &str, cx: &mut App) {
        if let Some(threads) = SideThreads::try_global(cx) {
            threads.btw_delete(&self.session_id, turn, thread_id, cx);
        }
    }
    fn stop(&self, turn: &[Block], thread_id: &str, cx: &mut App) {
        if let Some(threads) = SideThreads::try_global(cx) {
            threads.btw_stop(&self.session_id, turn, thread_id, cx);
        }
    }
    fn set_model(
        &self,
        turn: &[Block],
        thread_id: &str,
        model: &str,
        settings: &ModelSettings,
        cx: &mut App,
    ) {
        if let Some(threads) = SideThreads::try_global(cx) {
            threads.btw_set_model(
                &self.session_id,
                turn,
                thread_id,
                model,
                settings.clone(),
                cx,
            );
        }
    }
    fn composer_host(&self) -> Rc<dyn ComposerHost> {
        self.composer.clone()
    }
}

pub struct SessionOrchestration;

impl OrchestrationRuns for SessionOrchestration {
    fn runs(&self, cx: &App) -> Vec<OrchestrationRunView> {
        if Orchestration::try_global(cx).is_none() {
            return Vec::new();
        }
        Orchestration::orchestrator(cx)
            .read(cx)
            .snapshot()
            .iter()
            .filter_map(|run| {
                serde_json::to_value(run.as_ref())
                    .ok()
                    .and_then(|value| serde_json::from_value(value).ok())
            })
            .collect()
    }
    fn observe(&self, on_change: Box<dyn Fn(&mut App)>, cx: &mut App) -> Subscription {
        cx.observe(&Orchestration::orchestrator(cx), move |_, cx| on_change(cx))
    }
    fn hydrate(&self, lead_id: &str, cx: &mut App) -> Task<Result<(), String>> {
        Orchestration::hydrate(lead_id, cx)
    }
    fn resume_blocker(&self, lead_id: &str, cx: &App) -> Option<ResumeBlocker> {
        let blocked = Orchestration::orchestrator(cx)
            .read(cx)
            .resume_blocker(lead_id, None, cx)?;
        Some(ResumeBlocker {
            id: blocked.id,
            title: blocked.title,
        })
    }
    fn resume_lead_busy(&self, lead_id: &str, cx: &App) -> bool {
        Orchestration::orchestrator(cx)
            .read(cx)
            .resume_lead_busy(lead_id, cx)
    }
    fn cancel_task(&self, lead_id: &str, task_id: &str, cx: &mut App) -> Task<Result<(), String>> {
        let weak = Orchestration::orchestrator(cx).downgrade();
        let (lead, task) = (lead_id.to_string(), task_id.to_string());
        cx.spawn(async move |cx| orchestrator::cancel_task(&weak, &lead, &task, cx).await)
    }
    fn start(
        &self,
        lead_id: &str,
        harnesses: &[HarnessId],
        max_workers: i64,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        Orchestration::resume(lead_id, harnesses.to_vec(), max_workers, cx)
    }
}

impl OrchestrationActions for SessionOrchestration {
    fn update(&self, lead_id: &str, block_id: &str, proposal: OrchestrationProposal, cx: &mut App) {
        if let Err(error) = package::update_orchestration_card(lead_id, block_id, &proposal, cx) {
            monocode_app::bridge::dialogs::alert(&error, true, cx);
        }
    }
    fn confirm(&self, lead_id: &str, block_id: &str, cx: &mut App) -> Task<Result<(), String>> {
        package::confirm_orchestration_card(lead_id, block_id, cx)
    }
    fn retry(&self, lead_id: &str, block_id: &str, cx: &mut App) {
        package::retry_orchestration_card(lead_id, block_id, cx);
    }
    fn open(&self, session_id: &str, cx: &mut App) {
        if let Some(workspace) = ActiveWorkspace::get(cx).and_then(|weak| weak.upgrade()) {
            workspace
                .update(cx, |workspace, cx| workspace.open_session(session_id, cx))
                .detach();
        }
    }
    fn can_open_agents(&self) -> bool {
        true
    }
    fn open_agents(&self, workers: Vec<OrchestrationWorkerDetail>, cx: &mut App) {
        for worker in workers {
            self.open(&worker.session_id, cx);
        }
    }
}

struct LiveModelMenuSource {
    catalog: monocode_harness::core::catalog::SharedCatalog,
    prefs: monocode_settings::Kv,
    availability: monocode_harness::HarnessAvailabilityStore,
    registry: monocode_harness::HarnessRegistry,
    probe: monocode_harness::HarnessAvailabilityProbe,
}

impl monocode_view_transcript::threads::ModelMenuSource for LiveModelMenuSource {
    fn models_for(&self, harness: HarnessId) -> Vec<monocode_core::models::AgentModel> {
        self.catalog.read().models_for(harness).to_vec()
    }
    fn has_live_catalog(&self, harness: HarnessId) -> bool {
        self.catalog.has_live_catalog(harness)
    }
    fn available(&self, harness: HarnessId) -> bool {
        self.availability.is_harness_available(harness)
    }
    fn probed(&self) -> bool {
        self.availability.has_probed_harness_availability()
    }
    fn visible(&self, harness: HarnessId) -> bool {
        monocode_core::models::ModelPrefs::from_local_storage(|key| self.prefs.get_item(key))
            .is_picker_provider_visible(harness)
    }
    fn preferred_model_id(&self, harness: HarnessId) -> String {
        let prefs =
            monocode_core::models::ModelPrefs::from_local_storage(|key| self.prefs.get_item(key));
        let availability = self.availability.snapshot();
        let projects = monocode_core::project_providers::ProjectProviders::parse(
            self.prefs
                .get_item(monocode_core::project_providers::PROJECT_PROVIDER_SETTINGS_KEY)
                .as_deref(),
        );
        monocode_core::models::ModelEnv {
            catalog: &self.catalog.read(),
            prefs: &prefs,
            availability: &availability,
            projects: &projects,
        }
        .preferred_model_id(harness)
    }
    fn merge_model_settings(
        &self,
        model: &monocode_core::models::AgentModel,
        current: &ModelSettings,
    ) -> ModelSettings {
        self.catalog
            .read()
            .merge_model_settings(model, Some(current))
    }
    fn probe_availability(&self) {
        let probe = self.probe.clone();
        self.registry.spawner().spawn(Box::pin(async move {
            probe.probe_harness_availability(false).await;
        }));
    }
    fn refresh_catalogs(&self, harnesses: &[HarnessId]) {
        let (registry, catalog, harnesses) = (
            self.registry.clone(),
            self.catalog.clone(),
            harnesses.to_vec(),
        );
        self.registry.spawner().spawn(Box::pin(async move {
            registry
                .refresh_harness_catalogs(harnesses, false, |id| catalog.has_live_catalog(id))
                .await;
        }));
    }
}

pub fn model_menu_source(
    cx: &App,
) -> Option<Rc<dyn monocode_view_transcript::threads::ModelMenuSource>> {
    let services = AppServices::try_global(cx)?;
    Some(Rc::new(LiveModelMenuSource {
        catalog: services.catalog.clone(),
        prefs: services.kv.clone(),
        availability: services.availability.clone(),
        registry: services.registry.clone(),
        probe: monocode_harness::HarnessAvailabilityProbe::new(
            services.registry.clone(),
            services.children.clone(),
            services.availability.clone(),
        ),
    }))
}

pub fn pick_side_model(
    session: &monocode_core::Session,
    turn_id: &str,
    target: &ModelTarget,
    handoff: bool,
    cx: &mut App,
) {
    let Some(threads) = SideThreads::try_global(cx) else {
        return;
    };
    let managed = session.orchestration_lead_id.is_some();
    let Some(turn) = btw::find_turn(&session.blocks, turn_id, managed) else {
        return;
    };
    if handoff {
        threads.handoff(&session.id, target, &turn, cx);
    } else {
        threads.second_opinion(&session.id, target, &turn, cx);
    }
}
