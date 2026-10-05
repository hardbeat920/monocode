//! Port of src/features/workspace/model/workspaceTabGroups.ts: which
//! project a workspace tab belongs to, closing tabs, and dropping sessions,
//! tabs, and panes onto other panes.

use std::collections::HashSet;

use crate::ids::random_uuid;
use crate::layout::{
    PaneEdge, PanePlace, SplitDir, WorkspaceTab, close_leaf, first_leaf_id, focused_file_tab,
    has_leaf, leaf, leaf_ids, place_layout, place_pane, replace_leaf_id, replace_pane_with_layout,
    split_pane,
};
use crate::paths::{project_name, same_project_path};
use crate::session_ref::SessionRef;

/// `workspaceTabCwd`: the first leaf session's project, else the focused
/// file's owning project or cwd. `None` for a projectless tab.
pub fn workspace_tab_cwd<S: SessionRef>(tab: &WorkspaceTab, sessions: &[S]) -> Option<String> {
    for id in leaf_ids(&tab.layout) {
        let session = sessions.iter().find(|entry| entry.session_id() == id);
        if let Some(session) = session {
            let cwd = session.session_cwd();
            if !cwd.is_empty() && cwd != "~" {
                return Some(cwd.to_string());
            }
        }
    }

    let file = focused_file_tab(tab)?;
    let cwd = file.project_cwd.as_deref().unwrap_or(&file.cwd);
    if !cwd.is_empty() && cwd != "~" {
        return Some(cwd.to_string());
    }
    None
}

/// `workspaceTabWorktree`: the working copy a tab runs in. That is its first
/// session's worktree, else the folder its focused file was opened from.
/// `None` when the tab has neither.
pub fn workspace_tab_worktree<S: SessionRef>(tab: &WorkspaceTab, sessions: &[S]) -> Option<String> {
    for id in leaf_ids(&tab.layout) {
        let Some(session) = sessions.iter().find(|entry| entry.session_id() == id) else {
            continue;
        };
        let cwd = session.session_cwd();
        if !cwd.is_empty() && cwd != "~" {
            let worktree = session
                .session_worktree_cwd()
                .filter(|worktree| !worktree.is_empty());
            return Some(worktree.unwrap_or(cwd).to_string());
        }
    }
    let cwd = &focused_file_tab(tab)?.cwd;
    (!cwd.is_empty() && cwd != "~").then(|| cwd.clone())
}

/// `tabInWorktree`: tabs without a working copy show in every worktree.
pub fn tab_in_worktree<S: SessionRef>(tab: &WorkspaceTab, sessions: &[S], worktree: &str) -> bool {
    workspace_tab_worktree(tab, sessions).is_none_or(|cwd| same_project_path(&cwd, worktree))
}

/// `focusedWorkspaceTabCwd`: the focused session's cwd, else the focused
/// file's project, else `workspace_tab_cwd`.
pub fn focused_workspace_tab_cwd<S: SessionRef>(
    tab: &WorkspaceTab,
    sessions: &[S],
) -> Option<String> {
    if let Some(session) = sessions
        .iter()
        .find(|entry| entry.session_id() == tab.focused_id)
    {
        return Some(session.session_cwd().to_string());
    }
    if let Some(file) = focused_file_tab(tab) {
        return Some(file.project_cwd.clone().unwrap_or_else(|| file.cwd.clone()));
    }
    workspace_tab_cwd(tab, sessions)
}

/// `workspaceTabProject`: the project folder name, `None` for home.
pub fn workspace_tab_project<S: SessionRef>(tab: &WorkspaceTab, sessions: &[S]) -> Option<String> {
    let cwd = workspace_tab_cwd(tab, sessions)?;
    let name = project_name(&cwd);
    (name != "~").then_some(name)
}

/// `findTabForProject`.
pub fn find_tab_for_project<'a, S: SessionRef>(
    tabs: &'a [WorkspaceTab],
    sessions: &[S],
    path: &str,
) -> Option<&'a WorkspaceTab> {
    tabs.iter().find(|tab| {
        workspace_tab_cwd(tab, sessions).is_some_and(|cwd| same_project_path(&cwd, path))
    })
}

