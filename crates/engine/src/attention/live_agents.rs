//! Port of src/features/sessions/model/liveAgents.ts: the agents panel rows,
//! working sessions plus finished ones the user has not looked at yet.

use std::collections::HashSet;

use monocode_core::HarnessId;
use monocode_core::block::{Block, BlockRole, HandoffStatus};
use monocode_core::session::{Session, session_display_title};

use super::approval_toast::tool_call_label;
use crate::runtime::in_flight::is_in_flight_session;

/// `LiveAgent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAgent {
    pub id: String,
    pub cwd: String,
    pub title: String,
    pub harness: HarnessId,
    pub activity: String,
    pub started_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub needs_approval: bool,
    pub done: bool,
}

/// `liveAgentsFromSessions`. Internal workers stay in their lead's panel.
pub fn live_agents_from_sessions(
    sessions: &[Session],
    unseen_finished_ids: &HashSet<String>,
) -> Vec<LiveAgent> {
    let mut agents: Vec<LiveAgent> = sessions
        .iter()
        .filter(|session| {
            session.inbox_ask.is_none()
                && session.orchestration_lead_id.is_none()
                && (is_in_flight_session(session) || unseen_finished_ids.contains(&session.id))
        })
        .map(|session| to_live_agent(session, unseen_finished_ids.contains(&session.id)))
        .collect();
    agents.sort_by(compare_live_agents);
    agents
}

/// `formatLiveElapsed`.
pub fn format_live_elapsed(started_at: i64, now: i64) -> String {
    let seconds = (monocode_core::js::round((now - started_at) as f64 / 1000.0) as i64).max(1);
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    let rest = seconds % 60;
    if minutes < 60 {
        return if rest > 0 {
            format!("{minutes}m {rest}s")
        } else {
            format!("{minutes}m")
        };
    }
    let hours = minutes / 60;
    let min_rest = minutes % 60;
    if min_rest > 0 {
        format!("{hours}h {min_rest}m")
    } else {
        format!("{hours}h")
    }
}

fn to_live_agent(session: &Session, unseen_finished: bool) -> LiveAgent {
    let pending = session.blocks.iter().find(|block| {
        block
            .approval
            .as_ref()
            .is_some_and(|approval| approval.decided.is_none())
    });
    let pending_question = session.pending_question.as_ref();
    let done = unseen_finished && !is_in_flight_session(session);
    let activity_block = pending.or_else(|| last_activity_block(&session.blocks));
    let activity = if done {
        "Done".to_string()
    } else if let Some(question) = pending_question {
        question
            .title
            .clone()
            .filter(|title| !title.is_empty())
            .or_else(|| {
                question
                    .questions
                    .first()
                    .map(|first| first.prompt.clone())
                    .filter(|prompt| !prompt.is_empty())
            })
            .unwrap_or_else(|| "Question".into())
    } else {
        activity_label(activity_block, &session.cwd)
    };
    LiveAgent {
        id: session.id.clone(),
        cwd: session.cwd.clone(),
        title: session_display_title(&session.title, session.harness),
        harness: session.harness,
        activity,
        started_at: last_user(&session.blocks).and_then(|block| block.started_at),
        duration_ms: if done {
            last_user(&session.blocks).and_then(|block| block.duration_ms)
        } else {
            None
        },
        needs_approval: pending.is_some() || pending_question.is_some(),
        done,
    }
}

fn compare_live_agents(a: &LiveAgent, b: &LiveAgent) -> std::cmp::Ordering {
    b.needs_approval
        .cmp(&a.needs_approval)
        .then(a.done.cmp(&b.done))
        .then(
            a.started_at
                .unwrap_or(i64::MAX)
                .cmp(&b.started_at.unwrap_or(i64::MAX)),
        )
}

fn is_preparing_handoff_block(block: &Block) -> bool {
    block.role == BlockRole::Handoff
        && block
            .handoff
            .as_ref()
            .is_some_and(|handoff| handoff.status == HandoffStatus::Preparing)
}

fn last_activity_block(blocks: &[Block]) -> Option<&Block> {
    blocks.iter().rev().find(|block| {
        block.role == BlockRole::Tool
            || block.role == BlockRole::Approval
            || is_preparing_handoff_block(block)
    })
}

/// The last user block (`turnStartedAt` and `turnDurationMs`).
fn last_user(blocks: &[Block]) -> Option<&Block> {
    blocks
        .iter()
        .rev()
        .find(|block| block.role == BlockRole::User)
}

