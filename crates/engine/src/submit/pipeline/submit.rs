//! `submitSession` up to the point the turn starts: every reason to refuse a
//! submission, queueing and steering while a turn runs, the project folder
//! check, and the optimistic user turn.

use std::rc::Rc;

use futures::FutureExt;
use gpui::{App, Context};
use monocode_core::attachment::display_attachments;
use monocode_core::block::{
    Block, BlockNotice, BlockRole, PlanBlockMeta, PlanStatus, SecondOpinionKind, TurnIntent,
};
use monocode_core::harness::HarnessId;
use monocode_core::harness_event::SteerTurnInput;
use monocode_core::notes::note_card_meta;
use monocode_core::orchestration::{
    OrchestrationChoice, OrchestrationProposal, OrchestrationProposalStatus, OrchestrationSettings,
    proposal_block,
};
use monocode_core::paths::path_key;
use monocode_core::reducer::{
    SystemEnv, UserTurnExtra, append_steer_user_mut, append_user_mut, now_ms, stop_streaming_mut,
};
use monocode_core::session::{
    MessageQueueStatus, QueuedMessage, WorkspaceMode, can_replace_session_title, session_work_cwd,
    title_from_prompt,
};
use monocode_core::settings::FollowUpBehavior;
use monocode_core::{Attachment, Extra, HarnessEvent, Session, js};
use monocode_harness::core::provider_accounts::{
    provider_account_exists, selected_provider_account_id, supports_provider_accounts,
};
use monocode_harness::core::session_title::should_generate_session_title;

use super::actions::{cancel_harness, forget_harness, sync_dock_badge};
use super::session_edits::{apply_user_turn_fields, with_plan_build_target};
use super::turn::{TurnRun, Wrap, run_turn};
use super::{Submit, SubmitOptions, settle};
use crate::runtime::engine::Engine;
use crate::submit::acceptance::{
    ControlOutcome, SubmissionAcceptance, SubmitError, submit_after_project_sync,
};
use crate::submit::edit_last_turn::{EditedResendAttempt, create_edited_resend_attempt};
use crate::submit::handoff::{
    append_preparing_handoff, handoff_turn_card, is_preparing_handoff, pending_handoff,
};
use crate::submit::message_queue::{
    QueueSubmitMode, dequeue_queued_message, queued_message_for_submit,
};
use crate::submit::operator_command::{
    OPERATOR_DEFAULT_PROMPT, consume_operator_command, operator_enabled_in_thread,
};
use crate::submit::paths::{is_equal_or_inside, looks_like_project};
use crate::submit::prefs::{KvStore, save_recent_model_choice};
use crate::submit::prompt::compose_note_message;
use crate::submit::second_opinion::SECOND_OPINION_TITLE;

/// Enqueue one event for a session and flush it now.
pub(crate) fn report(session_id: &str, event: HarnessEvent, cx: &mut App) {
    Engine::sessions(cx).update(cx, |sessions, cx| {
        sessions.enqueue_event(session_id, event, cx);
        sessions.flush(cx);
    });
}

fn status(session_id: &str, text: &str, cx: &mut App) {
    report(
        session_id,
        HarnessEvent::Status {
            text: text.to_string(),
        },
        cx,
    );
}

fn session_error(session_id: &str, message: &str, cx: &mut App) {
    report(
        session_id,
        HarnessEvent::SessionError {
            message: message.to_string(),
        },
        cx,
    );
}

/// `editedResend?.reject()`, with the composer's callback.
pub(crate) fn reject_edited(
    attempt: Option<&EditedResendAttempt>,
    options: &SubmitOptions,
    cx: &mut App,
) {
    let Some(rejection) = attempt.and_then(EditedResendAttempt::reject) else {
        return;
    };
    if let Some(callback) = &options.on_resend_rejected {
        callback(rejection, cx);
    }
}

fn blank(text: &str) -> bool {
    js::trim(text).is_empty()
}

