//! The parts of src/features/sessions/model/messageQueue.ts that
//! `submitSession` calls: which queued row may send, and how a sent row
//! leaves the queue.
//!
//! The attention package owns the queue and its dispatch (`attention::queue`
//! has the full port). These copies keep `--features submit` building on its
//! own; the lead can point both at one module once the packages merge.

use monocode_core::Session;
use monocode_core::session::{MessageQueueStatus, QueuedMessage};

use super::handoff::is_preparing_handoff;

/// How a queued row reaches `submit`: the idle head sends on its own, or the
/// user steers any row in now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueSubmitMode {
    Dispatch,
    Steer,
}

/// `queuedHead`.
pub fn queued_head(session: &Session) -> Option<&QueuedMessage> {
    session.queued_messages.as_ref()?.first()
}

/// `isEditingQueuedHead`: hold auto-dispatch only while the row about to
/// send is being edited.
pub fn is_editing_queued_head(session: &Session) -> bool {
    queued_head(session)
        .is_some_and(|head| session.editing_queued_message_id.as_deref() == Some(head.id.as_str()))
}

/// `dequeueQueuedMessage`.
pub fn dequeue_queued_message(session: &mut Session, message_id: &str) {
    let remaining: Vec<QueuedMessage> = session
        .queued_messages
        .take()
        .unwrap_or_default()
        .into_iter()
        .filter(|message| message.id != message_id)
        .collect();
    if remaining.is_empty() {
        session.queue_status = None;
    } else {
        session.queued_messages = Some(remaining);
    }
    if session.editing_queued_message_id.as_deref() == Some(message_id) {
        session.editing_queued_message_id = None;
    }
}

/// `canDispatchQueuedHead`: the idle session can send its queued head.
pub fn can_dispatch_queued_head(session: &Session) -> bool {
    if session.is_busy() || session.usage_limit.is_some() {
        return false;
    }
    if matches!(
        session.queue_status,
        Some(MessageQueueStatus::Paused | MessageQueueStatus::Resuming)
    ) {
        return false;
    }
    if queued_head(session).is_none() || is_editing_queued_head(session) {
        return false;
    }
    !is_preparing_handoff(session)
}

/// `queuedMessageForSubmit`: resolve a queued row for auto-dispatch (the
/// idle head) or an explicit steer (any row).
pub fn queued_message_for_submit<'a>(
    session: &'a Session,
    message_id: &str,
    mode: QueueSubmitMode,
) -> Option<&'a QueuedMessage> {
    let message = session
        .queued_messages
        .as_ref()?
        .iter()
        .find(|entry| entry.id == message_id)?;
    if mode == QueueSubmitMode::Steer {
        return Some(message);
    }
    if queued_head(session).map(|head| head.id.as_str()) != Some(message_id) {
        return None;
    }
    can_dispatch_queued_head(session).then_some(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::submit::handoff::append_preparing_handoff;
    use monocode_core::HarnessId;
    use monocode_core::session::UsageLimit;

    fn queued(id: &str) -> QueuedMessage {
        QueuedMessage {
            id: id.into(),
            text: id.into(),
            attachments: vec![],
            note_card: None,
            handoff_card: None,
            intent: None,
        }
    }

    fn chat() -> Session {
        Session {
            queued_messages: Some(vec![queued("a"), queued("b")]),
            queue_status: Some(MessageQueueStatus::Active),
            ..Session::blank("s", HarnessId::Claude, "claude:default", "/tmp/project")
        }
    }

    #[test]
    fn returns_the_first_queued_follow_up() {
        assert_eq!(queued_head(&chat()).unwrap().id, "a");
        let empty = Session {
            queued_messages: None,
            ..chat()
        };
        assert!(queued_head(&empty).is_none());
    }

    #[test]
    fn is_true_only_when_the_head_row_is_the_one_being_edited() {
        assert!(!is_editing_queued_head(&chat()));
        let editing = |id: &str| Session {
            editing_queued_message_id: Some(id.into()),
            ..chat()
        };
        assert!(is_editing_queued_head(&editing("a")));
        assert!(!is_editing_queued_head(&editing("b")));
    }

    #[test]
    fn dispatches_an_idle_session_with_a_queued_head() {
        assert!(can_dispatch_queued_head(&chat()));
    }

    #[test]
    fn holds_while_busy_paused_resuming_usage_limited_or_editing_the_head() {
        assert!(!can_dispatch_queued_head(&Session {
            busy: Some(true),
            ..chat()
        }));
        for status in [MessageQueueStatus::Paused, MessageQueueStatus::Resuming] {
            assert!(!can_dispatch_queued_head(&Session {
                queue_status: Some(status),
                ..chat()
            }));
        }
        assert!(!can_dispatch_queued_head(&Session {
            usage_limit: Some(UsageLimit {
                resets_at: Some(1_000),
                resume_at_reset: None
            }),
            ..chat()
        }));
        assert!(!can_dispatch_queued_head(&Session {
            editing_queued_message_id: Some("a".into()),
            ..chat()
        }));
        assert!(can_dispatch_queued_head(&Session {
            editing_queued_message_id: Some("b".into()),
            ..chat()
        }));
    }

    #[test]
    fn does_not_dispatch_during_a_preparing_handoff_or_with_an_empty_queue() {
        let preparing = append_preparing_handoff(
            &Session {
                queued_messages: Some(vec![queued("a")]),
                ..chat()
            },
            HarnessId::Claude,
            HarnessId::Cursor,
        );
        assert!(!can_dispatch_queued_head(&preparing));
        assert!(!can_dispatch_queued_head(&Session {
            queued_messages: None,
            ..chat()
        }));
    }

    #[test]
    fn drops_the_id_and_clears_queue_state_when_the_last_item_goes() {
        let mut one = Session {
            queued_messages: Some(vec![queued("a")]),
            ..chat()
        };
        dequeue_queued_message(&mut one, "a");
        assert_eq!(one.queued_messages, None);
        assert_eq!(one.queue_status, None);
    }

    #[test]
    fn keeps_editing_another_row_after_the_head_is_sent() {
        let mut next = Session {
            editing_queued_message_id: Some("b".into()),
            ..chat()
        };
        dequeue_queued_message(&mut next, "a");
        let ids: Vec<&str> = next
            .queued_messages
            .iter()
            .flatten()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(ids, ["b"]);
        assert_eq!(next.editing_queued_message_id.as_deref(), Some("b"));
        assert_eq!(next.queue_status, Some(MessageQueueStatus::Active));
    }

    #[test]
    fn only_auto_dispatches_the_idle_head_and_steers_any_row() {
        let session = chat();
        assert_eq!(
            queued_message_for_submit(&session, "a", QueueSubmitMode::Dispatch)
                .map(|m| m.id.as_str()),
            Some("a")
        );
        assert!(queued_message_for_submit(&session, "b", QueueSubmitMode::Dispatch).is_none());
        let busy = Session {
            busy: Some(true),
            ..chat()
        };
        assert!(queued_message_for_submit(&busy, "a", QueueSubmitMode::Dispatch).is_none());
        assert_eq!(
            queued_message_for_submit(&busy, "b", QueueSubmitMode::Steer).map(|m| m.id.as_str()),
            Some("b")
        );
        let paused = Session {
            queue_status: Some(MessageQueueStatus::Paused),
            ..chat()
        };
        assert!(queued_message_for_submit(&paused, "a", QueueSubmitMode::Steer).is_some());
        assert!(queued_message_for_submit(&session, "missing", QueueSubmitMode::Steer).is_none());
    }
}
