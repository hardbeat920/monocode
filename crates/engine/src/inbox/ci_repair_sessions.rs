//! Port of src/features/inbox/model/ciRepairSessions.ts: the chats a CI
//! repair may run in.

use std::collections::{HashMap, HashSet};

use monocode_core::block::{BlockRole, HandoffStatus};
use monocode_core::session::Session;

use crate::runtime::session_history::summary_from_session;
use crate::runtime::session_store::SessionSummary;

/// `isPreparingHandoff` from src/features/sessions/model/handoff.ts: a
/// handoff block is still writing its recap.
pub fn is_preparing_handoff(session: &Session) -> bool {
    session.blocks.iter().any(|block| {
        block.role == BlockRole::Handoff
            && block
                .handoff
                .as_ref()
                .is_some_and(|handoff| handoff.status == HandoffStatus::Preparing)
    })
}

/// Busy, switching provider, or preparing a handoff.
pub fn session_unavailable_for_repair(session: &Session) -> bool {
    session.is_busy() || session.pending_switch.is_some() || is_preparing_handoff(session)
}

/// `ciRepairSessions`: history and open chats, with an open chat's summary
/// winning over its stored one, minus busy chats and removed worktrees.
pub fn ci_repair_sessions(history: &[SessionSummary], sessions: &[Session]) -> Vec<SessionSummary> {
    let unavailable: HashSet<&str> = sessions
        .iter()
        .filter(|session| session_unavailable_for_repair(session))
        .map(|session| session.id.as_str())
        .collect();
    // `new Map(entries)`: a later entry replaces the value but keeps the
    // first entry's position.
    let mut order: Vec<String> = Vec::new();
    let mut by_id: HashMap<String, SessionSummary> = HashMap::new();
    let live = sessions
        .iter()
        .filter(|session| session.inbox_ask.is_none())
        .map(|session| summary_from_session(session, None));
    for summary in history.iter().cloned().chain(live) {
        if !by_id.contains_key(&summary.id) {
            order.push(summary.id.clone());
        }
        by_id.insert(summary.id.clone(), summary);
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .filter(|session| {
            !unavailable.contains(session.id.as_str()) && session.worktree_removed != Some(true)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use monocode_core::block::Block;
    use monocode_core::harness::HarnessId;
    use serde_json::json;

    use super::*;

    fn session(id: &str, harness: HarnessId) -> Session {
        Session::blank(id, harness, "", "/web")
    }

    fn ids(summaries: &[SessionSummary]) -> Vec<&str> {
        summaries
            .iter()
            .map(|summary| summary.id.as_str())
            .collect()
    }

    #[test]
    fn excludes_removed_worktrees_from_history_and_live_chats() {
        let ready = session("ready", HarnessId::Claude);
        let mut removed = session("removed-live", HarnessId::Claude);
        removed.worktree_removed = Some(true);
        let mut removed_history = removed.clone();
        removed_history.id = "removed-history".into();
        let mut stale = removed.clone();
        stale.worktree_removed = Some(false);
        let history: Vec<SessionSummary> = [removed_history, stale, ready]
            .iter()
            .map(|session| summary_from_session(session, None))
            .collect();
        assert_eq!(ids(&ci_repair_sessions(&history, &[removed])), ["ready"]);
    }

    #[test]
    fn offers_available_chats_without_restoring_unavailable_live_chats_from_history() {
        let closed = session("closed", HarnessId::Claude);
        let mut ready = session("ready", HarnessId::Claude);
        ready.title = "Current title".into();
        let mut busy = session("busy", HarnessId::Claude);
        busy.busy = Some(true);
        let mut switching = session("switching", HarnessId::Codex);
        switching.pending_switch = Some(
            serde_json::from_value(
                json!({ "from": "claude", "fromModel": "sonnet", "fromSettings": {} }),
            )
            .unwrap(),
        );
        let mut preparing = session("preparing", HarnessId::Claude);
        preparing.blocks.push(
            serde_json::from_value::<Block>(json!({
                "id": "handoff",
                "role": "handoff",
                "text": "",
                "handoff": { "from": "claude", "to": "codex", "status": "preparing", "pending": false },
            }))
            .unwrap(),
        );
        let mut saved = ready.clone();
        saved.title = "Saved title".into();
        let history: Vec<SessionSummary> = [&closed, &saved, &busy, &switching, &preparing]
            .iter()
            .map(|session| summary_from_session(session, None))
            .collect();
        let choices = ci_repair_sessions(&history, &[ready, busy, switching, preparing]);
        assert_eq!(ids(&choices), ["closed", "ready"]);
        assert_eq!(choices[1].title, "Current title");
    }
}
