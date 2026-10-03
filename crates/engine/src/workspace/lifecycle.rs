//! Port of src/features/sessions/model/sessionWorkspaceLifecycle.ts: take a
//! session out of every tab when it is archived or deleted, closing tabs it
//! leaves empty and replacing the last one with a blank chat.

use std::collections::HashSet;

use monocode_core::Session;
use monocode_layout::workspace_tab_groups::{
    WorkspaceTabClosePlan, WorkspaceTabCloseScope, plan_workspace_tab_close,
};
use monocode_layout::{
    EditorPane, WorkspaceTab, close_leaf, first_leaf_id, is_session_changes_tab, leaf, leaf_ids,
    remove_pane,
};

/// `SessionWorkspaceRemoval`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionWorkspaceRemoval {
    pub tabs: Vec<WorkspaceTab>,
    pub sessions: Vec<Session>,
    pub active_tab_id: String,
    pub closed_tabs: Vec<WorkspaceTab>,
}

/// The input of `removeSessionFromWorkspace`.
pub struct RemoveSession<'a> {
    pub tabs: &'a [WorkspaceTab],
    pub sessions: &'a [Session],
    pub session_id: &'a str,
    pub active_tab_id: &'a str,
    pub scope: WorkspaceTabCloseScope,
}

/// `removeSessionFromWorkspace`. `create_replacement` builds the chat that
/// fills a tab the scope keeps open. `can_close_tab` preserves file panes
/// that changed while an asynchronous close was pending; pass `|_| true`
/// for the TypeScript default.
pub fn remove_session_from_workspace(
    input: RemoveSession<'_>,
    mut create_replacement: impl FnMut(Option<&Session>) -> Session,
    can_close_tab: impl Fn(&WorkspaceTab) -> bool,
) -> SessionWorkspaceRemoval {
    let RemoveSession {
        tabs,
        sessions,
        session_id,
        active_tab_id,
        scope,
    } = input;
    let seed = sessions.iter().find(|session| session.id == session_id);
    let mut next_tabs: Vec<WorkspaceTab> = tabs.to_vec();
    let mut next_sessions: Vec<Session> = sessions
        .iter()
        .filter(|session| session.id != session_id)
        .cloned()
        .collect();
    let mut remaining_session_ids: HashSet<String> = next_sessions
        .iter()
        .map(|session| session.id.clone())
        .collect();
    let mut next_active_tab_id = active_tab_id.to_string();
    let mut closed_tabs = Vec::new();

    for original in tabs {
        let had_session = leaf_ids(&original.layout).iter().any(|id| id == session_id);
        let Some(index) = next_tabs.iter().position(|tab| tab.id == original.id) else {
            continue;
        };

        let cleaned = remove_session_documents(&next_tabs[index], session_id);
        if !had_session && let Some(cleaned) = &cleaned {
            next_tabs[index] = cleaned.clone();
            continue;
        }
        let tab = cleaned.clone().unwrap_or_else(|| original.clone());
        let remaining_conversations = leaf_ids(&tab.layout)
            .iter()
            .any(|id| remaining_session_ids.contains(id));

        if (remaining_conversations || (cleaned.is_some() && !can_close_tab(original)))
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
                && let Some(next) = next
            {
                next_active_tab_id = next;
            }
            continue;
        }

        let replacement = create_replacement(seed);
        remaining_session_ids.insert(replacement.id.clone());
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

/// `removeSessionDocuments`: drop the session's plan and session-changes
/// tabs. `None` when that empties the whole layout.
fn remove_session_documents(tab: &WorkspaceTab, session_id: &str) -> Option<WorkspaceTab> {
    let owned = |file: &monocode_layout::FilePaneTab| {
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
        .any(|pane| pane.files.iter().any(owned))
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
            .filter(|file| !owned(file))
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

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::{Extra, HarnessId};
    use monocode_layout::{
        LayoutNode, OpenEditorTabOptions, SessionChangesSource, SplitDir, SplitNode, new_file_tab,
        new_plan_tab, new_tab, open_editor_tab,
    };

    fn session(id: &str, cwd: &str) -> Session {
        Session::blank(id, HarnessId::Cursor, "cursor:auto", cwd)
    }

    fn tab(id: &str, session_id: &str) -> WorkspaceTab {
        WorkspaceTab {
            id: id.into(),
            ..new_tab(session_id)
        }
    }

    fn split(id: &str, dir: SplitDir, children: Vec<LayoutNode>, sizes: Vec<f64>) -> LayoutNode {
        LayoutNode::Split(SplitNode {
            id: id.into(),
            dir,
            children,
            sizes,
            extra: Extra::new(),
        })
    }

    fn remove(
        tabs: &[WorkspaceTab],
        sessions: &[Session],
        session_id: &str,
        active_tab_id: &str,
        scope: WorkspaceTabCloseScope,
    ) -> SessionWorkspaceRemoval {
        remove_session_from_workspace(
            RemoveSession {
                tabs,
                sessions,
                session_id,
                active_tab_id,
                scope,
            },
            |seed| session("replacement", seed.map_or("~", |seed| seed.cwd.as_str())),
            |_| true,
        )
    }

    const MONO: &str = "/projects/monocode";

    fn ids(tabs: &[WorkspaceTab]) -> Vec<&str> {
        tabs.iter().map(|tab| tab.id.as_str()).collect()
    }

    fn session_ids(sessions: &[Session]) -> Vec<&str> {
        sessions.iter().map(|session| session.id.as_str()).collect()
    }

    #[test]
    fn closes_the_sole_session_tab_with_its_files_instead_of_promoting_them() {
        let file = new_file_tab(&format!("{MONO}/README.md"), MONO, false, None, None);
        let closing = WorkspaceTab {
            layout: split(
                "split",
                SplitDir::Right,
                vec![leaf("s1"), leaf("editor")],
                vec![0.5, 0.5],
            ),
            focused_id: "s1".into(),
            editor_panes: vec![EditorPane::new(
                "editor",
                vec![file.clone()],
                file.id.clone(),
            )],
            ..tab("closing", "s1")
        };
        let result = remove(
            &[closing, tab("other", "s2")],
            &[session("s1", MONO), session("s2", "/projects/ruler")],
            "s1",
            "closing",
            WorkspaceTabCloseScope::Workspace,
        );
        assert_eq!(ids(&result.closed_tabs), vec!["closing"]);
        assert_eq!(ids(&result.tabs), vec!["other"]);
        assert_eq!(session_ids(&result.sessions), vec!["s2"]);
        assert_eq!(result.active_tab_id, "other");
    }

    #[test]
    fn retains_file_panes_and_another_conversation_in_a_shared_workspace() {
        let file = new_file_tab(&format!("{MONO}/README.md"), MONO, false, None, None);
        let inner = split(
            "inner",
            SplitDir::Down,
            vec![leaf("s2"), leaf("editor")],
            vec![0.5, 0.5],
        );
        let shared = WorkspaceTab {
            layout: split(
                "outer",
                SplitDir::Right,
                vec![leaf("s1"), inner.clone()],
                vec![0.5, 0.5],
            ),
            focused_id: "s1".into(),
            editor_panes: vec![EditorPane::new(
                "editor",
                vec![file.clone()],
                file.id.clone(),
            )],
            ..tab("shared", "s1")
        };
        let result = remove(
            std::slice::from_ref(&shared),
            &[session("s1", MONO), session("s2", MONO)],
            "s1",
            "shared",
            WorkspaceTabCloseScope::Workspace,
        );
        assert!(result.closed_tabs.is_empty());
        assert_eq!(result.tabs.len(), 1);
        assert_eq!(result.tabs[0].editor_panes, shared.editor_panes);
        assert_eq!(result.tabs[0].layout, inner);
        assert_eq!(session_ids(&result.sessions), vec!["s2"]);
    }

    #[test]
    fn closes_session_scoped_changes_with_the_session_while_preserving_other_files() {
        let session_changes = monocode_layout::FilePaneTab {
            review: Some(true),
            session_changes: Some(SessionChangesSource::new("s1")),
            ..monocode_layout::FilePaneTab::new("changes", MONO, MONO)
        };
        let readme = new_file_tab(&format!("{MONO}/README.md"), MONO, false, None, None);
        let shared = WorkspaceTab {
            layout: split(
                "split",
                SplitDir::Right,
                vec![leaf("s1"), leaf("s2"), leaf("editor")],
                vec![1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0],
            ),
            focused_id: "s1".into(),
            editor_panes: vec![EditorPane::new(
                "editor",
                vec![session_changes.clone(), readme.clone()],
                session_changes.id.clone(),
            )],
            ..tab("shared", "s1")
        };
        let result = remove(
            &[shared],
            &[session("s1", MONO), session("s2", MONO)],
            "s1",
            "shared",
            WorkspaceTabCloseScope::Workspace,
        );
        assert_eq!(result.tabs[0].editor_panes[0].files, vec![readme.clone()]);
        assert_eq!(result.tabs[0].editor_panes[0].active_file_id, readme.id);
    }

    #[test]
    fn replaces_the_final_project_tab_with_a_blank_session() {
        let result = remove(
            &[tab("only", "s1")],
            &[session("s1", MONO)],
            "s1",
            "only",
            WorkspaceTabCloseScope::Project,
        );
        assert_eq!(ids(&result.closed_tabs), vec!["only"]);
        assert_eq!(result.tabs[0].id, "only");
        assert_eq!(result.tabs[0].focused_id, "replacement");
        assert_eq!(session_ids(&result.sessions), vec!["replacement"]);
    }

    #[test]
    fn does_not_leave_the_project_when_closing_its_final_tab_in_project_scope() {
        let result = remove(
            &[tab("ruler", "r1"), tab("monocode", "s1")],
            &[session("r1", "/projects/ruler"), session("s1", MONO)],
            "s1",
            "monocode",
            WorkspaceTabCloseScope::Project,
        );
        assert_eq!(ids(&result.tabs), vec!["ruler", "monocode"]);
        assert_eq!(result.active_tab_id, "monocode");
        assert_eq!(result.tabs[1].focused_id, "replacement");
    }

    #[test]
    fn removes_only_the_archived_sessions_plans_in_a_shared_workspace() {
        let own = new_plan_tab("s1", "p1", "Own plan", MONO);
        let other = new_plan_tab("s2", "p2", "Other plan", MONO);
        let readme = new_file_tab(&format!("{MONO}/README.md"), MONO, false, None, None);
        let shared = WorkspaceTab {
            layout: split(
                "split",
                SplitDir::Right,
                vec![leaf("s1"), leaf("s2")],
                vec![0.5, 0.5],
            ),
            ..tab("shared", "s1")
        };
        let pin = OpenEditorTabOptions {
            pin: true,
            ..Default::default()
        };
        let with_files = [own, other.clone(), readme.clone()]
            .iter()
            .fold(shared, |tab, file| open_editor_tab(&tab, file, &pin));
        let result = remove(
            &[with_files],
            &[session("s1", MONO), session("s2", MONO)],
            "s1",
            "shared",
            WorkspaceTabCloseScope::Workspace,
        );
        assert_eq!(result.tabs[0].editor_panes[0].files, vec![other, readme]);
        assert!(!leaf_ids(&result.tabs[0].layout).contains(&"s1".to_string()));
    }

    #[test]
    fn removes_plan_panes_even_after_their_conversation_moved_to_another_tab() {
        let plan = new_plan_tab("s1", "p1", "Plan", MONO);
        let other = open_editor_tab(&tab("other", "s2"), &plan, &OpenEditorTabOptions::default());
        let result = remove(
            &[tab("own", "s1"), other],
            &[session("s1", MONO), session("s2", MONO)],
            "s1",
            "other",
            WorkspaceTabCloseScope::Workspace,
        );
        assert_eq!(result.tabs.len(), 1);
        assert!(result.tabs[0].editor_panes.is_empty());
        assert_eq!(result.tabs[0].focused_id, "s2");
        assert_eq!(result.tabs[0].layout, leaf("s2"));
    }

    #[test]
    fn replaces_a_standalone_plan_pane_without_leaving_a_dangling_layout_leaf() {
        let plan = new_plan_tab("s1", "p1", "Plan", MONO);
        let only = WorkspaceTab {
            editor_panes: vec![EditorPane::new(
                "editor",
                vec![plan.clone()],
                plan.id.clone(),
            )],
            ..tab("only", "editor")
        };
        let result = remove(
            &[only],
            &[session("s1", MONO)],
            "s1",
            "only",
            WorkspaceTabCloseScope::Workspace,
        );
        assert!(result.tabs[0].editor_panes.is_empty());
        assert_eq!(result.tabs[0].layout, leaf("replacement"));
        assert_eq!(session_ids(&result.sessions), vec!["replacement"]);
    }
}
