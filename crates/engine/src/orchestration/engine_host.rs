//! The orchestration worker flows from src/app/App.tsx (the object passed to
//! `orchestrator.bind`, lines 8629-8992): create, integrate, and clean up
//! worker checkouts, and submit, steer, answer, and stop worker turns.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, AppContext as _, AsyncApp, Task};
use monocode_core::harness_event::{ApprovalDecision, SteerTurnInput};
use monocode_core::models::ModelPrefs;
use monocode_core::paths::path_key;
use monocode_core::reducer::apply::append_steer_user;
use monocode_core::session::{new_session, session_work_cwd};
use monocode_core::user_question::UserQuestionReply;
use monocode_core::{HARNESSES, HarnessAvailability, ModelEnv, ProjectProviders, Session};
use monocode_process::control::{ControlHost, control_attach_worker, control_turn_finished};
use monocode_process::harness::{HarnessHost, harness_kill};

use super::host::{
    ChoiceModel, Done, HarnessChoice, OrchestrationHost, WorkerIntegration, WorkerPreparation,
};
use super::peers::OrchestrationPeers;
use super::state::{
    OrchestrationRun, OrchestrationTask, WorkspaceKind, WorkspacePolicy,
    orchestration_checkout_cwd, orchestration_project_cwd, workspace_identity,
};
use crate::history::History;
use crate::projects::actions::{
    create_orchestration_worktree, list_worktrees, remove_orchestration_branch,
    remove_orchestration_worktree,
};
use crate::projects::worktrees::{
    detach_session_worktree, detach_summary_worktree, orchestration_worktree_branch_name,
};
use crate::runtime::reducer::session_child_harnesses;
use crate::runtime::session_store::should_persist_session;
use crate::runtime::util::project_path::same_project_path;
use crate::runtime::{Engine, notify_review_changed};
use crate::submit::acceptance::submit_with_settlement;
use crate::submit::{OnSettled, Submit, SubmitConfig, SubmitOptions};

/// The app's `OrchestrationHost` over the engine.
pub struct EngineHost {
    /// The control server, for worker scratch directories and turn grants.
    pub control: Option<Arc<ControlHost>>,
    /// The harness process host, for `harness_kill`.
    pub harness_host: Option<HarnessHost>,
    /// The grant owner for workers.
    pub owner: String,
    pub peers: Rc<dyn OrchestrationPeers>,
}

fn submit_config(cx: &App) -> Option<SubmitConfig> {
    Submit::try_global(cx).map(|submit| submit.read(cx).config().clone())
}

fn open_session(id: &str, cx: &App) -> Option<Session> {
    Engine::sessions(cx).read(cx).get(id).cloned()
}

/// `stopHarnessSession` for every harness that may hold a child for this
/// session.
async fn stop_children(session: Option<Session>, cx: &mut AsyncApp) {
    let Some(session) = session else {
        return;
    };
    let Some(config) = cx.update(|cx| submit_config(cx)) else {
        return;
    };
    let stops = session_child_harnesses(&session)
        .into_iter()
        .map(|harness| {
            let registry = config.registry.clone();
            let id = session.id.clone();
            async move {
                if let Err(error) = registry.stop_harness_session(harness, &id).await {
                    log::debug!("[orchestration] stop {harness} {id}: {error:#}");
                }
            }
        });
    futures::future::join_all(stops).await;
}

impl EngineHost {
    /// `invoke("harness_kill")` then `invoke("control_turn_finished")`.
    fn kill(&self, id: &str, cx: &mut AsyncApp) -> Task<()> {
        let harness_host = self.harness_host.clone();
        let control = self.control.clone();
        let id = id.to_string();
        cx.background_spawn(async move {
            if let Some(host) = &harness_host
                && let Err(error) = harness_kill(host, id.clone())
            {
                log::debug!("[orchestration] kill {id}: {error}");
            }
            if let Some(control) = &control {
                control_turn_finished(control, id);
            }
        })
    }