/// `findOpenSessionTab`: a tab is only openable as a chat when its session
/// object is also mounted.
pub fn find_open_session_tab<'a, S: SessionRef>(
    tabs: &'a [WorkspaceTab],
    sessions: &[S],
    session_id: &str,
) -> Option<&'a WorkspaceTab> {
    if !sessions
        .iter()
        .any(|session| session.session_id() == session_id)
    {
        return None;
    }
    tabs.iter().find(|tab| has_leaf(&tab.layout, session_id))
}

/// `switchSessionInTab`: show another session in the focused pane without
/// changing the active tab.
pub fn switch_session_in_tab(
    tabs: &[WorkspaceTab],
    active_tab_id: &str,
    current_session_id: &str,
    target_session_id: &str,
) -> Option<Vec<WorkspaceTab>> {
    let active_tab = tabs.iter().find(|tab| tab.id == active_tab_id)?;
    if active_tab.diff_focused == Some(true) {
        return None;
    }
    if active_tab.focused_id != current_session_id {
        return None;
    }
    if !has_leaf(&active_tab.layout, current_session_id) {
        return None;
    }
    if current_session_id == target_session_id {
        return None;
    }

    let target_tab_id = tabs
        .iter()
        .find(|tab| has_leaf(&tab.layout, target_session_id))
        .map(|tab| tab.id.clone());
    if target_tab_id.as_deref() == Some(active_tab_id) {
        return Some(
            tabs.iter()
                .map(|tab| {
                    if tab.id == active_tab_id {
                        WorkspaceTab {
                            focused_id: target_session_id.to_string(),
                            diff_focused: Some(false),
                            ..tab.clone()
                        }
                    } else {
                        tab.clone()
                    }
                })
                .collect(),
        );
    }

    Some(
        tabs.iter()
            .map(|tab| {
                if tab.id == active_tab_id {
                    return WorkspaceTab {
                        layout: replace_leaf_id(&tab.layout, current_session_id, target_session_id),
                        focused_id: target_session_id.to_string(),
                        diff_focused: Some(false),
                        ..tab.clone()
                    };
                }
                if Some(&tab.id) == target_tab_id.as_ref() {
                    let focused_id = if tab.focused_id == target_session_id {
                        current_session_id.to_string()
                    } else {
                        tab.focused_id.clone()
                    };
                    return WorkspaceTab {
                        layout: replace_leaf_id(&tab.layout, target_session_id, current_session_id),
                        focused_id,
                        ..tab.clone()
                    };
                }
                tab.clone()
            })
            .collect(),
    )
}

/// `openAddToChatSessionPane`: add a chat beside a file-only tab so an
/// add-to-chat request has a target. `None` when a session pane exists.
pub fn open_add_to_chat_session_pane<S: SessionRef>(
    tab: &WorkspaceTab,
    sessions: &[S],
    session_id: &str,
) -> Option<WorkspaceTab> {
    let pane_ids = leaf_ids(&tab.layout);
    let session_ids: HashSet<&str> = sessions
        .iter()
        .map(|session| session.session_id())
        .collect();
    if pane_ids.iter().any(|id| session_ids.contains(id.as_str())) {
        return None;
    }

    let source_id = if pane_ids.contains(&tab.focused_id) {
        tab.focused_id.clone()
    } else {
        first_leaf_id(&tab.layout).to_string()
    };
    Some(WorkspaceTab {
        layout: split_pane(&tab.layout, &source_id, SplitDir::Right, session_id),
        focused_id: session_id.to_string(),
        diff_focused: Some(false),
        ..tab.clone()
    })
}

/// `filterTabsForProject`.
pub fn filter_tabs_for_project<S: SessionRef>(
    tabs: &[WorkspaceTab],
    sessions: &[S],
    path: &str,
) -> Vec<WorkspaceTab> {
    tabs.iter()
        .filter(|tab| {
            workspace_tab_cwd(tab, sessions).is_some_and(|cwd| same_project_path(&cwd, path))
        })
        .cloned()
        .collect()
}

/// `WorkspaceTabCloseScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspaceTabCloseScope {
    Project,
    Workspace,
}

/// `WorkspaceTabClosePlan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceTabClosePlan {
    Keep,
    Close { next_active_tab_id: Option<String> },
}

