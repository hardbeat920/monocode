//! The asynchronous half of `submitSession`: the draft worktree, the
//! handoff recap, the provider rewind for an edited resend, the turn itself
//! with its event routing, orchestration proposals, and the cleanup when the
//! turn ends.
//!
//! Harness adapters report events on any thread. They go through a channel
//! to this foreground task, which applies them in order and drains the rest
//! once the adapter's future resolves, so the order matches the TypeScript
//! callbacks.

use std::rc::Rc;

use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::{AsyncApp, WeakEntity};
use monocode_core::block::{Block, PlanStatus, TurnIntent};
use monocode_core::handoff::HandoffComposerCard;
use monocode_core::harness_event::{HarnessSessionInput, RewindLastTurnInput, SendTurnInput};
use monocode_core::orchestration::{
    OrchestrationProposal, OrchestrationProposalStatus, with_orchestration_proposal,
};
use monocode_core::paths::path_key;
use monocode_core::plan::{
    build_plan_prompt, is_provider_failure_text, plan_turn_key, plan_turn_prompt,
};
use monocode_core::reducer::{now_ms, promote_last_assistant_to_plan_mut, stop_streaming_mut};
use monocode_core::session::{PendingHarnessSwitch, session_work_cwd};
use monocode_core::{Attachment, HarnessEvent, HarnessId, Session, js};
use monocode_harness::core::registry::{AcceptedHook, EventSink, TitleInput};
use monocode_harness::core::text_harness::pick_text_harness;

use super::actions::nudge_after_turn;
use super::session_edits::{
    keep_tail, named_worktree_branch, shell_path, temporary_worktree_branch_name, with_plan_status,
};
use super::submit::{CommitTurn, reject_edited, report};
use super::{Submit, SubmitConfig, SubmitOptions, settle};
use crate::runtime::checkpoint::{begin_session_turn, notify_review_changed};
use crate::runtime::edits::{nudge_open_editors, track_session_edits};
use crate::runtime::engine::Engine;
use crate::runtime::in_flight::CONTINUE_PROMPT;
use crate::submit::acceptance::{ControlOutcome, ControlStatus};
use crate::submit::attachments::prepare_attachments;
use crate::submit::edit_last_turn::EditedResendAttempt;
use crate::submit::handoff::{
    choose_handoff_brief, complete_handoff, consume_handoff, is_preparing_handoff,
    should_ask_outgoing_agent, user_messages_after_handoff, wrap_handoff_prompt,
};
use crate::submit::handoff_turn::{OutgoingHandoffInput, request_outgoing_handoff};
use crate::submit::hooks::SubmitPeers;
use crate::submit::prompt::prepare_prompt;
use crate::submit::skills::{SkillCatalog, SkillCatalogContext};

/// What an adapter reports while a harness call runs.
pub(crate) enum TurnSignal {
    Event(Box<HarnessEvent>),
    /// `onAccepted`: the provider took the user turn.
    Accepted,
}

/// An event sink and accepted hook that feed one channel.
pub(crate) fn signal_channel() -> (EventSink, AcceptedHook, async_channel::Receiver<TurnSignal>) {
    let (sender, receiver) = async_channel::unbounded();
    let events = sender.clone();
    let sink: EventSink = std::sync::Arc::new(move |event| {
        let _ = events.try_send(TurnSignal::Event(Box::new(event)));
    });
    let accepted: AcceptedHook = std::sync::Arc::new(move || {
        let _ = sender.try_send(TurnSignal::Accepted);
    });
    (sink, accepted, receiver)
}

/// Run a harness call and hand each signal to `on_signal` as it arrives.
/// Signals sent before the call resolved are handled before it returns.
pub(crate) async fn drive<T>(
    operation: BoxFuture<'static, T>,
    signals: &async_channel::Receiver<TurnSignal>,
    mut on_signal: impl FnMut(TurnSignal),
) -> T {
    let mut operation = operation.fuse();
    loop {
        let next = signals.recv().fuse();
        futures::pin_mut!(next);
        futures::select_biased! {
            signal = next => match signal {
                Ok(signal) => on_signal(signal),
                Err(_) => {
                    let result = operation.await;
                    return result;
                }
            },
            result = operation => {
                while let Ok(signal) = signals.try_recv() {
                    on_signal(signal);
                }
                return result;
            }
        }
    }
}

