//! Port of the "local orchestration" cases in orchestration.test.ts, driven
//! through the `Orchestrator` entity with the fakes in `testing`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use futures::FutureExt;
use futures::channel::oneshot;
use gpui::{Entity, Task, TestAppContext};
use monocode_core::block::{BlockApproval, BlockRole, ToolPreview, ToolPreviewKind};
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::orchestration::OrchestrationProposal;
use monocode_core::user_question::{UserQuestionPrompt, UserQuestionReply};
use monocode_core::{Block, Extra, HarnessEvent, HarnessId};
use serde_json::{Value, json};

use super::host::{ChoiceModel, HarnessChoice};
use super::orchestrator::{
    Orchestrator, delete_session, handle, hydrate, start, start_approved, stop_run,
};
use super::state::{
    DispatchStage, DispatchState, OrchestrationRun, OrchestrationTask, RunStatus, TaskStatus,
    normalize_orchestration_run,
};
use super::testing::{FakeHost, FakeStore, Once, complete, finish, orchestrator, session};
use crate::submit::{ControlOutcome, ControlStatus};

struct F {
    o: Entity<Orchestrator>,
    store: Rc<FakeStore>,
    host: Rc<FakeHost>,
    request: Cell<u32>,
}

fn setup(cx: &mut TestAppContext) -> F {
    let store = FakeStore::new();
    let mut lead = session("lead", HarnessId::Claude, "/repo");
    lead.busy = Some(true);
    let host = FakeHost::new(vec![lead]);
    let o = orchestrator(store.clone(), host.clone(), cx);
    F {
        o,
        store,
        host,
        request: Cell::new(0),
    }
}

fn outcome(status: ControlStatus, text: &str, error: Option<&str>) -> ControlOutcome {
    ControlOutcome {
        status,
        text: text.into(),
        error: error.map(str::to_string),
    }
}

fn completed(text: &str) -> ControlOutcome {
    outcome(ControlStatus::Completed, text, None)
}

fn failed(text: &str, error: &str) -> ControlOutcome {
    outcome(ControlStatus::Failed, text, Some(error))
}

impl F {
    fn call_id(
        &self,
        cx: &mut TestAppContext,
        action: &str,
        input: Value,
        id: &str,
    ) -> Task<Result<Value, String>> {
        let weak = self.o.downgrade();
        let input = input.as_object().cloned().unwrap_or_default();
        let (action, id) = (action.to_string(), id.to_string());
        cx.spawn(|mut cx| async move { handle(&weak, "lead", &id, &action, &input, &mut cx).await })
    }

    fn call_id_now(
        &self,
        cx: &mut TestAppContext,
        action: &str,
        input: Value,
        id: &str,
    ) -> Result<Value, String> {
        let task = self.call_id(cx, action, input, id);
        finish(cx, task)
    }

    fn call_task(
        &self,
        cx: &mut TestAppContext,
        action: &str,
        input: Value,
    ) -> Task<Result<Value, String>> {
        self.request.set(self.request.get() + 1);
        let id = format!("request-{}", self.request.get());
        self.call_id(cx, action, input, &id)
    }

    fn call(&self, cx: &mut TestAppContext, action: &str, input: Value) -> Result<Value, String> {
        let task = self.call_task(cx, action, input);
        finish(cx, task)
    }

    fn start_with(
        &self,
        cx: &mut TestAppContext,
        harnesses: Vec<HarnessId>,
        workers: i64,
    ) -> Result<(), String> {
        let weak = self.o.downgrade();
        let task = cx.spawn(|mut cx| async move {
            start(&weak, "lead", &harnesses, workers, None, &mut cx).await
        });
        finish(cx, task)
    }

    fn start(&self, cx: &mut TestAppContext) {
        self.host.set_busy("lead", false);
        self.start_with(cx, vec![HarnessId::Codex], 2).unwrap();
        self.host.set_busy("lead", true);
    }

    fn start_approved(
        &self,
        cx: &mut TestAppContext,
        card: OrchestrationProposal,
    ) -> Result<(), String> {
        let task = self.start_approved_task(cx, card);
        finish(cx, task)
    }

    fn start_approved_task(
        &self,
        cx: &mut TestAppContext,
        card: OrchestrationProposal,
    ) -> Task<Result<(), String>> {
        let weak = self.o.downgrade();
        cx.spawn(
            |mut cx| async move { start_approved(&weak, "lead", "card", &card, &mut cx).await },
        )
    }

    fn delegate(
        &self,
        cx: &mut TestAppContext,
        files: &[&str],
        extra: Value,
    ) -> Result<Value, String> {
        let mut input = json!({
            "title": "Task",
            "prompt": "Implement the bounded change",
            "harness": "codex",
            "files": files,
        });
        for (key, value) in extra.as_object().cloned().unwrap_or_default() {
            input[key] = value;
        }
        self.call(cx, "delegate", input)
    }

    fn run(&self, cx: &mut TestAppContext) -> Option<Rc<OrchestrationRun>> {
        self.o.read_with(cx, |o, _| o.run("lead"))
    }

    fn tasks(&self, cx: &mut TestAppContext) -> Vec<OrchestrationTask> {
        self.run(cx)
            .map(|run| run.tasks.clone())
            .unwrap_or_default()
    }

    fn statuses(&self, cx: &mut TestAppContext) -> Vec<TaskStatus> {
        self.tasks(cx).iter().map(|task| task.status).collect()
    }

    fn complete(&self, cx: &mut TestAppContext, id: &str, result: ControlOutcome) {
        let done = self.host.take_completion(id).expect("a pending turn");
        complete(cx, done, result);
    }

    fn stop_run(&self, cx: &mut TestAppContext) {
        let weak = self.o.downgrade();
        let task = cx.spawn(|mut cx| async move { stop_run(&weak, "lead", &mut cx).await });
        finish(cx, task).unwrap();
    }

    fn observe(&self, cx: &mut TestAppContext, id: &str, event: HarnessEvent) {
        self.o.update(cx, |o, cx| o.observe(id, &event, cx));
    }
}

fn proposal() -> OrchestrationProposal {
    serde_json::from_value(json!({
        "version": 1,
        "leadId": "lead",
        "cwd": "/repo",
        "request": "Build settings",
        "author": { "harness": "claude", "model": "claude:test", "name": "Lead" },
        "settings": {
            "choices": [{ "harness": "codex", "model": "codex:test", "name": "Test" }],
            "maxWorkers": 2
        },
        "status": "ready",
        "title": "Settings",
        "summary": "Split the work",
        "tasks": [
            {
                "id": "ui", "title": "UI", "prompt": "User edited instructions",
                "harness": "codex", "model": "codex:test", "files": ["src/ui"], "dependsOn": ["types"]
            },
            {
                "id": "types", "title": "Types", "prompt": "Define the types",
                "harness": "codex", "model": "codex:test", "files": ["src/types"], "dependsOn": []
            }
        ]
    }))
    .unwrap()
}

fn write(path: &str) -> HarnessEvent {
    HarnessEvent::ToolStarted {
        agent_model: None,
        call_id: path.into(),
        title: "Edit".into(),
        kind: None,
        status: None,
        background: None,
        preview: Some(ToolPreview {
            path: Some(path.into()),
            ..ToolPreview::new(ToolPreviewKind::Write)
        }),
        paths: None,
    }
}

fn write_update(path: &str, status: &str) -> HarnessEvent {
    HarnessEvent::ToolUpdated {
        agent_model: None,
        call_id: path.into(),
        title: Some("Write".into()),
        kind: None,
        status: Some(status.into()),
        detail: None,
        preview: Some(ToolPreview {
            path: Some(path.into()),
            ..ToolPreview::new(ToolPreviewKind::Write)
        }),
        paths: None,
    }
}

fn approval(request_id: i64, text: &str) -> Block {
    Block {
        approval: Some(BlockApproval {
            request_id,
            decided: None,
            extra: Extra::new(),
        }),
        ..Block::new("ask", BlockRole::Approval, text)
    }
}

