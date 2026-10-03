//! Port of src/features/notifications/model/approvalToast.ts: the pending
//! approval or question a toast shows for a session the user is not looking
//! at.

use monocode_core::block::Block;
use monocode_core::paths::display_path;
use monocode_core::reducer::{ToolTitleInput, compose_tool_title};
use monocode_core::session::Session;
use monocode_layout::{WorkspaceTab, leaf_ids};

use super::notifications::InputKind;

/// `PendingApprovalNotice`.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingApprovalNotice {
    pub session_id: String,
    pub request_id: i64,
    pub label: String,
    pub kind: InputKind,
    pub block: Option<Block>,
}

/// `toolCallLabel` from transcriptActivity.ts: the row's one-line label,
/// such as `Read src/app.ts`.
pub fn tool_call_label(block: &Block, cwd: Option<&str>) -> String {
    let tool = block.tool.as_ref();
    let preview = tool.and_then(|tool| tool.preview.as_ref());
    let path =
        match preview.and_then(|preview| preview.path.as_deref().filter(|path| !path.is_empty())) {
            Some(path) => Some(display_path(path, cwd)),
            None => preview.and_then(|preview| preview.file_name.clone()),
        };
    let title = if block.text.is_empty() {
        tool.and_then(|tool| tool.title.as_deref())
    } else {
        Some(block.text.as_str())
    };
    let label = compose_tool_title(&ToolTitleInput {
        kind: tool.and_then(|tool| tool.kind.as_deref()),
        title,
        path: path.as_deref(),
        query: preview.and_then(|preview| preview.query.as_deref()),
        preview_kind: preview.map(|preview| preview.kind),
        cwd,
        ..Default::default()
    });
    if label.is_empty() {
        "Working".into()
    } else {
        label
    }
}

/// `pendingApprovalForSession`: the latest undecided approval or clarifying
/// question in a session. A parked question wins over a tool approval.
pub fn pending_approval_for_session(session: &Session) -> Option<PendingApprovalNotice> {
    if let Some(question) = &session.pending_question {
        let label = question
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
            .unwrap_or_else(|| "Question".into());
        return Some(PendingApprovalNotice {
            session_id: session.id.clone(),
            request_id: question.request_id,
            label,
            kind: InputKind::Question,
            block: None,
        });
    }
    let block = session.blocks.iter().rev().find(|block| {
        block
            .approval
            .as_ref()
            .is_some_and(|approval| approval.decided.is_none())
    })?;
    Some(PendingApprovalNotice {
        session_id: session.id.clone(),
        request_id: block.approval.as_ref()?.request_id,
        label: tool_call_label(block, Some(&session.cwd)),
        kind: InputKind::Approval,
        block: Some(block.clone()),
    })
}

/// `isSessionConversationFocused`: the session's pane is focused on the
/// active tab and the composer has focus.
pub fn is_session_conversation_focused(
    session_id: &str,
    active_tab_id: &str,
    tabs: &[WorkspaceTab],
    composer_focused: bool,
) -> bool {
    let Some(tab) = tabs.iter().find(|tab| tab.id == active_tab_id) else {
        return false;
    };
    if !leaf_ids(&tab.layout).iter().any(|id| id == session_id) {
        return false;
    }
    if tab.focused_id != session_id {
        return false;
    }
    composer_focused
}