/// `preparePrompt` with the prompt hooks.
pub(crate) async fn prepare(
    text: &str,
    harness: HarnessId,
    session_id: &str,
    cwd: &str,
    skills: &SkillCatalog,
    peers: &SubmitPeers,
    cx: &AsyncApp,
) -> String {
    let context = SkillCatalogContext::new(harness, cwd).with_session(session_id);
    prepare_prompt(
        text,
        &context,
        skills,
        |text| cx.update(|cx| peers.prompt.apply_file_mentions(text, cwd, cx)),
        |text| cx.update(|cx| peers.prompt.apply_notes(text, cx)),
    )
    .await
}

/// A recap waiting to reach the incoming provider.
#[derive(Debug, Clone)]
pub(crate) struct Wrap {
    pub from: HarnessId,
    pub text: String,
}

/// Everything the turn task needs, captured when the turn starts.
pub(crate) struct TurnRun {
    pub session_id: String,
    pub generation: u64,
    pub current: Session,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub options: SubmitOptions,
    pub intent: TurnIntent,
    pub harness_text: String,
    pub raw_command: bool,
    pub operator_matched: bool,
    pub operator_access: bool,
    pub provider_account_id: Option<String>,
    pub initial_work_cwd: String,
    pub create_draft_worktree: bool,
    pub is_first_turn: bool,
    #[allow(dead_code)]
    pub placeholder_title: bool,
    pub title_seed: String,
    pub generate_title: bool,
    pub approved_plan: Option<Block>,
    pub edited: Option<Rc<EditedResendAttempt>>,
    pub edited_provider_turn_id: Option<String>,
    pub pending_switch: Option<PendingHarnessSwitch>,
    pub queued_handoff: Option<Wrap>,
    pub handoff_card: Option<HandoffComposerCard>,
    pub proposal_id: Option<String>,
    pub proposal_draft: Option<OrchestrationProposal>,
    pub commit: Rc<CommitTurn>,
    pub config: SubmitConfig,
    pub peers: SubmitPeers,
    pub skills: SkillCatalog,
}

/// The turn's mutable state (the `let` bindings in the TypeScript closure).
struct TurnState {
    control_text: String,
    control_error: Option<String>,
    proposal: Option<OrchestrationProposal>,
    proposal_text: String,
    native_proposal_text: String,
    completed_proposal: Option<OrchestrationProposal>,
    provider_failure_seen: bool,
    native_plan_seen: bool,
    build_succeeded: bool,
    pending_edited_events: Vec<HarnessEvent>,
    work_cwd: String,
    wrap: Option<Wrap>,
    plan_event_key: String,
}

impl TurnState {
    fn proposal_response(&self) -> &str {
        if self.native_proposal_text.is_empty() {
            &self.proposal_text
        } else {
            &self.native_proposal_text
        }
    }
}

/// Run the turn and report the outcome to a managed caller.
pub(crate) async fn run_turn(this: WeakEntity<Submit>, run: TurnRun, cx: &mut AsyncApp) {
    let cx: &AsyncApp = cx;
    let mut state = TurnState {
        control_text: String::new(),
        control_error: Some("Turn did not complete".into()),
        proposal: run.proposal_draft.clone(),
        proposal_text: String::new(),
        native_proposal_text: String::new(),
        completed_proposal: None,
        provider_failure_seen: false,
        native_plan_seen: false,
        build_succeeded: false,
        pending_edited_events: Vec::new(),
        work_cwd: run.initial_work_cwd.clone(),
        wrap: None,
        plan_event_key: String::new(),
    };
    let mut outcome = ControlOutcome::failed("Turn did not complete");
    match run.outer(&mut state, cx).await {
        Ok(Some(finished)) => outcome = finished,
        Ok(None) => {}
        Err(message) => {
            outcome = ControlOutcome {
                status: ControlStatus::Failed,
                text: state.control_text.clone(),
                error: Some(message.clone()),
            };
            if run.gen_current(cx) {
                cx.update(|cx| {
                    report(
                        &run.session_id,
                        HarnessEvent::SessionError {
                            message: message.clone(),
                        },
                        cx,
                    )
                });
                run.fail_session(&state, &message, cx);
            }
        }
    }
    let cancelled = !run.gen_current(cx);
    cx.update(|cx| {
        reject_edited(run.edited.as_deref(), &run.options, cx);
        let outcome = if cancelled {
            ControlOutcome {
                status: ControlStatus::Cancelled,
                text: state.control_text.clone(),
                error: None,
            }
        } else {
            outcome
        };
        settle(&run.options, outcome, cx);
    });
    if run.edited.is_some() {
        let id = run.session_id.clone();
        let _ = this.update(&mut cx.clone(), |this, _| this.edited_resends.finish(&id));
    }
}

