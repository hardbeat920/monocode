//! Port of src/features/sessions/model/editLastTurn.ts: edit the last user
//! message and resend it, rewinding provider state first.
//!
//! The TypeScript attempt object called `onRejected` itself. Here
//! [`EditedResendAttempt::reject`] returns the rejection to report, once, and
//! the caller runs the composer's callback with its own context.

use std::cell::Cell;
use std::collections::HashSet;

use monocode_core::session::EditedResendRejection;
use monocode_core::{Attachment, Block, BlockRole, HarnessId, Session, js};

use super::operator_command::{is_operator_user_turn, operator_user_prompt};

/// `harnessSupportsEditLastTurn`: harnesses that can rewind provider state
/// before resending an edited prompt.
pub fn harness_supports_edit_last_turn(harness: HarnessId) -> bool {
    matches!(
        harness,
        HarnessId::Pi | HarnessId::Omp | HarnessId::Codex | HarnessId::Opencode
    )
}

/// `lastUserTurnStartIndex`: the user block that starts the latest turn.
pub fn last_user_turn_start_index(blocks: &[Block]) -> Option<usize> {
    blocks.iter().rposition(|block| {
        block.role == BlockRole::User && !block.is_internal() && !block.is_draft()
    })
}

/// `lastUserTurnBlock`.
pub fn last_user_turn_block(blocks: &[Block]) -> Option<&Block> {
    last_user_turn_start_index(blocks).map(|index| &blocks[index])
}

/// `truncateBeforeLastUserTurn`.
pub fn truncate_before_last_user_turn(blocks: &[Block]) -> Vec<Block> {
    match last_user_turn_start_index(blocks) {
        Some(start) => blocks[..start].to_vec(),
        None => blocks.to_vec(),
    }
}

/// `lastEditableTurnStartIndex`: for a steered Codex turn, the first user
/// message of that provider turn.
pub fn last_editable_turn_start_index(session: &Session) -> Option<usize> {
    let latest = last_user_turn_start_index(&session.blocks)?;
    let Some(provider_turn_id) = session.blocks[latest]
        .provider_turn_id
        .as_deref()
        .filter(|id| !id.is_empty())
    else {
        return Some(latest);
    };
    if session.harness != HarnessId::Codex {
        return Some(latest);
    }
    let mut start = latest;
    for index in (0..latest).rev() {
        let block = &session.blocks[index];
        if block.role == BlockRole::User
            && block.provider_turn_id.as_deref() != Some(provider_turn_id)
        {
            break;
        }
        if block.role == BlockRole::User && !block.is_internal() && !block.is_draft() {
            start = index;
        }
    }
    Some(start)
}

/// `truncateBeforeLastEditableTurn`.
pub fn truncate_before_last_editable_turn(session: &Session) -> Vec<Block> {
    match last_editable_turn_start_index(session) {
        Some(start) => session.blocks[..start].to_vec(),
        None => session.blocks.clone(),
    }
}

/// `EditedResendPreparation`.
#[derive(Debug, Clone, PartialEq)]
pub struct EditedResendPreparation {
    pub blocks: Vec<Block>,
    pub provider_turn_id: Option<String>,
}

/// `prepareEditedResend`.
pub fn prepare_edited_resend(session: &Session) -> Option<EditedResendPreparation> {
    if !can_edit_last_turn(session) {
        return None;
    }
    let block = last_user_turn_block(&session.blocks)?;
    Some(EditedResendPreparation {
        blocks: truncate_before_last_editable_turn(session),
        provider_turn_id: block.provider_turn_id.clone().filter(|id| !id.is_empty()),
    })
}

/// `replaceEditedResend`.
pub fn replace_edited_resend(session: &Session) -> Session {
    Session {
        blocks: truncate_before_last_editable_turn(session),
        ..session.clone()
    }
}

/// `EditedResendAttempt`: one edit-and-resend attempt and its failure
/// recovery.
#[derive(Debug)]
pub struct EditedResendAttempt {
    pub blocks: Vec<Block>,
    pub provider_turn_id: Option<String>,
    provider_rewound: Cell<bool>,
    accepted: Cell<bool>,
    rejected: Cell<bool>,
}