    fn model_env_session(
        &self,
        task: &OrchestrationTask,
        project_cwd: &str,
        lead: &Session,
        cx: &App,
    ) -> Session {
        let config = submit_config(cx);
        let catalog = config
            .as_ref()
            .map(|config| config.catalog.snapshot())
            .unwrap_or_default();
        let prefs = config
            .as_ref()
            .map(|config| ModelPrefs::from_local_storage(|key| config.kv.get_item(key)))
            .unwrap_or_default();
        let availability = HarnessAvailability::default();
        let projects = ProjectProviders::default();
        let env = ModelEnv {
            catalog: &catalog,
            prefs: &prefs,
            availability: &availability,
            projects: &projects,
        };
        let mut fresh = new_session(
            &env,
            task.session_id.clone(),
            task.harness,
            project_cwd,
            Some(&task.model),
            Some(lead.runtime_mode),
            None,
        );
        if let Some(settings) = &task.model_settings {
            let model = catalog.resolve_model(task.harness, Some(&task.model));
            fresh.model_settings = catalog.merge_model_settings(&model, Some(settings));
        }
        fresh
    }
}

async fn create_worker(
    host: Rc<EngineHost>,
    run: OrchestrationRun,
    task: OrchestrationTask,
    cx: &mut AsyncApp,
) -> Result<WorkerPreparation, String> {
    let project_cwd = orchestration_project_cwd(&run);
    let lead_checkout = orchestration_checkout_cwd(&run);
    let Some(lead) = cx.update(|cx| open_session(&run.lead_id, cx)) else {
        return Err("Lead session is unavailable".into());
    };
    let workspace = if task.workspace_policy == Some(WorkspacePolicy::Shared) {
        workspace_identity(&project_cwd, &lead_checkout, None)
    } else if let Some(retained) = &task.workspace {
        let listed = cx.update(|cx| list_worktrees(&lead_checkout, cx)).await?;
        let Some(tree) = listed
            .worktrees
            .iter()
            .find(|entry| path_key(&entry.path) == path_key(&retained.checkout_cwd))
        else {
            return Err("This worker's retained worktree is missing. Its saved changes cannot be retried automatically.".into());
        };
        workspace_identity(
            &project_cwd,
            &tree.path,
            tree.branch.as_deref().or(retained.branch.as_deref()),
        )
    } else {
        let branch = orchestration_worktree_branch_name(&task.id);
        let tree = cx
            .update(|cx| create_orchestration_worktree(&lead_checkout, &branch, cx))
            .await?;
        workspace_identity(&project_cwd, &tree.path, tree.branch.as_deref())
    };
    let checkout = workspace.checkout_cwd.clone();
    let scratch_dir = match &host.control {
        Some(control) => {
            let (control, owner, lead_id, session_id) = (
                control.clone(),
                host.owner.clone(),
                run.lead_id.clone(),
                task.session_id.clone(),
            );
            Some(
                cx.background_spawn(async move {
                    control_attach_worker(&control, &owner, lead_id, session_id)
                })
                .await?,
            )
        }
        None => None,
    };
    let shared_checkout = same_project_path(&project_cwd, &checkout);
    let sessions = cx.update(|cx| Engine::sessions(cx));
    let writer = cx.update(|cx| Engine::writer(cx));
    if let Some(existing) = cx.update(|cx| open_session(&task.session_id, cx)) {
        if existing.harness != task.harness
            || existing.model != task.model
            || !same_project_path(&existing.cwd, &project_cwd)
            || (existing.worktree_removed != Some(true)
                && !same_project_path(session_work_cwd(&existing), &checkout))
        {
            return Err("This worker's configuration changed. Restore its approved harness, model and project before retrying.".into());
        }
        // The lead's runtime mode governs its agents, including across a
        // change mid-run: auto stays auto, supervised asks the lead.
        let mut synced = existing;
        synced.cwd = project_cwd.clone();
        synced.worktree_cwd = (!shared_checkout).then(|| checkout.clone());
        synced.branch = workspace.branch.clone();
        synced.worktree_removed = Some(false);
        synced.runtime_mode = lead.runtime_mode;
        synced.orchestration_lead_id = Some(run.lead_id.clone());
        cx.update(|_| writer.upsert_session(&synced)).await?;
        sessions.update(cx, |sessions, cx| sessions.upsert(synced, cx));
        return Ok(WorkerPreparation {
            scratch_dir,
            workspace,
        });
    }
    let restored = cx
        .update(|cx| crate::runtime::sessions::get_stored_session(&task.session_id, cx))
        .await;
    if restored
        .as_ref()
        .is_some_and(|restored| restored.harness != task.harness || restored.model != task.model)
    {
        return Err(
            "The saved worker no longer matches its approved model. Create a new assignment."
                .into(),
        );
    }
    let mut worker = match restored {
        Some(mut restored) => {
            restored.busy = Some(false);
            restored.cwd = project_cwd.clone();
            restored.worktree_cwd = (!shared_checkout).then(|| checkout.clone());
            restored.branch = if shared_checkout {
                None
            } else {
                workspace.branch.clone()
            };
            restored.worktree_removed = Some(false);
            restored.runtime_mode = lead.runtime_mode;
            restored
        }
        None => {
            let mut fresh = cx.update(|cx| host.model_env_session(&task, &project_cwd, &lead, cx));
            if !shared_checkout {
                fresh.worktree_cwd = Some(checkout.clone());
                fresh.branch = workspace.branch.clone();
            }
            fresh.id = task.session_id.clone();
            fresh.title = task.title.clone();
            fresh
        }
    };
    worker.orchestration_lead_id = Some(run.lead_id.clone());
    if let Some(provider) = worker
        .provider_session_id
        .clone()
        .filter(|id| !id.is_empty())
        && let Some(config) = cx.update(|cx| submit_config(cx))
    {
        config.registry.bind_harness_session(
            worker.harness,
            &worker.id,
            &provider,
            session_work_cwd(&worker),
            worker.provider_account_id.as_deref(),
            Some(&worker.blocks),
        );
    }
    cx.update(|_| writer.upsert_session(&worker)).await?;
    // Workers belong to the lead's agent panel; no workspace tab is created.
    sessions.update(cx, |sessions, cx| sessions.upsert(worker, cx));
    Ok(WorkerPreparation {
        scratch_dir,
        workspace,
    })
}

