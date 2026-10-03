//! The harness bridge: the runtime's `HarnessHooks` and attention's
//! `ApprovalRouter` over the harness registry. Port of the registry calls
//! App.tsx made directly (`bindHarnessSession`, `forgetHarnessSession`,
//! `respondHarnessApproval`, and the others), which the engine reaches
//! through these hooks.

use std::sync::Arc;

use gpui::{App, AppContext as _, Global, Task, WeakEntity};
use monocode_core::harness_event::{ApprovalDecision, HarnessEvent};
use monocode_core::session::session_work_cwd;
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{HarnessId, ModelSettings, Session};
use monocode_engine::attention::ApprovalRouter;
use monocode_engine::runtime::{HarnessHooks, RecoveredSession};
use monocode_engine::workspace::Workspace;
use monocode_harness::core::catalog::SharedCatalog;
use monocode_harness::providers::cursor;
use monocode_harness::{HarnessAvailabilityProbe, HarnessRegistry};
use monocode_process::harness::HarnessHost;

/// The window's workspace, for the approval router's "open the session"
/// calls. The window sets it when it opens.
#[derive(Default)]
pub struct ActiveWorkspace(pub Option<WeakEntity<Workspace>>);

impl Global for ActiveWorkspace {}

impl ActiveWorkspace {
    pub fn set(workspace: WeakEntity<Workspace>, cx: &mut App) {
        cx.set_global(ActiveWorkspace(Some(workspace)));
    }

    pub fn get(cx: &App) -> Option<WeakEntity<Workspace>> {
        cx.try_global::<ActiveWorkspace>()
            .and_then(|active| active.0.clone())
    }
}

/// `HarnessHooks` over the registry.
pub struct AppHarnessHooks {
    pub registry: HarnessRegistry,
    pub catalog: SharedCatalog,
    pub host: HarnessHost,
    pub probe: HarnessAvailabilityProbe,
    pub cursor_store: Arc<dyn cursor::CursorStore>,
}

impl HarnessHooks for AppHarnessHooks {
    fn is_live_harness(&self, harness: HarnessId) -> bool {
        self.registry.is_live_harness(harness)
    }

    fn bind_session(&self, session: &Session, _cx: &mut App) {
        let Some(provider_session_id) = session.provider_session_id.as_deref() else {
            return;
        };
        self.registry.bind_harness_session(
            session.harness,
            &session.id,
            provider_session_id,
            session_work_cwd(session),
            session.provider_account_id.as_deref(),
            Some(&session.blocks),
        );
    }

    fn forget_session(&self, harness: HarnessId, session_id: &str, cx: &mut App) -> Task<()> {
        let registry = self.registry.clone();
        let session_id = session_id.to_string();
        cx.background_spawn(async move {
            if let Err(error) = registry.forget_harness_session(harness, &session_id).await {
                log::debug!("[monocode] forget {harness} {session_id}: {error:#}");
            }
        })
    }

    fn cancel_turn(&self, harness: HarnessId, session_id: &str, cx: &mut App) -> Task<()> {
        let registry = self.registry.clone();
        let session_id = session_id.to_string();
        cx.background_spawn(async move {
            if let Err(error) = registry.cancel_harness_turn(harness, &session_id).await {
                log::debug!("[monocode] cancel {harness} {session_id}: {error:#}");
            }
        })
    }

    fn kill_all_children(&self, cx: &mut App) -> Task<()> {
        let host = self.host.clone();
        cx.background_spawn(async move {
            smol::unblock(move || host.kill_all()).await;
        })
    }

    fn recover_loaded_session(&self, session: Session, cx: &mut App) -> Task<RecoveredSession> {
        if session.harness != HarnessId::Cursor {
            return Task::ready(RecoveredSession {
                session,
                persist: false,
            });
        }
        // `recoverCursorSubagents`: Cursor's own store has the subagent
        // steps its ACP events leave out.
        let store = self.cursor_store.clone();
        cx.background_spawn(async move {
            let before = serde_json::to_value(&session.blocks).ok();
            let reader = cursor::store::StoreReader::new(store, false);
            let session = cursor::recover_cursor_subagents(session, &reader, |session, event| {
                monocode_core::reducer::apply_harness_event(&session, event)
            })
            .await;
            let persist = serde_json::to_value(&session.blocks).ok() != before;
            RecoveredSession { session, persist }
        })
    }

    fn probe_availability(&self, cx: &mut App) {
        let probe = self.probe.probe_harness_availability(false);
        cx.background_spawn(probe).detach();
    }

    fn refresh_catalogs(&self, harnesses: Vec<HarnessId>, cx: &mut App) -> Task<()> {
        let registry = self.registry.clone();
        let catalog = self.catalog.clone();
        cx.background_spawn(async move {
            registry
                .refresh_harness_catalogs(harnesses, false, |id| catalog.has_live_catalog(id))
                .await;
        })
    }

    fn resolve_model(&self, session: &Session, _cx: &App) -> Option<(String, ModelSettings)> {
        let catalog = self.catalog.read();
        let resolved = catalog.resolve_model(session.harness, Some(&session.model));
        let settings = catalog.merge_model_settings(&resolved, Some(&session.model_settings));
        Some((resolved.id, settings))
    }
}

/// Where approval and question answers go: the local registry. Remote
/// projects and the orchestrator are not part of M1.
pub struct AppApprovalRouter {
    pub registry: HarnessRegistry,
}

impl ApprovalRouter for AppApprovalRouter {
    fn respond_approval(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
        decision: ApprovalDecision,
        _cx: &mut App,
    ) {
        self.registry
            .respond_harness_approval(harness, session_id, request_id, decision);
    }

    fn respond_question(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
        reply: &UserQuestionReply,
        _cx: &mut App,
    ) {
        self.registry
            .respond_harness_question(harness, session_id, request_id, reply.clone());
    }

    fn keep_question_open(
        &self,
        harness: HarnessId,
        session_id: &str,
        request_id: i64,
        _cx: &mut App,
    ) {
        self.registry
            .keep_harness_question_open(harness, session_id, request_id);
    }

    fn focus_open_session(&self, session_id: &str, cx: &mut App) -> bool {
        let Some(workspace) = ActiveWorkspace::get(cx).and_then(|weak| weak.upgrade()) else {
            return false;
        };
        workspace.update(cx, |workspace, cx| {
            workspace.focus_open_session(session_id, cx)
        })
    }

    fn open_history_session(&self, session_id: &str, cx: &mut App) {
        let Some(workspace) = ActiveWorkspace::get(cx).and_then(|weak| weak.upgrade()) else {
            return;
        };
        workspace
            .update(cx, |workspace, cx| workspace.open_session(session_id, cx))
            .detach();
    }
}

/// Harness events for a session, outside a turn: what the engine's
/// `enqueue_event` takes. Kept here so views do not reach into the reducer.
pub fn enqueue(session_id: &str, event: HarnessEvent, cx: &mut App) {
    monocode_engine::runtime::Engine::sessions(cx).update(cx, |sessions, cx| {
        sessions.enqueue_event(session_id, event, cx);
    });
}