/// `planWorkspaceTabClose`: which tab to activate after closing one.
pub fn plan_workspace_tab_close<S: SessionRef>(
    tabs: &[WorkspaceTab],
    sessions: &[S],
    closing_tab_id: &str,
    scope: WorkspaceTabCloseScope,
) -> WorkspaceTabClosePlan {
    plan_workspace_tab_close_in(tabs, sessions, closing_tab_id, scope, None)
}

/// `worktreeOf`: the worktree a tab belongs to.
pub type WorktreeOf<'a> = &'a dyn Fn(&WorkspaceTab) -> Option<String>;

/// `planWorkspaceTabClose` with `worktreeOf`. When given, the next tab also
/// has to share the closing tab's worktree, so closing a tab never leaves
/// the worktree on screen.
pub fn plan_workspace_tab_close_in<S: SessionRef>(
    tabs: &[WorkspaceTab],
    sessions: &[S],
    closing_tab_id: &str,
    scope: WorkspaceTabCloseScope,
    worktree_of: Option<WorktreeOf<'_>>,
) -> WorkspaceTabClosePlan {
    let Some(closing_index) = tabs.iter().position(|tab| tab.id == closing_tab_id) else {
        return WorkspaceTabClosePlan::Keep;
    };

    let remaining: Vec<&WorkspaceTab> =
        tabs.iter().filter(|tab| tab.id != closing_tab_id).collect();
    if remaining.is_empty() {
        return WorkspaceTabClosePlan::Keep;
    }

    let global_target = remaining
        .get(closing_index.saturating_sub(1))
        .or(remaining.first())
        .map(|tab| tab.id.clone());
    if scope == WorkspaceTabCloseScope::Workspace {
        return WorkspaceTabClosePlan::Close {
            next_active_tab_id: global_target,
        };
    }

    let Some(closing_cwd) = workspace_tab_cwd(&tabs[closing_index], sessions) else {
        return WorkspaceTabClosePlan::Close {
            next_active_tab_id: global_target,
        };
    };

    let closing_worktree = worktree_of.and_then(|worktree_of| worktree_of(&tabs[closing_index]));
    let same_scope = |tab: &&WorkspaceTab| {
        if !workspace_tab_cwd(tab, sessions)
            .is_some_and(|cwd| same_project_path(&cwd, &closing_cwd))
        {
            return false;
        }
        let Some(closing_worktree) = &closing_worktree else {
            return true;
        };
        worktree_of
            .and_then(|worktree_of| worktree_of(tab))
            .is_none_or(|worktree| same_project_path(&worktree, closing_worktree))
    };
    let before = tabs[..closing_index].iter().rev().find(same_scope);
    let after = || tabs[closing_index + 1..].iter().find(same_scope);
    if let Some(tab) = before.or_else(after) {
        return WorkspaceTabClosePlan::Close {
            next_active_tab_id: Some(tab.id.clone()),
        };
    }

    // Deck mode is one project (and worktree) at a time. Closing the last tab
    // there must not jump to another one's tab; the caller keeps this one.
    WorkspaceTabClosePlan::Keep
}

/// The result of `applyPlaceSessionOnPane`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSession<S> {
    pub tabs: Vec<WorkspaceTab>,
    pub sessions: Vec<S>,
    pub active_tab_id: String,
}

/// Input to `applyPlaceSessionOnPane`.
pub struct PlaceSessionOnPane<'a, S> {
    pub tabs: &'a [WorkspaceTab],
    pub sessions: &'a [S],
    pub session_id: &'a str,
    pub target_id: &'a str,
    pub edge: PaneEdge,
    pub replace_target: bool,
    pub scope: WorkspaceTabCloseScope,
}