async fn integrate_worker(
    host: Rc<EngineHost>,
    run: OrchestrationRun,
    task: OrchestrationTask,
    cx: &mut AsyncApp,
) -> Result<WorkerIntegration, String> {
    let Some(from_cwd) = task.workspace.as_ref().map(|w| w.checkout_cwd.clone()) else {
        return Err("This worker's isolated checkout is unavailable".into());
    };
    let session = cx.update(|cx| open_session(&task.session_id, cx));
    stop_children(session, cx).await;
    host.kill(&task.session_id, cx).await;
    let checkpoints = cx.update(|cx| Engine::checkpoints(cx));
    checkpoints.flush_session_checkpoint(&task.session_id).await;
    let lead_checkout = orchestration_checkout_cwd(&run);
    let listed = cx.update(|cx| list_worktrees(&lead_checkout, cx)).await?;
    let worker_tree = listed
        .worktrees
        .iter()
        .find(|tree| same_project_path(&tree.path, &from_cwd));
    let lead_tree = listed
        .worktrees
        .iter()
        .find(|tree| same_project_path(&tree.path, &lead_checkout));
    let (Some(worker_tree), Some(lead_tree)) = (worker_tree, lead_tree) else {
        return Err(
            "The worker or lead checkout is no longer registered. The worker worktree was kept."
                .into(),
        );
    };
    if worker_tree.head != lead_tree.head {
        return Err("The worker or lead branch moved while this task was running. The worker worktree was kept for manual review.".into());
    }
    let applied = checkpoints
        .apply(&task.session_id, &from_cwd, &lead_checkout)
        .await?;
    Ok(WorkerIntegration {
        files: applied.files,
        already_applied: applied.already_applied as i64,
    })
}