impl Submit {
    /// `onSubmit`: the composer's submit. `true` when the turn was accepted
    /// or is still being checked, so the composer can clear.
    pub fn on_submit(
        &mut self,
        session_id: &str,
        text: &str,
        attachments: Vec<Attachment>,
        options: SubmitOptions,
        cx: &mut Context<Self>,
    ) -> bool {
        self.submit(session_id, text, attachments, options, cx)
            .accepted_now()
    }

    /// `submitSession`: send a user turn, queue or steer it while a turn
    /// runs, or say why it cannot go.
    pub fn submit(
        &mut self,
        session_id: &str,
        text: &str,
        attachments: Vec<Attachment>,
        options: SubmitOptions,
        cx: &mut Context<Self>,
    ) -> SubmissionAcceptance {
        let sessions = Engine::sessions(cx);
        let peers = self.peers.clone();
        if let Some(remote) = sessions.read(cx).get(session_id).cloned()
            && peers.remote.is_remote(&remote.cwd, cx)
        {
            return SubmissionAcceptance::Ready(peers.remote.submit(
                session_id,
                text,
                &attachments,
                &options,
                cx,
            ));
        }
        if self.edited_resends.is_active(session_id) {
            return SubmissionAcceptance::Ready(false);
        }
        if let Some(error) = peers
            .orchestration
            .submission_error(session_id, options.managed, cx)
        {
            status(session_id, &error, cx);
            return SubmissionAcceptance::Ready(false);
        }
        if options.managed {
            let unavailable = {
                let sessions = sessions.read(cx);
                sessions.get(session_id).is_none_or(|target| {
                    target.is_busy()
                        || target.pending_switch.is_some()
                        || is_preparing_handoff(target)
                        || sessions.is_removing(session_id)
                })
            };
            if unavailable {
                settle(
                    &options,
                    ControlOutcome::failed("Session is unavailable or already running"),
                    cx,
                );
                return SubmissionAcceptance::Ready(false);
            }
        }
        {
            let sessions = sessions.read(cx);
            if sessions.is_removing(session_id) || sessions.switching_worktree(session_id).is_some()
            {
                return SubmissionAcceptance::Ready(false);
            }
        }
        let stored = sessions.read(cx).get(session_id).cloned();
        if options.app_request_id.is_some() && stored.as_ref().is_some_and(Session::is_busy) {
            return SubmissionAcceptance::Ready(false);
        }
        if options.ci_repair.is_some()
            && stored.as_ref().is_some_and(|stored| {
                stored.is_busy() || stored.pending_switch.is_some() || is_preparing_handoff(stored)
            })
        {
            return SubmissionAcceptance::Ready(false);
        }
        let Some(stored) = stored else {
            return SubmissionAcceptance::Ready(false);
        };
        let removing_paths = peers.projects.removing_worktree_paths(cx);
        if stored.worktree_removed == Some(true)
            || removing_paths
                .iter()
                .any(|path| is_equal_or_inside(session_work_cwd(&stored), path))
        {
            return SubmissionAcceptance::Ready(false);
        }
        let draft_block = options.draft_block_id.as_ref().and_then(|id| {
            stored
                .blocks
                .iter()
                .find(|block| &block.id == id && block.role == BlockRole::User && block.is_draft())
                .cloned()
        });
        if options.draft_block_id.is_some() && draft_block.is_none() {
            return SubmissionAcceptance::Ready(false);
        }
        let mut current = stored.clone();
        if let Some(draft) = &draft_block {
            current.blocks.retain(|block| block.id != draft.id);
        }
        if let Some(target) = &options.build_target {
            with_plan_build_target(&mut current, target, &self.config.catalog.read());
        }
        let edited = if options.resend_edited {
            create_edited_resend_attempt(&current).map(Rc::new)
        } else {
            None
        };
        if options.resend_edited && edited.is_none() {
            return SubmissionAcceptance::Ready(false);
        }
        if let Some(edited) = &edited {
            current.blocks = edited.blocks.clone();
        }
        let intent = options.intent.unwrap_or_default();
        if intent == TurnIntent::Orchestrate
            && peers
                .orchestration
                .run_status_for_session(session_id, cx)
                .is_some_and(|status| status == "active" || status == "paused")
        {
            status(
                session_id,
                "Stop the current orchestration run before preparing another proposal.",
                cx,
            );
            return SubmissionAcceptance::Ready(false);
        }
        let approved_plan = options.plan_block_id.as_ref().and_then(|id| {
            current
                .blocks
                .iter()
                .find(|block| &block.id == id && block.role == BlockRole::Plan)
                .cloned()
        });
        if intent == TurnIntent::Build
            && approved_plan.as_ref().is_none_or(|plan| blank(&plan.text))
        {
            return SubmissionAcceptance::Ready(false);
        }
        if let Some(queued_id) = &options.queued_message_id {
            let mode = if options.follow_up_behavior == Some(FollowUpBehavior::Steer) {
                QueueSubmitMode::Steer
            } else {
                QueueSubmitMode::Dispatch
            };
            if queued_message_for_submit(&current, queued_id, mode).is_none() {
                return SubmissionAcceptance::Ready(false);
            }
        }
        let note_card = match &options.note_card {
            Some(card) => card.clone(),
            None => current.note_card.clone(),
        };
        let handoff_card = match &options.handoff_card {
            Some(card) => card.clone(),
            None => current.handoff_card.clone(),
        };
        if blank(text) && attachments.is_empty() && note_card.is_none() && handoff_card.is_none() {
            return SubmissionAcceptance::Ready(false);
        }
        if is_preparing_handoff(&current) {
            return SubmissionAcceptance::Ready(false);
        }
        save_recent_model_choice(&self.config.kv, current.harness, &current.model);
        let initial_work_cwd = session_work_cwd(&current).to_string();
        let create_draft_worktree = current.worktree_cwd.as_deref().is_none_or(str::is_empty)
            && current.workspace_mode == Some(WorkspaceMode::Worktree);
        let store = KvStore(self.config.kv.clone());
        let account_provider =
            supports_provider_accounts(current.harness).then_some(current.harness);
        let provider_account_id = account_provider.map(|provider| {
            current.provider_account_id.clone().unwrap_or_else(|| {
                selected_provider_account_id(&store, provider, Some(&current.cwd))
            })
        });
        if let (Some(provider), Some(account)) = (account_provider, provider_account_id.as_deref())
            && !account.is_empty()
            && !provider_account_exists(&store, provider, Some(account))
        {
            session_error(
                session_id,
                "This conversation uses a removed provider account. Switch accounts from the usage control to start a new conversation.",
                cx,
            );
            return SubmissionAcceptance::Ready(false);
        }
        let submitted_text = if intent == TurnIntent::Build {
            "Build approved plan".to_string()
        } else {
            text.to_string()
        };
        let operator = consume_operator_command(&submitted_text);
        if operator.matched
            && (intent != TurnIntent::Default
                || current.orchestration_lead_id.is_some()
                || current.inbox_ask.is_some()
                || peers.orchestration.led_run_status(session_id, cx).is_some())
        {
            status(
                session_id,
                "Use /operator from a regular session turn, outside an orchestration run.",
                cx,
            );
            return SubmissionAcceptance::Ready(false);
        }
        let operator_access = operator.matched || operator_enabled_in_thread(&current.blocks);
        let prompt_text = if operator.matched {
            let trimmed = js::trim(&operator.text);
            if trimmed.is_empty() {
                OPERATOR_DEFAULT_PROMPT.to_string()
            } else {
                trimmed.to_string()
            }
        } else {
            submitted_text.clone()
        };
        let raw_command = !operator.matched
            && self
                .skills
                .is_native_command_prompt(&submitted_text, current.harness);
        let ci_context = options
            .ci_repair
            .as_ref()
            .map(|repair| repair.prompt.clone())
            .or_else(|| options.ci_context.clone());
        let harness_text = match &options.ci_repair {
            Some(repair) => repair.prompt.clone(),
            None if raw_command => submitted_text.clone(),
            None => compose_note_message(note_card.as_ref(), &prompt_text),
        };
        let pending_switch = current
            .pending_switch
            .clone()
            .filter(|pending| pending.from != current.harness);

        if current.is_busy() && pending_switch.is_none() {
            return self.follow_up(
                FollowUp {
                    session_id,
                    text,
                    attachments,
                    options: &options,
                    current: &current,
                    intent,
                    operator_matched: operator.matched,
                    raw_command,
                    submitted_text: &submitted_text,
                    harness_text,
                    note_card,
                    handoff_card,
                    work_cwd: initial_work_cwd,
                },
                cx,
            );
        }

        if !options.project_location_ready
            && looks_like_project(&current.cwd)
            && current.worktree_cwd.as_deref().is_none_or(str::is_empty)
        {
            return self.submit_after_location_sync(
                session_id,
                text,
                attachments,
                options,
                &current,
                edited,
                cx,
            );
        }

        let generation = sessions.update(cx, |sessions, _| sessions.bump_turn_gen(session_id));
        let registry = self.config.registry.clone();
        let proposal_id =
            (intent == TurnIntent::Orchestrate).then(|| uuid::Uuid::new_v4().to_string());
        let proposal_draft = proposal_id.as_ref().map(|_| OrchestrationProposal {
            version: 1,
            lead_id: session_id.to_string(),
            cwd: current.cwd.clone(),
            checkout_cwd: Some(initial_work_cwd.clone()),
            request: harness_text.clone(),
            author: OrchestrationChoice {
                harness: current.harness,
                model: current.model.clone(),
                name: self
                    .config
                    .catalog
                    .read()
                    .resolve_model(current.harness, Some(&current.model))
                    .name,
                extra: Extra::new(),
            },
            settings: OrchestrationSettings {
                choices: Vec::new(),
                max_workers: 2,
                extra: Extra::new(),
            },
            status: OrchestrationProposalStatus::Planning,
            title: "Orchestration plan".into(),
            summary: String::new(),
            tasks: Vec::new(),
            error: None,
            response: None,
            extra: Extra::new(),
        });
        let is_first_turn = current.blocks.is_empty();
        let placeholder_title =
            can_replace_session_title(&current.title, current.harness, current.harness.label())
                || draft_block.is_some();
        let title_seed = if is_first_turn
            && current.inbox_card.is_none()
            && current.note_card.is_none()
            && placeholder_title
        {
            title_from_prompt(
                if operator.matched {
                    &prompt_text
                } else {
                    &submitted_text
                },
                current.harness,
                &attachments,
            )
        } else {
            current.title.clone()
        };
        let visible = display_attachments(&attachments);
        let card = options
            .second_opinion
            .clone()
            .or_else(|| handoff_card.as_ref().map(handoff_turn_card));
        let visible_text = if operator.matched {
            prompt_text.clone()
        } else if card
            .as_ref()
            .is_some_and(|card| card.kind == Some(SecondOpinionKind::Handoff))
        {
            submitted_text.clone()
        } else if card.is_some() {
            SECOND_OPINION_TITLE.to_string()
        } else {
            submitted_text.clone()
        };
        let cards = UserTurnExtra {
            second_opinion: if raw_command { None } else { card },
            note_card: if raw_command {
                None
            } else {
                note_card.as_ref().map(note_card_meta)
            },
            ci_context: ci_context.clone(),
            monocode: operator.matched,
            intent: matches!(intent, TurnIntent::Plan | TurnIntent::Orchestrate).then_some(intent),
            app_request_id: options.app_request_id.clone(),
            // The orchestrator writes these turns, not the user; hide them.
            internal: options.managed,
        };
        let live = registry.is_live_harness(current.harness);
        let queued_handoff = if live && pending_switch.is_none() {
            pending_handoff(&current)
        } else {
            None
        };

        if let Some(pending) = &pending_switch
            && current.is_busy()
        {
            cancel_harness(&registry, pending.from, session_id, cx);
        }

        peers
            .attention
            .dismiss_notices_for_continued_session(session_id, cx);
        let commit = Rc::new(CommitTurn {
            session_id: session_id.to_string(),
            draft_block_id: draft_block.as_ref().map(|block| block.id.clone()),
            build_target: options.build_target.clone(),
            is_first_turn,
            title_seed: title_seed.clone(),
            provider_account_id: provider_account_id.clone(),
            create_draft_worktree,
            keep_cards: raw_command || options.ci_repair.is_some(),
            edited: edited.clone(),
            approved_plan: approved_plan
                .as_ref()
                .filter(|_| intent == TurnIntent::Build)
                .map(|plan| plan.id.clone()),
            queued_message_id: options.queued_message_id.clone(),
            live,
            pending_switch: pending_switch.as_ref().map(|pending| pending.from),
            visible_text,
            visible,
            cards,
        });
        if !options.resend_edited {
            commit.apply(&self.config.catalog, cx);
        }

        if !live {
            if let Some(pending) = &pending_switch {
                forget_harness(&registry, pending.from, session_id, cx);
            }
            reject_edited(edited.as_deref(), &options, cx);
            settle(
                &options,
                ControlOutcome::failed("Harness is not connected"),
                cx,
            );
            return SubmissionAcceptance::Ready(true);
        }
        if edited.is_some() && registry.can_rewind_harness_last_turn(current.harness) {
            self.edited_resends.start(session_id);
            sessions.update(cx, |sessions, cx| {
                sessions.update(session_id, cx, |session| session.busy = Some(true));
            });
            sync_dock_badge(cx);
        }
        if let (Some(proposal_id), Some(draft)) = (&proposal_id, &proposal_draft) {
            let block = proposal_block(proposal_id.clone(), draft);
            sessions.update(cx, |sessions, cx| {
                sessions.update(session_id, cx, |session| session.blocks.push(block));
            });
        }

        let run = TurnRun {
            session_id: session_id.to_string(),
            generation,
            current: current.clone(),
            text: text.to_string(),
            attachments,
            options: options.clone(),
            intent,
            harness_text,
            raw_command,
            operator_matched: operator.matched,
            operator_access,
            provider_account_id,
            initial_work_cwd,
            create_draft_worktree,
            is_first_turn,
            placeholder_title,
            title_seed,
            generate_title: should_generate_session_title(
                is_first_turn,
                placeholder_title,
                options.refresh_title,
            ),
            approved_plan,
            edited_provider_turn_id: edited
                .as_ref()
                .and_then(|edited| edited.provider_turn_id.clone()),
            edited,
            pending_switch,
            queued_handoff: queued_handoff.map(|handoff| Wrap {
                from: handoff.from,
                text: handoff.text,
            }),
            handoff_card,
            proposal_id,
            proposal_draft,
            commit,
            config: self.config.clone(),
            peers,
            skills: self.skills.clone(),
        };
        cx.spawn(async move |this, cx| run_turn(this, run, cx).await)
            .detach();
        SubmissionAcceptance::Ready(true)
    }