#[gpui::test]
fn stops_and_forgets_a_deleted_lead_without_persisting_it_again(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src"], json!({})).unwrap();
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Running);
    let task = f.tasks(cx)[0].clone();
    let removed = Rc::new(RefCell::new(Vec::new()));
    let (host, store, entity, async_cx) =
        (f.host.clone(), f.store.clone(), f.o.clone(), cx.to_async());
    let seen = removed.clone();
    let weak = f.o.downgrade();
    let deletion = cx.spawn(|mut cx| async move {
        delete_session(
            &weak,
            "lead",
            move || {
                let lead_busy = host.session_busy("lead");
                let status =
                    entity.read_with(&async_cx, |o, _| o.run("lead").map(|run| run.status));
                seen.borrow_mut().push((lead_busy, status));
                store.saved.borrow_mut().remove("lead");
                async { Ok(()) }
            },
            &mut cx,
        )
        .await
    });
    finish(cx, deletion).unwrap();
    assert_eq!(*removed.borrow(), vec![(false, Some(RunStatus::Stopped))]);
    assert!(f.host.stops.borrow().contains(&task.session_id));
    assert!(f.o.read_with(cx, |o, _| o.snapshot()).is_empty());
    f.store.saves.borrow_mut().clear();
    if let Some(done) = f.host.take_completion(&task.session_id) {
        complete(cx, done, completed("Late result"));
    }
    let weak = f.o.downgrade();
    finish(
        cx,
        cx.spawn(|mut cx| async move { hydrate(&weak, "lead", &mut cx).await }),
    )
    .unwrap();
    f.o.update(cx, |o, cx| o.sync(cx));
    cx.run_until_parked();
    assert!(f.o.read_with(cx, |o, _| o.snapshot()).is_empty());
    assert!(f.store.saves.borrow().is_empty());
}

#[gpui::test]
fn reloads_a_deleted_workers_pruned_run_and_preserves_state_if_deletion_fails(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    assert_eq!(task.status, TaskStatus::Running);
    let weak = f.o.downgrade();
    let id = task.session_id.clone();
    let failed_delete = cx.spawn(|mut cx| async move {
        delete_session(
            &weak,
            &id,
            || async { Err("Delete failed".to_string()) },
            &mut cx,
        )
        .await
    });
    assert!(
        finish(cx, failed_delete)
            .unwrap_err()
            .contains("Delete failed")
    );
    assert_eq!(f.tasks(cx).len(), 1);
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Stopped);
    let weak = f.o.downgrade();
    let id = task.session_id.clone();
    let store = f.store.clone();
    let deletion = cx.spawn(|mut cx| async move {
        delete_session(
            &weak,
            &id,
            move || {
                let mut saved = store.saved.borrow_mut();
                let run = saved.get_mut("lead").unwrap();
                run.tasks.clear();
                run.requests.clear();
                drop(saved);
                store.saves.borrow_mut().clear();
                async { Ok(()) }
            },
            &mut cx,
        )
        .await
    });
    finish(cx, deletion).unwrap();
    assert!(f.tasks(cx).is_empty());
    assert!(
        f.o.read_with(cx, |o, _| o.for_session(&task.session_id))
            .is_none()
    );
    assert!(f.store.saves.borrow().is_empty());
}

#[gpui::test]
fn starts_exactly_the_approved_assignments_and_preserves_forward_dependencies(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    let mut card = proposal();
    card.tasks[1].model_settings =
        Some([("reasoningEffort".to_string(), "xhigh".to_string())].into());
    assert_eq!(f.host.submit_count(), 0);
    f.start_approved(cx, card).unwrap();
    assert_eq!(f.host.created.borrow().len(), 1);
    assert_eq!(
        f.statuses(cx),
        vec![TaskStatus::Queued, TaskStatus::Running]
    );
    let tasks = f.tasks(cx);
    assert_eq!(tasks[0].prompt, "User edited instructions");
    assert_eq!(tasks[0].depends_on, vec![tasks[1].id.clone()]);
    assert_eq!(
        tasks[1].model_settings.as_ref().unwrap()["reasoningEffort"],
        "xhigh"
    );
    assert_eq!(
        f.host.created.borrow()[0].model_settings.as_ref().unwrap()["reasoningEffort"],
        "xhigh"
    );
    assert_eq!(f.run(cx).unwrap().proposal_id.as_deref(), Some("card"));
    assert_eq!(f.store.saved.borrow()["lead"].tasks.len(), 2);
    let submits = f.host.submits.borrow();
    let worker = submits
        .iter()
        .find(|(id, prompt)| id != "lead" && prompt.contains("<monocode_assignment>"))
        .unwrap();
    assert!(worker.1.contains("Define the types"));
    let lead = submits.iter().find(|(id, _)| id == "lead").unwrap();
    assert!(lead.1.contains("do not delegate duplicates"));
    assert!(
        lead.1
            .contains("\"modelSettings\":{\"reasoningEffort\":\"xhigh\"}")
    );
}

#[gpui::test]
fn names_the_conversation_that_blocks_a_paused_run_from_resuming(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    f.start_approved(cx, proposal()).unwrap();
    f.complete(cx, "lead", failed("", "Lead interrupted"));
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Paused);
    assert!(
        f.tasks(cx)
            .iter()
            .all(|task| !matches!(task.status, TaskStatus::Running | TaskStatus::Cancelling))
    );
    let mut investigation = session("investigation", HarnessId::Codex, "/repo");
    investigation.title = "Investigating the failure".into();
    investigation.busy = Some(true);
    f.host.sessions.borrow_mut().push(investigation);
    let blocker =
        f.o.read_with(cx, |o, cx| o.resume_blocker("lead", None, cx).map(|s| s.id));
    assert_eq!(blocker.as_deref(), Some("investigation"));
    assert_eq!(
        f.start_with(cx, vec![HarnessId::Codex], 2).unwrap_err(),
        "\"Investigating the failure\" is still running in this checkout. Stop it before resuming orchestration."
    );
}

#[gpui::test]
fn checks_the_new_checkout_rather_than_a_stopped_runs_old_checkout(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.stop_run(cx);
    f.host.with_session("lead", |lead| {
        lead.worktree_cwd = Some("/repo-worktrees/new".into())
    });
    let mut old = session("old-checkout-work", HarnessId::Codex, "/repo");
    old.title = "Old checkout work".into();
    old.busy = Some(true);
    f.host.sessions.borrow_mut().push(old);
    f.start_with(cx, vec![HarnessId::Codex], 2).unwrap();
    assert_eq!(
        f.run(cx).unwrap().workspace.as_ref().unwrap().checkout_cwd,
        "/repo-worktrees/new"
    );
}

#[gpui::test]
fn never_launches_a_partial_plan_when_one_assignment_has_invalid_scopes(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    f.store
        .scopes_once
        .borrow_mut()
        .push_back(Once::Fail("Scope escapes project".into()));
    assert!(
        f.start_approved(cx, proposal())
            .unwrap_err()
            .contains("Scope escapes")
    );
    assert!(f.store.enabled.borrow().is_empty());
    assert_eq!(f.host.submit_count(), 0);
}

#[gpui::test]
fn requires_a_ready_proposal_and_enforces_the_exact_model_pool_for_later_cli_calls(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    let mut planning = proposal();
    planning.status = monocode_core::orchestration::OrchestrationProposalStatus::Planning;
    assert!(
        f.start_approved(cx, planning)
            .unwrap_err()
            .contains("completed proposal")
    );
    *f.host.choices.borrow_mut() = vec![HarnessChoice {
        harness: HarnessId::Codex,
        models: vec![
            ChoiceModel {
                id: "codex:test".into(),
                name: "Test".into(),
            },
            ChoiceModel {
                id: "codex:extra".into(),
                name: "Unselected".into(),
            },
        ],
    }];
    f.start_approved(cx, proposal()).unwrap();
    let result = f.call(cx, "list", json!({})).unwrap();
    assert_eq!(
        result["harnesses"][0]["models"],
        json!([{ "id": "codex:test", "name": "Test" }])
    );
    assert!(
        f.delegate(cx, &["extra"], json!({ "model": "codex:extra" }))
            .unwrap_err()
            .contains("model ID returned by list")
    );
}