fn activity_label(block: Option<&Block>, cwd: &str) -> String {
    let Some(block) = block else {
        return "Working".into();
    };
    if is_preparing_handoff_block(block) {
        return "Preparing a handoff".into();
    }
    tool_call_label(block, Some(cwd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::notifications::tests::{approval_block, question};
    use monocode_core::block::{BlockTool, ToolPreview, ToolPreviewKind};

    fn user(started_at: i64, duration_ms: Option<i64>) -> Block {
        let mut block = Block::new("u1", BlockRole::User, "go");
        block.started_at = Some(started_at);
        block.duration_ms = duration_ms;
        block
    }

    fn chat(id: &str, cwd: &str, busy: bool, blocks: Vec<Block>) -> Session {
        let mut session = Session::blank(id, HarnessId::Claude, "sonnet", cwd);
        session.title = "claude · Fix the sidebar".into();
        session.busy = busy.then_some(true);
        session.blocks = if blocks.is_empty() {
            vec![user(1_000, None)]
        } else {
            blocks
        };
        session
    }

    fn edit(id: &str, status: &str) -> Block {
        let mut block = Block::new(id, BlockRole::Tool, "Edited src/App.tsx");
        block.tool = Some(BlockTool {
            kind: Some("edit".into()),
            title: Some("Edited src/App.tsx".into()),
            status: Some(status.into()),
            preview: Some(
                serde_json::from_value::<ToolPreview>(serde_json::json!({
                    "kind": "write",
                    "path": "src/App.tsx",
                    "fileName": "App.tsx",
                }))
                .unwrap(),
            ),
            ..Default::default()
        });
        assert_eq!(
            block.tool.as_ref().unwrap().preview.as_ref().unwrap().kind,
            ToolPreviewKind::Write
        );
        block
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn keeps_internal_workers_in_their_leads_agent_panel() {
        let lead = chat("lead", "/repo", true, vec![]);
        let mut worker = chat("worker", "/repo", true, vec![]);
        worker.orchestration_lead_id = Some("lead".into());
        let ids: Vec<String> = live_agents_from_sessions(&[lead, worker], &none())
            .into_iter()
            .map(|agent| agent.id)
            .collect();
        assert_eq!(ids, ["lead"]);
    }

    #[test]
    fn skips_idle_sessions() {
        assert!(
            live_agents_from_sessions(&[chat("a", "/tmp/a", false, vec![])], &none()).is_empty()
        );
    }

    #[test]
    fn maps_a_busy_turn_into_a_live_agent() {
        let session = chat(
            "s",
            "/tmp/agent-terminal",
            true,
            vec![user(1_000, None), edit("t1", "in_progress")],
        );
        assert_eq!(
            live_agents_from_sessions(&[session], &none()),
            vec![LiveAgent {
                id: "s".into(),
                cwd: "/tmp/agent-terminal".into(),
                title: "Fix the sidebar".into(),
                harness: HarnessId::Claude,
                activity: "Edited src/App.tsx".into(),
                started_at: Some(1_000),
                duration_ms: None,
                needs_approval: false,
                done: false,
            }]
        );
    }

    #[test]
    fn puts_sessions_waiting_on_approval_first() {
        let working = chat(
            "working",
            "/tmp/a",
            true,
            vec![user(1_000, None), edit("t1", "in_progress")],
        );
        let waiting = chat(
            "waiting",
            "/tmp/b",
            true,
            vec![
                user(2_000, None),
                approval_block("a1", BlockRole::Approval, "run rm", 1),
            ],
        );
        let agents = live_agents_from_sessions(&[working, waiting], &none());
        let ids: Vec<&str> = agents.iter().map(|agent| agent.id.as_str()).collect();
        assert_eq!(ids, ["waiting", "working"]);
        assert!(agents[0].needs_approval);
    }

    #[test]
    fn treats_a_parked_clarifying_question_as_needing_approval() {
        let mut waiting = chat("ask", "/tmp/ask", true, vec![]);
        waiting.pending_question = Some(question(4, Some("Which file?"), &["Which file?"]));
        let agent = &live_agents_from_sessions(&[waiting], &none())[0];
        assert_eq!(agent.activity, "Which file?");
        assert!(agent.needs_approval);
        assert!(!agent.done);
    }

    #[test]
    fn sorts_working_agents_by_longest_running_turn_first() {
        let newer = chat("new", "/tmp/new", true, vec![user(5_000, None)]);
        let older = chat("old", "/tmp/old", true, vec![user(1_000, None)]);
        let cwds: Vec<String> = live_agents_from_sessions(&[newer, older], &none())
            .into_iter()
            .map(|agent| agent.cwd)
            .collect();
        assert_eq!(cwds, ["/tmp/old", "/tmp/new"]);
    }

    #[test]
    fn keeps_an_unfocused_finished_session_until_it_is_seen() {
        let finished = chat(
            "done",
            "/tmp/done",
            false,
            vec![user(1_000, Some(12_000)), edit("t1", "completed")],
        );
        assert!(live_agents_from_sessions(std::slice::from_ref(&finished), &none()).is_empty());
        let unseen: HashSet<String> = ["done".to_string()].into_iter().collect();
        assert_eq!(
            live_agents_from_sessions(&[finished], &unseen),
            vec![LiveAgent {
                id: "done".into(),
                cwd: "/tmp/done".into(),
                title: "Fix the sidebar".into(),
                harness: HarnessId::Claude,
                activity: "Done".into(),
                started_at: Some(1_000),
                duration_ms: Some(12_000),
                needs_approval: false,
                done: true,
            }]
        );
    }

    #[test]
    fn keeps_a_working_session_above_a_finished_one() {
        let working = chat("working", "/tmp/a", true, vec![user(5_000, None)]);
        let finished = chat("finished", "/tmp/b", false, vec![user(1_000, Some(8_000))]);
        let unseen: HashSet<String> = ["finished".to_string()].into_iter().collect();
        let cwds: Vec<String> = live_agents_from_sessions(&[finished, working], &unseen)
            .into_iter()
            .map(|agent| agent.cwd)
            .collect();
        assert_eq!(cwds, ["/tmp/a", "/tmp/b"]);
    }

    #[test]
    fn formats_seconds_minutes_and_hours() {
        assert_eq!(format_live_elapsed(0, 1_000), "1s");
        assert_eq!(format_live_elapsed(0, 38_000), "38s");
        assert_eq!(format_live_elapsed(0, 72_000), "1m 12s");
        assert_eq!(format_live_elapsed(0, 120_000), "2m");
        assert_eq!(format_live_elapsed(0, 3_600_000), "1h");
        assert_eq!(format_live_elapsed(0, 3_720_000), "1h 2m");
    }
}