impl TurnRun {
    fn harness(&self) -> HarnessId {
        self.current.harness
    }

    fn gen_current(&self, cx: &AsyncApp) -> bool {
        cx.update(|cx| Engine::sessions(cx).read(cx).turn_gen(&self.session_id)) == self.generation
    }

    fn enqueue(&self, event: HarnessEvent, cx: &AsyncApp) {
        cx.update(|cx| {
            Engine::sessions(cx).update(cx, |sessions, cx| {
                sessions.enqueue_event(&self.session_id, event, cx)
            });
        });
    }

    fn flush(&self, cx: &AsyncApp) {
        cx.update(|cx| Engine::sessions(cx).update(cx, |sessions, cx| sessions.flush(cx)));
    }

    fn update_session(&self, cx: &AsyncApp, update: impl FnOnce(&mut Session)) {
        cx.update(|cx| {
            Engine::sessions(cx).update(cx, |sessions, cx| {
                sessions.update(&self.session_id, cx, update);
            });
        });
    }

    fn latest(&self, cx: &AsyncApp) -> Option<Session> {
        cx.update(|cx| Engine::sessions(cx).read(cx).get(&self.session_id).cloned())
    }

    /// The outer `catch`: end the turn with the error on the transcript.
    fn fail_session(&self, state: &TurnState, message: &str, cx: &AsyncApp) {
        let proposal = self.proposal_id.clone().zip(state.proposal.clone());
        let peers = self.peers.clone();
        self.update_session(cx, |session| {
            stop_streaming_mut(session, now_ms());
            session.worktree_preparing = None;
            if let Some((id, draft)) = proposal {
                let failed = peers
                    .orchestration
                    .complete_proposal(&draft, "", Some(message));
                *session = with_orchestration_proposal(session, &id, &failed);
            }
        });
    }

    /// Everything after the optimistic commit. `Err` is the outer `catch`;
    /// `Ok(Some(outcome))` is the turn's outcome when it ran to the end.
    async fn outer(
        &self,
        state: &mut TurnState,
        cx: &AsyncApp,
    ) -> Result<Option<ControlOutcome>, String> {
        let id = self.session_id.as_str();
        let registry = self.config.registry.clone();
        if self.create_draft_worktree {
            let branch = temporary_worktree_branch_name(&uuid::Uuid::new_v4().to_string());
            let base = self
                .current
                .worktree_base
                .clone()
                .filter(|base| !base.is_empty())
                .unwrap_or_else(|| "HEAD".into());
            let tree = cx
                .update(|cx| {
                    self.peers.projects.create_worktree(
                        &self.current.cwd,
                        &branch,
                        &base,
                        false,
                        cx,
                    )
                })
                .await?;
            state.work_cwd = tree.path.clone();
            if let Some(proposal) = &mut state.proposal {
                proposal.checkout_cwd = Some(tree.path.clone());
            }
            let (path, tree_branch) = (tree.path.clone(), tree.branch.clone());
            self.update_session(cx, |session| {
                session.worktree_cwd = Some(path);
                session.branch = tree_branch;
                session.workspace_mode = None;
                session.worktree_base = None;
                session.worktree_preparing = None;
            });
            cx.update(|cx| notify_review_changed(Some(id), cx));
            self.name_worktree_branch(&tree.path, cx);
        }
        self.launch_title_generation(&state.work_cwd, cx);
        if !self.gen_current(cx) {
            return Ok(None);
        }
        if let (Some(proposal_id), Some(proposal)) = (&self.proposal_id, &mut state.proposal) {
            let settings = cx
                .update(|cx| self.peers.orchestration.discover_settings(cx))
                .await;
            if !self.gen_current(cx) {
                return Ok(None);
            }
            proposal.settings = settings;
            let discovering = proposal.clone();
            self.update_session(cx, |session| {
                *session = with_orchestration_proposal(session, proposal_id, &discovering);
            });
        }
        state.wrap = self
            .handoff_card
            .as_ref()
            .map(|card| Wrap {
                from: card.from,
                text: card.brief.clone(),
            })
            .or_else(|| self.queued_handoff.clone());
        if let Some(pending) = &self.pending_switch {
            let mut agent_text = String::new();
            if should_ask_outgoing_agent(&self.current) && registry.is_live_harness(pending.from) {
                agent_text = request_outgoing_handoff(
                    &registry,
                    OutgoingHandoffInput {
                        harness: pending.from,
                        session_id: id.to_string(),
                        cwd: state.work_cwd.clone(),
                        model: pending.from_model.clone(),
                        model_settings: Some(pending.from_settings.clone()),
                        provider_account_id: pending.from_provider_account_id.clone(),
                        user_request: self.text.clone(),
                    },
                )
                .await;
            }
            if !self.gen_current(cx) {
                return Ok(None);
            }
            let latest = self.latest(cx).unwrap_or_else(|| self.current.clone());
            let brief = choose_handoff_brief(&agent_text, &latest, Some(&self.text));
            registry
                .forget_harness_session(pending.from, id)
                .await
                .map_err(|error| error.to_string())?;
            if !self.gen_current(cx) {
                return Ok(None);
            }
            state.wrap = Some(Wrap {
                from: pending.from,
                text: brief,
            });
        }

        state.plan_event_key =
            plan_turn_key(self.generation as i64, &uuid::Uuid::new_v4().to_string());
        let in_orchestration = cx
            .update(|cx| self.peers.orchestration.run_status_for_session(id, cx))
            .is_some();
        if self.current.inbox_ask.is_none() && !in_orchestration {
            let _ = cx
                .update(|cx| begin_session_turn(id, &state.work_cwd, cx))
                .await;
        }
        if !self.gen_current(cx) {
            return Ok(None);
        }

        if let Err(message) = self.attempt(state, cx).await {
            self.recover_edited_resend(state, cx);
            if !self.gen_current(cx) {
                return Ok(None);
            }
            if let Some(wrap) = state.wrap.clone() {
                self.reveal_handoff(&wrap.text, cx);
            }
            let message = if message.is_empty() {
                format!("{} adapter failed", self.harness())
            } else {
                message
            };
            state.control_error = Some(message.clone());
            if !state.provider_failure_seen {
                self.enqueue(HarnessEvent::SessionError { message }, cx);
            }
            state.provider_failure_seen = true;
        }
        if !self.gen_current(cx) {
            return Ok(None);
        }
        Ok(Some(self.finish(state, cx).await))
    }