async fn cleanup_worker(
    host: Rc<EngineHost>,
    run: OrchestrationRun,
    task: OrchestrationTask,
    only_if_unchanged: bool,
    cx: &mut AsyncApp,
) -> Result<bool, String> {
    let Some(workspace) = task
        .workspace
        .clone()
        .filter(|workspace| workspace.kind == WorkspaceKind::Worktree)
    else {
        return Ok(true);
    };
    let path = workspace.checkout_cwd.clone();
    let lead_checkout = orchestration_checkout_cwd(&run);
    let checkpoints = cx.update(|cx| Engine::checkpoints(cx));
    checkpoints.flush_session_checkpoint(&task.session_id).await;
    let listed = cx.update(|cx| list_worktrees(&lead_checkout, cx)).await?;
    let worker_tree = listed
        .worktrees
        .iter()
        .find(|tree| path_key(&tree.path) == path_key(&path));
    let exists = worker_tree.is_some();
    if !exists && only_if_unchanged {
        return Ok(false);
    }
    let lead_tree = listed
        .worktrees
        .iter()
        .find(|tree| same_project_path(&tree.path, &lead_checkout));
    let moved = match (worker_tree, lead_tree) {
        (Some(worker), Some(lead)) => worker.head != lead.head,
        _ => true,
    };
    if exists && moved {
        if only_if_unchanged {
            return Ok(false);
        }
        return Err("The worker or lead branch moved before cleanup. The worker worktree was kept for manual review.".into());
    }
    if only_if_unchanged {
        if !checkpoints.cleanup_safe(&task.session_id, &path).await? {
            return Ok(false);
        }
    } else if exists {
        // Re-verify immediately before destructive cleanup. The operation is
        // idempotent, so this also finishes a partially applied integration.
        checkpoints
            .apply(&task.session_id, &path, &lead_checkout)
            .await?;
    }
    let sessions = cx.update(|cx| Engine::sessions(cx));
    if exists {
        cx.update(|cx| {
            if let Some(submit) = Submit::try_global(cx) {
                submit.update(cx, |submit, cx| submit.stop(&task.session_id, true, cx));
            }
        });
        let session = cx.update(|cx| open_session(&task.session_id, cx));
        stop_children(session.clone(), cx).await;
        host.kill(&task.session_id, cx).await;
        cx.update(|cx| Engine::writer(cx).flush_session_writes())
            .await;
        cx.update(|cx| host.peers.check_open_worktree_files(&path, cx));
        let removed = cx
            .update(|cx| remove_orchestration_worktree(&lead_checkout, &path, cx))
            .await?;
        let affected: HashSet<String> = std::iter::once(task.session_id.clone())
            .chain(removed.session_ids.iter().cloned())
            .collect();
        let project_cwd = removed.project_cwd.clone();
        sessions.update(cx, |sessions, cx| {
            sessions.update_all(cx, |session| {
                affected
                    .contains(&session.id)
                    .then(|| detach_session_worktree(session, &project_cwd, &path))
            });
        });
        cx.update(|cx| {
            if let Some(history) = cx.try_global::<crate::history::HistoryPackage>() {
                let history: gpui::Entity<History> = history.history.clone();
                history.update(cx, |history, cx| {
                    history.patch_summaries(
                        &|row| {
                            affected
                                .contains(&row.id)
                                .then(|| detach_summary_worktree(row, &project_cwd, &path))
                        },
                        cx,
                    );
                });
            }
        });
        if let Some(session) = session
            && let Some(config) = cx.update(|cx| submit_config(cx))
        {
            for harness in session_child_harnesses(&session) {
                if let Err(error) = config
                    .registry
                    .forget_harness_session(harness, &session.id)
                    .await
                {
                    log::debug!("[orchestration] forget {harness}: {error:#}");
                }
            }
        }
    } else if let Some(detached) = cx
        .update(|cx| open_session(&task.session_id, cx))
        .filter(|session| session.worktree_removed != Some(true))
    {
        let next = detach_session_worktree(&detached, &orchestration_project_cwd(&run), &path);
        sessions.update(cx, |sessions, cx| sessions.upsert(next.clone(), cx));
        if should_persist_session(&next) {
            cx.update(|cx| Engine::writer(cx).upsert_session(&next))
                .await?;
        }
    }
    if let Some(branch) = &workspace.branch {
        cx.update(|cx| remove_orchestration_branch(&lead_checkout, branch, cx))
            .await?;
    }
    checkpoints.forget(&task.session_id).await?;
    cx.update(|cx| notify_review_changed(Some(&task.session_id), cx));
    Ok(true)
}

impl OrchestrationHost for Rc<EngineHost> {
    fn session(&self, id: &str, cx: &App) -> Option<Session> {
        open_session(id, cx)
    }

    fn sessions(&self, cx: &App) -> Vec<Session> {
        Engine::sessions(cx).read(cx).all().to_vec()
    }

    fn choices(&self, cx: &App) -> Vec<HarnessChoice> {
        let Some(config) = submit_config(cx) else {
            return Vec::new();
        };
        let catalog = config.catalog.snapshot();
        HARNESSES
            .into_iter()
            .filter(|harness| (config.is_harness_available)(*harness))
            .map(|harness| HarnessChoice {
                harness,
                models: catalog
                    .models_for(harness)
                    .iter()
                    .map(|model| ChoiceModel {
                        id: model.id.clone(),
                        name: model.name.clone(),
                    })
                    .collect(),
            })
            .collect()
    }

    fn create_worker(
        &self,
        run: &OrchestrationRun,
        task: &OrchestrationTask,
        cx: &mut App,
    ) -> Task<Result<WorkerPreparation, String>> {
        let (host, run, task) = (self.clone(), run.clone(), task.clone());
        cx.spawn(async move |cx| create_worker(host, run, task, cx).await)
    }