/// `applyPlaceSessionOnPane`: drop a session onto a pane edge in the tab
/// that owns `target_id`. Already-open chats move; a blank target is
/// replaced; a chat open in another tab is relocated (that tab closes when
/// it was the last leaf). `create_replacement` builds the session that fills
/// a tab the scope will not close.
pub fn apply_place_session_on_pane<S: SessionRef + Clone>(
    input: PlaceSessionOnPane<'_, S>,
    mut create_replacement: impl FnMut(Option<&S>) -> S,
) -> Option<PlacedSession<S>> {
    let PlaceSessionOnPane {
        tabs,
        sessions,
        session_id,
        target_id,
        edge,
        replace_target,
        scope,
    } = input;
    if session_id == target_id {
        return None;
    }
    let target_index = tabs
        .iter()
        .position(|tab| has_leaf(&tab.layout, target_id))?;

    let mut next_sessions: Vec<S> = if replace_target {
        sessions
            .iter()
            .filter(|session| session.session_id() != target_id)
            .cloned()
            .collect()
    } else {
        sessions.to_vec()
    };
    let target_tab_id = tabs[target_index].id.clone();

    let mut next_tabs: Vec<WorkspaceTab> = tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            if index != target_index {
                return tab.clone();
            }
            let layout = if replace_target {
                replace_leaf_id(&tab.layout, target_id, session_id)
            } else {
                place_pane(&tab.layout, session_id, target_id, edge)
            };
            WorkspaceTab {
                layout,
                focused_id: session_id.to_string(),
                diff_focused: Some(false),
                ..tab.clone()
            }
        })
        .collect();

    for tab in next_tabs.clone() {
        if tab.id == target_tab_id {
            continue;
        }
        if !has_leaf(&tab.layout, session_id) {
            continue;
        }
        let Some(tab_index) = next_tabs.iter().position(|entry| entry.id == tab.id) else {
            continue;
        };

        if let Some(closed) = close_leaf(&tab, session_id) {
            next_tabs[tab_index] = closed;
            continue;
        }

        let close_plan = plan_workspace_tab_close(&next_tabs, &next_sessions, &tab.id, scope);
        if matches!(close_plan, WorkspaceTabClosePlan::Close { .. }) {
            next_tabs.retain(|entry| entry.id != tab.id);
            continue;
        }

        let replacement = create_replacement(
            next_sessions
                .iter()
                .find(|session| session.session_id() == session_id),
        );
        let replacement_id = replacement.session_id().to_string();
        next_sessions.push(replacement);
        next_tabs[tab_index] = WorkspaceTab {
            layout: leaf(replacement_id.clone()),
            focused_id: replacement_id,
            editor_panes: Vec::new(),
            terminal_panes: Vec::new(),
            diff_open: Some(false),
            diff_focused: Some(false),
            ..tab
        };
    }

    Some(PlacedSession {
        tabs: next_tabs,
        sessions: next_sessions,
        active_tab_id: target_tab_id,
    })
}

/// The result of `applyPlaceTabOnPane`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedTab<S> {
    pub tabs: Vec<WorkspaceTab>,
    pub sessions: Vec<S>,
    pub active_tab_id: String,
    pub focused_id: String,
}

/// `applyPlaceTabOnPane`: drop a complete workspace tab onto a pane edge.
/// Its split tree and surface panes move together, and the source title
/// tab is removed.
pub fn apply_place_tab_on_pane<S: SessionRef + Clone>(
    tabs: &[WorkspaceTab],
    sessions: &[S],
    source_tab_id: &str,
    target_id: &str,
    edge: PaneEdge,
    replace_target: bool,
) -> Option<PlacedTab<S>> {
    let source = tabs.iter().find(|tab| tab.id == source_tab_id)?;
    let target = tabs.iter().find(|tab| has_leaf(&tab.layout, target_id))?;
    if source.id == target.id {
        return None;
    }

    let target_ids: HashSet<String> = leaf_ids(&target.layout).into_iter().collect();
    if leaf_ids(&source.layout)
        .iter()
        .any(|id| target_ids.contains(id))
    {
        return None;
    }

    let layout = if replace_target {
        replace_pane_with_layout(&target.layout, target_id, &source.layout)
    } else {
        place_layout(&target.layout, &source.layout, target_id, edge)
    };
    let mut editor_panes = target.editor_panes.clone();
    editor_panes.extend(source.editor_panes.iter().cloned());
    let mut terminal_panes = target.terminal_panes.clone();
    terminal_panes.extend(source.terminal_panes.iter().cloned());
    let merged = WorkspaceTab {
        layout,
        focused_id: source.focused_id.clone(),
        editor_panes,
        terminal_panes,
        diff_focused: Some(false),
        ..target.clone()
    };
    let next_tabs = tabs
        .iter()
        .filter(|tab| tab.id != source.id)
        .map(|tab| {
            if tab.id == target.id {
                merged.clone()
            } else {
                tab.clone()
            }
        })
        .collect();
    let next_sessions = if replace_target {
        sessions
            .iter()
            .filter(|session| session.session_id() != target_id)
            .cloned()
            .collect()
    } else {
        sessions.to_vec()
    };

    Some(PlacedTab {
        tabs: next_tabs,
        sessions: next_sessions,
        active_tab_id: target.id.clone(),
        focused_id: source.focused_id.clone(),
    })
}