#[gpui::test]
fn does_not_duplicate_work_when_confirmation_is_repeated(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    let first = f.start_approved_task(cx, proposal());
    let second = f.start_approved_task(cx, proposal());
    cx.run_until_parked();
    let results = [
        first.now_or_never().unwrap(),
        second.now_or_never().unwrap(),
    ];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(f.store.enabled.borrow().len(), 1);
    assert_eq!(f.tasks(cx).len(), 2);
}

#[gpui::test]
fn ignores_unavailable_unused_catalog_models_but_blocks_an_unavailable_assignment(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    let mut card = proposal();
    card.settings.choices.push(
        serde_json::from_value(
            json!({ "harness": "claude", "model": "claude:removed", "name": "Removed" }),
        )
        .unwrap(),
    );
    f.start_approved(cx, card.clone()).unwrap();
    assert_eq!(f.run(cx).unwrap().allowed_harnesses, vec![HarnessId::Codex]);
    assert_eq!(
        f.run(cx).unwrap().allowed_models,
        Some(proposal().settings.choices)
    );

    let unavailable = setup(cx);
    unavailable.host.set_busy("lead", false);
    card.tasks[0].harness = HarnessId::Claude;
    card.tasks[0].model = "claude:removed".into();
    assert!(
        unavailable
            .start_approved(cx, card)
            .unwrap_err()
            .contains("An assigned model is no longer available")
    );
    assert!(unavailable.store.enabled.borrow().is_empty());
    assert_eq!(unavailable.host.submit_count(), 0);
}

#[gpui::test]
fn records_the_selected_worktree_separately_from_the_project_identity(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.host.with_session("lead", |lead| {
        lead.worktree_cwd = Some("/repo-worktrees/feature".into());
        lead.branch = Some("feature".into());
    });
    f.start(cx);
    let run = f.run(cx).unwrap();
    assert_eq!(run.version, 2);
    assert_eq!(run.cwd, "/repo");
    let workspace = run.workspace.clone().unwrap();
    assert_eq!(workspace.project_cwd, "/repo");
    assert_eq!(workspace.checkout_cwd, "/repo-worktrees/feature");
    assert_eq!(workspace.kind, super::state::WorkspaceKind::Worktree);
    assert_eq!(workspace.branch.as_deref(), Some("feature"));
    assert_eq!(
        *f.store.enabled.borrow(),
        vec![("lead".to_string(), "/repo-worktrees/feature".to_string())]
    );
    f.delegate(cx, &["src/a"], json!({})).unwrap();
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Running);
    assert_eq!(
        f.tasks(cx)[0].scopes,
        vec!["/repo-worktrees/feature/src/a".to_string()]
    );
    // The dispatch is persisted in the lead's checkout before the worker exists.
    let saves = f.store.saves.borrow();
    let first = saves
        .iter()
        .find(|run| !run.dispatch_list().is_empty())
        .unwrap();
    assert_eq!(first.dispatch_list()[0].workspace.project_cwd, "/repo");
    assert_eq!(
        first.dispatch_list()[0].workspace.checkout_cwd,
        "/repo-worktrees/feature"
    );
}

#[gpui::test]
fn starts_an_approved_proposal_only_in_the_checkout_it_inspected(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    f.host.with_session("lead", |lead| {
        lead.worktree_cwd = Some("/repo-worktrees/feature".into())
    });
    let mut card = proposal();
    card.checkout_cwd = Some("/repo-worktrees/feature".into());
    f.start_approved(cx, card.clone()).unwrap();
    assert_eq!(
        f.run(cx).unwrap().workspace.as_ref().unwrap().checkout_cwd,
        "/repo-worktrees/feature"
    );

    let moved = setup(cx);
    moved.host.set_busy("lead", false);
    moved.host.with_session("lead", |lead| {
        lead.worktree_cwd = Some("/repo-worktrees/other".into())
    });
    assert!(
        moved
            .start_approved(cx, card.clone())
            .unwrap_err()
            .contains("proposal's checkout")
    );

    let switched = setup(cx);
    switched.host.set_busy("lead", false);
    switched.host.with_session("lead", |lead| {
        lead.worktree_cwd = Some("/repo-worktrees/feature".into())
    });
    let sessions = switched.host.sessions.clone();
    *switched.store.scopes_impl.borrow_mut() = Box::new(move |cwd, files| {
        if let Some(lead) = sessions.borrow_mut().iter_mut().find(|s| s.id == "lead") {
            lead.worktree_cwd = Some("/repo-worktrees/other".into());
        }
        files.iter().map(|file| format!("{cwd}/{file}")).collect()
    });
    assert!(
        switched
            .start_approved(cx, card)
            .unwrap_err()
            .contains("proposal's checkout")
    );
    assert!(switched.store.enabled.borrow().is_empty());
}

#[gpui::test]
fn persists_dispatch_authority_and_binds_review_to_the_completed_attempt(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src/a"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    let task = f.tasks(cx)[0].clone();
    let dispatch_id = task.active_dispatch_id.clone().unwrap();
    let saved = f.store.saved.borrow()["lead"].clone();
    let dispatch = saved
        .dispatch_list()
        .iter()
        .find(|dispatch| dispatch.id == dispatch_id)
        .unwrap();
    assert_eq!(dispatch.task_id, task.id);
    assert_eq!(dispatch.state, DispatchState::Running);
    assert_eq!(dispatch.stage, DispatchStage::TurnSubmitted);
    f.complete(cx, &task.session_id, completed("Done"));
    let settled = f.tasks(cx)[0].clone();
    assert_eq!(settled.status, TaskStatus::Completed);
    assert_eq!(settled.active_dispatch_id, None);
    assert_eq!(
        settled.last_dispatch_id.as_deref(),
        Some(dispatch_id.as_str())
    );
    let run = f.run(cx).unwrap();
    let dispatch = &run.dispatch_list()[0];
    assert_eq!(dispatch.id, dispatch_id);
    assert_eq!(dispatch.state, DispatchState::Completed);
    assert_eq!(dispatch.stage, DispatchStage::Settled);
    assert_eq!(dispatch.result.as_deref(), Some("Done"));
    f.call(cx, "review", json!({ "taskId": task.id })).unwrap();
    assert_eq!(
        f.tasks(cx)[0].accepted_dispatch_id.as_deref(),
        Some(dispatch_id.as_str())
    );
    assert_eq!(*f.host.integrated.borrow(), vec![task.id.clone()]);
    assert!(f.host.cleanups.borrow().contains(&(task.id.clone(), false)));
    assert_eq!(f.tasks(cx)[0].workspace, None);
    assert_eq!(
        f.run(cx).unwrap().dispatch_list()[0].stage,
        DispatchStage::Cleaned
    );
}

#[gpui::test]
fn reports_a_cancelled_dirty_worktree_instead_of_silently_orphaning_it(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src/a"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    assert!(
        task.workspace
            .as_ref()
            .unwrap()
            .checkout_cwd
            .contains("/worktrees/")
    );
    f.host.cleanup_result.set(false);
    f.call(cx, "cancel", json!({ "taskId": task.id })).unwrap();
    let result = f.call(cx, "finish", json!({})).unwrap();
    assert_eq!(
        result,
        json!({ "finished": true, "cleanupPending": [format!("/worktrees/{}", task.id)] })
    );
    assert_eq!(
        f.tasks(cx)[0].workspace.as_ref().unwrap().checkout_cwd,
        format!("/worktrees/{}", task.id)
    );
    assert!(f.host.cleanups.borrow().contains(&(task.id.clone(), true)));
}

