//! Port of src/features/sessions/model/inFlight.ts: turns a quit would cut
//! short, the quit snapshot, and the interrupt note.

use std::collections::{HashMap, HashSet};

use monocode_core::Session;
use monocode_core::block::{Block, BlockNotice, BlockRole};
use monocode_core::session::session_needs_input;
use serde_json::Value;

use super::reducer::{now_ms, stop_streaming};
use super::session_store::InFlightRef;

/// `INTERRUPT_MESSAGE`.
pub const INTERRUPT_MESSAGE: &str = "Turn interrupted when MonoCode quit.";

/// `CONTINUE_PROMPT`.
pub const CONTINUE_PROMPT: &str = "Continue from where you left off.";

/// `ResumedWorkspace`: the workspace a boot restores.
///
/// The TypeScript type also held the tabs, the active tab, the project
/// terminals, the project return memory, and the dock side. Those are the
/// workspace package's types, so `layout` carries them in the workspace
/// snapshot's JSON shape and `WorkspaceHooks` reads and writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumedWorkspace {
    pub sessions: Vec<Session>,
    pub project_cwd: String,
    pub layout: Value,
}

/// `isInFlightSession`: a turn or approval that would be lost if the app
/// died now.
pub fn is_in_flight_session(session: &Session) -> bool {
    session.worktree_removed != Some(true) && (session.is_busy() || session_needs_input(session))
}

/// `hasInFlightSessions`.
pub fn has_in_flight_sessions(sessions: &[Session]) -> bool {
    sessions.iter().any(is_in_flight_session)
}

/// `inFlightRefs`: busy chats in tab order, then parked ones still running
/// after their tab closed. Only persistable sessions can come back after a
/// real quit. `tab_session_ids` is every tab's panes in order.
pub fn in_flight_refs(sessions: &[Session], tab_session_ids: &[String]) -> Vec<InFlightRef> {
    let by_id: HashMap<&str, &Session> = sessions
        .iter()
        .map(|session| (session.id.as_str(), session))
        .collect();
    let mut seen = HashSet::new();
    let mut refs = Vec::new();
    let mut push = |session: Option<&Session>| {
        let Some(session) = session else {
            return;
        };
        if session.inbox_ask.is_some() || seen.contains(&session.id) {
            return;
        }
        if !is_in_flight_session(session) || !can_resume_after_quit(session) {
            return;
        }
        seen.insert(session.id.clone());
        refs.push(InFlightRef {
            session_id: session.id.clone(),
            cwd: session.cwd.clone(),
        });
    };
    for id in tab_session_ids {
        push(by_id.get(id.as_str()).copied());
    }
    for session in sessions {
        push(Some(session));
    }
    refs
}

/// `quitWhileBusyMessage`.
pub fn quit_while_busy_message(count: usize) -> String {
    if count == 1 {
        return "1 chat is still running. Quit anyway? It will resume when you reopen MonoCode."
            .into();
    }
    format!(
        "{count} chats are still running. Quit anyway? They will resume when you reopen MonoCode."
    )
}

/// `markTurnInterrupted`: seal streams and tools and record that a real
/// quit cut the turn short. Idempotent for the current turn only, so a later
/// turn after Continue still gets a fresh note.
pub fn mark_turn_interrupted(session: &Session) -> Session {
    let mut sealed = seal_open_work(&stop_streaming(session, now_ms()));
    sealed.busy = Some(false);
    if last_block_is_interrupt(&sealed) {
        return sealed;
    }
    sealed.blocks.push(Block {
        notice: Some(BlockNotice::Interrupt),
        ..Block::new(
            uuid::Uuid::new_v4().to_string(),
            BlockRole::System,
            INTERRUPT_MESSAGE,
        )
    });
    sealed
}

/// `workspaceFromResumed`: one tab per restored chat, built by
/// `layout_for_sessions` (the workspace package's `newTab`).
pub fn workspace_from_resumed(
    sessions: Vec<Session>,
    layout_for_sessions: impl FnOnce(&[String]) -> Value,
) -> Option<ResumedWorkspace> {
    let sessions: Vec<Session> = sessions
        .into_iter()
        .filter(|session| session.inbox_ask.is_none())
        .collect();
    let first = sessions.first()?;
    let project_cwd = first.cwd.clone();
    let ids: Vec<String> = sessions.iter().map(|session| session.id.clone()).collect();
    Some(ResumedWorkspace {
        layout: layout_for_sessions(&ids),
        sessions,
        project_cwd,
    })
}

