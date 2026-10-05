//! Port of the tab helpers at the end of src/app/App.tsx (lines
//! 11080-11294): `conversationTitle`, `selectedChangePath`,
//! `selectedChangeKind`, `selectedCommitSha`, `isBlankWorkspaceTab`,
//! `toTitleTab`, and `dropOpenFiles`.

use std::collections::HashSet;

use gpui::App;
use monocode_core::paths::{basename, display_path};
use monocode_core::session::{session_display_title, session_needs_input};
use monocode_core::{HarnessId, Session};
use monocode_layout::paths::{is_remote_project_path, project_name};
use monocode_layout::project_return::is_blank_session;
use monocode_layout::terminal_tab::terminal_tab_label;
use monocode_layout::{
    EditorPane, FilePaneTab, GitFileDiffKind, WorkspaceTab, first_leaf_id, focused_file_tab,
    is_commit_tab, is_filesystem_tab, is_terminal_tab, leaf_ids, preview_workspace_file,
    remove_pane, sibling_leaf_id,
};

use super::delegate::WorkspaceDelegate;

/// `releaseNotesTitle` from src/app/model/releaseNotes.ts.
pub fn release_notes_title(version: &str) -> String {
    format!("What's new in MonoCode {version}")
}

/// `Tab` in src/app/shell/TitleBar.tsx: what the title bar draws for one
/// workspace tab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleTab {
    pub id: String,
    /// Project folder name, e.g. `agent-terminal`.
    pub project: String,
    /// Focused conversation title; empty for a fresh session.
    pub title: String,
    /// Other conversation titles in this tab, focused session omitted.
    pub more: Vec<String>,
    pub session_count: usize,
    pub harnesses: Vec<HarnessId>,
    /// Harnesses with an in-flight turn in this tab.
    pub busy_harnesses: Vec<HarnessId>,
    /// Harnesses with a finished response that has not been focused yet.
    pub done_harnesses: Vec<HarnessId>,
    /// Open file names, active files first.
    pub files: Vec<String>,
    /// A split layout with more than one pane.
    pub multi_pane: bool,
    /// Focus is on a file or terminal pane rather than a conversation.
    pub file_focused: bool,
    /// The sole pane is a fresh conversation with no user turn or file.
    pub blank: bool,
    pub group_id: Option<String>,
    pub dirty: bool,
    pub terminal: bool,
    /// The file id when the whole tab is one preview file.
    pub preview_file_id: Option<String>,
}

/// `conversationTitle`: the tab label for a session, empty for a new one.
pub fn conversation_title(session: &Session, delegate: &dyn WorkspaceDelegate, cx: &App) -> String {
    let remote = if is_remote_project_path(&session.cwd)
        && delegate.remote_session_for(&session.id, cx).is_some()
    {
        delegate.remote_summary(session, cx)
    } else {
        None
    };
    let title = match &remote {
        Some(remote) => session_display_title(&remote.title, remote.harness),
        None => session_display_title(&session.title, session.harness),
    };
    if title == "New session" {
        String::new()
    } else {
        title
    }
}

/// `selectedChangePath`: the reviewed file the focused pane shows, relative
/// to the git root.
pub fn selected_change_path(tab: &WorkspaceTab, git_cwd: Option<&str>) -> Option<String> {
    let file = focused_file_tab(tab)?;
    if !is_filesystem_tab(file) || file.review != Some(true) {
        return None;
    }
    let base = git_cwd.filter(|cwd| !cwd.is_empty()).unwrap_or(&file.cwd);
    Some(display_path(&file.path, Some(base)))
}

/// `selectedChangeKind`.
pub fn selected_change_kind(tab: &WorkspaceTab) -> Option<GitFileDiffKind> {
    let file = focused_file_tab(tab)?;
    if file.review == Some(true) {
        file.change_kind
    } else {
        None
    }
}

/// `selectedCommitSha`: the commit the focused pane, or any pane, shows.
pub fn selected_commit_sha(tab: &WorkspaceTab) -> Option<String> {
    if let Some(focused) = focused_file_tab(tab)
        && let Some(commit) = &focused.commit
    {
        return Some(commit.sha.clone());
    }
    tab.editor_panes.iter().find_map(|pane| {
        let file = pane
            .files
            .iter()
            .find(|entry| entry.id == pane.active_file_id)?;
        if is_commit_tab(file) {
            file.commit.as_ref().map(|commit| commit.sha.clone())
        } else {
            None
        }
    })
}