    /// The inner `try`. `Err` carries the message the `catch` reports.
    async fn attempt(&self, state: &mut TurnState, cx: &AsyncApp) -> Result<(), String> {
        let id = self.session_id.as_str();
        let registry = self.config.registry.clone();
        let harness = self.harness();
        let prepared =
            prepare_attachments(self.config.attachment_io.as_ref(), &self.attachments).await;
        let prompt = match (&self.approved_plan, self.intent) {
            (Some(plan), TurnIntent::Build) => build_plan_prompt(&plan.text),
            _ => {
                prepare(
                    &self.harness_text,
                    harness,
                    id,
                    &state.work_cwd,
                    &self.skills,
                    &self.peers,
                    cx,
                )
                .await
            }
        };
        let turn_prompt = match &state.proposal {
            Some(proposal) => match self.options.orchestration_retry.as_ref().filter(|retry| {
                retry
                    .response
                    .as_deref()
                    .is_some_and(|response| !response.is_empty())
            }) {
                Some(retry) => self
                    .peers
                    .orchestration
                    .repair_prompt(&OrchestrationProposal {
                        error: retry.error.clone(),
                        response: retry.response.clone(),
                        ..proposal.clone()
                    }),
                None => self.peers.orchestration.planning_prompt(
                    &prompt,
                    &proposal.settings,
                    proposal.checkout_cwd.as_deref().unwrap_or(&proposal.cwd),
                ),
            },
            None if self.intent == TurnIntent::Plan && !self.raw_command => {
                plan_turn_prompt(&prompt)
            }
            None => prompt,
        };
        let earlier = if self.queued_handoff.is_some() {
            user_messages_after_handoff(&self.current)
        } else {
            Vec::new()
        };
        if let Some(edited) = &self.edited
            && registry.can_rewind_harness_last_turn(harness)
        {
            let (sink, _accepted, signals) = signal_channel();
            let rewind = registry.rewind_harness_last_turn(
                harness,
                RewindLastTurnInput {
                    session: self.session_input(&state.work_cwd),
                    provider_turn_id: self.edited_provider_turn_id.clone(),
                    text: None,
                    attachments: None,
                },
                sink,
            );
            let rewound = drive(rewind, &signals, |signal| {
                if let TurnSignal::Event(event) = signal
                    && self.gen_current(cx)
                {
                    self.enqueue(*event, cx);
                }
            })
            .await;
            if let Err(error) = rewound {
                self.flush(cx);
                cx.update(|cx| reject_edited(Some(edited), &self.options, cx));
                return Err(error.to_string());
            }
            edited.mark_provider_rewound();
            self.flush(cx);
            if !self.gen_current(cx) {
                let latest = self.latest(cx);
                let untouched = latest.as_ref().is_none_or(|latest| {
                    !latest.is_busy()
                        && latest.provider_session_id == self.current.provider_session_id
                });
                if untouched {
                    let _ = registry.forget_harness_session(harness, id).await;
                    if latest.is_some() {
                        let provider = self.current.provider_session_id.clone();
                        self.update_session(cx, |session| {
                            if session.provider_session_id == provider {
                                session.provider_session_id = None;
                            }
                        });
                    }
                }
                self.recover_edited_resend(state, cx);
                return Ok(());
            }
        }

        let inbox_ask = (!self.raw_command)
            .then(|| self.current.inbox_ask.clone())
            .flatten();
        let body = match &state.wrap {
            Some(wrap) if !self.raw_command => {
                let request = js::trim(&turn_prompt);
                wrap_handoff_prompt(
                    &wrap.text,
                    wrap.from,
                    if request.is_empty() {
                        CONTINUE_PROMPT
                    } else {
                        request
                    },
                    &earlier,
                )
            }
            _ => turn_prompt,
        };
        let body = self.peers.inbox.ask_prompt(inbox_ask.as_ref(), body);
        let mut send_text = cx.update(|cx| self.peers.orchestration.prompt(id, body, cx));
        if self.operator_matched {
            let cli = format!("{} app", shell_path(&(self.config.app_cli_path)()?));
            send_text.push_str(&format!(
                "\n\n<monocode_app>\nThe user's Operator command enables app access in this thread, including later turns without the command. You can start session tabs or split session panes right or down, list and create project worktrees, choose a new session's checkout, read and continue other project sessions, save unsent drafts, organize session folders, and read or write saved notes through its local CLI. Run `{cli} --help` for exact commands and JSON fields, then use it as needed for the user's request. When reading another session, start with its latest two or three user/assistant exchanges. Request older exchanges with nextBefore or a larger excerpt only if needed. The CLI uses a session credential already in your environment; never print it. New sessions inherit this session's permission mode unless runtimeMode is set explicitly. For a new session with a draft, call sessions.start with its prompt and draft:true; do not submit a seed prompt. The returned ID can be used as besideSessionId to split its pane again or moved into a folder immediately. A normal sessions.start submits its prompt but returns after acceptance, so do not wait for that agent to finish before organizing it.\n</monocode_app>"
            ));
        }
        self.send_turn(send_text, prepared, state, cx).await?;
        self.accept_edited_resend(state, cx);
        if let Some(draft) = state.proposal.clone()
            && !state.provider_failure_seen
        {
            // `completeOrRepairOrchestrationProposal`.
            let first =
                self.peers
                    .orchestration
                    .complete_proposal(&draft, state.proposal_response(), None);
            let can_repair =
                self.gen_current(cx) && !is_provider_failure_text(state.proposal_response());
            let completed = if first.status != OrchestrationProposalStatus::Invalid || !can_repair {
                first
            } else {
                state.proposal_text.clear();
                state.native_proposal_text.clear();
                let repair = self.peers.orchestration.repair_prompt(&first);
                self.send_turn(repair, Vec::new(), state, cx).await?;
                if state.provider_failure_seen {
                    return Err(state
                        .control_error
                        .clone()
                        .unwrap_or_else(|| "The lead could not repair the proposal.".into()));
                }
                self.peers
                    .orchestration
                    .complete_proposal(&draft, state.proposal_response(), None)
            };
            state.completed_proposal = Some(completed);
        }
        if !self.gen_current(cx) {
            return Ok(());
        }
        if let Some(wrap) = state.wrap.clone() {
            let raw = self.raw_command;
            self.update_session(cx, |session| {
                if is_preparing_handoff(session) {
                    *session = complete_handoff(session, &wrap.text);
                }
                // A command owns its arguments; deliver the recap with the next chat prompt.
                if !raw {
                    *session = consume_handoff(session);
                }
            });
        }
        state.build_succeeded = true;
        Ok(())
    }

