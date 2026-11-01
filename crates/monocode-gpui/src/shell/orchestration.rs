//! Orchestrator runs. A reviewed plan creates real native sessions; the app,
//! not the model, decides dispatch, outcomes and the hand-back to the lead.
//! Every change is saved, so a restart pauses a run instead of losing it.
//! Approvals stay in each worker's chat and show on the lead's card.
use super::*;
use crate::transcript::orchestration::{Action, OrchestrationAction, Phase, RunView, TaskView};
use monocode_core::{
    agent::Settings,
    orchestration::{self, Outcome, Run, State, Summary, Team},
};
use std::time::Duration;

/// Runs kept per lead, so earlier plans in the thread keep their results.
const KEPT_RUNS: usize = 5;
/// How long a launched worker may take to begin its turn, per check.
const LAUNCH_GRACE: Duration = Duration::from_secs(5);
const LAUNCH_CHECKS: usize = 24;

#[derive(Default)]
pub(super) struct Runs {
    /// Each lead's recent runs, oldest first. Only the last one can be active.
    by_lead: HashMap<i64, Vec<Run>>,
    /// Background saves, chained per lead so they land in order.
    writes: HashMap<i64, gpui::Task<()>>,
    /// Leads whose cards refresh shortly, following workers' progress.
    refreshing: HashSet<i64>,
    /// Launched workers and their turn generation before the launch.
    launches: HashMap<i64, u64>,
    /// Leads whose automatic report was tried for their current run.
    report_tried: HashSet<i64>,
    /// The team each lead picked for its next run. Kept after starting, so the
    /// next plan of that chat begins with the same choice.
    teams: HashMap<i64, Team>,
}
impl Runs {
    /// Saved runs come back paused: nothing ran while the app was closed.
    pub(super) fn restore(
        saved: HashMap<i64, Vec<Run>>,
        summaries: &mut HashMap<i64, Summary>,
    ) -> Self {
        // Thread children recompute theirs on their next event.
        for summary in summaries.values_mut() {
            summary.live = false;
        }
        let by_lead = saved
            .into_iter()
            .filter(|(_, runs)| !runs.is_empty())
            .map(|(lead, runs)| (lead, runs.into_iter().map(Run::restored).collect()))
            .collect();
        Self {
            by_lead,
            ..Self::default()
        }
    }
    fn latest(&self, lead: i64) -> Option<&Run> {
        self.by_lead.get(&lead)?.last()
    }
    fn latest_mut(&mut self, lead: i64) -> Option<&mut Run> {
        self.by_lead.get_mut(&lead)?.last_mut()
    }
    pub(super) fn active(&self, lead: i64) -> bool {
        self.latest(lead).is_some_and(Run::active)
    }
    pub(super) fn has_runs(&self, lead: i64) -> bool {
        self.by_lead.contains_key(&lead)
    }
    /// The lead and assignment a worker session belongs to.
    fn assignment(&self, session: i64) -> Option<(i64, usize)> {
        self.by_lead.iter().find_map(|(lead, runs)| {
            let ix = runs
                .last()?
                .tasks
                .iter()
                .position(|task| task.session == Some(session))?;
            Some((*lead, ix))
        })
    }
}

/// The lead's settings for its workers, without the lead's one-shot state.
fn worker_settings(lead: &Conversation) -> Settings {
    let mut settings = lead.settings.clone();
    settings.model = lead.current_model();
    settings.plan = false;
    settings.orchestrator = false;
    settings.operator = false;
    settings.operator_enabled = false;
    settings.handoff = None;
    settings.pending_provider_switch = None;
    settings.remote_action = None;
    settings.prepared_prompt = None;
    settings.usage_limit = None;
    settings.turn_model = None;
    settings.new_worktree = false;
    settings.worktree_base = None;
    settings.mono = None;
    settings.mono_message_id = None;
    settings.internal_message = false;
    settings.turns.clear();
    settings.environment.clear();
    settings.attachments.clear();
    settings
}