/// `isBlankWorkspaceTab`: one blank chat and nothing else.
pub fn is_blank_workspace_tab(
    tab: &WorkspaceTab,
    sessions: &[Session],
    delegate: &dyn WorkspaceDelegate,
    cx: &App,
) -> bool {
    if tab.editor_panes.iter().any(|pane| !pane.files.is_empty()) {
        return false;
    }
    if tab.terminal_panes.iter().any(|pane| !pane.files.is_empty()) {
        return false;
    }
    let ids = leaf_ids(&tab.layout);
    if ids.len() != 1 {
        return false;
    }
    if delegate.remote_session_for(&ids[0], cx).is_some()
        || delegate.remote_pending_worktree(&ids[0], cx)
    {
        return false;
    }
    is_blank_session(sessions.iter().find(|entry| entry.id == ids[0]))
}

/// `toTitleTab`.
pub fn to_title_tab(
    tab: &WorkspaceTab,
    sessions: &[Session],
    dirty_files: &HashSet<String>,
    unseen_finished_ids: &HashSet<String>,
    delegate: &dyn WorkspaceDelegate,
    cx: &App,
) -> TitleTab {
    let pane_ids = leaf_ids(&tab.layout);
    let multi_pane = pane_ids.len() > 1;
    let tab_sessions: Vec<&Session> = pane_ids
        .iter()
        .filter_map(|id| sessions.iter().find(|session| &session.id == id))
        .collect();
    let session_focused = tab_sessions
        .iter()
        .any(|session| session.id == tab.focused_id);
    let file_focused = !session_focused
        && (tab
            .editor_panes
            .iter()
            .any(|pane| pane.id == tab.focused_id)
            || tab
                .terminal_panes
                .iter()
                .any(|pane| pane.id == tab.focused_id));
    let focused = sessions
        .iter()
        .find(|session| session.id == tab.focused_id)
        .or(tab_sessions.first().copied());

    let mut harnesses = Vec::new();
    let mut busy_harnesses = Vec::new();
    let mut done_harnesses = Vec::new();
    let ordered: Vec<&Session> = match focused {
        Some(focused) => std::iter::once(focused)
            .chain(
                tab_sessions
                    .iter()
                    .copied()
                    .filter(|session| session.id != focused.id),
            )
            .collect(),
        None => tab_sessions.clone(),
    };
    for session in &ordered {
        if session.is_busy()
            && !session_needs_input(session)
            && !busy_harnesses.contains(&session.harness)
        {
            busy_harnesses.push(session.harness);
        }
        if unseen_finished_ids.contains(&session.id) && !done_harnesses.contains(&session.harness) {
            done_harnesses.push(session.harness);
        }
        if !harnesses.contains(&session.harness) {
            harnesses.push(session.harness);
        }
    }

    let mut files: Vec<String> = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut push_file = |file: &FilePaneTab| {
        let key = if file.terminal == Some(true) {
            format!("terminal:{}", file.id)
        } else if let Some(plan) = &file.plan {
            format!("plan:{}", plan.block_id)
        } else if let Some(notes) = &file.release_notes {
            format!("release-notes:{}", notes.version)
        } else {
            file.path.clone()
        };
        if !seen_keys.insert(key) {
            return;
        }
        let plan_title = file
            .plan
            .as_ref()
            .map(|plan| monocode_core::js::trim(&plan.title).to_string())
            .filter(|title| !title.is_empty());
        files.push(plan_title.unwrap_or_else(|| {
            if let Some(notes) = &file.release_notes {
                release_notes_title(&notes.version)
            } else if file.terminal == Some(true) {
                terminal_tab_label(file)
            } else {
                basename(&file.path)
            }
        }));
    };
    let focused_pane: Option<&EditorPane> = tab
        .editor_panes
        .iter()
        .find(|pane| pane.id == tab.focused_id)
        .or_else(|| {
            tab.terminal_panes
                .iter()
                .find(|pane| pane.id == tab.focused_id)
        });
    let focused_pane_id = focused_pane.map(|pane| pane.id.as_str());
    let other_panes = tab
        .editor_panes
        .iter()
        .chain(&tab.terminal_panes)
        .filter(|pane| Some(pane.id.as_str()) != focused_pane_id);
    let panes: Vec<&EditorPane> = focused_pane.into_iter().chain(other_panes).collect();
    for pane in &panes {
        if let Some(active) = pane
            .files
            .iter()
            .find(|file| file.id == pane.active_file_id)
        {
            push_file(active);
        }
    }
    for pane in &panes {
        for file in &pane.files {
            push_file(file);
        }
    }

    let more: Vec<String> = tab_sessions
        .iter()
        .filter(|session| Some(session.id.as_str()) != focused.map(|focused| focused.id.as_str()))
        .map(|session| conversation_title(session, delegate, cx))
        .filter(|title| !title.is_empty())
        .collect();

    let has_terminal = tab
        .terminal_panes
        .iter()
        .any(|pane| pane.files.iter().any(is_terminal_tab));
    let focused_file = focused_file_tab(tab);

    TitleTab {
        id: tab.id.clone(),
        project: match (focused, focused_file) {
            (Some(focused), _) => project_name(&focused.cwd),
            (None, Some(file)) => project_name(file.project_cwd.as_deref().unwrap_or(&file.cwd)),
            (None, None) => "~".into(),
        },
        title: focused
            .map(|focused| conversation_title(focused, delegate, cx))
            .unwrap_or_default(),
        more,
        session_count: tab_sessions.len(),
        terminal: has_terminal && harnesses.is_empty(),
        harnesses,
        busy_harnesses,
        done_harnesses,
        files,
        multi_pane,
        file_focused,
        blank: is_blank_workspace_tab(tab, sessions, delegate, cx),
        dirty: tab.editor_panes.iter().any(|pane| {
            pane.files
                .iter()
                .any(|file| is_filesystem_tab(file) && dirty_files.contains(&file.id))
        }),
        preview_file_id: preview_workspace_file(tab).map(|file| file.id.clone()),
        group_id: tab.group_id.clone(),
    }
}