    /// The busy branch of `submitSession`: queue the message, or hand it to
    /// the running turn.
    fn follow_up(&mut self, follow: FollowUp<'_>, cx: &mut Context<Self>) -> SubmissionAcceptance {
        let FollowUp {
            session_id,
            text,
            attachments,
            options,
            current,
            intent,
            operator_matched,
            raw_command,
            submitted_text,
            harness_text,
            note_card,
            handoff_card,
            work_cwd,
        } = follow;
        if operator_matched
            && options.queued_message_id.is_some()
            && options.follow_up_behavior == Some(FollowUpBehavior::Steer)
        {
            status(
                session_id,
                "/operator starts a new turn after the current turn finishes.",
                cx,
            );
            return SubmissionAcceptance::Ready(false);
        }
        let behavior = if current.worktree_preparing == Some(true)
            || intent == TurnIntent::Plan
            || intent == TurnIntent::Orchestrate
            || operator_matched
        {
            FollowUpBehavior::Queue
        } else if current
            .background_tasks
            .as_ref()
            .is_some_and(|tasks| !tasks.is_empty())
        {
            // The agent has yielded and only background work is left, which
            // may never end (a dev server). Queuing would park the message
            // behind it, so hand it to the agent now.
            FollowUpBehavior::Steer
        } else {
            options.follow_up_behavior.unwrap_or_else(|| {
                monocode_settings::settings_store::load_follow_up_behavior(&self.config.kv)
            })
        };
        let sessions = Engine::sessions(cx);
        if behavior == FollowUpBehavior::Queue {
            let message = QueuedMessage {
                id: uuid::Uuid::new_v4().to_string(),
                text: text.to_string(),
                attachments,
                note_card,
                handoff_card,
                intent: Some(intent),
            };
            sessions.update(cx, |sessions, cx| {
                sessions.update(session_id, cx, |session| {
                    if !raw_command {
                        session.inbox_card = None;
                        session.note_card = None;
                        session.handoff_card = None;
                    }
                    session
                        .queued_messages
                        .get_or_insert_with(Vec::new)
                        .push(message);
                    session.queue_status = Some(
                        if session.queue_status == Some(MessageQueueStatus::Paused) {
                            MessageQueueStatus::Paused
                        } else {
                            MessageQueueStatus::Active
                        },
                    );
                });
            });
            self.peers
                .attention
                .dismiss_notices_for_continued_session(session_id, cx);
            return SubmissionAcceptance::Ready(true);
        }
        let registry = self.config.registry.clone();
        if !registry.is_live_harness(current.harness)
            || !registry.can_steer_harness(current.harness)
        {
            // Harnesses that cannot steer (fx) used to drop the message on the
            // floor here, so a follow-up sent mid-turn just vanished. Say so.
            status(
                session_id,
                &format!(
                    "{} cannot take a follow-up mid-turn — wait for this turn to finish, or stop it first.",
                    current.harness
                ),
                cx,
            );
            return SubmissionAcceptance::Ready(false);
        }
        self.peers
            .attention
            .dismiss_notices_for_continued_session(session_id, cx);
        let visible = display_attachments(&attachments);
        let cards = UserTurnExtra {
            note_card: note_card.as_ref().map(note_card_meta),
            ..UserTurnExtra::default()
        };
        let has_cards = cards.note_card.is_some();
        let queued_id = options.queued_message_id.clone();
        let submitted = submitted_text.to_string();
        {
            let catalog = self.config.catalog.read();
            sessions.update(cx, |sessions, cx| {
                sessions.update(session_id, cx, |session| {
                    if !raw_command {
                        session.inbox_card = None;
                        session.note_card = None;
                        session.handoff_card = None;
                    }
                    if let Some(queued_id) = &queued_id {
                        dequeue_queued_message(session, queued_id);
                    }
                    append_steer_user_mut(
                        &mut SystemEnv,
                        &catalog,
                        session,
                        &submitted,
                        &visible,
                        has_cards.then_some(&cards),
                    );
                });
            });
        }
        let harness = current.harness;
        let id = session_id.to_string();
        let model = current.model.clone();
        let model_settings = current.model_settings.clone();
        let inbox_ask = (!raw_command).then(|| current.inbox_ask.clone()).flatten();
        let io = self.config.attachment_io.clone();
        let skills = self.skills.clone();
        let peers = self.peers.clone();
        cx.spawn(async move |_, cx| {
            let prepared =
                crate::submit::attachments::prepare_attachments(io.as_ref(), &attachments).await;
            let prompt =
                super::turn::prepare(&harness_text, harness, &id, &work_cwd, &skills, &peers, cx)
                    .await;
            let text = peers.inbox.ask_prompt(inbox_ask.as_ref(), prompt);
            let steer = registry.steer_harness_turn(
                harness,
                SteerTurnInput {
                    session_id: id.clone(),
                    cwd: work_cwd,
                    model,
                    model_settings: Some(model_settings),
                    text,
                    attachments: Some(prepared),
                },
            );
            if let Err(error) = steer.await {
                let message = error.to_string();
                let message = if message.is_empty() {
                    format!("{harness} could not steer the active turn")
                } else {
                    message
                };
                cx.update(|cx| session_error(&id, &message, cx));
            }
        })
        .detach();
        SubmissionAcceptance::Ready(true)
    }