impl Shell {
    pub(super) fn observe_orchestration(
        &mut self,
        view: &Entity<Conversation>,
        id: i64,
        cx: &mut Context<Self>,
    ) {
        self.observations.push(cx.subscribe(
            view,
            move |this, _, event: &OrchestrationAction, cx| {
                this.orchestration_action(id, event.clone(), cx)
            },
        ));
        let transcript = view.read(cx).transcript.clone();
        self.observations.push(cx.subscribe(
            &transcript,
            move |this, _, event: &OrchestrationAction, cx| {
                this.orchestration_action(id, event.clone(), cx)
            },
        ));
        // Workers stream between session events; their cards follow along.
        self.observations.push(cx.observe(view, move |this, _, cx| {
            this.orchestration_worker_changed(id, cx)
        }));
    }

    pub(super) fn orchestration_action(
        &mut self,
        lead: i64,
        event: OrchestrationAction,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.sessions.get(&lead).cloned() else {
            return;
        };
        let pending = view
            .read(cx)
            .orchestration_plan
            .as_ref()
            .map(|(call, _)| call.clone().unwrap_or_default());
        let latest = self
            .orchestrations
            .latest(lead)
            .map(|run| run.call.clone().unwrap_or_default());
        let reviewed = pending.as_ref() == Some(&event.call);
        let current = latest.as_ref() == Some(&event.call);
        match event.action {
            Action::Start if reviewed => self.start_orchestration(lead, cx),
            Action::Dismiss if reviewed => {
                view.update(cx, |v, cx| v.dismiss_orchestration_plan(cx));
            }
            Action::Remove(id) if reviewed => {
                view.update(cx, |v, cx| v.remove_plan_task(&id, cx));
            }
            Action::Team(team) if reviewed => match team {
                Some(team) => {
                    self.orchestrations.teams.insert(lead, team);
                }
                None => {
                    self.orchestrations.teams.remove(&lead);
                }
            },
            Action::Stop if current => self.stop_orchestration(lead, cx),
            Action::Resume if current => self.resume_orchestration(lead, cx),
            Action::Retry(ix) if current => self.retry_assignment(lead, ix, cx),
            Action::SendReport if current => self.send_orchestration_report(lead, true, cx),
            _ => {}
        }
        self.push_orchestration_views(lead, cx);
    }

    /// Why the lead's plan cannot start now.
    fn orchestration_blocked(&self, lead: i64, cx: &App) -> Option<String> {
        let view = self.sessions.get(&lead)?.read(cx);
        if view.is_remote() {
            return Some("Orchestration is not available on remote projects.".into());
        }
        if view.busy || view.external_submission_pending() {
            return Some("Starts once this chat's current turn finishes.".into());
        }
        if self.orchestrations.active(lead) {
            return Some("This chat is already running an orchestration.".into());
        }
        let cwd = view
            .settings
            .worktree_cwd
            .as_deref()
            .unwrap_or(&view.summary.cwd);
        self.orchestrations
            .by_lead
            .iter()
            .any(|(other, runs)| {
                *other != lead
                    && runs
                        .last()
                        .is_some_and(|run| run.active() && run.cwd == cwd)
            })
            .then(|| "Another orchestration is using this checkout.".into())
    }

