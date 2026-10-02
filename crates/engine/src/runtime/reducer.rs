//! A thin wrapper over the transcript reducer in `monocode_core::reducer`
//! (the port of src/integrations/harness/core/apply.ts and preview.ts).
//! The engine reaches the reducer only through this module, so a change of
//! reducer signature is a change here.
//!
//! `session_child_harnesses` is a port of handoff.ts, and
//! `last_user_block_id` of the App.tsx helper.

use monocode_core::block::BlockRole;
use monocode_core::block::HandoffStatus;
use monocode_core::block::ToolPreview;
use monocode_core::harness_event::HarnessEvent;
use monocode_core::reducer;
use monocode_core::{HarnessId, Session};
use serde_json::Value;

/// A transcript reducer: apply `events` in place and report whether the
/// session changed. `Sessions` holds one so tests can swap it.
pub type Reducer = fn(&mut Session, &[HarnessEvent]) -> bool;

/// `applyHarnessEvents`, in place.
pub fn apply_harness_events(session: &mut Session, events: &[HarnessEvent]) -> bool {
    reducer::apply_harness_events_mut(&mut reducer::SystemEnv, session, events)
}

/// `Date.now()`.
pub fn now_ms() -> i64 {
    reducer::now_ms()
}

/// `stopStreaming`: end the turn.
pub fn stop_streaming(session: &Session, ended_at: i64) -> Session {
    reducer::stop_streaming(session, ended_at)
}

/// `isEditTool` from preview.ts.
pub fn is_edit_tool(
    kind: Option<&str>,
    title: Option<&str>,
    preview: Option<&ToolPreview>,
) -> bool {
    reducer::is_edit_tool(kind, title, preview)
}

/// `titleFromToolInput` from preview.ts.
pub fn title_from_tool_input(name: &str, kind: &str, input: &Value) -> String {
    let record = input.as_object().cloned().unwrap_or_default();
    reducer::title_from_tool_input(name, kind, &record)
}

/// `sessionChildHarnesses` from handoff.ts: the session's harness, the one
/// a pending switch leaves, and the source of a handoff still preparing.
pub fn session_child_harnesses(session: &Session) -> Vec<HarnessId> {
    let mut ids = vec![session.harness];
    let mut add = |id: HarnessId| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    if let Some(switch) = session.pending_switch.as_ref() {
        add(switch.from);
    }
    let last = session
        .blocks
        .iter()
        .rev()
        .find(|block| block.role == BlockRole::Handoff)
        .and_then(|block| block.handoff.as_ref());
    if let Some(handoff) = last
        && handoff.status == HandoffStatus::Preparing
    {
        add(handoff.from);
    }
    ids
}

/// `lastUserBlockId` from App.tsx.
pub fn last_user_block_id(session: &Session) -> Option<&str> {
    session
        .blocks
        .iter()
        .rev()
        .find(|block| block.role == BlockRole::User)
        .map(|block| block.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::Block;
    use monocode_core::block::{Extra, HandoffMeta};

    fn session() -> Session {
        Session::blank("s", HarnessId::Claude, "claude:opus", "/repo")
    }

    #[test]
    fn child_harnesses_include_switch_and_preparing_handoff() {
        let mut s = session();
        assert_eq!(session_child_harnesses(&s), vec![HarnessId::Claude]);
        s.pending_switch = Some(monocode_core::session::PendingHarnessSwitch {
            from: HarnessId::Codex,
            from_model: "m".into(),
            from_settings: Default::default(),
            from_provider_session_id: None,
            from_provider_account_id: None,
        });
        s.blocks.push(Block {
            handoff: Some(HandoffMeta {
                from: HarnessId::Cursor,
                to: HarnessId::Claude,
                status: HandoffStatus::Preparing,
                pending: None,
                extra: Extra::new(),
            }),
            ..Block::new("h", BlockRole::Handoff, "")
        });
        assert_eq!(
            session_child_harnesses(&s),
            vec![HarnessId::Claude, HarnessId::Codex, HarnessId::Cursor]
        );
    }

    #[test]
    fn finds_the_last_user_block() {
        let mut s = session();
        assert_eq!(last_user_block_id(&s), None);
        s.blocks = vec![
            Block::new("u1", BlockRole::User, "a"),
            Block::new("a1", BlockRole::Assistant, "b"),
            Block::new("u2", BlockRole::User, "c"),
            Block::new("a2", BlockRole::Assistant, "d"),
        ];
        assert_eq!(last_user_block_id(&s), Some("u2"));
    }

    #[test]
    fn reports_unchanged_sessions() {
        let mut s = session();
        assert!(!apply_harness_events(&mut s, &[]));
        assert!(apply_harness_events(
            &mut s,
            &[HarnessEvent::MessageDelta { text: "hi".into() }]
        ));
        assert_eq!(s.blocks.last().map(|b| b.text.as_str()), Some("hi"));
    }
}
