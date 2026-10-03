//! The composer actions other than submit: model and mode changes, persisted
//! drafts, plan edits, Stop, and manual compaction.

use std::time::Duration;

use gpui::{App, Context};
use monocode_core::block::{
    Block, BlockRole, PlanBlockMeta, PlanBuildTarget, PlanStatus, TurnIntent,
};
use monocode_core::harness_event::HarnessSessionInput;
use monocode_core::models::SaveSettingsMode;
use monocode_core::reducer::{SystemEnv, apply_harness_event_mut, now_ms, stop_streaming_mut};
use monocode_core::session::{
    can_replace_session_title, remove_session_draft, session_draft_block, session_work_cwd,
    title_from_prompt,
};
use monocode_core::{Attachment, Extra, HarnessEvent, HarnessId, ModelSettings, RuntimeMode, js};
use monocode_harness::core::provider_accounts::{
    selected_provider_account_id, supports_provider_accounts,
};
use monocode_harness::core::registry::HarnessRegistry;

use super::session_edits::{apply_switch_plan, with_harness_choice};
use super::turn::{TurnSignal, drive, signal_channel};
use super::{Submit, SubmitOptions};
use crate::runtime::checkpoint::notify_review_changed;
use crate::runtime::engine::Engine;
use crate::runtime::session_store::should_persist_session;
use crate::submit::handoff::{
    build_deterministic_handoff, complete_handoff, is_preparing_handoff, plan_composer_switch,
    session_child_harnesses,
};
use crate::submit::prefs::{
    KvStore, load_model_prefs, save_last_model_settings, save_recent_model_choice,
};
use monocode_core::handoff::ComposerSwitchPlan;
use monocode_core::session::MessageQueueStatus;

/// Detach a registry call that only matters for its side effect.
pub(crate) fn forget_harness(
    registry: &HarnessRegistry,
    harness: HarnessId,
    session_id: &str,
    cx: &mut App,
) {
    let registry = registry.clone();
    let session_id = session_id.to_string();
    cx.spawn(async move |_| {
        let _ = registry.forget_harness_session(harness, &session_id).await;
    })
    .detach();
}

pub(crate) fn cancel_harness(
    registry: &HarnessRegistry,
    harness: HarnessId,
    session_id: &str,
    cx: &mut App,
) {
    let registry = registry.clone();
    let session_id = session_id.to_string();
    cx.spawn(async move |_| {
        let _ = registry.cancel_harness_turn(harness, &session_id).await;
    })
    .detach();
}

/// `syncDockBadge(sessionsRef.current)`.
pub(crate) fn sync_dock_badge(cx: &mut App) {
    let hooks = Engine::hooks(cx);
    let sessions = Engine::sessions(cx).read(cx).all().to_vec();
    hooks.attention.sync_dock_badge(&sessions, cx);
}

/// Refresh what a finished or stopped turn may have changed on disk.
pub(crate) fn nudge_after_turn(work_cwd: Option<&str>, cx: &mut App) {
    let hooks = Engine::hooks(cx);
    hooks.workspace.nudge_workspace(work_cwd, cx);
    hooks.workspace.notify_git_changed(cx);
    hooks.workspace.nudge_watched_files(None, cx);
    let timer = cx.background_executor().timer(Duration::from_millis(150));
    cx.spawn(async move |cx| {
        timer.await;
        cx.update(|cx| Engine::hooks(cx).workspace.nudge_watched_files(None, cx));
    })
    .detach();
}