    pub(super) fn start_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        let Some(view) = self.sessions.get(&lead).cloned() else {
            return;
        };
        if let Some(reason) = self.orchestration_blocked(lead, cx) {
            self.error = Some(reason);
            cx.notify();
            return;
        }
        let Some((call, plan)) = view.update(cx, |v, cx| v.take_orchestration_plan(cx)) else {
            return;
        };
        if let Err(error) = orchestration::validate(&plan) {
            self.error = Some(error);
            cx.notify();
            return;
        }
        let conversation = view.read(cx);
        let cwd = conversation
            .settings
            .worktree_cwd
            .clone()
            .unwrap_or_else(|| conversation.summary.cwd.clone());
        let run = Run::new(
            plan,
            call,
            cwd,
            conversation.summary.provider.clone(),
            worker_settings(conversation),
        )
        .with_team(self.orchestrations.teams.get(&lead).copied());
        let runs = self.orchestrations.by_lead.entry(lead).or_default();
        runs.push(run);
        if runs.len() > KEPT_RUNS {
            runs.remove(0);
        }
        self.orchestrations.report_tried.remove(&lead);
        self.schedule_orchestration(lead, cx);
    }

    pub(super) fn stop_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        let Some(run) = self.orchestrations.latest_mut(lead) else {
            return;
        };
        run.stop();
        let running: Vec<i64> = run
            .tasks
            .iter()
            .filter(|task| task.state == State::Running)
            .filter_map(|task| task.session)
            .collect();
        for session in running {
            match self.sessions.get(&session) {
                // The stop comes back as the worker's outcome.
                Some(view) => view.update(cx, |v, cx| v.stop_if_busy(cx)),
                None => {
                    if let Some(run) = self.orchestrations.latest_mut(lead)
                        && let Some(ix) = run.tasks.iter().position(|t| t.session == Some(session))
                    {
                        run.finish(ix, Outcome::Stopped, "Stopped before it reported.");
                    }
                }
            }
        }
        self.schedule_orchestration(lead, cx);
    }

    fn resume_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        let Some(run) = self.orchestrations.latest_mut(lead) else {
            return;
        };
        let continued = run.resume();
        self.orchestrations.report_tried.remove(&lead);
        for ix in continued {
            self.relaunch_worker(lead, ix, cx);
        }
        self.schedule_orchestration(lead, cx);
    }

    fn retry_assignment(&mut self, lead: i64, ix: usize, cx: &mut Context<Self>) {
        let Some(run) = self.orchestrations.latest_mut(lead) else {
            return;
        };
        if ix >= run.tasks.len() {
            return;
        }
        let continues = run.retry(ix);
        self.orchestrations.report_tried.remove(&lead);
        if continues {
            self.relaunch_worker(lead, ix, cx);
        }
        self.schedule_orchestration(lead, cx);
    }

    /// Sends a finished run's report to the lead. The automatic attempt waits
    /// for an idle lead with an empty composer; the button only for idle.
    fn send_orchestration_report(&mut self, lead: i64, manual: bool, cx: &mut Context<Self>) {
        let Some(run) = self.orchestrations.latest(lead) else {
            return;
        };
        if run.reported || !run.finished() {
            if manual && !run.finished() {
                self.error = Some("Wait for the workers to stop before sending the report.".into());
                cx.notify();
            }
            return;
        }
        let Some(view) = self.sessions.get(&lead).cloned() else {
            return;
        };
        let ready = {
            let v = view.read(cx);
            if manual {
                !v.busy && !v.external_submission_pending()
            } else {
                v.available_for_managed_run(cx)
            }
        };
        if !ready {
            if manual {
                self.error = Some("Send the report once this chat's current turn finishes.".into());
                cx.notify();
            }
            return;
        }
        let report = run.report();
        if !manual {
            self.orchestrations.report_tried.insert(lead);
        }
        if view.update(cx, |v, cx| v.send_coordinator_report(report, cx)) {
            if let Some(run) = self.orchestrations.latest_mut(lead) {
                run.reported = true;
            }
            self.save_orchestration(lead, cx);
        } else {
            let status = view.read(cx).status_text().to_owned();
            self.error = Some(format!(
                "The orchestration report could not be sent: {status}"
            ));
            cx.notify();
        }
    }

    pub(super) fn orchestration_session_changed(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some((lead, ix)) = self.orchestrations.assignment(id) {
            self.settle_worker(lead, ix, cx);
            self.schedule_orchestration(lead, cx);
        }
        // The lead's own turns decide whether its plan can start and when its
        // report goes out.
        if self.orchestrations.has_runs(id) {
            self.schedule_orchestration(id, cx);
        } else if self.sessions.get(&id).is_some_and(|v| {
            let v = v.read(cx);
            v.orchestration_plan.is_some() || v.orchestration_active
        }) {
            self.push_orchestration_views(id, cx);
        }
    }

    /// Records a worker's outcome once its turn has really ended.
    fn settle_worker(&mut self, lead: i64, ix: usize, cx: &mut Context<Self>) {
        let Some(run) = self.orchestrations.latest(lead) else {
            return;
        };
        let task = &run.tasks[ix];
        let Some(session) = task.session.filter(|_| task.state == State::Running) else {
            return;
        };
        let Some(view) = self.sessions.get(&session) else {
            return;
        };
        let worker = view.read(cx);
        if worker.busy || worker.external_submission_pending() || worker.thread_pending() {
            return;
        }
        // No turn yet: the launch check decides whether it ever starts.
        if self
            .orchestrations
            .launches
            .get(&session)
            .is_some_and(|before| *before == worker.turn_generation())
        {
            return;
        }
        let (outcome, report, edited) = worker.worker_outcome(cx);
        self.orchestrations.launches.remove(&session);
        if let Some(run) = self.orchestrations.latest_mut(lead) {
            let outside = orchestration::outside_scope(&run.plan.tasks[ix], &run.cwd, &edited);
            run.tasks[ix].outside_scope = outside;
            run.finish(ix, outcome, &report);
            // A tiered worker that failed mid-run is replaced by a stronger one.
            run.escalate(ix);
        }
    }

    /// Fails a worker whose turn never began. Launches can be dropped without
    /// an error (account switching, worktree setup, shutdown).
    fn watch_launch(&mut self, lead: i64, session: i64, before: u64, cx: &mut Context<Self>) {
        self.orchestrations.launches.insert(session, before);
        cx.spawn(async move |this, cx| {
            for _ in 0..LAUNCH_CHECKS {
                cx.background_executor().timer(LAUNCH_GRACE).await;
                let done = this
                    .update(cx, |this, cx| this.check_launch(lead, session, before, cx))
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();
    }
    /// True once the launch needs no more checks.
    fn check_launch(
        &mut self,
        lead: i64,
        session: i64,
        before: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.orchestrations.launches.get(&session) != Some(&before) {
            return true;
        }
        let Some(view) = self.sessions.get(&session) else {
            return true;
        };
        let worker = view.read(cx);
        if worker.turn_generation() != before {
            self.orchestrations.launches.remove(&session);
            if let Some((owner, ix)) = self.orchestrations.assignment(session)
                && owner == lead
            {
                self.settle_worker(lead, ix, cx);
                self.schedule_orchestration(lead, cx);
            }
            return true;
        }
        if worker.busy || worker.external_submission_pending() {
            return false;
        }
        let status = worker.status_text().to_owned();
        self.orchestrations.launches.remove(&session);
        if let Some(run) = self.orchestrations.latest_mut(lead)
            && let Some(ix) = run
                .tasks
                .iter()
                .position(|t| t.session == Some(session) && t.state == State::Running)
        {
            run.finish(ix, Outcome::NotStarted, &status);
            self.schedule_orchestration(lead, cx);
        }
        true
    }

    /// Dispatches what can start, keeps the sidebar and cards current, saves
    /// the run and hands a finished run's report back to the lead.
    fn schedule_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        let Some(run) = self.orchestrations.latest_mut(lead) else {
            self.push_orchestration_views(lead, cx);
            return;
        };
        let starts = run.dispatch();
        let mut summary = run.summary();
        // The lead's thread-tool children share its sidebar rail.
        if let Some(previous) = self.orchestration_summaries.get(&lead) {
            summary.tasks.extend(
                previous
                    .tasks
                    .iter()
                    .filter(|t| {
                        t.session
                            .is_some_and(|child| self.thread_parents.get(&child) == Some(&lead))
                    })
                    .cloned(),
            );
        }
        self.set_agent_summary(lead, summary, cx);
        self.save_orchestration(lead, cx);
        for ix in starts {
            self.launch_worker(lead, ix, cx);
        }
        let awaiting = self
            .orchestrations
            .latest(lead)
            .is_some_and(Run::awaiting_report);
        if awaiting && !self.orchestrations.report_tried.contains(&lead) {
            self.send_orchestration_report(lead, false, cx);
        }
        self.push_orchestration_views(lead, cx);
        self.invalidate_chrome(cx);
        cx.notify();
    }

    fn save_orchestration(&mut self, lead: i64, cx: &mut Context<Self>) {
        let runs = self
            .orchestrations
            .by_lead
            .get(&lead)
            .cloned()
            .unwrap_or_default();
        let previous = self.orchestrations.writes.remove(&lead);
        let store = self.store.clone();
        let write = cx.spawn(async move |_, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            cx.background_executor()
                .spawn(async move {
                    let _ = locked(&store).and_then(|s| s.save_orchestration_runs(lead, &runs));
                })
                .await;
        });
        self.orchestrations.writes.insert(lead, write);
    }

    /// Creates a worker session for a newly dispatched assignment.
    fn launch_worker(&mut self, lead: i64, ix: usize, cx: &mut Context<Self>) {
        let Some(project) = self
            .sessions
            .get(&lead)
            .map(|v| v.read(cx).summary.cwd.clone())
        else {
            return;
        };
        let Some(run) = self.orchestrations.latest(lead) else {
            return;
        };
        let task = run.plan.tasks[ix].clone();
        let cwd = run.cwd.clone();
        let mut settings = run.settings.clone();
        let provider = match task.provider.clone() {
            Some(provider) if provider != run.provider => {
                // The lead's model and account belong to its own harness.
                settings.model = None;
                settings.effort = None;
                settings.fast = false;
                settings.service_tier = None;
                settings.thinking = None;
                settings.model_settings.clear();
                settings.account = Default::default();
                provider
            }
            _ => run.provider.clone(),
        };
        if let Some(model) = run.model_for(ix) {
            settings.model = Some(model);
            settings.model_settings.clear();
        }
        let prompt = run.worker_prompt(ix);
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let root = std::path::Path::new(&cwd)
                .canonicalize()
                .map_err(|e| e.to_string())?;
            // Existing symlink parents must not redirect an assignment outside the checkout.
            for file in &task.files {
                let mut path = root.join(file);
                while !path.exists() {
                    if !path.pop() {
                        break;
                    }
                }
                if !path
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .starts_with(&root)
                {
                    return Err(format!("Assignment scope escapes the checkout: {file}"));
                }
            }
            let store = locked(&store)?;
            let mut summary = store.create(&provider, &project)?;
            summary.title = format!("Agent · {}", task.title);
            store.title(summary.id, &summary.title)?;
            settings.worktree_cwd = Some(cwd);
            store.save_agent_settings(summary.id, &settings)?;
            Ok::<_, String>((summary, settings))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let starting = this.orchestrations.latest(lead).is_some_and(|run| {
                    run.tasks
                        .get(ix)
                        .is_some_and(|t| t.state == State::Starting)
                });
                match result {
                    Ok((summary, settings)) if starting => {
                        let id = summary.id;
                        if let Some(run) = this.orchestrations.latest_mut(lead) {
                            run.tasks[ix].session = Some(id);
                            run.tasks[ix].state = State::Running;
                        }
                        // The sidebar lists it under the lead before its view exists.
                        let summary_now = this.orchestrations.latest(lead).map(Run::summary);
                        if let Some(summary_now) = summary_now {
                            this.set_agent_summary(lead, summary_now, cx);
                        }
                        this.add(summary, cx);
                        if let Some(view) = this.sessions.get(&id).cloned() {
                            let before = view.read(cx).turn_generation();
                            view.update(cx, |v, cx| {
                                v.managed = true;
                                v.install_lead_transfer(settings, Some(prompt), cx)
                            });
                            this.watch_launch(lead, id, before, cx);
                        }
                    }
                    Ok((summary, _)) => {
                        // Stopped while it was being created: nothing to keep.
                        let store = this.store.clone();
                        cx.background_executor()
                            .spawn(async move {
                                let _ = locked(&store).and_then(|mut s| s.delete(summary.id));
                            })
                            .detach();
                    }
                    Err(error) => {
                        if let Some(run) = this.orchestrations.latest_mut(lead)
                            && run.tasks[ix].state == State::Starting
                        {
                            run.finish(ix, Outcome::Failed, &error);
                        }
                    }
                }
                this.schedule_orchestration(lead, cx);
            });
        })
        .detach();
    }

    /// Sends a retried or resumed assignment back to its worker session.
    fn relaunch_worker(&mut self, lead: i64, ix: usize, cx: &mut Context<Self>) {
        let Some(run) = self.orchestrations.latest(lead) else {
            return;
        };
        let task = &run.tasks[ix];
        let Some(session) = task.session else {
            return;
        };
        let prompt = if task.launched {
            orchestration::CONTINUE_PROMPT.to_owned()
        } else {
            run.worker_prompt(ix)
        };
        let failure = match self.ensure_session(session, cx) {
            None => Some("The worker chat no longer exists."),
            Some(view) if view.read(cx).busy || view.read(cx).external_submission_pending() => {
                Some("The worker was busy. Retry once it is idle.")
            }
            Some(view) => {
                let before = view.read(cx).turn_generation();
                view.update(cx, |v, cx| {
                    v.managed = true;
                    v.send_thread_prompt(prompt, cx)
                });
                self.watch_launch(lead, session, before, cx);
                None
            }
        };
        if let Some(failure) = failure
            && let Some(run) = self.orchestrations.latest_mut(lead)
        {
            run.finish(ix, Outcome::Failed, failure);
        }
    }

    /// Workers' streaming progress, at most once a second per lead.
    fn orchestration_worker_changed(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some((lead, _)) = self.orchestrations.assignment(id) else {
            return;
        };
        if !self.orchestrations.refreshing.insert(lead) {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                this.orchestrations.refreshing.remove(&lead);
                this.push_orchestration_views(lead, cx);
            });
        })
        .detach();
    }

    /// The lead's cards: its recent runs, and the plan under review.
    pub(super) fn push_orchestration_views(&mut self, lead: i64, cx: &mut Context<Self>) {
        let Some(view) = self.sessions.get(&lead).cloned() else {
            return;
        };
        let conversation = view.read(cx);
        let runs = self.orchestrations.by_lead.get(&lead);
        let count = runs.map_or(0, Vec::len);
        let lead_busy = conversation.busy || conversation.external_submission_pending();
        let mut views: Vec<(Option<String>, RunView)> = runs
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(at, run)| {
                let phase = if at + 1 < count {
                    Phase::Inactive
                } else if run.paused {
                    Phase::Paused
                } else if run.stopped {
                    Phase::Stopped
                } else if !run.finished() {
                    Phase::Running
                } else {
                    Phase::Finished {
                        reported: run.reported,
                        sending: (!run.reported && lead_busy)
                            .then(|| "The report is sent once this chat is idle.".into()),
                    }
                };
                let tasks = run
                    .plan
                    .tasks
                    .iter()
                    .zip(&run.tasks)
                    .enumerate()
                    .map(|(ix, (task, state))| {
                        let worker = state
                            .session
                            .and_then(|id| self.sessions.get(&id))
                            .map(|v| v.read(cx));
                        let running = state.state == State::Running;
                        let note = run.waiting_on(ix).or_else(|| match state.state {
                            State::Blocked => Some("a dependency did not finish".into()),
                            State::Interrupted => Some("interrupted by a restart".into()),
                            State::Failed => state
                                .report
                                .lines()
                                .find(|line| !line.trim().is_empty())
                                .map(|line| monocode_core::response::limit(line.trim(), 120)),
                            _ => None,
                        });
                        TaskView {
                            state: state.state,
                            needs_input: running
                                && worker.is_some_and(|w| w.mobile_waiting(cx).is_some()),
                            session: state.session,
                            harness: task
                                .provider
                                .clone()
                                .unwrap_or_else(|| run.provider.clone()),
                            model: worker
                                .map(|w| w.driver_label())
                                .or_else(|| run.model_for(ix))
                                .or_else(|| run.settings.model.clone())
                                .unwrap_or_default(),
                            steps: if running || state.state.settled() {
                                worker.map_or(0, |w| w.turn_steps(cx))
                            } else {
                                0
                            },
                            note,
                            outside_scope: state.outside_scope.clone(),
                            report: monocode_core::response::limit_middle(
                                state.report.trim(),
                                1_200,
                            ),
                        }
                    })
                    .collect();
                (
                    run.call.clone(),
                    RunView {
                        plan: run.plan.clone(),
                        phase,
                        tasks,
                        team: run.team,
                    },
                )
            })
            .collect();
        if let Some((call, plan)) = &conversation.orchestration_plan {
            let blocked = self.orchestration_blocked(lead, cx);
            views.push((
                call.clone(),
                RunView::review(
                    plan.clone(),
                    Phase::Review { blocked },
                    &conversation.summary.provider,
                    &conversation.driver_label(),
                    self.orchestrations.teams.get(&lead).copied(),
                ),
            ));
        }
        let active = self.orchestrations.active(lead);
        view.update(cx, |v, cx| v.set_orchestration_views(views, active, cx));
    }

    /// A deleted lead's run ends with it; a deleted worker's assignment stops.
    pub(super) fn forget_orchestration(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some(runs) = self.orchestrations.by_lead.remove(&id) {
            self.orchestrations.writes.remove(&id);
            self.orchestrations.report_tried.remove(&id);
            for session in runs
                .last()
                .into_iter()
                .flat_map(|run| &run.tasks)
                .filter(|task| task.state == State::Running)
                .filter_map(|task| task.session)
            {
                if let Some(view) = self.sessions.get(&session) {
                    view.update(cx, |v, cx| v.stop_if_busy(cx));
                }
            }
        }
        self.orchestrations.launches.remove(&id);
        if let Some((lead, ix)) = self.orchestrations.assignment(id) {
            if let Some(run) = self.orchestrations.latest_mut(lead) {
                if matches!(run.tasks[ix].state, State::Starting | State::Running) {
                    run.finish(ix, Outcome::Stopped, "The worker chat was deleted.");
                }
                run.tasks[ix].session = None;
            }
            self.schedule_orchestration(lead, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use monocode_core::orchestration::{Plan, Task};

    fn plan(files: &[&str]) -> Plan {
        Plan {
            title: "Review".into(),
            summary: String::new(),
            tasks: files
                .iter()
                .enumerate()
                .map(|(ix, file)| Task {
                    id: format!("task-{ix}"),
                    title: format!("Assignment {ix}"),
                    prompt: "Inspect and report".into(),
                    files: vec![(*file).into()],
                    depends_on: Vec::new(),
                    provider: None,
                    model: None,
                    difficulty: None,
                })
                .collect(),
        }
    }

    #[gpui::test]
    fn reviewed_dispatch_caps_workers_serializes_scopes_and_stops_before_launch(
        cx: &mut TestAppContext,
    ) {
        let (window, _, lead) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&lead].update(cx, |v, cx| {
                    v.install_transfer(Settings::default(), None, cx);
                    v.orchestration_plan = Some((
                        Some("call-1".into()),
                        plan(&["src", "src/main.rs", "tests", "docs", "examples", "scripts"]),
                    ));
                });
                shell.start_orchestration(lead, cx);
                assert!(
                    shell.sessions[&lead].read(cx).orchestration_plan.is_none(),
                    "starting takes the plan out of review"
                );
                let run = shell.orchestrations.latest(lead).unwrap();
                assert_eq!(
                    run.tasks
                        .iter()
                        .filter(|t| t.state == State::Starting)
                        .count(),
                    4
                );
                assert_eq!(
                    run.tasks[1].state,
                    State::Pending,
                    "src/main.rs waits for src"
                );
                assert!(shell.orchestrations.active(lead));
                shell.stop_orchestration(lead, cx);
                let run = shell.orchestrations.latest(lead).unwrap();
                assert!(run.tasks.iter().all(|t| t.state == State::Stopped));
                assert!(!shell.orchestrations.active(lead));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |shell, _, cx| {
                assert!(shell.sessions.values().all(|v| !v.read(cx).busy));
                // Workers created after the stop are deleted, not kept as empty chats.
                assert!(
                    shell
                        .orchestrations
                        .latest(lead)
                        .unwrap()
                        .tasks
                        .iter()
                        .all(|t| t.session.is_none())
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn the_team_chosen_on_the_review_card_sets_each_claude_workers_model(cx: &mut TestAppContext) {
        let (window, _, lead) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&lead].update(cx, |v, cx| {
                    v.install_transfer(Settings::default(), None, cx);
                    v.summary.provider = "claude".into();
                    let mut plan = plan(&["src", "tests"]);
                    plan.tasks[0].difficulty = Some(9);
                    plan.tasks[1].difficulty = Some(2);
                    v.orchestration_plan = Some((Some("call-1".into()), plan));
                });
                let pick = |shell: &mut Shell, call: &str, team, cx: &mut Context<Shell>| {
                    shell.orchestration_action(
                        lead,
                        OrchestrationAction {
                            call: call.into(),
                            action: Action::Team(team),
                        },
                        cx,
                    )
                };
                // A card that is not the plan under review changes nothing.
                pick(shell, "other-call", Some(Team::Strong), cx);
                assert_eq!(shell.orchestrations.teams.get(&lead), None);
                pick(shell, "call-1", Some(Team::Balanced), cx);
                assert_eq!(shell.orchestrations.teams.get(&lead), Some(&Team::Balanced));
                pick(shell, "call-1", None, cx);
                assert_eq!(shell.orchestrations.teams.get(&lead), None);
                pick(shell, "call-1", Some(Team::Cheap), cx);
                shell.start_orchestration(lead, cx);
                let run = shell.orchestrations.latest(lead).unwrap();
                assert_eq!(run.team, Some(Team::Cheap));
                assert_eq!(run.model_for(0).as_deref(), Some("sonnet"), "level 9");
                assert_eq!(run.model_for(1).as_deref(), Some("haiku"), "level 2");
                // The choice cannot change once the run has started.
                pick(shell, "call-1", Some(Team::Strong), cx);
                assert_eq!(
                    shell.orchestrations.latest(lead).unwrap().team,
                    Some(Team::Cheap)
                );
                shell.stop_orchestration(lead, cx);
            })
            .unwrap();
        cx.run_until_parked();
    }

    #[gpui::test]
    fn a_busy_lead_cannot_start_and_a_deleted_lead_ends_its_run(cx: &mut TestAppContext) {
        let (window, _, lead) = crate::shell::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                shell.sessions[&lead].update(cx, |v, cx| {
                    v.install_transfer(Settings::default(), None, cx);
                    v.orchestration_plan = Some((Some("call-1".into()), plan(&["src"])));
                    v.busy = true;
                });
                shell.start_orchestration(lead, cx);
                assert!(
                    shell
                        .error
                        .as_deref()
                        .is_some_and(|e| e.contains("finishes"))
                );
                assert!(shell.orchestrations.latest(lead).is_none());
                assert!(shell.sessions[&lead].read(cx).orchestration_plan.is_some());
                shell.sessions[&lead].update(cx, |v, _| v.busy = false);
                shell.start_orchestration(lead, cx);
                assert!(shell.orchestrations.active(lead));
                shell.forget_orchestration(lead, cx);
                assert!(shell.orchestrations.latest(lead).is_none());
            })
            .unwrap();
        cx.run_until_parked();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod shots;