#[gpui::test]
fn keeps_an_isolated_worker_recoverable_when_integration_conflicts(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src/a"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    f.complete(cx, &task.session_id, completed("Done"));
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Completed);
    f.host
        .integrate_failures
        .borrow_mut()
        .push_back("lead checkout changed".into());
    assert!(
        f.call(cx, "review", json!({ "taskId": task.id }))
            .unwrap_err()
            .contains("lead checkout changed")
    );
    assert!(!f.tasks(cx)[0].accepted);
    assert_eq!(
        f.tasks(cx)[0].workspace.as_ref().unwrap().checkout_cwd,
        format!("/worktrees/{}", task.id)
    );
    assert!(f.host.cleanups.borrow().is_empty());
    f.call(cx, "review", json!({ "taskId": task.id })).unwrap();
    assert!(f.tasks(cx)[0].accepted);
    assert_eq!(f.host.integrated.borrow().len(), 2);
}

#[gpui::test]
fn does_not_let_a_late_completion_settle_a_newer_retry(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src/a"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    let task = f.tasks(cx)[0].clone();
    let late = f.host.take_completion(&task.session_id).unwrap();
    f.call(cx, "cancel", json!({ "taskId": task.id })).unwrap();
    f.call(cx, "message", json!({ "taskId": task.id, "text": "Retry" }))
        .unwrap();
    assert_eq!(f.host.submit_count(), 2);
    let retry_dispatch = f.tasks(cx)[0].active_dispatch_id.clone().unwrap();
    complete(cx, late, completed("Stale result"));
    let current = f.tasks(cx)[0].clone();
    assert_eq!(current.status, TaskStatus::Running);
    assert_eq!(current.active_dispatch_id, Some(retry_dispatch));
    assert_eq!(current.result, "");
    assert_eq!(f.run(cx).unwrap().dispatch_list().len(), 2);
}

#[gpui::test]
fn runs_disjoint_workers_concurrently_and_queues_overlap(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src/a"], json!({})).unwrap();
    f.delegate(cx, &["src/b"], json!({})).unwrap();
    f.delegate(cx, &["src/a/file.ts"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 2);
    assert_eq!(
        f.statuses(cx),
        vec![TaskStatus::Running, TaskStatus::Running, TaskStatus::Queued]
    );
    let first = f.tasks(cx)[0].session_id.clone();
    f.complete(cx, &first, completed("Implemented A"));
    assert_eq!(f.tasks(cx)[2].status, TaskStatus::Running);
    assert!(!f.tasks(cx)[0].accepted);
}

#[gpui::test]
fn keeps_a_dependency_queued_until_the_lead_accepts_the_upstream_result(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src/types.ts"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    let upstream = f.tasks(cx)[0].clone();
    f.delegate(cx, &["src/ui"], json!({ "dependsOn": [upstream.id] }))
        .unwrap();
    f.complete(cx, &upstream.session_id, completed("Types ready"));
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Completed);
    assert_eq!(f.tasks(cx)[1].status, TaskStatus::Queued);
    f.call(cx, "review", json!({ "taskId": upstream.id }))
        .unwrap();
    assert_eq!(f.tasks(cx)[1].status, TaskStatus::Running);
}

#[gpui::test]
fn deduplicates_command_retries_and_rejects_foreign_tasks_or_unapproved_harnesses(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.start(cx);
    let input = json!({ "title": "A", "prompt": "Implement", "harness": "codex", "files": ["a"] });
    let first = f
        .call_id_now(cx, "delegate", input.clone(), "same")
        .unwrap();
    let again = f
        .call_id_now(cx, "delegate", input.clone(), "same")
        .unwrap();
    assert_eq!(again, first);
    assert_eq!(f.tasks(cx).len(), 1);
    let mut changed = input.clone();
    changed["title"] = json!("B");
    assert!(
        f.call_id_now(cx, "delegate", changed, "same")
            .unwrap_err()
            .contains("different input")
    );
    assert!(
        f.call(cx, "get", json!({ "taskId": "foreign" }))
            .unwrap_err()
            .contains("does not belong")
    );
    assert!(
        f.delegate(cx, &["a"], json!({ "harness": "pi" }))
            .unwrap_err()
            .contains("not allowed")
    );
}

#[gpui::test]
fn does_not_dispatch_a_worker_if_its_task_cannot_be_persisted(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.store
        .save_failures
        .borrow_mut()
        .push_back("Disk full".into());
    assert!(
        f.delegate(cx, &["a"], json!({}))
            .unwrap_err()
            .contains("Disk full")
    );
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Paused);
    assert_eq!(f.host.submit_count(), 0);
}

#[gpui::test]
fn persists_a_delegation_and_its_retry_receipt_in_the_same_snapshot(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    let input = json!({ "title": "A", "prompt": "Implement", "harness": "codex", "files": ["a"] });
    f.call_id_now(cx, "delegate", input.clone(), "durable")
        .unwrap();
    let first_task_save = f
        .store
        .saves
        .borrow()
        .iter()
        .find(|run| run.tasks.len() == 1)
        .cloned()
        .unwrap();
    let receipt = first_task_save.requests["durable"].result.clone();
    assert_eq!(receipt["taskId"], json!(first_task_save.tasks[0].id));
    let restored = orchestrator(f.store.clone(), f.host.clone(), cx);
    let weak = restored.downgrade();
    finish(
        cx,
        cx.spawn(|mut cx| async move { hydrate(&weak, "lead", &mut cx).await }),
    )
    .unwrap();
    let weak = restored.downgrade();
    let object = input.as_object().cloned().unwrap();
    let again = finish(
        cx,
        cx.spawn(|mut cx| async move {
            handle(&weak, "lead", "durable", "delegate", &object, &mut cx).await
        }),
    )
    .unwrap();
    assert_eq!(again, receipt);
    assert_eq!(
        restored.read_with(cx, |o, _| o.run("lead").unwrap().tasks.len()),
        1
    );
}

#[gpui::test]
fn restored_typescript_receipt_reuses_the_delegation_without_dispatching_again(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.start(cx);
    let input = json!({"title":"A","prompt":"Implement","harness":"codex","files":["a"]});
    let result = f
        .call_id_now(cx, "delegate", input.clone(), "legacy-retry")
        .unwrap();
    let legacy_signature = r#"{"action":"delegate","input":{"title":"A","prompt":"Implement","harness":"codex","files":["a"]}}"#;
    f.store
        .saved
        .borrow_mut()
        .get_mut("lead")
        .unwrap()
        .requests
        .get_mut("legacy-retry")
        .unwrap()
        .signature = legacy_signature.into();
    let restored = orchestrator(f.store.clone(), f.host.clone(), cx);
    let weak = restored.downgrade();
    finish(
        cx,
        cx.spawn(|mut cx| async move { hydrate(&weak, "lead", &mut cx).await }),
    )
    .unwrap();
    let submissions = f.host.submit_count();
    let workers = f.host.created.borrow().len();
    let scopes = f.store.scope_calls.get();
    let tasks = restored.read_with(cx, |o, _| o.run("lead").unwrap().tasks.clone());
    let dispatches = restored.read_with(cx, |o, _| o.run("lead").unwrap().dispatch_list().to_vec());
    let weak = restored.downgrade();
    let object: serde_json::Map<String, Value> = ["files", "harness", "prompt", "title"]
        .into_iter()
        .map(|key| (key.into(), input[key].clone()))
        .collect();
    let retry = finish(
        cx,
        cx.spawn(|mut cx| async move {
            handle(&weak, "lead", "legacy-retry", "delegate", &object, &mut cx).await
        }),
    )
    .expect("the unchanged TypeScript receipt must return its saved result");
    assert_eq!(retry, result);
    cx.run_until_parked();
    assert_eq!(f.host.submit_count(), submissions);
    assert_eq!(f.host.created.borrow().len(), workers);
    assert_eq!(f.store.scope_calls.get(), scopes);
    assert_eq!(
        restored.read_with(cx, |o, _| o.run("lead").unwrap().tasks.clone()),
        tasks
    );
    assert_eq!(
        restored.read_with(cx, |o, _| o.run("lead").unwrap().dispatch_list().to_vec()),
        dispatches
    );
    assert_eq!(
        restored.read_with(cx, |o, _| o.run("lead").unwrap().requests["legacy-retry"]
            .signature
            .clone()),
        legacy_signature
    );
    let weak = restored.downgrade();
    let mut changed = input.as_object().cloned().unwrap();
    changed.insert("prompt".into(), json!("Different work"));
    let conflict = finish(
        cx,
        cx.spawn(|mut cx| async move {
            handle(&weak, "lead", "legacy-retry", "delegate", &changed, &mut cx).await
        }),
    )
    .unwrap_err();
    assert!(conflict.contains("different input"));
}