/// `hiddenApprovalNotices`: pending requests outside the focused
/// conversation. Inbox Asks and orchestrated workers never toast; a worker
/// answers to its lead.
pub fn hidden_approval_notices(
    sessions: &[Session],
    active_tab_id: &str,
    tabs: &[WorkspaceTab],
    composer_focused: bool,
) -> Vec<PendingApprovalNotice> {
    sessions
        .iter()
        .filter(|session| session.inbox_ask.is_none() && session.orchestration_lead_id.is_none())
        .filter_map(|session| {
            let pending = pending_approval_for_session(session)?;
            (!is_session_conversation_focused(&session.id, active_tab_id, tabs, composer_focused))
                .then_some(pending)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::notifications::tests::question;
    use monocode_core::block::{ApprovalDecided, BlockApproval, BlockRole, BlockTool};
    use monocode_core::{Extra, HarnessId};
    use monocode_layout::{SplitDir, new_tab, split_pane};

    fn block(role: BlockRole, request_id: i64, decided: Option<ApprovalDecided>) -> Block {
        let text = if role == BlockRole::Tool {
            "Run command"
        } else {
            "Approve?"
        };
        let mut block = Block::new(uuid::Uuid::new_v4().to_string(), role, text);
        if role == BlockRole::Tool {
            block.tool = Some(BlockTool {
                title: Some("Run command".into()),
                kind: Some("execute".into()),
                ..Default::default()
            });
        }
        block.approval = Some(BlockApproval {
            request_id,
            decided,
            extra: Extra::new(),
        });
        block
    }

    fn session(id: &str) -> Session {
        Session::blank(id, HarnessId::Claude, "sonnet", "~")
    }

    #[test]
    fn never_toasts_a_worker_approval_which_its_lead_answers_instead() {
        let mut worker = session("worker");
        worker.orchestration_lead_id = Some("lead".into());
        worker.blocks = vec![block(BlockRole::Tool, 42, None)];
        let lead_tab = new_tab("lead");
        let other_tab = new_tab("unrelated");
        let tabs = vec![lead_tab.clone(), other_tab.clone()];
        for active in [&lead_tab, &other_tab] {
            assert!(
                hidden_approval_notices(std::slice::from_ref(&worker), &active.id, &tabs, true)
                    .is_empty()
            );
        }
        // The same approval on an ordinary session still reaches the user.
        let mut solo = worker.clone();
        solo.orchestration_lead_id = None;
        assert_eq!(
            hidden_approval_notices(&[solo], &other_tab.id, &tabs, true)[0].session_id,
            "worker"
        );
    }

    #[test]
    fn returns_the_latest_undecided_approval() {
        let mut session = session("s");
        session.blocks = vec![
            block(BlockRole::Tool, 1, Some(ApprovalDecided::Allow)),
            block(BlockRole::Tool, 2, None),
        ];
        let pending = pending_approval_for_session(&session).unwrap();
        assert_eq!(pending.request_id, 2);
        assert_eq!(pending.label, "Shell");
        assert_eq!(pending.kind, InputKind::Approval);
    }

    #[test]
    fn prefers_a_parked_clarifying_question_over_a_tool_approval() {
        let mut session = session("s");
        session.blocks = vec![block(BlockRole::Tool, 2, None)];
        session.pending_question = Some(question(9, Some("Which file?"), &["Which file?"]));
        let pending = pending_approval_for_session(&session).unwrap();
        assert_eq!(pending.request_id, 9);
        assert_eq!(pending.kind, InputKind::Question);
        assert_eq!(pending.label, "Which file?");
    }

    #[test]
    fn is_focused_only_when_the_session_pane_is_focused_on_the_active_tab() {
        let session = session("s");
        let tab = new_tab(&session.id);
        let tabs = vec![tab.clone()];
        assert!(is_session_conversation_focused(
            &session.id,
            &tab.id,
            &tabs,
            true
        ));
        assert!(!is_session_conversation_focused(
            &session.id,
            &tab.id,
            &tabs,
            false
        ));
        assert!(!is_session_conversation_focused(
            "other", &tab.id, &tabs, true
        ));
    }

    #[test]
    fn omits_approvals_that_are_already_in_the_focused_conversation() {
        let mut visible = session("visible");
        visible.blocks = vec![block(BlockRole::Tool, 1, None)];
        let mut hidden = session("hidden");
        hidden.blocks = vec![block(BlockRole::Tool, 2, None)];
        let visible_tab = new_tab(&visible.id);
        let hidden_tab = new_tab(&hidden.id);
        let ids: Vec<String> = hidden_approval_notices(
            &[visible, hidden.clone()],
            &visible_tab.id,
            &[visible_tab.clone(), hidden_tab],
            true,
        )
        .into_iter()
        .map(|notice| notice.session_id)
        .collect();
        assert_eq!(ids, [hidden.id]);
    }

    #[test]
    fn shows_approvals_in_another_pane_on_the_same_tab() {
        let mut left = session("left");
        left.blocks = vec![block(BlockRole::Tool, 1, None)];
        let right = session("right");
        let mut tab = new_tab(&left.id);
        tab.layout = split_pane(&tab.layout, &left.id, SplitDir::Right, &right.id);
        tab.focused_id = right.id.clone();
        let ids: Vec<String> =
            hidden_approval_notices(&[left.clone(), right], &tab.id, &[tab.clone()], true)
                .into_iter()
                .map(|notice| notice.session_id)
                .collect();
        assert_eq!(ids, [left.id]);
    }
}