/// `wasTurnInterrupted`.
pub fn was_turn_interrupted(session: &Session) -> bool {
    last_block_is_interrupt(session)
}

/// `canAutoContinue`: the provider thread exists and the quit note is still
/// the last block. A Continue appends after it, so this stays one-shot.
pub fn can_auto_continue(session: &Session) -> bool {
    session.worktree_removed != Some(true)
        && session
            .provider_session_id
            .as_ref()
            .is_some_and(|id| !id.is_empty())
        && !session.is_busy()
        && last_block_is_interrupt(session)
}

fn last_block_is_interrupt(session: &Session) -> bool {
    session
        .blocks
        .last()
        .is_some_and(|last| last.role == BlockRole::System && last.text == INTERRUPT_MESSAGE)
}

/// `inFlightSnapshotKey`.
pub fn in_flight_snapshot_key(refs: &[InFlightRef]) -> String {
    refs.iter()
        .map(|entry| entry.session_id.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `shouldWriteInFlightSnapshot`: skip the first idle paint after boot so a
/// restored snapshot is not wiped before the turn is marked busy again.
/// Once this process has seen a live in-flight chat, an empty list is a real
/// "turn finished" and should clear.
pub fn should_write_in_flight_snapshot(
    key: &str,
    refs: &[InFlightRef],
    previous_key: Option<&str>,
    saw_in_flight: bool,
) -> bool {
    if previous_key == Some(key) {
        return false;
    }
    !(refs.is_empty() && !saw_in_flight)
}

fn can_resume_after_quit(session: &Session) -> bool {
    session.worktree_removed != Some(true)
        && session.cwd != "~"
        && session
            .blocks
            .iter()
            .any(|block| block.role == BlockRole::User)
}

fn seal_open_work(session: &Session) -> Session {
    let mut next = session.clone();
    next.busy = Some(false);
    next.blocks = session
        .blocks
        .iter()
        .filter_map(|block| {
            let undecided = block
                .approval
                .as_ref()
                .is_some_and(|approval| approval.decided.is_none());
            if block.role == BlockRole::Approval && undecided {
                return None;
            }
            if !should_cancel_tool(block) {
                return Some(block.clone());
            }
            let mut sealed = block.clone();
            sealed.streaming = Some(false);
            if let Some(tool) = sealed.tool.as_mut() {
                tool.status = Some("cancelled".into());
            }
            if undecided {
                sealed.approval = None;
            }
            Some(sealed)
        })
        .collect();
    next
}

fn should_cancel_tool(block: &Block) -> bool {
    if block
        .approval
        .as_ref()
        .is_some_and(|approval| approval.decided.is_none())
    {
        return true;
    }
    let Some(tool) = block.tool.as_ref() else {
        return false;
    };
    if block.is_streaming() {
        return true;
    }
    let status = tool.status.as_deref().unwrap_or("").to_lowercase();
    matches!(status.as_str(), "in_progress" | "pending" | "running")
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;
    use monocode_core::block::{BlockApproval, BlockTool, Extra};
    use monocode_core::user_question::{UserQuestion, UserQuestionPrompt};

    fn chat(id: &str, cwd: &str) -> Session {
        let mut session = Session::blank(id, HarnessId::Cursor, "cursor:auto", cwd);
        session.blocks = vec![Block::new("u1", BlockRole::User, "hello")];
        session
    }

    fn busy(id: &str, cwd: &str) -> Session {
        Session {
            busy: Some(true),
            ..chat(id, cwd)
        }
    }

    fn tool(id: &str, status: &str, streaming: bool) -> Block {
        Block {
            streaming: streaming.then_some(true),
            tool: Some(BlockTool {
                status: Some(status.into()),
                title: Some(id.into()),
                ..BlockTool::default()
            }),
            ..Block::new(id, BlockRole::Tool, "edit")
        }
    }

    fn approval(id: &str, request_id: i64) -> Block {
        Block {
            approval: Some(BlockApproval {
                request_id,
                decided: None,
                extra: Extra::new(),
            }),
            ..Block::new(id, BlockRole::Approval, "allow?")
        }
    }

    fn refs(entries: &[(&str, &str)]) -> Vec<InFlightRef> {
        entries
            .iter()
            .map(|(id, cwd)| InFlightRef {
                session_id: id.to_string(),
                cwd: cwd.to_string(),
            })
            .collect()
    }

    #[test]
    fn is_true_for_a_busy_turn_a_live_approval_or_a_parked_question() {
        assert!(!is_in_flight_session(&chat("a", "/tmp/a")));
        assert!(is_in_flight_session(&busy("a", "/tmp/a")));
        let mut with_approval = chat("a", "/tmp/a");
        with_approval.blocks.push(approval("a1", 1));
        assert!(is_in_flight_session(&with_approval));
        let question: UserQuestion = serde_json::from_value(serde_json::json!({
            "id": "q1", "prompt": "Which file?", "multiSelect": false, "allowCustom": true,
            "options": [{ "id": "a.ts", "label": "a.ts" }]
        }))
        .unwrap();
        let mut asking = chat("a", "/tmp/a");
        asking.pending_question = Some(UserQuestionPrompt {
            request_id: 4,
            title: None,
            questions: vec![question],
            auto_resolve_at: None,
        });
        assert!(is_in_flight_session(&asking));
    }

    #[test]
    fn walks_open_tabs_first_then_parked_busy_sessions() {
        let parked = busy("parked", "/tmp/parked");
        let open_busy = busy("open", "/tmp/open");
        let idle = chat("idle", "/tmp/idle");
        let mut blank = Session::blank("blank", HarnessId::Cursor, "m", "/tmp/blank");
        blank.busy = Some(true);
        let tabs = vec![idle.id.clone(), open_busy.id.clone()];
        assert_eq!(
            in_flight_refs(&[parked, open_busy, idle, blank], &tabs),
            refs(&[("open", "/tmp/open"), ("parked", "/tmp/parked")])
        );
    }

    #[test]
    fn skips_chats_that_were_never_persisted() {
        let mut blank = Session::blank("blank", HarnessId::Cursor, "m", "/tmp/a");
        blank.busy = Some(true);
        assert!(in_flight_refs(std::slice::from_ref(&blank), &[blank.id.clone()]).is_empty());
        assert!(has_in_flight_sessions(&[blank]));
    }

    #[test]
    fn does_not_resume_a_session_whose_worktree_was_removed() {
        let mut removed = busy("r", "/tmp/a");
        removed.worktree_cwd = Some("/tmp/a-worktrees/feature".into());
        removed.worktree_removed = Some(true);
        assert!(!is_in_flight_session(&removed));
        assert!(in_flight_refs(std::slice::from_ref(&removed), &[removed.id.clone()]).is_empty());
    }

    #[test]
    fn seals_the_stream_cancels_open_tools_and_appends_a_system_note() {
        let mut session = busy("a", "/tmp/a");
        session.blocks = vec![
            Block {
                started_at: Some(1_000),
                ..Block::new("u1", BlockRole::User, "hello")
            },
            Block {
                streaming: Some(true),
                ..Block::new("a1", BlockRole::Assistant, "Working")
            },
            tool("t1", "running", true),
            approval("p1", 7),
        ];
        let interrupted = mark_turn_interrupted(&session);
        assert_eq!(interrupted.busy, Some(false));
        let blocks = &interrupted.blocks;
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0].id, "u1");
        assert!(blocks[0].duration_ms.is_some());
        assert_eq!(
            (blocks[1].id.as_str(), blocks[1].streaming),
            ("a1", Some(false))
        );
        assert_eq!(blocks[1].text, "Working");
        assert_eq!(blocks[2].streaming, Some(false));
        assert_eq!(
            blocks[2].tool.as_ref().unwrap().status.as_deref(),
            Some("cancelled")
        );
        assert_eq!(blocks[3].role, BlockRole::System);
        assert_eq!(blocks[3].text, INTERRUPT_MESSAGE);
        assert_eq!(blocks[3].notice, Some(BlockNotice::Interrupt));
    }

    #[test]
    fn does_not_append_the_interrupt_note_twice_for_the_same_turn() {
        let once = mark_turn_interrupted(&busy("a", "/tmp/a"));
        let twice = mark_turn_interrupted(&once);
        let notes = twice
            .blocks
            .iter()
            .filter(|b| b.text == INTERRUPT_MESSAGE)
            .count();
        assert_eq!(notes, 1);
    }

    #[test]
    fn appends_a_new_note_when_a_later_turn_is_quit_after_continue() {
        let mut first = busy("a", "/tmp/a");
        first.provider_session_id = Some("p1".into());
        let mut continued = mark_turn_interrupted(&first);
        continued.blocks.extend([
            Block::new("c1", BlockRole::User, CONTINUE_PROMPT),
            Block::new("a2", BlockRole::Assistant, "resumed"),
            Block::new("u2", BlockRole::User, "edit the readme"),
            tool("t2", "completed", false),
        ]);
        continued.busy = Some(true);
        let second = mark_turn_interrupted(&continued);
        let notes = second
            .blocks
            .iter()
            .filter(|b| b.text == INTERRUPT_MESSAGE)
            .count();
        assert_eq!(notes, 2);
        assert_eq!(second.blocks.last().unwrap().text, INTERRUPT_MESSAGE);
        assert!(can_auto_continue(&second));
    }

    #[test]
    fn leaves_finished_tools_alone() {
        let mut session = busy("a", "/tmp/a");
        session.blocks = vec![
            Block::new("u1", BlockRole::User, "hello"),
            tool("t1", "completed", false),
        ];
        let interrupted = mark_turn_interrupted(&session);
        assert_eq!(
            interrupted.blocks[1]
                .tool
                .as_ref()
                .unwrap()
                .status
                .as_deref(),
            Some("completed")
        );
    }

    #[test]
    fn mentions_resume_on_reopen() {
        assert!(quit_while_busy_message(1).contains("1 chat is still running"));
        assert!(quit_while_busy_message(3).contains("3 chats are still running"));
    }

    #[test]
    fn opens_one_tab_per_restored_chat() {
        let first = chat("first", "/tmp/a");
        let second = chat("second", "/tmp/b");
        let workspace = workspace_from_resumed(vec![first, second], |ids| {
            serde_json::json!({ "tabs": ids.iter().map(|id| serde_json::json!({ "focusedId": id })).collect::<Vec<_>>() })
        })
        .unwrap();
        assert_eq!(workspace.project_cwd, "/tmp/a");
        let ids: Vec<_> = workspace.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["first", "second"]);
        assert_eq!(workspace.layout["tabs"][0]["focusedId"], "first");
    }

    #[test]
    fn returns_none_when_there_is_nothing_to_restore() {
        assert!(workspace_from_resumed(Vec::new(), |_| Value::Null).is_none());
    }

    #[test]
    fn needs_a_provider_thread_and_an_interrupt_note_as_the_last_block() {
        let mut with_thread = busy("a", "/tmp/a");
        with_thread.provider_session_id = Some("p1".into());
        let interrupted = mark_turn_interrupted(&with_thread);
        assert!(can_auto_continue(&interrupted));
        let mut idle = chat("a", "/tmp/a");
        idle.provider_session_id = Some("p1".into());
        assert!(!can_auto_continue(&idle));
        assert!(!can_auto_continue(&mark_turn_interrupted(&busy(
            "a", "/tmp/a"
        ))));
        let mut continued = interrupted.clone();
        continued
            .blocks
            .push(Block::new("c1", BlockRole::User, CONTINUE_PROMPT));
        assert!(!can_auto_continue(&continued));
        assert!(!can_auto_continue(&Session {
            busy: Some(true),
            ..interrupted.clone()
        }));
        assert!(!can_auto_continue(&Session {
            worktree_removed: Some(true),
            ..interrupted
        }));
    }

    #[test]
    fn does_not_wipe_a_disk_snapshot_on_the_first_idle_paint() {
        assert!(!should_write_in_flight_snapshot("", &[], None, false));
    }

    #[test]
    fn writes_when_a_chat_becomes_in_flight() {
        assert!(should_write_in_flight_snapshot(
            "a",
            &refs(&[("a", "/tmp")]),
            None,
            false
        ));
    }

    #[test]
    fn clears_after_this_process_has_seen_an_in_flight_chat() {
        assert!(should_write_in_flight_snapshot("", &[], Some("a"), true));
    }

    #[test]
    fn skips_unchanged_keys() {
        assert!(!should_write_in_flight_snapshot(
            "a",
            &refs(&[("a", "/tmp")]),
            Some("a"),
            true
        ));
    }
}