impl Submit {
    /// `onModelChange`: pick a provider and model. Changing provider mid
    /// conversation arms a handoff for the next send.
    pub fn set_model(
        &mut self,
        session_id: &str,
        harness: HarnessId,
        model: &str,
        cx: &mut Context<Self>,
    ) {
        let sessions = Engine::sessions(cx);
        let Some(current) = sessions.read(cx).get(session_id).cloned() else {
            return;
        };
        if is_preparing_handoff(&current) {
            return;
        }
        let kv = self.config.kv.clone();
        let (resolved, model_settings) = {
            let catalog = self.config.catalog.read();
            let resolved = catalog.resolve_model(harness, Some(model));
            save_recent_model_choice(&kv, resolved.harness, &resolved.id);
            save_last_model_settings(&kv, &current.model_settings, SaveSettingsMode::Fill);
            let last = load_model_prefs(&kv).last_model_settings;
            let settings =
                catalog.preferred_model_settings(&resolved, Some(&current.model_settings), &last);
            (resolved, settings)
        };
        let plan = plan_composer_switch(&current, harness);
        if let ComposerSwitchPlan::Empty { forget } = &plan {
            forget_harness(&self.config.registry, *forget, session_id, cx);
        }
        sessions.update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                with_harness_choice(session, harness, &resolved.id, model_settings);
                apply_switch_plan(session, plan);
            });
        });
    }

    /// `onModelSettingsChange`.
    pub fn set_model_settings(
        &mut self,
        session_id: &str,
        model_settings: ModelSettings,
        cx: &mut Context<Self>,
    ) {
        save_last_model_settings(
            &self.config.kv,
            &model_settings,
            SaveSettingsMode::Overwrite,
        );
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                session.model_settings = model_settings
            });
        });
    }

    /// `onRuntimeModeChange`.
    pub fn set_runtime_mode(
        &mut self,
        session_id: &str,
        runtime_mode: RuntimeMode,
        cx: &mut Context<Self>,
    ) {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                session.runtime_mode = runtime_mode
            });
        });
    }

    /// `onSaveDraft`: keep an unsent message on the transcript without
    /// starting the agent. `false` when the session cannot take one.
    pub fn save_draft(
        &mut self,
        session_id: &str,
        text: &str,
        attachments: Vec<Attachment>,
        app_request_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let sessions = Engine::sessions(cx);
        let current = sessions.read(cx).get(session_id).cloned();
        if let Some(current) = &current
            && self.peers.remote.is_remote(&current.cwd, cx)
        {
            return self
                .peers
                .remote
                .save_draft(session_id, text, &attachments, cx);
        }
        let Some(current) = current else {
            return false;
        };
        if current.is_busy()
            || session_draft_block(&current.blocks).is_some()
            || current.inbox_ask.is_some()
            || current.worktree_removed == Some(true)
            || (js::trim(text).is_empty() && attachments.is_empty())
        {
            return false;
        }
        let placeholder =
            can_replace_session_title(&current.title, current.harness, current.harness.label());
        let title = if placeholder {
            title_from_prompt(text, current.harness, &attachments)
        } else {
            current.title.clone()
        };
        let text = text.to_string();
        sessions.update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                if session_draft_block(&session.blocks).is_some() {
                    return;
                }
                session.title = title;
                session.blocks.push(Block {
                    attachments: (!attachments.is_empty()).then_some(attachments),
                    draft: Some(true),
                    app_request_id: app_request_id.filter(|id| !id.is_empty()),
                    ..Block::new(uuid::Uuid::new_v4().to_string(), BlockRole::User, text)
                });
            });
        });
        true
    }

    /// `onRemoveDraft`. A session that held only the draft leaves storage.
    pub fn remove_draft(
        &mut self,
        session_id: &str,
        draft_block_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let sessions = Engine::sessions(cx);
        let Some(current) = sessions.read(cx).get(session_id).cloned() else {
            return false;
        };
        let Some(without_draft) = remove_session_draft(&current, draft_block_id) else {
            return false;
        };
        if !should_persist_session(&without_draft) {
            // TODO(port): App.tsx also cleared `pendingPersist` and
            // `lastPersistedUserBlock`; `Sessions` does not expose them yet
            // (NEEDS.md).
            sessions.update(cx, |sessions, _| {
                sessions.forget_persisted(session_id);
                sessions.invalidate_loaded(session_id);
            });
            self.peers.history.draft_session_discarded(session_id, cx);
            Engine::writer(cx)
                .discard_draft_session_record(session_id)
                .detach();
        }
        let draft_block_id = draft_block_id.to_string();
        sessions.update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                if let Some(next) = remove_session_draft(session, &draft_block_id) {
                    *session = next;
                }
            });
        });
        true
    }

    /// `onUpdatePlan`: the user edited a plan before building it.
    pub fn update_plan(
        &mut self,
        session_id: &str,
        block_id: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let sessions = Engine::sessions(cx);
        let Some(session) = sessions.read(cx).get(session_id) else {
            return;
        };
        if self.peers.remote.is_remote(&session.cwd, cx) || session.is_busy() {
            return;
        }
        sessions.update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                for block in &mut session.blocks {
                    let locked = block.plan.as_ref().is_some_and(|plan| {
                        matches!(
                            plan.status,
                            PlanStatus::Streaming | PlanStatus::Building | PlanStatus::Built
                        )
                    });
                    if block.id != block_id || block.role != BlockRole::Plan || locked {
                        continue;
                    }
                    let original = block
                        .plan
                        .as_ref()
                        .and_then(|plan| plan.original_text.clone())
                        .unwrap_or_else(|| block.text.clone());
                    let plan = block.plan.get_or_insert_with(|| PlanBlockMeta {
                        key: None,
                        status: PlanStatus::Ready,
                        original_text: None,
                        approved_text: None,
                        edited: None,
                        extra: Extra::new(),
                    });
                    plan.status = PlanStatus::Ready;
                    plan.edited = Some(text != original);
                    plan.original_text = Some(original);
                    block.text = text.to_string();
                }
            });
        });
    }

    /// `onBuildPlan`: send the approved plan, optionally with another
    /// provider and model.
    pub fn build_plan(
        &mut self,
        session_id: &str,
        block_id: &str,
        target: Option<PlanBuildTarget>,
        cx: &mut Context<Self>,
    ) {
        let sessions = Engine::sessions(cx);
        let session = sessions.read(cx).get(session_id).cloned();
        if let Some(session) = &session
            && self.peers.remote.is_remote(&session.cwd, cx)
        {
            self.peers
                .remote
                .build_plan(session_id, block_id, target.as_ref(), cx);
            return;
        }
        let Some(session) = session else {
            return;
        };
        let Some(block) = session.blocks.iter().find(|entry| entry.id == block_id) else {
            return;
        };
        let locked = block.plan.as_ref().is_some_and(|plan| {
            matches!(
                plan.status,
                PlanStatus::Streaming | PlanStatus::Building | PlanStatus::Built
            )
        });
        if session.is_busy()
            || block.role != BlockRole::Plan
            || block.orchestration.is_some()
            || js::trim(&block.text).is_empty()
            || locked
        {
            return;
        }
        if target.is_some() {
            save_last_model_settings(
                &self.config.kv,
                &session.model_settings,
                SaveSettingsMode::Fill,
            );
        }
        self.on_submit(
            session_id,
            "Build approved plan",
            Vec::new(),
            SubmitOptions {
                intent: Some(TurnIntent::Build),
                plan_block_id: Some(block_id.to_string()),
                build_target: target,
                ..SubmitOptions::default()
            },
            cx,
        );
    }

    /// `onCompactContext`: ask the provider to summarize older context.
    /// `true` when the request was handled, including the notice that the
    /// provider cannot compact.
    pub fn compact(&mut self, session_id: &str, cx: &mut Context<Self>) -> bool {
        let sessions = Engine::sessions(cx);
        let current = sessions.read(cx).get(session_id).cloned();
        if let Some(current) = &current
            && self.peers.remote.is_remote(&current.cwd, cx)
        {
            return self.peers.remote.compact(session_id, cx);
        }
        let Some(current) = current else {
            return false;
        };
        if current.is_busy() || current.worktree_removed == Some(true) {
            return false;
        }
        let registry = self.config.registry.clone();
        if !registry.can_compact_harness_context(current.harness) {
            let notice = HarnessEvent::Status {
                text: format!(
                    "{} does not support manual context compaction.",
                    current.harness.title()
                ),
            };
            sessions.update(cx, |sessions, cx| {
                sessions.update(session_id, cx, |session| {
                    apply_harness_event_mut(&mut SystemEnv, session, &notice);
                });
            });
            sync_dock_badge(cx);
            return true;
        }

        let generation = sessions.update(cx, |sessions, _| sessions.bump_turn_gen(session_id));
        let work_cwd = session_work_cwd(&current).to_string();
        sessions.update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                session.busy = Some(true);
                apply_harness_event_mut(
                    &mut SystemEnv,
                    session,
                    &HarnessEvent::Status {
                        text: "Compacting context…".into(),
                    },
                );
            });
        });
        sync_dock_badge(cx);

        let provider_account_id = supports_provider_accounts(current.harness).then(|| {
            current.provider_account_id.clone().unwrap_or_else(|| {
                selected_provider_account_id(
                    &KvStore(self.config.kv.clone()),
                    current.harness,
                    Some(&current.cwd),
                )
            })
        });
        let input = HarnessSessionInput {
            session_id: session_id.to_string(),
            cwd: work_cwd,
            model: current.model.clone(),
            model_settings: Some(current.model_settings.clone()),
            provider_account_id,
            runtime_mode: current.runtime_mode,
            intent: None,
            controls_agents: None,
            app_access: None,
        };
        let harness = current.harness;
        let id = session_id.to_string();
        cx.spawn(async move |_, cx| {
            let current_gen =
                |cx: &gpui::AsyncApp| cx.update(|cx| Engine::sessions(cx).read(cx).turn_gen(&id));
            let enqueue = |event: HarnessEvent, cx: &gpui::AsyncApp| {
                cx.update(|cx| {
                    Engine::sessions(cx)
                        .update(cx, |sessions, cx| sessions.enqueue_event(&id, event, cx));
                });
            };
            let (sink, _accepted, signals) = signal_channel();
            let result = drive(
                registry.compact_harness_context(harness, input, sink),
                &signals,
                |signal| {
                    if let TurnSignal::Event(event) = signal
                        && current_gen(cx) == generation
                    {
                        enqueue(*event, cx);
                    }
                },
            )
            .await;
            if current_gen(cx) != generation {
                return;
            }
            match result {
                Ok(()) => enqueue(
                    HarnessEvent::Status {
                        text: "Compacted context".into(),
                    },
                    cx,
                ),
                Err(error) => {
                    let message = error.to_string();
                    let message = if message.is_empty() {
                        format!("{harness} could not compact this context")
                    } else {
                        message
                    };
                    enqueue(HarnessEvent::SessionError { message }, cx);
                }
            }
            cx.update(|cx| {
                Engine::sessions(cx).update(cx, |sessions, cx| {
                    sessions.flush(cx);
                    sessions.update(&id, cx, |session| session.busy = Some(false));
                });
                sync_dock_badge(cx);
            });
        })
        .detach();
        true
    }

    /// `onStop`: end the turn. Unless `managed`, a session in an
    /// orchestration run stops the run instead.
    pub fn stop(&mut self, session_id: &str, managed: bool, cx: &mut Context<Self>) {
        let sessions = Engine::sessions(cx);
        let session = sessions.read(cx).get(session_id).cloned();
        if let Some(session) = &session
            && self.peers.remote.is_remote(&session.cwd, cx)
        {
            self.peers.remote.stop(session_id, cx);
            return;
        }
        if !managed
            && let Some(stopping) = self.peers.orchestration.stop_for_session(session_id, cx)
        {
            stopping.detach();
            return;
        }
        sessions.update(cx, |sessions, cx| {
            sessions.bump_turn_gen(session_id);
            sessions.flush(cx);
        });
        if let Some(session) = &session {
            for harness in session_child_harnesses(session) {
                cancel_harness(&self.config.registry, harness, session_id, cx);
            }
        }
        sessions.update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                stop_streaming_mut(session, now_ms());
                if is_preparing_handoff(session) {
                    let brief = build_deterministic_handoff(session, None, None);
                    *session = complete_handoff(session, &brief);
                }
                session.worktree_preparing = None;
                if session
                    .queued_messages
                    .as_ref()
                    .is_some_and(|queue| !queue.is_empty())
                {
                    session.queue_status = Some(MessageQueueStatus::Paused);
                }
            });
        });
        notify_review_changed(Some(session_id), cx);
        if let Some(session) = &session {
            nudge_after_turn(Some(session_work_cwd(session)), cx);
        }
    }
}