    fn session_input(&self, work_cwd: &str) -> HarnessSessionInput {
        HarnessSessionInput {
            session_id: self.session_id.clone(),
            cwd: work_cwd.to_string(),
            model: self.current.model.clone(),
            model_settings: Some(self.current.model_settings.clone()),
            provider_account_id: self.provider_account_id.clone(),
            runtime_mode: self.current.runtime_mode,
            intent: None,
            controls_agents: None,
            app_access: None,
        }
    }

    /// `sendTurn`.
    async fn send_turn(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        state: &mut TurnState,
        cx: &AsyncApp,
    ) -> Result<(), String> {
        let lead_active = cx
            .update(|cx| {
                self.peers
                    .orchestration
                    .led_run_status(&self.session_id, cx)
            })
            .is_some_and(|status| status == "active");
        let input = SendTurnInput {
            session: HarnessSessionInput {
                intent: Some(if self.intent == TurnIntent::Orchestrate {
                    TurnIntent::Plan
                } else {
                    self.intent
                }),
                // A /operator user turn enables app access for this thread;
                // orchestration leads retain their separate control access.
                controls_agents: Some(self.operator_access || lead_active),
                app_access: Some(self.operator_access),
                ..self.session_input(&state.work_cwd)
            },
            text,
            attachments: Some(attachments),
        };
        let (sink, accepted, signals) = signal_channel();
        let send = self.config.registry.send_harness_turn(
            self.harness(),
            input,
            sink,
            self.edited.is_some().then_some(accepted),
        );
        drive(send, &signals, |signal| match signal {
            TurnSignal::Accepted => self.accept_edited_resend(state, cx),
            TurnSignal::Event(event) => self.route_turn_event(*event, state, cx),
        })
        .await
        .map_err(|error| error.to_string())
    }