impl EditedResendAttempt {
    pub fn mark_provider_rewound(&self) {
        self.provider_rewound.set(true);
    }

    pub fn mark_accepted(&self) {
        self.accepted.set(true);
    }

    pub fn is_accepted(&self) -> bool {
        self.accepted.get()
    }

    /// `replace`.
    pub fn replace(&self, session: &Session) -> Session {
        replace_edited_resend(session)
    }

    /// `recoverAfterFailure`: keep the rewound history when the provider
    /// already removed the old turn.
    pub fn recover_after_failure(&self, current: &Session) -> Session {
        if self.provider_rewound.get() && !self.accepted.get() {
            replace_edited_resend(current)
        } else {
            current.clone()
        }
    }

    /// `reject`: the rejection to hand to `onResendRejected`, or `None` when
    /// the attempt was accepted or already rejected.
    pub fn reject(&self) -> Option<EditedResendRejection> {
        if self.accepted.get() || self.rejected.get() {
            return None;
        }
        self.rejected.set(true);
        Some(EditedResendRejection {
            provider_rewound: self.provider_rewound.get(),
        })
    }
}

/// `createEditedResendAttempt`.
pub fn create_edited_resend_attempt(session: &Session) -> Option<EditedResendAttempt> {
    let preparation = prepare_edited_resend(session)?;
    Some(EditedResendAttempt {
        blocks: preparation.blocks,
        provider_turn_id: preparation.provider_turn_id,
        provider_rewound: Cell::new(false),
        accepted: Cell::new(false),
        rejected: Cell::new(false),
    })
}

/// `EditedResendCoordinator`: keeps concurrent submissions out while a
/// provider rewind is in progress.
#[derive(Debug, Default)]
pub struct EditedResendCoordinator {
    active: HashSet<String>,
}

impl EditedResendCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self, session_id: &str) -> bool {
        self.active.contains(session_id)
    }

    pub fn start(&mut self, session_id: &str) -> bool {
        self.active.insert(session_id.to_string())
    }

    pub fn finish(&mut self, session_id: &str) {
        self.active.remove(session_id);
    }
}

/// `LastTurnRecall`.
#[derive(Debug, Clone, PartialEq)]
pub struct LastTurnRecall {
    pub text: String,
    pub attachments: Vec<Attachment>,
}

/// `lastTurnRecall`: the last user message, for the composer to edit.
pub fn last_turn_recall(session: &Session) -> Option<LastTurnRecall> {
    let block = last_user_turn_block(&session.blocks)?;
    let attachments = block.attachments.clone().unwrap_or_default();
    if js::trim(&block.text).is_empty() && attachments.is_empty() {
        return None;
    }
    Some(LastTurnRecall {
        text: if is_operator_user_turn(block) {
            format!("/operator {}", operator_user_prompt(block))
        } else {
            block.text.clone()
        },
        attachments,
    })
}