#[gpui::test]
fn restored_typescript_approval_receipt_accepts_ordinary_integral_spelling(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    f.host.with_session(&task.session_id, |worker| {
        worker.blocks = vec![approval(7, "Owned fixture")]
    });
    let input = json!({"taskId":task.id,"requestId":7.0,"decision":"deny"});
    let result = f
        .call_id_now(cx, "respond", input.clone(), "legacy-approval")
        .unwrap();
    assert_eq!(f.host.approvals.borrow().len(), 1);
    let mut legacy_input = input.clone();
    legacy_input["requestId"] = json!(7);
    let legacy_signature =
        serde_json::to_string(&json!({"action":"respond","input":legacy_input})).unwrap();
    f.store
        .saved
        .borrow_mut()
        .get_mut("lead")
        .unwrap()
        .requests
        .get_mut("legacy-approval")
        .unwrap()
        .signature = legacy_signature.clone();
    let restored = orchestrator(f.store.clone(), f.host.clone(), cx);
    let weak = restored.downgrade();
    finish(
        cx,
        cx.spawn(|mut cx| async move { hydrate(&weak, "lead", &mut cx).await }),
    )
    .unwrap();
    let weak = restored.downgrade();
    let object = input.as_object().cloned().unwrap();
    assert_eq!(
        finish(
            cx,
            cx.spawn(|mut cx| async move {
                handle(
                    &weak,
                    "lead",
                    "legacy-approval",
                    "respond",
                    &object,
                    &mut cx,
                )
                .await
            })
        )
        .expect("TypeScript serializes valid request ID 7.0 as 7"),
        result
    );
    assert_eq!(f.host.approvals.borrow().len(), 1);
    assert_eq!(
        restored.read_with(cx, |o, _| {
            o.run("lead").unwrap().requests["legacy-approval"]
                .signature
                .clone()
        }),
        legacy_signature
    );
    let weak = restored.downgrade();
    let mut changed = input.as_object().cloned().unwrap();
    changed.insert("requestId".into(), json!(8));
    assert!(
        finish(
            cx,
            cx.spawn(|mut cx| async move {
                handle(
                    &weak,
                    "lead",
                    "legacy-approval",
                    "respond",
                    &changed,
                    &mut cx,
                )
                .await
            })
        )
        .unwrap_err()
        .contains("different input")
    );
}

#[gpui::test]
fn rejects_conflicting_reuse_of_an_in_flight_request_id(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    let (release, hold) = oneshot::channel();
    f.store.scopes_once.borrow_mut().push_back(Once::Hold(hold));
    let calls_before = f.store.scope_calls.get();
    let input = json!({ "title": "A", "prompt": "Implement", "harness": "codex", "files": ["a"] });
    let pending = f.call_id(cx, "delegate", input.clone(), "pending");
    cx.run_until_parked();
    assert!(f.store.scope_calls.get() > calls_before);
    let mut other = input.clone();
    other["files"] = json!(["b"]);
    assert!(
        f.call_id_now(cx, "delegate", other, "pending")
            .unwrap_err()
            .contains("different input")
    );
    release.send(Ok(vec!["/repo/a".to_string()])).unwrap();
    finish(cx, pending).unwrap();
    assert_eq!(f.tasks(cx).len(), 1);
}

#[gpui::test]
fn stops_an_active_worker_if_saving_another_assignment_fails(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    f.store
        .save_failures
        .borrow_mut()
        .push_back("Disk full".into());
    assert!(
        f.delegate(cx, &["b"], json!({}))
            .unwrap_err()
            .contains("Disk full")
    );
    let worker = f.tasks(cx)[0].session_id.clone();
    assert!(f.host.stops.borrow().contains(&worker));
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Paused);
    assert_eq!(f.host.submit_count(), 1);
}

#[gpui::test]
fn blocks_only_the_worker_that_escapes_its_scope_and_allows_an_explicit_rescope(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src/a"], json!({})).unwrap();
    f.delegate(cx, &["src/c"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 2);
    let offender = f.tasks(cx)[0].clone();
    let independent = f.tasks(cx)[1].clone();
    f.observe(cx, &offender.session_id, write("src/a/../b/file.ts"));
    cx.run_until_parked();
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Blocked);
    assert!(
        f.tasks(cx)[0]
            .error
            .as_ref()
            .unwrap()
            .contains("outside its assignment")
    );
    assert_eq!(f.tasks(cx)[1].status, TaskStatus::Running);
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Active);
    assert_eq!(f.run(cx).unwrap().error, None);
    assert!(f.host.stops.borrow().contains(&offender.session_id));
    assert!(!f.host.stops.borrow().contains(&independent.session_id));
    f.call(
        cx,
        "retry",
        json!({
            "taskId": offender.id,
            "text": "The additional file is required; continue carefully.",
            "files": ["src/a", "src/b"]
        }),
    )
    .unwrap();
    let retried = f.tasks(cx)[0].clone();
    assert_eq!(retried.status, TaskStatus::Running);
    assert_eq!(retried.files, vec!["src/a", "src/b"]);
    assert_eq!(retried.scopes, vec!["/repo/src/a", "/repo/src/b"]);
}

#[gpui::test]
fn migrates_an_old_global_scope_pause_into_one_blocked_task_and_resumable_collateral_work(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["src/a"], json!({ "title": "Offender" }))
        .unwrap();
    f.delegate(cx, &["src/b"], json!({ "title": "Independent" }))
        .unwrap();
    assert!(
        f.tasks(cx)
            .iter()
            .all(|task| task.active_dispatch_id.is_some())
    );
    let current = (*f.run(cx).unwrap()).clone();
    let reason = "Offender reported a write outside its assignment: /outside. Review the shared files before resuming.";
    let mut legacy = current.clone();
    legacy.status = RunStatus::Paused;
    legacy.error = Some(reason.into());
    for task in &mut legacy.tasks {
        task.status = TaskStatus::Failed;
        task.error = Some(reason.into());
        task.last_dispatch_id = task.active_dispatch_id.take();
    }
    for dispatch in legacy.dispatches.as_mut().unwrap() {
        dispatch.state = DispatchState::Failed;
        dispatch.error = Some(reason.into());
    }
    let migrated = normalize_orchestration_run(&legacy);
    assert_eq!(
        migrated.tasks.iter().map(|t| t.status).collect::<Vec<_>>(),
        vec![TaskStatus::Blocked, TaskStatus::Interrupted]
    );
    assert_eq!(
        migrated
            .tasks
            .iter()
            .map(|t| t.delivered)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
    assert_eq!(
        migrated
            .dispatch_list()
            .iter()
            .map(|d| d.state)
            .collect::<Vec<_>>(),
        vec![DispatchState::Blocked, DispatchState::Interrupted]
    );
    f.stop_run(cx);
}

/// `previewFromToolPart` for an OpenCode `write` call. The harness port of
/// that function still returns no preview (its `extract_tool_preview` is a
/// stub, see NEEDS.md), so this builds the same records and runs the core
/// reducer's extractor.
fn opencode_write_preview(path: &str) -> Option<ToolPreview> {
    let input = json!({ "filePath": path, "content": "test" });
    let update = json!({
        "title": "write", "name": "write", "kind": "edit", "input": input, "rawInput": input
    });
    let tool = json!({ "title": "write", "name": "write", "kind": "edit", "rawInput": input });
    monocode_core::reducer::preview::extract_tool_preview(
        update.as_object().unwrap(),
        tool.as_object().unwrap(),
    )
}