    /// `routeTurnEvent`: drop events from a superseded turn, and hold an
    /// edited resend's events until the provider accepts it.
    fn route_turn_event(&self, event: HarnessEvent, state: &mut TurnState, cx: &AsyncApp) {
        if !self.gen_current(cx) {
            return;
        }
        if let Some(edited) = &self.edited
            && !edited.is_accepted()
        {
            state.pending_edited_events.push(event);
            return;
        }
        self.apply_turn_event(event, state, cx);
    }

    /// `applyTurnEvent`.
    fn apply_turn_event(&self, event: HarnessEvent, state: &mut TurnState, cx: &AsyncApp) {
        cx.update(|cx| {
            self.peers
                .orchestration
                .observe(&self.session_id, &event, cx)
        });
        if self.options.on_settled.is_some() {
            match &event {
                HarnessEvent::MessageDelta { text } => {
                    state.control_text.push_str(text);
                    keep_tail(&mut state.control_text, 20_000);
                }
                HarnessEvent::MessageCompleted => state.control_text.push('\n'),
                _ => {}
            }
        }
        if let HarnessEvent::SessionError { message } = &event {
            state.control_error = Some(message.clone());
        }
        if let Some(wrap) = state.wrap.clone()
            && matches!(
                event,
                HarnessEvent::SessionStarted | HarnessEvent::SessionProviderBound { .. }
            )
        {
            self.reveal_handoff(&wrap.text, cx);
        }
        let work_cwd = state.work_cwd.clone();
        cx.update(|cx| {
            nudge_open_editors(&event, &work_cwd, cx);
            if self
                .peers
                .orchestration
                .run_status_for_session(&self.session_id, cx)
                .is_none()
            {
                track_session_edits(&self.session_id, &work_cwd, &event, cx);
            }
        });
        if let Some(routed) = self.route_plan_event(event, state) {
            self.enqueue(routed, cx);
        }
    }