    /// Wait for the project folder check, then submit again with
    /// `project_location_ready`.
    #[allow(clippy::too_many_arguments)]
    fn submit_after_location_sync(
        &mut self,
        session_id: &str,
        text: &str,
        attachments: Vec<Attachment>,
        options: SubmitOptions,
        current: &Session,
        edited: Option<Rc<EditedResendAttempt>>,
        cx: &mut Context<Self>,
    ) -> SubmissionAcceptance {
        let cwd = current.cwd.clone();
        let key = path_key(&cwd);
        let sync = match self.project_location_syncs.get(&key) {
            Some(sync) => sync.clone(),
            None => {
                let sync = self
                    .peers
                    .projects
                    .synchronize_project_location(&cwd, cx)
                    .shared();
                self.project_location_syncs
                    .insert(key.clone(), sync.clone());
                let pending = sync.clone();
                let done_key = key.clone();
                cx.spawn(async move |this, cx| {
                    let _ = pending.await;
                    let _ =
                        this.update(cx, |this, _| this.project_location_syncs.remove(&done_key));
                })
                .detach();
                sync
            }
        };
        let (sender, acceptance) = SubmissionAcceptance::deferred();
        let id = session_id.to_string();
        let text = text.to_string();
        let peers = self.peers.clone();
        cx.spawn(async move |this, cx| {
            let edited_for_error = edited.clone();
            let options_for_error = options.clone();
            let id_for_error = id.clone();
            let cx_ref: &gpui::AsyncApp = cx;
            let result = submit_after_project_sync(
                &cwd,
                sync,
                |from, to| cx_ref.update(|cx| peers.projects.apply_project_location_change(&from, &to, cx)),
                || {
                    let mut retry = options.clone();
                    retry.project_location_ready = true;
                    let acceptance = this
                        .update(&mut cx_ref.clone(), |this, cx| this.submit(&id, &text, attachments, retry, cx))
                        .unwrap_or(SubmissionAcceptance::Ready(false));
                    let edited = edited.clone();
                    let options = options.clone();
                    let cx = cx_ref.clone();
                    async move {
                        let accepted = acceptance.resolve().await?;
                        if !accepted {
                            cx.update(|cx| {
                                reject_edited(edited.as_deref(), &options, cx);
                                settle(
                                    &options,
                                    ControlOutcome::failed(
                                        "The chat became unavailable before the request could start. Try again when it is ready.",
                                    ),
                                    cx,
                                );
                            });
                        }
                        Ok::<bool, SubmitError>(accepted)
                    }
                    .boxed_local()
                },
                |error| {
                    let message = if error.message.is_empty() {
                        "The project folder could not be opened.".to_string()
                    } else {
                        error.message.clone()
                    };
                    cx_ref.update(|cx| {
                        session_error(&id_for_error, &message, cx);
                        reject_edited(edited_for_error.as_deref(), &options_for_error, cx);
                        settle(&options_for_error, ControlOutcome::failed(message.clone()), cx);
                    });
                },
            )
            .await;
            let _ = sender.send(result);
        })
        .detach();
        acceptance
    }
}