#[gpui::test]
fn allows_opencode_scratch_writes_only_in_that_workers_private_directory(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["brief.py"], json!({})).unwrap();
    f.delegate(cx, &["commands.py"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 2);
    let tasks = f.tasks(cx);
    let (worker, other) = (tasks[0].clone(), tasks[1].clone());
    let write = |path: &str| HarnessEvent::ToolUpdated {
        agent_model: None,
        call_id: path.into(),
        title: Some("Write".into()),
        kind: None,
        status: Some("running".into()),
        detail: None,
        preview: opencode_write_preview(path),
        paths: None,
    };
    let scratch = worker.scratch_dir.clone().unwrap();
    assert!(f.host.submits.borrow()[0].1.contains(&scratch));
    // macOS reports both /var and /private/var for the same file.
    *f.store.resolve_impl.borrow_mut() = Box::new(|path| {
        path.strip_prefix("/var/")
            .map(|rest| format!("/private/var/{rest}"))
            .unwrap_or_else(|| path.to_string())
    });
    f.observe(
        cx,
        &worker.session_id,
        write(&format!(
            "{}/helper.py",
            scratch.replacen("/private", "", 1)
        )),
    );
    f.observe(cx, &worker.session_id, write("brief.py"));
    cx.run_until_parked();
    assert_eq!(f.store.resolve_calls.get(), 2);
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Active);
    assert!(
        f.tasks(cx)
            .iter()
            .all(|task| task.status == TaskStatus::Running)
    );
    f.observe(
        cx,
        &worker.session_id,
        write(&format!("{}/helper.py", other.scratch_dir.clone().unwrap())),
    );
    cx.run_until_parked();
    assert_eq!(
        f.statuses(cx),
        vec![TaskStatus::Blocked, TaskStatus::Running]
    );
    assert!(
        f.tasks(cx)[0]
            .error
            .as_ref()
            .unwrap()
            .contains("outside its assignment")
    );
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Active);
}

#[gpui::test]
fn keeps_a_paused_run_inspectable_and_automatically_continues_interrupted_work_on_resume(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    f.start_approved(cx, proposal()).unwrap();
    assert_eq!(f.host.created.borrow().len(), 1);
    let interrupted = f
        .tasks(cx)
        .into_iter()
        .find(|t| t.title == "Types")
        .unwrap();
    let queued = f.tasks(cx).into_iter().find(|t| t.title == "UI").unwrap();
    f.complete(cx, "lead", failed("", "Lead provider disconnected"));
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Paused);
    let paused_task = f
        .tasks(cx)
        .into_iter()
        .find(|t| t.id == interrupted.id)
        .unwrap();
    assert_eq!(paused_task.status, TaskStatus::Interrupted);
    assert_eq!(queued.status, TaskStatus::Queued);
    let reason = f.run(cx).unwrap().error.clone().unwrap();
    let list = f.call(cx, "list", json!({})).unwrap();
    assert_eq!(list["run"]["status"], "paused");
    assert_eq!(list["run"]["error"], json!(reason));
    let get = f
        .call(cx, "get", json!({ "taskId": interrupted.id }))
        .unwrap();
    assert_eq!(get["runStatus"], "paused");
    assert_eq!(get["error"], json!(reason));
    // A paused wait returns immediately despite queued validation.
    let wait = f.call(cx, "wait", json!({ "timeoutSeconds": 25 })).unwrap();
    assert_eq!(wait["status"], "paused");
    assert!(
        wait["recovery"]
            .as_str()
            .unwrap()
            .contains("Do not retry mutations or keep polling")
    );
    assert!(
        f.call(
            cx,
            "message",
            json!({ "taskId": interrupted.id, "text": "Continue" })
        )
        .unwrap_err()
        .contains("click Resume")
    );
    assert!(
        f.call(cx, "finish", json!({}))
            .unwrap_err()
            .contains(&reason)
    );
    let before = f.host.submit_count();
    f.start_with(cx, vec![HarnessId::Codex], 2).unwrap();
    assert!(f.host.submit_count() > before);
    assert_eq!(
        f.run(cx).unwrap().last_pause_reason.as_deref(),
        Some(reason.as_str())
    );
    let current = f.tasks(cx);
    assert_eq!(
        current
            .iter()
            .find(|t| t.id == interrupted.id)
            .unwrap()
            .status,
        TaskStatus::Running
    );
    assert_eq!(
        current.iter().find(|t| t.id == queued.id).unwrap().status,
        TaskStatus::Queued
    );
    let resumed = f.host.submits.borrow()[before..]
        .iter()
        .find(|(id, _)| *id == interrupted.session_id)
        .cloned()
        .unwrap();
    assert!(resumed.1.contains("Continue the existing assignment"));
    assert!(resumed.1.contains("retained worker checkout"));
    f.stop_run(cx);
}

#[gpui::test]
fn does_not_allow_scratch_symlinks_to_escape_into_another_assignment(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["brief.py"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    let task = f.tasks(cx)[0].clone();
    f.store
        .resolve_once
        .borrow_mut()
        .push_back(Once::Value("/repo/commands.py".into()));
    f.observe(
        cx,
        &task.session_id,
        write(&format!(
            "{}/link/commands.py",
            task.scratch_dir.clone().unwrap()
        )),
    );
    cx.run_until_parked();
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Blocked);
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Active);
}

#[gpui::test]
fn waits_for_scope_verification_before_publishing_a_completed_result(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["brief.py"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    let (resolve, hold) = oneshot::channel();
    f.store
        .resolve_once
        .borrow_mut()
        .push_back(Once::Hold(hold));
    f.observe(
        cx,
        &task.session_id,
        write_update("/outside.py", "completed"),
    );
    f.complete(cx, &task.session_id, completed("Done"));
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Running);
    resolve.send(Ok("/outside.py".into())).unwrap();
    cx.run_until_parked();
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Blocked);
    assert!(
        f.call(cx, "review", json!({ "taskId": task.id }))
            .unwrap_err()
            .contains("blocked")
    );
}

#[gpui::test]
fn ignores_failed_write_reports_and_discards_late_checks_from_cancelled_attempts(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["brief.py"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    let task = f.tasks(cx)[0].clone();
    f.observe(cx, &task.session_id, write_update("/outside.py", "failed"));
    cx.run_until_parked();
    assert_eq!(f.store.resolve_calls.get(), 0);
    let (resolve, hold) = oneshot::channel();
    f.store
        .resolve_once
        .borrow_mut()
        .push_back(Once::Hold(hold));
    f.observe(cx, &task.session_id, write_update("/outside.py", "running"));
    f.complete(
        cx,
        &task.session_id,
        completed("Old result awaiting its write check"),
    );
    f.call(cx, "cancel", json!({ "taskId": task.id })).unwrap();
    f.call(
        cx,
        "message",
        json!({ "taskId": task.id, "text": "Try again within scope" }),
    )
    .unwrap();
    assert_eq!(f.host.submit_count(), 2);
    resolve.send(Ok("/outside.py".into())).unwrap();
    cx.run_until_parked();
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Active);
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Running);
    f.stop_run(cx);
}

#[gpui::test]
fn accepts_windows_drive_paths_inside_an_extended_canonical_scope(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.host
        .with_session("lead", |lead| lead.cwd = "D:/Projects/repo-a".into());
    // The worker shares the lead checkout, so its write scopes are the same
    // canonical forms the lead's scopes use.
    *f.host.worker_checkout.borrow_mut() = Some("D:/Projects/repo-a".into());
    *f.store.scopes_impl.borrow_mut() = Box::new(|_, files| {
        files
            .iter()
            .map(|file| format!("//?/d:/projects/repo-a/{file}"))
            .collect()
    });
    f.store
        .scopes_once
        .borrow_mut()
        .push_back(Once::Value(vec!["//?/d:/projects/repo-a".into()]));
    f.start(cx);
    f.delegate(cx, &["src/a"], json!({})).unwrap();
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Running);
    let task = f.tasks(cx)[0].clone();
    for path in [
        "src/a/relative.ts",
        "D:/Projects/repo-a/src/a/forward.ts",
        "D:\\Projects\\repo-a\\src\\a\\backward.ts",
    ] {
        f.observe(cx, &task.session_id, write(path));
    }
    cx.run_until_parked();
    assert_eq!(f.store.resolve_calls.get(), 3);
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Active);
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Running);
}