    /// `routePlanEvent`: proposal text stays out of the transcript, and a
    /// plan turn's native plan gets this turn's key.
    fn route_plan_event(&self, event: HarnessEvent, state: &mut TurnState) -> Option<HarnessEvent> {
        if matches!(event, HarnessEvent::SessionError { .. }) {
            state.provider_failure_seen = true;
        }
        if state.proposal.is_some() {
            match &event {
                HarnessEvent::MessageDelta { text } => {
                    state.proposal_text.push_str(text);
                    keep_tail(&mut state.proposal_text, 200_000);
                    return None;
                }
                HarnessEvent::MessageCompleted => {
                    state.proposal_text.push('\n');
                    return None;
                }
                HarnessEvent::Plan { text, append, .. } => {
                    if *append == Some(true) {
                        state.native_proposal_text.push_str(text);
                    } else {
                        state.native_proposal_text = text.clone();
                    }
                    return None;
                }
                _ => {}
            }
        }
        if self.intent != TurnIntent::Plan {
            return Some(event);
        }
        if let HarnessEvent::Plan {
            text,
            append,
            streaming,
            ..
        } = event
        {
            state.native_plan_seen = true;
            return Some(HarnessEvent::Plan {
                text,
                key: Some(state.plan_event_key.clone()),
                append,
                streaming,
            });
        }
        Some(event)
    }

    /// `acceptEditedResend`: the provider took the replacement, so show it
    /// and the events it already sent.
    fn accept_edited_resend(&self, state: &mut TurnState, cx: &AsyncApp) {
        let Some(edited) = &self.edited else {
            return;
        };
        if edited.is_accepted() {
            return;
        }
        cx.update(|cx| self.commit.apply(&self.config.catalog, cx));
        edited.mark_accepted();
        for event in std::mem::take(&mut state.pending_edited_events) {
            self.apply_turn_event(event, state, cx);
        }
    }

    /// `recoverEditedResend`.
    fn recover_edited_resend(&self, state: &mut TurnState, cx: &AsyncApp) {
        let Some(edited) = &self.edited else {
            return;
        };
        if edited.is_accepted() {
            return;
        }
        state.pending_edited_events.clear();
        if let Some(previous) = self.latest(cx) {
            let recovered = edited.recover_after_failure(&previous);
            self.update_session(cx, |session| *session = recovered);
        }
    }

    /// `revealHandoff`.
    fn reveal_handoff(&self, brief: &str, cx: &AsyncApp) {
        self.update_session(cx, |session| {
            if is_preparing_handoff(session) {
                *session = complete_handoff(session, brief);
                session.busy = Some(true);
            }
        });
    }

    /// The inner `finally`: settle the transcript and say how the turn ended.
    async fn finish(&self, state: &mut TurnState, cx: &AsyncApp) -> ControlOutcome {
        let id = self.session_id.as_str();
        self.flush(cx);
        let failed = state.provider_failure_seen
            || is_provider_failure_text(&state.control_text)
            || !state.build_succeeded;
        let outcome = ControlOutcome {
            status: if failed {
                ControlStatus::Failed
            } else {
                ControlStatus::Completed
            },
            text: js::trim(&state.control_text).to_string(),
            error: if state.provider_failure_seen {
                state.control_error.clone()
            } else {
                None
            },
        };
        // A failed provider can leave its process alive with a dead event
        // stream or poisoned turn state. Park it now; the next prompt will
        // reconnect and resume through a fresh transport.
        if state.provider_failure_seen {
            let _ = self
                .config
                .registry
                .stop_harness_session(self.harness(), id)
                .await;
        }
        let flush = cx.update(|cx| Engine::checkpoints(cx).flush_session_checkpoint(id));
        flush.await;

        let provider_failure_seen = state.provider_failure_seen;
        let build_succeeded = state.build_succeeded;
        let response = state.proposal_response().to_string();
        let proposal = self.proposal_id.clone().zip(state.proposal.clone());
        let completed = state.completed_proposal.clone();
        let error = outcome.error.clone();
        let native_plan_seen = state.native_plan_seen;
        let plan_key = state.plan_event_key.clone();
        let intent = self.intent;
        let approved = self.approved_plan.as_ref().map(|plan| plan.id.clone());
        let peers = self.peers.clone();
        self.update_session(cx, |session| {
            stop_streaming_mut(session, now_ms());
            let provider_failed = provider_failure_seen
                || is_provider_failure_text(&super::session_edits::last_assistant_text_in_turn(
                    session,
                ));
            if let Some((proposal_id, draft)) = proposal {
                let finalized = match completed {
                    Some(completed) if !provider_failed && build_succeeded => completed,
                    _ => {
                        let error = (provider_failed || !build_succeeded).then(|| {
                            error.unwrap_or_else(|| "The lead could not finish planning.".into())
                        });
                        peers
                            .orchestration
                            .complete_proposal(&draft, &response, error.as_deref())
                    }
                };
                *session = with_orchestration_proposal(session, &proposal_id, &finalized);
            } else if intent == TurnIntent::Plan && !native_plan_seen && !provider_failed {
                promote_last_assistant_to_plan_mut(session, Some(&plan_key));
            }
            if let Some(plan_id) = &approved
                && intent == TurnIntent::Build
            {
                let status = if build_succeeded && !provider_failed {
                    PlanStatus::Built
                } else {
                    PlanStatus::Ready
                };
                with_plan_status(session, plan_id, status);
            }
        });
        let work_cwd = state.work_cwd.clone();
        cx.update(|cx| {
            // Next tick: the notifier waits until the flush above has applied,
            // so the banner quotes the reply's final text.
            self.peers.attention.announce_finished_later(id, cx);
            notify_review_changed(Some(id), cx);
            nudge_after_turn(Some(&work_cwd), cx);
        });
        outcome
    }