/// Everything the busy branch needs.
struct FollowUp<'a> {
    session_id: &'a str,
    text: &'a str,
    attachments: Vec<Attachment>,
    options: &'a SubmitOptions,
    current: &'a Session,
    intent: TurnIntent,
    operator_matched: bool,
    raw_command: bool,
    submitted_text: &'a str,
    harness_text: String,
    note_card: Option<monocode_core::notes::NoteComposerCard>,
    handoff_card: Option<monocode_core::handoff::HandoffComposerCard>,
    work_cwd: String,
}

/// `commitSubmittedTurn`: the optimistic user turn. It runs at once, or for
/// an edited resend once the provider accepts the replacement.
pub(crate) struct CommitTurn {
    session_id: String,
    draft_block_id: Option<String>,
    build_target: Option<monocode_core::block::PlanBuildTarget>,
    is_first_turn: bool,
    title_seed: String,
    provider_account_id: Option<String>,
    create_draft_worktree: bool,
    keep_cards: bool,
    edited: Option<Rc<EditedResendAttempt>>,
    approved_plan: Option<String>,
    queued_message_id: Option<String>,
    live: bool,
    pending_switch: Option<HarnessId>,
    visible_text: String,
    visible: Vec<Attachment>,
    cards: UserTurnExtra,
}

