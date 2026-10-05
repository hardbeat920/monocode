//! The `Queues` entity: follow-ups waiting for the current turn.
//!
//! Ports the App.tsx queue effect (lines 7391-7447), which sends an idle
//! session's queued head as the next turn, and the queue callbacks
//! (`onDeleteQueuedMessage`, `onQueuedMessageEditingChange`,
//! `onEditQueuedMessage`, `onSteerQueuedMessage`, `onResumeQueue`). Turns go
//! through the `AttentionSubmit` hook.

use std::collections::HashSet;

use gpui::{App, Context, Entity, Subscription, Task};
use monocode_core::block::TurnIntent;
use monocode_core::harness_event::HarnessEvent;
use monocode_core::session::MessageQueueStatus;
use monocode_core::settings::FollowUpBehavior;

use super::Attention;
use super::hooks::SubmitRequest;
use super::queue::{
    QueueSubmitMode, can_dispatch_queued_head, dequeue_queued_message, queued_message_for_submit,
};
use crate::runtime::engine::Engine;
use crate::runtime::in_flight::CONTINUE_PROMPT;
use crate::runtime::sessions::Sessions;

/// The status line a busy session shows when the user steers an
/// orchestration request into it.
pub const ORCHESTRATION_WAITS_STATUS: &str =
    "Orchestration planning will start after the current turn finishes.";

/// Auto-dispatch of queued follow-ups.
pub struct Queues {
    /// `queueDispatchingRef`: sessions with a dispatch scheduled.
    dispatching: HashSet<String>,
    /// The current run's timers. A new run drops them, as the effect
    /// cleanup cleared its timeouts.
    scheduled: Vec<Task<()>>,
    scheduled_ids: HashSet<String>,
    _observe: Subscription,
}

impl Queues {
    pub fn new(sessions: &Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        let mut queues = Self {
            dispatching: HashSet::new(),
            scheduled: Vec::new(),
            scheduled_ids: HashSet::new(),
            _observe: cx.observe(sessions, |this, sessions, cx| this.run(sessions, cx)),
        };
        queues.run(sessions.clone(), cx);
        queues
    }

    /// Sessions with a dispatch scheduled for the next tick.
    pub fn dispatching(&self) -> &HashSet<String> {
        &self.dispatching
    }

    /// The queue effect, run after every `Sessions` change.
    fn run(&mut self, sessions: Entity<Sessions>, cx: &mut Context<Self>) {
        // The previous run's cleanup.
        self.scheduled.clear();
        for id in self.scheduled_ids.drain() {
            self.dispatching.remove(&id);
        }

        let mut resumed = Vec::new();
        let mut dispatch = Vec::new();
        for session in sessions.read(cx).all() {
            let queued = session.queued_messages.as_deref().unwrap_or(&[]);
            if session.is_busy() || queued.is_empty() {
                continue;
            }
            if session.queue_status == Some(MessageQueueStatus::Resuming) {
                resumed.push(session.id.clone());
                continue;
            }
            if !can_dispatch_queued_head(session) || self.dispatching.contains(&session.id) {
                continue;
            }
            let Some(next) = queued.first() else {
                continue;
            };
            dispatch.push((session.id.clone(), next.id.clone()));
        }

        for (session_id, head_id) in dispatch {
            self.dispatching.insert(session_id.clone());
            self.scheduled_ids.insert(session_id.clone());
            // `setTimeout(0)`: the next tick, after this change has settled.
            self.scheduled.push(cx.spawn(async move |this, cx| {
                let request = this
                    .update(cx, |this, cx| this.dispatch_head(&session_id, &head_id, cx))
                    .ok()
                    .flatten();
                if let Some(request) = request {
                    cx.update(|cx| Attention::submit(cx).submit(request, cx));
                }
            }));
        }

        if !resumed.is_empty() {
            // The continued turn already ran; the queue goes back to active.
            sessions.update(cx, |sessions, cx| {
                for id in &resumed {
                    sessions.update(id, cx, |session| {
                        session.queue_status = Some(MessageQueueStatus::Active)
                    });
                }
            });
        }
    }