/// `TitleTab.project` alone: the project folder name the title bar shows,
/// which `projectOfTab` read back for tab groups.
pub fn title_tab_project(tab: &WorkspaceTab, sessions: &[Session]) -> String {
    let focused = sessions
        .iter()
        .find(|session| session.id == tab.focused_id)
        .or_else(|| {
            leaf_ids(&tab.layout)
                .iter()
                .find_map(|id| sessions.iter().find(|session| &session.id == id))
        });
    match (focused, focused_file_tab(tab)) {
        (Some(focused), _) => project_name(&focused.cwd),
        (None, Some(file)) => project_name(file.project_cwd.as_deref().unwrap_or(&file.cwd)),
        (None, None) => "~".into(),
    }
}

/// `dropOpenFiles`: close the filesystem tabs whose path `should_drop`
/// matches, and the panes they empty.
pub fn drop_open_files(tab: &WorkspaceTab, should_drop: impl Fn(&str) -> bool) -> WorkspaceTab {
    let mut layout = tab.layout.clone();
    let mut focused_id = tab.focused_id.clone();
    let mut editor_panes = Vec::new();
    for pane in &tab.editor_panes {
        let files: Vec<FilePaneTab> = pane
            .files
            .iter()
            .filter(|file| !is_filesystem_tab(file) || !should_drop(&file.path))
            .cloned()
            .collect();
        if files.is_empty() {
            let sibling = sibling_leaf_id(&layout, &pane.id);
            if let Some(without_pane) = remove_pane(&layout, &pane.id) {
                layout = without_pane;
                if focused_id == pane.id {
                    focused_id = sibling.unwrap_or_else(|| first_leaf_id(&layout).to_string());
                }
            }
            continue;
        }
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
    }
    WorkspaceTab {
        layout,
        focused_id,
        editor_panes,
        ..tab.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::delegate::NoDelegate;
    use gpui::TestAppContext;
    use monocode_core::block::{Block, BlockRole};
    use monocode_layout::{
        CommitTabSource, OpenEditorTabOptions, new_commit_tab, new_file_tab, new_tab,
        new_terminal_file, open_editor_tab, open_terminal_tab,
    };

    fn chat(id: &str, harness: HarnessId, title: &str) -> Session {
        let mut session = Session::blank(id, harness, "", "/Users/me/repo");
        session.title = title.into();
        session.blocks = vec![Block::new("u1", BlockRole::User, "hi")];
        session
    }

    #[gpui::test]
    fn describes_a_tab_for_the_title_bar(cx: &mut TestAppContext) {
        let a = chat("a", HarnessId::Claude, "Fix the build");
        let mut b = chat("b", HarnessId::Codex, "Review");
        b.busy = Some(true);
        let tab = new_tab("a");
        let tab = WorkspaceTab {
            layout: monocode_layout::split_pane(
                &tab.layout,
                "a",
                monocode_layout::SplitDir::Right,
                "b",
            ),
            ..tab
        };
        let file = new_file_tab(
            "/Users/me/repo/src/lib.rs",
            "/Users/me/repo",
            false,
            None,
            None,
        );
        let tab = open_editor_tab(
            &tab,
            &file,
            &OpenEditorTabOptions {
                pin: true,
                ..Default::default()
            },
        );
        let terminal = new_terminal_file("/Users/me/repo", Some("zsh"), None);
        let tab = open_terminal_tab(&tab, &terminal, None);
        let dirty = HashSet::from([file.id.clone()]);
        let unseen = HashSet::from(["a".to_string()]);
        let sessions = vec![a, b];
        let title = cx.update(|cx| to_title_tab(&tab, &sessions, &dirty, &unseen, &NoDelegate, cx));
        assert_eq!(title.project, "repo");
        assert!(title.multi_pane);
        assert_eq!(title.session_count, 2);
        assert_eq!(title.harnesses, vec![HarnessId::Claude, HarnessId::Codex]);
        assert_eq!(title.busy_harnesses, vec![HarnessId::Codex]);
        assert_eq!(title.done_harnesses, vec![HarnessId::Claude]);
        assert!(title.dirty);
        assert!(!title.terminal);
        assert!(!title.blank);
        assert!(title.files.contains(&"lib.rs".to_string()));
        assert!(title.files.contains(&"zsh".to_string()));
    }

    #[gpui::test]
    fn recognizes_a_blank_tab(cx: &mut TestAppContext) {
        let blank = Session::blank("a", HarnessId::Claude, "", "/Users/me/repo");
        let tab = new_tab("a");
        let blank_tab = cx.update(|cx| {
            is_blank_workspace_tab(&tab, std::slice::from_ref(&blank), &NoDelegate, cx)
        });
        assert!(blank_tab);
        let title = cx.update(|cx| conversation_title(&blank, &NoDelegate, cx));
        assert_eq!(title, "");
    }

    #[test]
    fn drops_files_inside_a_deleted_folder() {
        let kept = new_file_tab("/r/keep.rs", "/r", false, None, None);
        let gone = new_file_tab("/r/old/a.rs", "/r", false, None, None);
        let pin = OpenEditorTabOptions {
            pin: true,
            ..Default::default()
        };
        let tab = open_editor_tab(&new_tab("s1"), &gone, &pin);
        let tab = open_editor_tab(&tab, &kept, &pin);
        let dropped = drop_open_files(&tab, |path| {
            crate::workspace::paths::is_equal_or_inside(path, "/r/old")
        });
        assert_eq!(dropped.editor_panes[0].files, vec![kept]);

        let only = open_editor_tab(&new_tab("s1"), &gone, &pin);
        let pane_id = only.editor_panes[0].id.clone();
        let only = WorkspaceTab {
            focused_id: pane_id,
            ..only
        };
        let dropped = drop_open_files(&only, |path| path.starts_with("/r/old"));
        assert!(dropped.editor_panes.is_empty());
        assert_eq!(dropped.focused_id, "s1");
        assert_eq!(leaf_ids(&dropped.layout), vec!["s1".to_string()]);
    }

    #[test]
    fn finds_the_selected_change_and_commit() {
        let review = new_file_tab(
            "/r/src/a.rs",
            "/r",
            true,
            Some(GitFileDiffKind::Unstaged),
            None,
        );
        let tab = open_editor_tab(&new_tab("s1"), &review, &OpenEditorTabOptions::default());
        let tab = WorkspaceTab {
            focused_id: tab.editor_panes[0].id.clone(),
            ..tab
        };
        assert_eq!(
            selected_change_path(&tab, Some("/r")).as_deref(),
            Some("src/a.rs")
        );
        assert_eq!(selected_change_kind(&tab), Some(GitFileDiffKind::Unstaged));
        let commit = new_commit_tab("/r", CommitTabSource::new("abc123", "abc", "Fix"), None);
        let tab = open_editor_tab(&new_tab("s1"), &commit, &OpenEditorTabOptions::default());
        assert_eq!(selected_commit_sha(&tab).as_deref(), Some("abc123"));
        assert_eq!(release_notes_title("0.6.0"), "What's new in MonoCode 0.6.0");
    }
}