impl CommitTurn {
    pub(crate) fn apply(
        &self,
        catalog: &monocode_harness::core::catalog::SharedCatalog,
        cx: &mut App,
    ) {
        let catalog = catalog.read();
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(&self.session_id, cx, |session| {
                self.commit(session, &catalog)
            });
        });
    }

    fn commit(&self, session: &mut Session, catalog: &monocode_core::ModelCatalog) {
        if let Some(draft) = &self.draft_block_id {
            session.blocks.retain(|block| &block.id != draft);
        }
        if let Some(target) = &self.build_target {
            with_plan_build_target(session, target, catalog);
        }
        let titled = if self.is_first_turn {
            self.title_seed.clone()
        } else {
            session.title.clone()
        };
        session.provider_account_id = self.provider_account_id.clone();
        session.usage_limit = None;
        if self.create_draft_worktree {
            session.worktree_preparing = Some(true);
        }
        if !self.keep_cards {
            session.inbox_card = None;
            session.note_card = None;
            session.handoff_card = None;
        }
        if let Some(edited) = &self.edited {
            *session = edited.replace(session);
        }
        if let Some(plan_id) = &self.approved_plan {
            for block in &mut session.blocks {
                if &block.id == plan_id && block.role == BlockRole::Plan {
                    let text = block.text.clone();
                    let plan = block.plan.get_or_insert_with(|| PlanBlockMeta {
                        key: None,
                        status: PlanStatus::Ready,
                        original_text: None,
                        approved_text: None,
                        edited: None,
                        extra: Extra::new(),
                    });
                    plan.status = PlanStatus::Building;
                    plan.approved_text = Some(text);
                }
            }
        }
        if let Some(queued) = &self.queued_message_id {
            dequeue_queued_message(session, queued);
        }
        if !self.live {
            session.title = titled;
            session.pending_switch = None;
            session.busy = Some(false);
            let mut user = Block {
                attachments: (!self.visible.is_empty()).then(|| self.visible.clone()),
                ..Block::new(
                    uuid::Uuid::new_v4().to_string(),
                    BlockRole::User,
                    &self.visible_text,
                )
            };
            apply_user_turn_fields(&mut user, &self.cards);
            session.blocks.push(user);
            session.blocks.push(Block {
                notice: Some(BlockNotice::Error),
                ..Block::new(
                    uuid::Uuid::new_v4().to_string(),
                    BlockRole::System,
                    format!(
                        "{} is not connected yet — install and sign in to that provider, then retry.",
                        session.harness
                    ),
                )
            });
            return;
        }
        session.title = titled;
        if let Some(from) = self.pending_switch {
            session.pending_switch = None;
            stop_streaming_mut(session, now_ms());
            let to = session.harness;
            *session = append_preparing_handoff(session, from, to);
        }
        append_user_mut(
            &mut SystemEnv,
            catalog,
            session,
            &self.visible_text,
            &self.visible,
            Some(&self.cards),
        );
    }
}