/// `canEditLastTurn`.
pub fn can_edit_last_turn(session: &Session) -> bool {
    if session.inbox_ask.is_some() || session.is_busy() || session.pending_question.is_some() {
        return false;
    }
    if session.editing_queued_message_id.is_some() {
        return false;
    }
    if session
        .queued_messages
        .as_ref()
        .is_some_and(|queue| !queue.is_empty())
    {
        return false;
    }
    if !harness_supports_edit_last_turn(session.harness) {
        return false;
    }
    let Some(block) = last_user_turn_block(&session.blocks) else {
        return false;
    };
    if block.is_draft() {
        return false;
    }
    if session.harness == HarnessId::Codex
        && block.provider_turn_id.as_deref().is_none_or(str::is_empty)
    {
        return false;
    }
    if block.second_opinion.is_some()
        || block.note_card.is_some()
        || block.ci_context.as_deref().is_some_and(|c| !c.is_empty())
    {
        return false;
    }
    if session
        .blocks
        .iter()
        .any(|entry| entry.role == BlockRole::Handoff)
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::session::QueuedMessage;

    fn user(id: &str, text: &str) -> Block {
        Block::new(id, BlockRole::User, text)
    }

    fn assistant(id: &str, text: &str) -> Block {
        Block::new(id, BlockRole::Assistant, text)
    }

    fn chat(blocks: Vec<Block>) -> Session {
        Session {
            blocks,
            ..Session::blank("s", HarnessId::Pi, "pi-model", "/tmp")
        }
    }

    fn ids(blocks: &[Block]) -> Vec<&str> {
        blocks.iter().map(|block| block.id.as_str()).collect()
    }

    #[test]
    fn blocks_editing_ci_repair_requests_whose_context_is_absent_from_the_composer() {
        let session = chat(vec![
            Block {
                ci_context: Some(
                    "Checked commit: abc123\nRun tests: expected 200, received 500".into(),
                ),
                ..user("repair", "Fix 1 failed CI check for acme/web PR #42.")
            },
            assistant("reply", "Fixed the failing check."),
        ]);
        assert!(!can_edit_last_turn(&session));
        assert_eq!(prepare_edited_resend(&session), None);
    }

    #[test]
    fn finds_the_latest_user_turn() {
        let blocks = vec![
            user("u1", "first"),
            assistant("a1", "ok"),
            user("u2", "second"),
            assistant("a2", "done"),
        ];
        assert_eq!(last_user_turn_start_index(&blocks), Some(2));
        assert_eq!(ids(&truncate_before_last_user_turn(&blocks)), ["u1", "a1"]);
    }

    #[test]
    fn ignores_draft_user_blocks_when_selecting_the_editable_turn() {
        let blocks = vec![
            user("submitted", "keep this"),
            assistant("reply", "answer"),
            Block {
                draft: Some(true),
                ..user("draft", "saved draft")
            },
        ];
        let session = chat(blocks.clone());
        assert_eq!(last_user_turn_start_index(&blocks), Some(0));
        assert_eq!(
            last_turn_recall(&session),
            Some(LastTurnRecall {
                text: "keep this".into(),
                attachments: vec![]
            })
        );
        assert!(can_edit_last_turn(&session));
    }

    #[test]
    fn ignores_internal_user_turns_when_selecting_the_editable_turn() {
        let blocks = vec![
            user("visible", "keep this"),
            assistant("reply", "answer"),
            Block {
                internal: Some(true),
                ..user("internal", "hidden orchestration prompt")
            },
        ];
        let session = chat(blocks.clone());
        assert_eq!(last_user_turn_start_index(&blocks), Some(0));
        assert!(truncate_before_last_user_turn(&blocks).is_empty());
        assert_eq!(last_turn_recall(&session).unwrap().text, "keep this");
        assert!(can_edit_last_turn(&session));
    }

    #[test]
    fn recalls_the_last_user_message() {
        let session = chat(vec![user("u1", "hello"), assistant("a1", "hi")]);
        assert_eq!(
            last_turn_recall(&session),
            Some(LastTurnRecall {
                text: "hello".into(),
                attachments: vec![]
            })
        );
    }

    #[test]
    fn restores_operator_when_editing_an_activation_turn() {
        let session = chat(vec![
            Block {
                monocode: Some(true),
                ..user("u1", "list notes")
            },
            assistant("a1", "Here they are."),
        ]);
        assert_eq!(
            last_turn_recall(&session).unwrap().text,
            "/operator list notes"
        );
    }

    #[test]
    fn allows_edit_on_idle_pi_sessions_without_queued_follow_ups() {
        let session = chat(vec![user("u1", "hello"), assistant("a1", "hi")]);
        assert!(can_edit_last_turn(&session));
    }

    #[test]
    fn rewinds_the_whole_codex_turn_when_the_last_message_was_steered() {
        let turn = |id: &str, text: &str, turn: &str| Block {
            provider_turn_id: Some(turn.into()),
            ..user(id, text)
        };
        let session = Session {
            blocks: vec![
                turn("u1", "first", "t1"),
                assistant("a1", "done"),
                turn("u2", "second", "t2"),
                assistant("a2", "working"),
                turn("u3", "focus on tests", "t2"),
                assistant("a3", "updated"),
            ],
            ..Session::blank("s", HarnessId::Codex, "gpt", "/tmp")
        };
        assert_eq!(last_editable_turn_start_index(&session), Some(2));
        assert_eq!(
            ids(&truncate_before_last_editable_turn(&session)),
            ["u1", "a1"]
        );
        let prepared = prepare_edited_resend(&session).unwrap();
        assert_eq!(prepared.provider_turn_id.as_deref(), Some("t2"));
        assert_eq!(ids(&prepared.blocks), ["u1", "a1"]);
        assert_eq!(ids(&replace_edited_resend(&session).blocks), ["u1", "a1"]);
    }

    #[test]
    fn restores_edit_mode_when_the_provider_never_rewound() {
        let session = chat(vec![user("u1", "hello"), assistant("a1", "hi")]);
        let attempt = create_edited_resend_attempt(&session).unwrap();
        assert_eq!(attempt.recover_after_failure(&session), session);
        assert_eq!(
            attempt.reject(),
            Some(EditedResendRejection {
                provider_rewound: false
            })
        );
        assert_eq!(attempt.reject(), None);
    }

    #[test]
    fn keeps_provider_and_local_history_rewound_when_replacement_is_rejected() {
        let session = chat(vec![
            user("u1", "first"),
            assistant("a1", "done"),
            user("u2", "replace me"),
            assistant("a2", "old answer"),
        ]);
        let attempt = create_edited_resend_attempt(&session).unwrap();
        attempt.mark_provider_rewound();
        let recovered = attempt.recover_after_failure(&session);
        assert_eq!(ids(&recovered.blocks), ["u1", "a1"]);
        assert_eq!(
            attempt.reject(),
            Some(EditedResendRejection {
                provider_rewound: true
            })
        );
    }

    #[test]
    fn does_not_reject_or_roll_back_a_replacement_accepted_by_the_provider() {
        let session = chat(vec![user("u1", "hello"), assistant("a1", "hi")]);
        let attempt = create_edited_resend_attempt(&session).unwrap();
        attempt.mark_provider_rewound();
        attempt.mark_accepted();
        assert_eq!(attempt.reject(), None);
        assert!(attempt.is_accepted());
        assert_eq!(attempt.recover_after_failure(&session), session);
    }

    #[test]
    fn coordinates_only_one_rewind_per_session() {
        let mut coordinator = EditedResendCoordinator::new();
        assert!(coordinator.start("one"));
        assert!(coordinator.is_active("one"));
        assert!(!coordinator.start("one"));
        assert!(coordinator.start("two"));
        coordinator.finish("one");
        assert!(!coordinator.is_active("one"));
        assert!(coordinator.start("one"));
    }

    #[test]
    fn allows_edit_on_idle_opencode_sessions() {
        let session = Session {
            harness: HarnessId::Opencode,
            ..chat(vec![user("u1", "hello"), assistant("a1", "hi")])
        };
        assert!(can_edit_last_turn(&session));
    }

    #[test]
    fn blocks_edit_while_busy_queued_or_on_unsupported_harnesses() {
        let base = chat(vec![user("u1", "hello"), assistant("a1", "hi")]);
        assert!(!can_edit_last_turn(&Session {
            busy: Some(true),
            ..base.clone()
        }));
        assert!(!can_edit_last_turn(&Session {
            queued_messages: Some(vec![QueuedMessage {
                id: "q1".into(),
                text: "next".into(),
                attachments: vec![],
                note_card: None,
                handoff_card: None,
                intent: None,
            }]),
            ..base
        }));
        assert!(!can_edit_last_turn(&Session {
            harness: HarnessId::Claude,
            ..chat(vec![])
        }));
    }
}
