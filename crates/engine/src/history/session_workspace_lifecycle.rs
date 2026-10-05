//! Port of src/features/sessions/model/sessionWorkspaceLifecycle.ts: which
//! tabs close, shrink, or get a replacement chat when a session leaves the
//! workspace. Session removal is its only caller, so it lives here.

use monocode_core::Session;
use monocode_layout::layout::{
    EditorPane, WorkspaceTab, close_leaf, first_leaf_id, is_session_changes_tab, leaf, leaf_ids,
    remove_pane,
};
use monocode_layout::workspace_tab_groups::{
    WorkspaceTabClosePlan, WorkspaceTabCloseScope, plan_workspace_tab_close,
};

/// `SessionWorkspaceRemoval`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionWorkspaceRemoval {
    pub tabs: Vec<WorkspaceTab>,
    pub sessions: Vec<Session>,
    pub active_tab_id: String,
    pub closed_tabs: Vec<WorkspaceTab>,
}

/// The input to `removeSessionFromWorkspace`.
pub struct RemoveSessionFromWorkspace<'a> {
    pub tabs: &'a [WorkspaceTab],
    pub sessions: &'a [Session],
    pub session_id: &'a str,
    pub active_tab_id: &'a str,
    pub scope: WorkspaceTabCloseScope,
    /// A new blank chat for a tab that must stay open, seeded from the
    /// removed session when it is open.
    pub create_replacement: &'a mut dyn FnMut(Option<&Session>) -> Session,
    /// Preserve file panes changed while an asynchronous close was pending.
    /// `None` closes every tab it may.
    pub can_close_tab: Option<&'a dyn Fn(&WorkspaceTab) -> bool>,
}

/// `removeSessionFromWorkspace`.
pub fn remove_session_from_workspace(
    input: RemoveSessionFromWorkspace<'_>,
) -> SessionWorkspaceRemoval {
    let RemoveSessionFromWorkspace {
        tabs,
        sessions,
        session_id,
        active_tab_id,
        scope,
        create_replacement,
        can_close_tab,
    } = input;
    let can_close = |tab: &WorkspaceTab| can_close_tab.is_none_or(|check| check(tab));
    let seed = sessions.iter().find(|session| session.id == session_id);
    let mut next_tabs: Vec<WorkspaceTab> = tabs.to_vec();
    let mut next_sessions: Vec<Session> = sessions
        .iter()
        .filter(|session| session.id != session_id)
        .cloned()
        .collect();
    let mut remaining_session_ids: Vec<String> =
        next_sessions.iter().map(|s| s.id.clone()).collect();
    let mut next_active_tab_id = active_tab_id.to_string();
    let mut closed_tabs = Vec::new();

    for original in tabs {
        let had_session = leaf_ids(&original.layout).iter().any(|id| id == session_id);
        let Some(index) = next_tabs.iter().position(|tab| tab.id == original.id) else {
            continue;
        };

        let cleaned = remove_session_documents(&next_tabs[index], session_id);
        if !had_session && let Some(cleaned) = cleaned.as_ref() {
            next_tabs[index] = cleaned.clone();
            continue;
        }
        let tab = cleaned.clone().unwrap_or_else(|| original.clone());
        let remaining_conversations = leaf_ids(&tab.layout)
            .iter()
            .any(|id| remaining_session_ids.contains(id));

        if (remaining_conversations || (cleaned.is_some() && !can_close(original)))
            && let Some(next) = close_leaf(&tab, session_id)
        {
            next_tabs[index] = next;
            continue;
        }

        closed_tabs.push(original.clone());
        let close_plan = plan_workspace_tab_close(&next_tabs, sessions, &tab.id, scope);
        if let WorkspaceTabClosePlan::Close {
            next_active_tab_id: next,
        } = close_plan
        {
            next_tabs.retain(|entry| entry.id != tab.id);
            if tab.id == next_active_tab_id
                && let Some(next) = next.filter(|next| !next.is_empty())
            {
                next_active_tab_id = next;
            }
            continue;
        }

        let replacement = create_replacement(seed);
        remaining_session_ids.push(replacement.id.clone());
        let replacement_id = replacement.id.clone();
        next_sessions.push(replacement);
        next_tabs[index] = WorkspaceTab {
            layout: leaf(replacement_id.clone()),
            focused_id: replacement_id,
            editor_panes: Vec::new(),
            terminal_panes: Vec::new(),
            diff_open: Some(false),
            diff_focused: Some(false),
            ..tab
        };
    }

    SessionWorkspaceRemoval {
        tabs: next_tabs,
        sessions: next_sessions,
        active_tab_id: next_active_tab_id,
        closed_tabs,
    }
}

/// `removeSessionDocuments`: close this session's plan and change tabs.
/// `None` when that left the tab with no pane.
fn remove_session_documents(tab: &WorkspaceTab, session_id: &str) -> Option<WorkspaceTab> {
    let belongs = |file: &monocode_layout::layout::FilePaneTab| {
        file.plan
            .as_ref()
            .is_some_and(|plan| plan.session_id == session_id)
            || (is_session_changes_tab(file)
                && file
                    .session_changes
                    .as_ref()
                    .is_some_and(|changes| changes.session_id == session_id))
    };
    if !tab
        .editor_panes
        .iter()
        .any(|pane| pane.files.iter().any(&belongs))
    {
        return Some(tab.clone());
    }
    let mut layout = tab.layout.clone();
    let mut focused_id = tab.focused_id.clone();
    let mut editor_panes: Vec<EditorPane> = Vec::new();

    for pane in &tab.editor_panes {
        let files: Vec<_> = pane
            .files
            .iter()
            .filter(|file| !belongs(file))
            .cloned()
            .collect();
        if !files.is_empty() {
            let active_file_id = if files.iter().any(|file| file.id == pane.active_file_id) {
                pane.active_file_id.clone()
            } else {
                files[0].id.clone()
            };
            editor_panes.push(EditorPane {
                files,
                active_file_id,
                ..pane.clone()
            });
            continue;
        }
        let next_layout = remove_pane(&layout, &pane.id)?;
        layout = next_layout;
        if focused_id == pane.id {
            focused_id = first_leaf_id(&layout).to_string();
        }
    }

    Some(WorkspaceTab {
        layout,
        focused_id,
        editor_panes,
        ..tab.clone()
    })
}