    fn integrate_worker(
        &self,
        run: &OrchestrationRun,
        task: &OrchestrationTask,
        cx: &mut App,
    ) -> Task<Result<WorkerIntegration, String>> {
        let (host, run, task) = (self.clone(), run.clone(), task.clone());
        cx.spawn(async move |cx| integrate_worker(host, run, task, cx).await)
    }

    fn cleanup_worker(
        &self,
        run: &OrchestrationRun,
        task: &OrchestrationTask,
        only_if_unchanged: bool,
        cx: &mut App,
    ) -> Task<Result<bool, String>> {
        let (host, run, task) = (self.clone(), run.clone(), task.clone());
        cx.spawn(async move |cx| cleanup_worker(host, run, task, only_if_unchanged, cx).await)
    }

    fn submit(&self, id: &str, text: &str, done: Done, cx: &mut App) {
        let (id, text) = (id.to_string(), text.to_string());
        let done = std::cell::RefCell::new(Some(done));
        let on_settled: OnSettled = Rc::new(move |outcome, cx| {
            if let Some(done) = done.borrow_mut().take() {
                done(outcome, cx);
            }
        });
        cx.spawn(async move |cx| {
            submit_with_settlement(
                cx,
                |on_settled, cx| match Submit::try_global(cx) {
                    Some(submit) => submit.update(cx, |submit, cx| {
                        submit.submit(
                            &id,
                            &text,
                            Vec::new(),
                            SubmitOptions {
                                managed: true,
                                on_settled: Some(on_settled),
                                ..Default::default()
                            },
                            cx,
                        )
                    }),
                    None => crate::submit::SubmissionAcceptance::Ready(false),
                },
                on_settled,
                "The selected agent session could not accept this turn.",
            )
            .await;
        })
        .detach();
    }

    fn stop(&self, id: &str, cx: &mut App) -> Task<Result<(), String>> {
        let session = open_session(id, cx);
        if let Some(submit) = Submit::try_global(cx) {
            submit.update(cx, |submit, cx| submit.stop(id, true, cx));
        }
        let host = self.clone();
        let id = id.to_string();
        cx.spawn(async move |cx| {
            stop_children(session, cx).await;
            // Also reap processes left behind before the session was restored.
            host.kill(&id, cx).await;
            Ok(())
        })
    }

    fn steer(&self, id: &str, text: &str, cx: &mut App) -> Task<Result<(), String>> {
        // Output that already arrived belongs before the guidance, and a
        // pending error can end the turn this checks for.
        Engine::sessions(cx).update(cx, |sessions, cx| sessions.flush(cx));
        let Some(session) = open_session(id, cx) else {
            return Task::ready(Err("This agent is no longer available".into()));
        };
        if !session.is_busy() {
            return Task::ready(Err(
                "This agent is not running a turn; send it a fresh one with message.".into(),
            ));
        }
        let Some(config) = submit_config(cx) else {
            return Task::ready(Err("This agent is no longer available".into()));
        };
        if !config.registry.is_live_harness(session.harness)
            || !config.registry.can_steer_harness(session.harness)
        {
            return Task::ready(Err(format!(
                "{} cannot take guidance mid-turn. Wait for the turn to finish, then use message.",
                session.harness
            )));
        }
        // Record it on the worker before dispatch, so its own transcript shows
        // why it changed course even if the harness call then fails.
        let catalog = config.catalog.snapshot();
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(id, cx, |entry| {
                *entry = append_steer_user(&catalog, entry, text, &[], None);
            });
        });
        let steer = config.registry.steer_harness_turn(
            session.harness,
            SteerTurnInput {
                session_id: id.to_string(),
                cwd: session_work_cwd(&session).to_string(),
                model: session.model.clone(),
                model_settings: Some(session.model_settings.clone()),
                text: text.to_string(),
                attachments: None,
            },
        );
        cx.background_spawn(async move { steer.await.map_err(|error| format!("{error:#}")) })
    }

    fn respond_approval(
        &self,
        id: &str,
        request_id: i64,
        decision: ApprovalDecision,
        cx: &mut App,
    ) {
        if let (Some(session), Some(config)) = (open_session(id, cx), submit_config(cx)) {
            config
                .registry
                .respond_harness_approval(session.harness, id, request_id, decision);
        }
    }

    fn answer_question(&self, id: &str, request_id: i64, reply: UserQuestionReply, cx: &mut App) {
        if let (Some(session), Some(config)) = (open_session(id, cx), submit_config(cx)) {
            config
                .registry
                .respond_harness_question(session.harness, id, request_id, reply);
        }
    }
}