#[gpui::test]
fn holds_ownership_until_a_cancelled_process_has_stopped(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    f.delegate(cx, &["a"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    let (stopped, hold) = oneshot::channel();
    f.host.stop_holds.borrow_mut().push_back(hold);
    let first = f.tasks(cx)[0].id.clone();
    let cancel = f.call_task(cx, "cancel", json!({ "taskId": first }));
    cx.run_until_parked();
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Cancelling);
    assert_eq!(f.tasks(cx)[1].status, TaskStatus::Queued);
    stopped.send(()).unwrap();
    finish(cx, cancel).unwrap();
    assert_eq!(f.tasks(cx)[1].status, TaskStatus::Running);
}

#[gpui::test]
fn stops_queued_work_and_suppresses_lead_continuation_on_stop(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    f.delegate(cx, &["a"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    let done = f.host.take_completion(&f.tasks(cx)[0].session_id).unwrap();
    f.stop_run(cx);
    complete(cx, done, completed("late output"));
    f.host.set_busy("lead", false);
    f.o.update(cx, |o, cx| o.sync(cx));
    cx.run_until_parked();
    assert_eq!(
        f.statuses(cx),
        vec![TaskStatus::Cancelled, TaskStatus::Cancelled]
    );
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Stopped);
    assert!(f.store.disabled.borrow().contains(&"lead".to_string()));
    assert_eq!(f.host.submit_count(), 1);
}

#[gpui::test]
fn returns_worker_output_to_an_idle_lead_once(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    f.host.set_busy("lead", false);
    let worker = f.tasks(cx)[0].session_id.clone();
    f.complete(cx, &worker, completed("Tests pass"));
    assert_eq!(f.host.submit_count(), 2);
    let submits = f.host.submits.borrow().clone();
    assert_eq!(submits[1].0, "lead");
    assert!(submits[1].1.contains("Tests pass"));
    f.o.update(cx, |o, cx| o.sync(cx));
    cx.run_until_parked();
    assert_eq!(f.host.submit_count(), 2);
}

#[gpui::test]
fn keeps_results_available_and_pauses_when_the_lead_cannot_continue(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    f.host.set_busy("lead", false);
    let worker = f.tasks(cx)[0].session_id.clone();
    f.complete(cx, &worker, completed("Result"));
    assert_eq!(f.host.submit_count(), 2);
    f.complete(cx, "lead", failed("", "Provider unavailable"));
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Paused);
    assert!(!f.tasks(cx)[0].delivered);
    f.o.update(cx, |o, cx| o.sync(cx));
    cx.run_until_parked();
    assert_eq!(f.host.submit_count(), 2);
}

#[gpui::test]
fn recovers_interrupted_tasks_without_claiming_completion_and_continues_them_on_resume(
    cx: &mut TestAppContext,
) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    assert_eq!(f.host.submit_count(), 1);
    assert_eq!(
        f.store.saved.borrow()["lead"].tasks[0].status,
        TaskStatus::Running
    );
    let restored = orchestrator(f.store.clone(), f.host.clone(), cx);
    let weak = restored.downgrade();
    finish(
        cx,
        cx.spawn(|mut cx| async move { hydrate(&weak, "lead", &mut cx).await }),
    )
    .unwrap();
    let run = restored.read_with(cx, |o, _| o.run("lead").unwrap());
    assert_eq!(run.status, RunStatus::Paused);
    assert_eq!(run.tasks[0].status, TaskStatus::Interrupted);
    assert_eq!(f.host.submit_count(), 1);
    let weak = restored.downgrade();
    finish(
        cx,
        cx.spawn(|mut cx| async move {
            start(&weak, "lead", &[HarnessId::Codex], 2, None, &mut cx).await
        }),
    )
    .unwrap();
    assert_eq!(f.host.submit_count(), 2);
    assert_eq!(
        restored.read_with(cx, |o, _| o.run("lead").unwrap().tasks[0].status),
        TaskStatus::Running
    );
    assert!(
        f.host.submits.borrow()[1]
            .1
            .contains("Continue the existing assignment")
    );
    let weak = restored.downgrade();
    finish(
        cx,
        cx.spawn(|mut cx| async move { stop_run(&weak, "lead", &mut cx).await }),
    )
    .unwrap();
}

#[gpui::test]
fn does_not_claim_a_turn_or_run_is_successful_before_review(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    assert!(
        f.call(cx, "finish", json!({}))
            .unwrap_err()
            .contains("Review all")
    );
    assert_eq!(f.host.submit_count(), 1);
    let worker = f.tasks(cx)[0].session_id.clone();
    f.complete(cx, &worker, failed("Partial edits", "Provider crashed"));
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Failed);
    let id = f.tasks(cx)[0].id.clone();
    assert!(
        f.call(cx, "review", json!({ "taskId": id }))
            .unwrap_err()
            .contains("Only a completed")
    );
}

#[gpui::test]
fn names_the_way_out_of_a_run_that_cannot_finish_yet(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    let worker = f.tasks(cx)[0].session_id.clone();
    f.complete(cx, &worker, failed("", "Provider crashed"));
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Failed);
    let id = f.tasks(cx)[0].id.clone();
    let review = f.call(cx, "review", json!({ "taskId": id })).unwrap_err();
    assert!(review.contains("message") && review.find("cancel") > review.find("message"));
    let finish_error = f.call(cx, "finish", json!({})).unwrap_err();
    assert!(finish_error.contains("Task (failed)"));
    assert!(finish_error.contains("message/retry") && finish_error.contains("cancel"));
    f.call(cx, "cancel", json!({ "taskId": id })).unwrap();
    assert_eq!(
        f.call(cx, "finish", json!({})).unwrap(),
        json!({ "finished": true })
    );
}

#[gpui::test]
fn rejects_mistyped_fields_instead_of_silently_dropping_them(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    // Silently ignoring depends_on would race two workers over one file.
    assert!(
        f.delegate(
            cx,
            &["a"],
            json!({ "depends_on": [], "modelId": "codex:test" })
        )
        .unwrap_err()
        .contains("Unknown delegate fields: depends_on, modelId")
    );
    assert!(
        f.call(cx, "finish", json!({ "taskId": "x" }))
            .unwrap_err()
            .contains("finish takes no input")
    );
    assert!(
        f.call(cx, "wait", json!({ "timeout": 5 }))
            .unwrap_err()
            .contains("wait accepts: timeoutSeconds")
    );
    assert!(f.tasks(cx).is_empty());
}

#[gpui::test]
fn points_a_bad_delegate_at_the_values_list_would_have_returned(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    assert!(
        f.delegate(cx, &["a"], json!({ "harness": "claude" }))
            .unwrap_err()
            .contains("Harness \"claude\" is not allowed in this run. Allowed: codex.")
    );
    assert!(
        f.delegate(cx, &["a"], json!({ "model": "codex:ghost" }))
            .unwrap_err()
            .contains("Choose a model ID returned by list for codex: codex:test.")
    );
    assert!(
        f.delegate(cx, &[], json!({}))
            .unwrap_err()
            .contains("at least one file")
    );
    assert!(
        f.call(
            cx,
            "delegate",
            json!({ "title": "T", "prompt": "P", "harness": "codex", "files": ["a"], "dependsOn": ["nope"] })
        )
        .unwrap_err()
        .contains("unknown or cancelled: nope")
    );
}

#[gpui::test]
fn keeps_the_retry_ledger_free_of_inherited_object_keys(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    let first = f
        .call_id_now(
            cx,
            "delegate",
            json!({ "title": "A", "prompt": "Implement", "harness": "codex", "files": ["a"] }),
            "constructor",
        )
        .unwrap();
    assert!(first.get("taskId").is_some());
}

#[gpui::test]
fn treats_an_action_named_after_an_object_member_as_unknown(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    for action in ["constructor", "toString", "__proto__"] {
        assert!(
            f.call(cx, action, json!({}))
                .unwrap_err()
                .starts_with(&format!("Unknown action \"{action}\""))
        );
    }
}