    /// The timer body: re-check the head against the latest session.
    fn dispatch_head(
        &mut self,
        session_id: &str,
        head_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<SubmitRequest> {
        self.dispatching.remove(session_id);
        let sessions = Engine::sessions(cx);
        let latest = sessions.read(cx).get(session_id)?;
        let head = latest.queued_messages.as_ref()?.first()?;
        if head.id != head_id || !can_dispatch_queued_head(latest) {
            return None;
        }
        Some(SubmitRequest {
            session_id: session_id.to_string(),
            text: head.text.clone(),
            attachments: head.attachments.clone(),
            follow_up_behavior: None,
            queued_message_id: Some(head.id.clone()),
            note_card: head.note_card.clone(),
            handoff_card: head.handoff_card.clone(),
            intent: head.intent,
        })
    }

    // The callbacks. They change `Sessions` and call the submit hook, so they
    // take the app rather than the entity.

    /// `onDeleteQueuedMessage`.
    pub fn delete(session_id: &str, message_id: &str, cx: &mut App) {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                *session = dequeue_queued_message(session, message_id);
            });
        });
    }

    /// `onQueuedMessageEditingChange`: hold auto-dispatch while the head row
    /// is being edited.
    pub fn set_editing(session_id: &str, message_id: Option<&str>, cx: &mut App) {
        let message_id = message_id.map(str::to_string);
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                session.editing_queued_message_id = message_id
            });
        });
    }

    /// `onEditQueuedMessage`.
    pub fn edit(session_id: &str, message_id: &str, text: &str, cx: &mut App) {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                if let Some(queued) = session.queued_messages.as_mut() {
                    for message in queued.iter_mut().filter(|message| message.id == message_id) {
                        message.text = text.to_string();
                    }
                }
                session.editing_queued_message_id = None;
            });
        });
    }

    /// `onSteerQueuedMessage`: send any queued row now. An orchestration
    /// request waits for the running turn instead.
    pub fn steer(session_id: &str, message_id: &str, cx: &mut App) {
        let sessions = Engine::sessions(cx);
        let (busy, message) = {
            let all = sessions.read(cx);
            let Some(session) = all.get(session_id) else {
                return;
            };
            let Some(message) =
                queued_message_for_submit(session, message_id, QueueSubmitMode::Steer)
            else {
                return;
            };
            (session.is_busy(), message.clone())
        };
        if message.intent == Some(TurnIntent::Orchestrate) && busy {
            sessions.update(cx, |sessions, cx| {
                sessions.enqueue_event(
                    session_id,
                    HarnessEvent::Status {
                        text: ORCHESTRATION_WAITS_STATUS.into(),
                    },
                    cx,
                );
                sessions.flush(cx);
            });
            return;
        }
        let request = SubmitRequest {
            session_id: session_id.to_string(),
            text: message.text,
            attachments: message.attachments,
            follow_up_behavior: Some(FollowUpBehavior::Steer),
            queued_message_id: Some(message.id),
            note_card: message.note_card,
            handoff_card: message.handoff_card,
            intent: message.intent,
        };
        Attention::submit(cx).submit(request, cx);
    }

    /// `onResumeQueue`: a paused queue resumes with a continue turn, then
    /// dispatches again once that turn ends.
    pub fn resume(session_id: &str, cx: &mut App) {
        let sessions = Engine::sessions(cx);
        let ready = sessions.read(cx).get(session_id).is_some_and(|session| {
            !session.is_busy()
                && session.queue_status == Some(MessageQueueStatus::Paused)
                && session
                    .queued_messages
                    .as_ref()
                    .is_some_and(|queued| !queued.is_empty())
        });
        if !ready {
            return;
        }
        sessions.update(cx, |sessions, cx| {
            sessions.update(session_id, cx, |session| {
                session.queue_status = Some(MessageQueueStatus::Resuming)
            });
        });
        let mut request = SubmitRequest::text(session_id, CONTINUE_PROMPT);
        request.follow_up_behavior = Some(FollowUpBehavior::Steer);
        Attention::submit(cx).submit(request, cx);
    }
}
