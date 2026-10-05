//! Port of src/features/sessions/model/messageQueue.ts: which queued
//! follow-up may send, and how a sent or deleted row leaves the queue.

use monocode_core::block::{BlockRole, HandoffStatus};
use monocode_core::session::{QueuedMessage, Session};

/// How a queued row reaches `onSubmit`: the idle head sends on its own
/// (`dispatch`), or the user sends any row now (`steer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueSubmitMode {
    Dispatch,
    Steer,
}

/// `isPreparingHandoff` from handoff.ts: a handoff block is still writing
/// its recap.
pub fn is_preparing_handoff(session: &Session) -> bool {
    session.blocks.iter().any(|block| {
        block.role == BlockRole::Handoff
            && block
                .handoff
                .as_ref()
                .is_some_and(|handoff| handoff.status == HandoffStatus::Preparing)
    })
}

/// `queuedHead`.
pub fn queued_head(session: &Session) -> Option<&QueuedMessage> {
    session.queued_messages.as_ref()?.first()
}

/// `isEditingQueuedHead`: hold auto-dispatch only while the item about to
/// send is being edited.
pub fn is_editing_queued_head(session: &Session) -> bool {
    queued_head(session)
        .is_some_and(|head| session.editing_queued_message_id.as_deref() == Some(head.id.as_str()))
}

/// `dequeueQueuedMessage`.
pub fn dequeue_queued_message(session: &Session, message_id: &str) -> Session {
    let queued: Vec<QueuedMessage> = session
        .queued_messages
        .iter()
        .flatten()
        .filter(|message| message.id != message_id)
        .cloned()
        .collect();
    let mut next = session.clone();
    let any = !queued.is_empty();
    next.queued_messages = any.then_some(queued);
    next.queue_status = if any { session.queue_status } else { None };
    if session.editing_queued_message_id.as_deref() == Some(message_id) {
        next.editing_queued_message_id = None;
    }
    next
}

/// `canDispatchQueuedHead`: the idle session can send its queued head as a
/// new turn. Busy, paused, resuming, usage-limited, preparing-handoff, and
/// editing-the-head sessions all wait.
pub fn can_dispatch_queued_head(session: &Session) -> bool {
    use monocode_core::session::MessageQueueStatus;
    if session.is_busy() {
        return false;
    }
    if session.usage_limit.is_some() {
        return false;
    }
    if matches!(
        session.queue_status,
        Some(MessageQueueStatus::Paused | MessageQueueStatus::Resuming)
    ) {
        return false;
    }
    if queued_head(session).is_none() {
        return false;
    }
    if is_editing_queued_head(session) {
        return false;
    }
    if is_preparing_handoff(session) {
        return false;
    }
    true
}