/// The result of `applyDetachPaneToTab`.
#[derive(Debug, Clone, PartialEq)]
pub struct DetachedPane {
    pub tabs: Vec<WorkspaceTab>,
    pub active_tab_id: String,
    pub focused_id: String,
}

/// `applyDetachPaneToTab`: extract one leaf from a split workspace and turn
/// it into a title tab beside `target_tab_id`. Surface pane metadata moves
/// with editor and terminal leaves. `create_tab_id` defaults to a random
/// UUID when `None`.
pub fn apply_detach_pane_to_tab(
    tabs: &[WorkspaceTab],
    pane_id: &str,
    target_tab_id: &str,
    position: PanePlace,
    create_tab_id: Option<&dyn Fn() -> String>,
) -> Option<DetachedPane> {
    let source_index = tabs.iter().position(|tab| has_leaf(&tab.layout, pane_id))?;
    if leaf_ids(&tabs[source_index].layout).len() < 2 {
        return None;
    }

    let source = &tabs[source_index];
    let remaining = close_leaf(source, pane_id)?;

    let editor_pane = source.editor_panes.iter().find(|pane| pane.id == pane_id);
    let terminal_pane = source.terminal_panes.iter().find(|pane| pane.id == pane_id);
    let next_source = WorkspaceTab {
        editor_panes: source
            .editor_panes
            .iter()
            .filter(|pane| pane.id != pane_id)
            .cloned()
            .collect(),
        terminal_panes: source
            .terminal_panes
            .iter()
            .filter(|pane| pane.id != pane_id)
            .cloned()
            .collect(),
        ..remaining
    };
    let without_detached: Vec<WorkspaceTab> = tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            if index == source_index {
                next_source.clone()
            } else {
                tab.clone()
            }
        })
        .collect();
    let target_index = without_detached
        .iter()
        .position(|tab| tab.id == target_tab_id)?;

    let insert_at = target_index + usize::from(position == PanePlace::After);
    let source_group = source.group_id.as_deref().filter(|group| !group.is_empty());
    let keeps_source_group = source_group.is_some_and(|group| {
        insert_at
            .checked_sub(1)
            .and_then(|before| without_detached.get(before))
            .is_some_and(|tab| tab.group_id.as_deref() == Some(group))
            || without_detached
                .get(insert_at)
                .is_some_and(|tab| tab.group_id.as_deref() == Some(group))
    });
    let tab_id = match create_tab_id {
        Some(create) => create(),
        None => random_uuid(),
    };
    let detached = WorkspaceTab {
        editor_panes: editor_pane.into_iter().cloned().collect(),
        terminal_panes: terminal_pane.into_iter().cloned().collect(),
        diff_open: Some(false),
        diff_focused: Some(false),
        group_id: if keeps_source_group {
            source.group_id.clone()
        } else {
            None
        },
        ..WorkspaceTab::new(tab_id, leaf(pane_id), pane_id)
    };
    let active_tab_id = detached.id.clone();
    let mut next_tabs = without_detached;
    next_tabs.insert(insert_at, detached);
    Some(DetachedPane {
        tabs: next_tabs,
        active_tab_id,
        focused_id: pane_id.to_string(),
    })
}

/// `isGroupableProject`.
pub fn is_groupable_project(project: Option<&str>) -> bool {
    project.is_some_and(|project| !project.is_empty() && project != "~")
}

/// `replaceGroupInTabOrder`: swap a contiguous slice of ids.
pub fn replace_group_in_tab_order(
    all_ids: &[String],
    start_index: usize,
    length: usize,
    new_group_ids: &[String],
) -> Vec<String> {
    let mut next = all_ids.to_vec();
    let start = start_index.min(next.len());
    let end = start.saturating_add(length).min(next.len());
    next.splice(start..end, new_group_ids.iter().cloned());
    next
}

#[cfg(test)]
#[path = "workspace_tab_groups_tests.rs"]
mod tests;