    /// The title request for a first turn (`launchTitleGeneration`).
    fn launch_title_generation(&self, work_cwd: &str, cx: &AsyncApp) {
        if !self.config.registry.is_live_harness(self.harness()) || !self.generate_title {
            return;
        }
        let message = if self.harness_text.is_empty() {
            self.attachments
                .iter()
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            self.harness_text.clone()
        };
        let registry = self.config.registry.clone();
        let harness = self.harness();
        let input = TitleInput {
            session_id: self.session_id.clone(),
            cwd: work_cwd.to_string(),
            message: message.clone(),
            provider_account_id: self.provider_account_id.clone(),
        };
        let refresh = self.options.refresh_title;
        let is_first_turn = self.is_first_turn;
        let generation = self.generation;
        let session_id = self.session_id.clone();
        let title_seed = self.title_seed.clone();
        let peers = self.peers.clone();
        let work_cwd = work_cwd.to_string();
        cx.spawn(async move |cx| {
            let Ok(generated) = registry.generate_harness_title(harness, input).await else {
                return;
            };
            if refresh
                && !is_first_turn
                && cx.update(|cx| Engine::sessions(cx).read(cx).turn_gen(&session_id)) != generation
            {
                return;
            }
            let hint = generated.as_ref().and_then(|generated| generated.work_item);
            let linked = cx
                .update(|cx| {
                    peers
                        .inbox
                        .resolve_linked_work_item(&message, &work_cwd, hint, cx)
                })
                .await;
            if generated.is_none() && linked.is_none() {
                return;
            }
            cx.update(|cx| {
                Engine::sessions(cx).update(cx, |sessions, cx| {
                    sessions.update(&session_id, cx, |session| {
                        if let Some(generated) = &generated
                            && (refresh
                                || monocode_core::session::can_replace_session_title(
                                    &session.title,
                                    session.harness,
                                    &title_seed,
                                ))
                        {
                            session.title = monocode_core::session::format_session_title(
                                session.harness,
                                &generated.title,
                            );
                        }
                        if let Some(linked) = linked
                            && session.linked_work_item.is_none()
                        {
                            session.linked_work_item = Some(linked);
                        }
                    });
                });
            });
        })
        .detach();
    }

    /// Name a new draft worktree's branch from the message.
    fn name_worktree_branch(&self, tree_path: &str, cx: &AsyncApp) {
        let message = if self.harness_text.is_empty() {
            self.attachments
                .iter()
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            self.harness_text.clone()
        };
        let registry = self.config.registry.clone();
        let available = self.config.is_harness_available.clone();
        let text_harness = pick_text_harness(Some(self.harness()), |id| available(id));
        let cwd = self.current.cwd.clone();
        let tree_path = tree_path.to_string();
        let session_id = self.session_id.clone();
        let peers = self.peers.clone();
        cx.spawn(async move |cx| {
            let Ok(Some(fragment)) = registry
                .generate_harness_branch_name(text_harness, &tree_path, &message)
                .await
            else {
                return;
            };
            let Some(branch) = named_worktree_branch(&fragment) else {
                return;
            };
            let rename = cx.update(|cx| {
                peers
                    .projects
                    .rename_worktree_branch(&cwd, &tree_path, &branch, cx)
            });
            let Ok(renamed) = rename.await else {
                return;
            };
            cx.update(|cx| {
                Engine::sessions(cx).update(cx, |sessions, cx| {
                    sessions.update(&session_id, cx, |session| {
                        if path_key(session_work_cwd(session)) == path_key(&tree_path) {
                            session.branch = renamed.branch.clone();
                        }
                    });
                });
            });
        })
        .detach();
    }
}