/// `queuedMessageForSubmit`: resolve a queued row for auto-dispatch (the
/// idle head) or an explicit Steer (any row).
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
    if !can_dispatch_queued_head(session) {
        return None;
    }
    Some(message)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use monocode_core::block::{Block, HandoffMeta};
    use monocode_core::session::{MessageQueueStatus, UsageLimit};
    use monocode_core::{Extra, HarnessId};

    pub(crate) fn queued(id: &str, text: &str) -> QueuedMessage {
        QueuedMessage {
            id: id.into(),
            text: text.into(),
            attachments: Vec::new(),
            note_card: None,
            handoff_card: None,
            intent: None,
        }
    }

    fn chat() -> Session {
        let mut session = Session::blank("s1", HarnessId::Claude, "sonnet", "/tmp/project");
        session.queued_messages = Some(vec![queued("a", "first"), queued("b", "second")]);
        session.queue_status = Some(MessageQueueStatus::Active);
        session
    }

    #[test]
    fn queued_head_returns_the_first_queued_follow_up() {
        assert_eq!(queued_head(&chat()).map(|m| m.id.as_str()), Some("a"));
        let mut empty = chat();
        empty.queued_messages = None;
        assert!(queued_head(&empty).is_none());
    }

    #[test]
    fn is_editing_queued_head_only_for_the_head_row() {
        assert!(!is_editing_queued_head(&chat()));
        let mut editing = chat();
        editing.editing_queued_message_id = Some("a".into());
        assert!(is_editing_queued_head(&editing));
        editing.editing_queued_message_id = Some("b".into());
        assert!(!is_editing_queued_head(&editing));
    }

    #[test]
    fn dispatches_an_idle_session_with_a_queued_head() {
        assert!(can_dispatch_queued_head(&chat()));
    }

    #[test]
    fn holds_while_the_session_is_busy_paused_or_resuming() {
        let mut busy = chat();
        busy.busy = Some(true);
        assert!(!can_dispatch_queued_head(&busy));
        let mut paused = chat();
        paused.queue_status = Some(MessageQueueStatus::Paused);
        assert!(!can_dispatch_queued_head(&paused));
        let mut resuming = chat();
        resuming.queue_status = Some(MessageQueueStatus::Resuming);
        assert!(!can_dispatch_queued_head(&resuming));
    }

    #[test]
    fn holds_while_the_last_turn_is_stopped_at_a_usage_limit() {
        let mut limited = chat();
        limited.usage_limit = Some(UsageLimit {
            resets_at: Some(1_000),
            resume_at_reset: None,
        });
        assert!(!can_dispatch_queued_head(&limited));
    }

    #[test]
    fn holds_only_when_the_head_item_is_being_edited() {
        let mut editing = chat();
        editing.editing_queued_message_id = Some("a".into());
        assert!(!can_dispatch_queued_head(&editing));
        editing.editing_queued_message_id = Some("b".into());
        assert!(can_dispatch_queued_head(&editing));
    }

    #[test]
    fn does_not_dispatch_during_a_preparing_handoff() {
        let mut preparing = chat();
        preparing.queued_messages = Some(vec![queued("a", "a")]);
        let mut block = Block::new("h1", BlockRole::Handoff, "");
        block.handoff = Some(HandoffMeta {
            from: HarnessId::Claude,
            to: HarnessId::Cursor,
            status: HandoffStatus::Preparing,
            pending: Some(false),
            extra: Extra::new(),
        });
        preparing.blocks.push(block);
        assert!(!can_dispatch_queued_head(&preparing));
    }

    #[test]
    fn does_not_dispatch_an_empty_queue() {
        let mut empty = chat();
        empty.queued_messages = None;
        assert!(!can_dispatch_queued_head(&empty));
    }

    #[test]
    fn drops_the_id_and_clears_queue_state_when_the_last_item_goes() {
        let mut one = chat();
        one.queued_messages = Some(vec![queued("a", "a")]);
        let next = dequeue_queued_message(&one, "a");
        assert!(next.queued_messages.is_none());
        assert!(next.queue_status.is_none());
    }

    #[test]
    fn keeps_editing_another_row_after_the_head_is_sent() {
        let mut editing = chat();
        editing.editing_queued_message_id = Some("b".into());
        let next = dequeue_queued_message(&editing, "a");
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
    fn only_auto_dispatches_the_idle_head() {
        let id = |m: Option<&QueuedMessage>| m.map(|m| m.id.clone());
        assert_eq!(
            id(queued_message_for_submit(
                &chat(),
                "a",
                QueueSubmitMode::Dispatch
            )),
            Some("a".into())
        );
        assert!(queued_message_for_submit(&chat(), "b", QueueSubmitMode::Dispatch).is_none());
        let mut busy = chat();
        busy.busy = Some(true);
        assert!(queued_message_for_submit(&busy, "a", QueueSubmitMode::Dispatch).is_none());
    }

    #[test]
    fn lets_steer_target_any_remaining_row_including_while_busy_or_paused() {
        let mut busy = chat();
        busy.busy = Some(true);
        assert_eq!(
            queued_message_for_submit(&busy, "b", QueueSubmitMode::Steer).map(|m| m.id.as_str()),
            Some("b")
        );
        let mut paused = chat();
        paused.queue_status = Some(MessageQueueStatus::Paused);
        assert_eq!(
            queued_message_for_submit(&paused, "a", QueueSubmitMode::Steer).map(|m| m.id.as_str()),
            Some("a")
        );
        assert!(queued_message_for_submit(&chat(), "missing", QueueSubmitMode::Steer).is_none());
    }
}
