//! The harness bridge: the runtime's `HarnessHooks` and attention's
//! `ApprovalRouter` over the harness registry. Port of the registry calls
//! App.tsx made directly (`bindHarnessSession`, `forgetHarnessSession`,
//! `respondHarnessApproval`, and the others), which the engine reaches
//! through these hooks.

pub mod dialogs;
pub mod peers;
pub mod projects;
pub mod shell;

use std::rc::Rc;
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

pub use peers::{AppOrchestrationPeers, AppWorkspaceDelegate};

/// The window label the engine packages key per-window state by (reminder
/// delivery, quick launches). The app has one main window.
pub const WINDOW_LABEL: &str = "main";

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
            let data_dir = crate::boot::AppServices::try_global(cx)
                .map(|services| services.data_dir.path.clone());
            return cx.background_spawn(async move {
                smol::unblock(move || recover_provider_transcript(session, data_dir.as_deref()))
                    .await
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

fn recover_provider_transcript(
    mut session: Session,
    data_dir: Option<&std::path::Path>,
) -> RecoveredSession {
    use monocode_engine::runtime::session_store::{
        backfill_claude_shell_commands, claude_shell_placeholder_ids,
    };
    use monocode_harness::providers::pi::interjections::{
        backfill_omp_interjections, omp_status_split_texts,
    };
    let mut persist = false;
    if let Some(provider_id) = session.provider_session_id.clone() {
        match session.harness {
            HarnessId::Claude => {
                let ids = claude_shell_placeholder_ids(&session);
                if let Some(data_dir) = data_dir
                    && !ids.is_empty()
                    && let Ok(commands) = monocode_git::fs::claude_shell_commands(
                        data_dir,
                        provider_id,
                        session.provider_account_id.clone(),
                        ids,
                    )
                    && let Some(blocks) = backfill_claude_shell_commands(&session.blocks, &commands)
                {
                    session.blocks = blocks;
                    persist = true;
                }
            }
            HarnessId::Omp => {
                let anchors: Vec<
                    monocode_harness::providers::pi::interjections::OmpInterjectionAnchor,
                > = monocode_git::fs::omp_session_interjections(provider_id.clone())
                    .ok()
                    .and_then(|anchors| serde_json::to_value(anchors).ok())
                    .and_then(|anchors| serde_json::from_value(anchors).ok())
                    .unwrap_or_default();
                let texts: Vec<monocode_harness::providers::pi::interjections::OmpAssistantText> =
                    if omp_status_split_texts(&session.blocks).is_empty() {
                        Vec::new()
                    } else {
                        monocode_git::fs::omp_active_assistant_texts(provider_id)
                            .ok()
                            .and_then(|texts| serde_json::to_value(texts).ok())
                            .and_then(|texts| serde_json::from_value(texts).ok())
                            .unwrap_or_default()
                    };
                if let std::borrow::Cow::Owned(blocks) =
                    backfill_omp_interjections(&session.blocks, &anchors, &texts)
                {
                    session.blocks = blocks;
                    persist = true;
                }
            }
            _ => {}
        }
    }
    RecoveredSession { session, persist }
}

/// Route approval and question answers to local providers or their paired
/// host. Orchestration requests focus their owning conversation.
pub struct AppApprovalRouter {
    pub registry: HarnessRegistry,
}

impl ApprovalRouter for AppApprovalRouter {
    fn is_remote(&self, session: &Session, cx: &App) -> bool {
        monocode_engine::remote::RemoteGlobal::is_remote_session(session, cx)
    }

    fn remote_approve(
        &self,
        session_id: &str,
        request_id: i64,
        decision: ApprovalDecision,
        cx: &mut App,
    ) {
        monocode_engine::remote::RemoteGlobal::approve(session_id, request_id, decision, cx);
    }

    fn remote_answer(
        &self,
        session_id: &str,
        request_id: i64,
        reply: &UserQuestionReply,
        cx: &mut App,
    ) {
        monocode_engine::remote::RemoteGlobal::answer(session_id, request_id, reply, cx);
    }

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

    fn orchestration_lead_for(&self, session_id: &str, cx: &App) -> Option<String> {
        monocode_engine::orchestration::Orchestration::try_global(cx)?;
        let run = monocode_engine::orchestration::Orchestration::orchestrator(cx)
            .read(cx)
            .for_session(session_id)?;
        Some(run.lead_id.clone())
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

/// Fill in the hooks between engine packages. Call once every package has
/// started.
pub fn install_peers(cx: &mut App) {
    use monocode_engine::automations::AutomationsPackage;
    use monocode_engine::history::HistoryPackage;
    use monocode_engine::inbox::inbox::Inbox;
    use monocode_engine::projects::ProjectsGlobal;
    use monocode_engine::side_threads::SideThreads;
    use monocode_engine::submit::Submit;

    ProjectsGlobal::set_hooks(cx, Rc::new(peers::AppProjectsHooks));
    if let Some(package) = HistoryPackage::try_global(cx) {
        let history = package.history.clone();
        history.update(cx, |history, _| {
            history.set_host(Rc::new(peers::AppHistoryHost))
        });
    }
    if let Some(inbox) = Inbox::try_global(cx) {
        inbox.update(cx, |inbox, _| {
            inbox.set_hooks(Rc::new(peers::AppInboxHooks))
        });
    }
    if let Some(submit) = Submit::try_global(cx) {
        let hooks = Rc::new(peers::AppSubmitPeers);
        submit.update(cx, |submit, _| {
            submit.set_peers(|peers| {
                peers.projects = hooks.clone();
                peers.attention = hooks.clone();
                peers.inbox = hooks.clone();
                peers.prompt = hooks.clone();
                peers.history = hooks;
                peers.remote = Rc::new(monocode_engine::remote::peers::SubmitRemote);
            })
        });
    }
    if cx.has_global::<SideThreads>() {
        SideThreads::global(cx).set_peers(Rc::new(peers::AppSideThreadPeers));
    }
    if let Some(package) = AutomationsPackage::try_global(cx) {
        let (automations, reminders, quick_launch) = (
            package.automations.clone(),
            package.reminders.clone(),
            package.quick_launch.clone(),
        );
        let host = Rc::new(peers::AppLaunchHost);
        automations.update(cx, |automations, _| automations.set_host(host.clone()));
        reminders.update(cx, |reminders, cx| {
            reminders.set_app(Rc::new(peers::AppReminderApp));
            reminders.attach_window(WINDOW_LABEL, host.clone(), cx);
        });
        quick_launch.update(cx, |quick_launch, cx| {
            quick_launch.attach_window(WINDOW_LABEL, host, cx)
        });
    }
}

/// A notification banner was clicked. Reminder banners carry
/// `reminder:<session id>:<due at>`; the others carry a session id.
pub fn notification_clicked(identifier: &str, cx: &mut App) {
    use monocode_engine::attention::Attention;
    use monocode_engine::automations::AutomationsPackage;

    peers::bring_forward(cx);
    if identifier.starts_with("reminder:") {
        if let Some(package) = AutomationsPackage::try_global(cx) {
            let reminders = package.reminders.clone();
            reminders.update(cx, |reminders, cx| {
                reminders.open_from_notification(identifier, cx)
            });
        }
        return;
    }
    if let Some(attention) = Attention::try_global(cx) {
        let notifier = attention.notifier.clone();
        notifier.update(cx, |notifier, cx| {
            notifier.notification_clicked(identifier, cx)
        });
    }
}

#[cfg(test)]
mod transcript_recovery_tests {
    use super::*;
    use monocode_core::block::{Block, BlockRole, BlockTool};

    #[test]
    fn restores_claude_commands_from_the_selected_account_and_marks_the_session_for_save() {
        let data_dir = std::env::temp_dir().join(format!(
            "monocode-transcript-repair-{}",
            uuid::Uuid::new_v4()
        ));
        let profile =
            monocode_process::harness::provider_account_path(&data_dir, "claude", "account-one")
                .unwrap();
        let project = profile.join("projects").join("test-project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("provider-one.jsonl"),serde_json::json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"tool-one","name":"Bash","input":{"command":"echo hello"}}]}}).to_string()).unwrap();
        let mut session = Session::blank(
            "session-one",
            HarnessId::Claude,
            "claude-model",
            "/test-project",
        );
        session.provider_session_id = Some("provider-one".into());
        session.provider_account_id = Some("account-one".into());
        session.blocks.push(Block {
            tool: Some(BlockTool {
                call_id: Some("tool-one".into()),
                kind: Some("execute".into()),
                detail: Some("hello".into()),
                ..Default::default()
            }),
            ..Block::new("block-one", BlockRole::Tool, "Shell")
        });
        let recovered = recover_provider_transcript(session, Some(&data_dir));
        assert!(recovered.persist);
        assert_eq!(recovered.session.blocks[0].text, "echo hello");
        assert_eq!(
            recovered.session.blocks[0]
                .tool
                .as_ref()
                .unwrap()
                .detail
                .as_deref(),
            Some("hello")
        );
        let again = recover_provider_transcript(recovered.session, Some(&data_dir));
        assert!(!again.persist);
        std::fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn missing_provider_logs_leave_the_stored_session_unchanged() {
        let mut session = Session::blank("s", HarnessId::Claude, "claude-model", "/test-project");
        session.provider_session_id = Some("missing-provider".into());
        session.provider_account_id = Some("missing-account".into());
        session
            .blocks
            .push(Block::new("b", BlockRole::Assistant, "Saved answer"));
        let recovered = recover_provider_transcript(
            session.clone(),
            Some(std::path::Path::new("/missing-data")),
        );
        assert!(!recovered.persist);
        assert_eq!(recovered.session, session);
    }
}