#[gpui::test]
fn routes_a_blocked_agent_to_the_lead_instead_of_the_user(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    f.host.with_session(&task.session_id, |worker| {
        worker.blocks = vec![approval(7, "rm -rf build")]
    });
    let view = f.call(cx, "get", json!({ "taskId": task.id })).unwrap();
    assert_eq!(view["needsInput"]["kind"], "approval");
    assert_eq!(view["needsInput"]["requestId"], 7);
    // A stale or invented requestId must never decide a live prompt.
    assert!(
        f.call(
            cx,
            "respond",
            json!({ "taskId": task.id, "requestId": 6, "decision": "allow" })
        )
        .unwrap_err()
        .contains("Stale requestId")
    );
    assert!(
        f.call(
            cx,
            "respond",
            json!({ "taskId": task.id, "requestId": 7, "decision": "maybe" })
        )
        .unwrap_err()
        .contains("decision must be \"allow\" or \"deny\"")
    );
    f.call(
        cx,
        "respond",
        json!({ "taskId": task.id, "requestId": 7, "decision": "deny" }),
    )
    .unwrap();
    assert_eq!(
        *f.host.approvals.borrow(),
        vec![(task.session_id.clone(), 7, ApprovalDecision::Deny)]
    );
}

#[gpui::test]
fn returns_from_wait_immediately_when_a_worker_already_needs_input(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    f.host.with_session(&task.session_id, |worker| {
        worker.blocks = vec![approval(7, "Run the check")]
    });
    // No clock advance: the wait must not start a long poll.
    let waited = f.call(cx, "wait", json!({ "timeoutSeconds": 20 })).unwrap();
    assert_eq!(waited["tasks"][0]["needsInput"]["kind"], "approval");
    assert_eq!(waited["tasks"][0]["needsInput"]["requestId"], 7);
}

#[gpui::test]
fn wakes_an_active_wait_as_soon_as_a_worker_needs_input(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    let mut waiting = f.call_task(cx, "wait", json!({ "timeoutSeconds": 20 }));
    cx.run_until_parked();
    assert!((&mut waiting).now_or_never().is_none());
    f.host.with_session(&task.session_id, |worker| {
        worker.blocks = vec![approval(8, "Run the check")]
    });
    f.o.update(cx, |o, cx| o.sync(cx));
    let waited = finish(cx, waiting).unwrap();
    assert_eq!(waited["tasks"][0]["needsInput"]["requestId"], 8);
}

#[gpui::test]
fn times_out_a_wait_when_nothing_changes(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    let mut waiting = f.call_task(cx, "wait", json!({ "timeoutSeconds": 2 }));
    cx.run_until_parked();
    assert!((&mut waiting).now_or_never().is_none());
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(3));
    let waited = finish(cx, waiting).unwrap();
    assert_eq!(waited["status"], "active");
    assert!(
        f.call(cx, "wait", json!({ "timeoutSeconds": 26 }))
            .unwrap_err()
            .contains("timeoutSeconds must be 0 to 25")
    );
}

#[gpui::test]
fn validates_the_leads_answer_against_the_agents_own_question(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    let question: UserQuestionPrompt = serde_json::from_value(json!({
        "requestId": 9,
        "title": "Pick a check",
        "questions": [{
            "id": "check",
            "prompt": "Which check?",
            "multiSelect": false,
            "allowCustom": false,
            "options": [{ "id": "unit", "label": "Unit" }]
        }]
    }))
    .unwrap();
    f.host.with_session(&task.session_id, |worker| {
        worker.pending_question = Some(question)
    });
    assert!(
        f.call(
            cx,
            "answer",
            json!({ "taskId": task.id, "requestId": 9, "answers": { "check": ["e2e"] } })
        )
        .unwrap_err()
        .contains("Unknown option")
    );
    assert!(
        f.call(
            cx,
            "answer",
            json!({ "taskId": task.id, "requestId": 9, "answers": { "nope": ["unit"] } })
        )
        .unwrap_err()
        .contains("Unknown question")
    );
    f.call(
        cx,
        "answer",
        json!({ "taskId": task.id, "requestId": 9, "answers": { "check": ["unit"] } }),
    )
    .unwrap();
    assert_eq!(
        *f.host.answers.borrow(),
        vec![(
            task.session_id.clone(),
            9,
            UserQuestionReply::Answered {
                answers: [("check".to_string(), vec!["unit".to_string()])].into(),
                custom: None,
            }
        )]
    );
}

#[gpui::test]
fn stops_the_agents_whenever_the_lead_stops_supervising(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.host.set_busy("lead", false);
    f.start_approved(cx, proposal()).unwrap();
    assert_eq!(f.host.created.borrow().len(), 1);
    let running = f
        .tasks(cx)
        .into_iter()
        .find(|t| t.title == "Types")
        .unwrap();
    assert_eq!(running.status, TaskStatus::Running);
    // The lead's turn dies. Its agents must not carry on without supervision.
    f.complete(cx, "lead", failed("", "Provider crashed"));
    assert_eq!(f.run(cx).unwrap().status, RunStatus::Paused);
    let tasks = f.tasks(cx);
    assert_eq!(
        tasks.iter().find(|t| t.title == "Types").unwrap().status,
        TaskStatus::Interrupted
    );
    assert!(f.host.stops.borrow().contains(&running.session_id));
    // Queued work is untouched, so resuming picks it up intact.
    assert_eq!(
        tasks.iter().find(|t| t.title == "UI").unwrap().status,
        TaskStatus::Queued
    );
}

#[gpui::test]
fn steers_a_running_agent_and_refuses_one_that_is_not(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    f.delegate(cx, &["a"], json!({})).unwrap();
    let task = f.tasks(cx)[0].clone();
    assert_eq!(task.status, TaskStatus::Running);
    f.call(
        cx,
        "steer",
        json!({ "taskId": task.id, "text": "Use the existing helper" }),
    )
    .unwrap();
    assert_eq!(
        *f.host.steers.borrow(),
        vec![(
            task.session_id.clone(),
            "Use the existing helper".to_string()
        )]
    );
    // Steering must not end the turn, so the agent keeps its work.
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Running);
    assert!(!f.host.stops.borrow().contains(&task.session_id));
    // A stopped agent takes a fresh turn instead, and the error says so.
    f.complete(cx, &task.session_id, completed("Done"));
    assert_eq!(f.tasks(cx)[0].status, TaskStatus::Completed);
    let error = f
        .call(
            cx,
            "steer",
            json!({ "taskId": task.id, "text": "Too late" }),
        )
        .unwrap_err();
    assert!(error.starts_with("Only a running agent can be steered"));
    assert!(error.contains("message"));
}

#[gpui::test]
fn blocks_ordinary_sessions_while_a_run_owns_their_checkout(cx: &mut TestAppContext) {
    let f = setup(cx);
    f.start(cx);
    let mut other = session("other", HarnessId::Claude, "/repo");
    other.busy = Some(false);
    f.host.sessions.borrow_mut().push(other);
    let (other_error, lead_error) = f.o.read_with(cx, |o, cx| {
        (
            o.submission_error("other", false, cx),
            o.submission_error("lead", false, cx),
        )
    });
    assert!(other_error.unwrap().contains("active orchestrator"));
    assert_eq!(lead_error, None);
}

#[gpui::test]
fn adds_the_control_envelope_only_while_the_run_is_active(cx: &mut TestAppContext) {
    let f = setup(cx);
    assert_eq!(f.o.read_with(cx, |o, _| o.prompt("lead", "Hi")), "Hi");
    f.start(cx);
    let prompt = f.o.read_with(cx, |o, _| o.prompt("lead", "Hi"));
    assert!(prompt.starts_with("Hi\n\n<monocode_orchestration>"));
    assert!(prompt.contains("/Applications/MonoCode.app/Contents/MacOS/monocode control --help"));
    f.stop_run(cx);
    assert_eq!(f.o.read_with(cx, |o, _| o.prompt("lead", "Hi")), "Hi");
}
