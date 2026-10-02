//! Port of src/features/projects/model/projectReturn.ts: which pane to show
//! when the user returns to a project.

use std::collections::HashMap;

use monocode_core::Session;
use monocode_core::block::BlockRole;
use monocode_core::paths::path_key;

use crate::layout::{WorkspaceTab, has_leaf, leaf_ids};
use crate::paths::same_project_path;
use crate::session_ref::SessionRef;

/// `ProjectReturnMemory`: project path key to the pane or tab id last used
/// there. Keeps insertion order like a JavaScript `Map`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectReturnMemory {
    entries: Vec<(String, String)>,
}

impl ProjectReturnMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// `memory.get(project)`.
    pub fn get(&self, project: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == project)
            .map(|(_, id)| id.as_str())
    }

    /// `memory.has(project)`.
    pub fn has(&self, project: &str) -> bool {
        self.get(project).is_some()
    }

    /// `memory.set(project, id)`: replace in place, or append.
    pub fn set(&mut self, project: impl Into<String>, id: impl Into<String>) {
        let project = project.into();
        let id = id.into();
        match self.entries.iter_mut().find(|(key, _)| *key == project) {
            Some(entry) => entry.1 = id,
            None => self.entries.push((project, id)),
        }
    }

    /// `memory.delete(project)`.
    pub fn delete(&mut self, project: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(key, _)| key != project);
        self.entries.len() != before
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(key, id)| (key.as_str(), id.as_str()))
    }
}

impl<K: Into<String>, V: Into<String>> FromIterator<(K, V)> for ProjectReturnMemory {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut memory = Self::new();
        for (key, id) in iter {
            memory.set(key, id);
        }
        memory
    }
}

/// `ProjectReturnDecision`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectReturnDecision {
    Keep,
    Activate {
        tab_id: String,
        pane_id: Option<String>,
    },
    ReuseBlank {
        session_id: String,
    },
    Create,
}

/// `isBlankSession`: idle, with no user turn yet.
pub fn is_blank_session(session: Option<&Session>) -> bool {
    let Some(session) = session else {
        return false;
    };
    if session.is_busy() {
        return false;
    }
    !session
        .blocks
        .iter()
        .any(|block| block.role == BlockRole::User)
}

/// `paneProjects`: pane id to the project path key it shows.
fn pane_projects<S: SessionRef>(tabs: &[WorkspaceTab], sessions: &[S]) -> HashMap<String, String> {
    let mut cwd_by_session: HashMap<&str, &str> = HashMap::new();
    for session in sessions {
        cwd_by_session.insert(session.session_id(), session.session_cwd());
    }
    let mut result = HashMap::new();

    for tab in tabs {
        for pane_id in leaf_ids(&tab.layout) {
            if let Some(cwd) = cwd_by_session.get(pane_id.as_str())
                && !cwd.is_empty()
                && *cwd != "~"
            {
                result.insert(pane_id, path_key(cwd));
            }
        }
        for pane in tab.editor_panes.iter().chain(&tab.terminal_panes) {
            let active = pane
                .files
                .iter()
                .find(|file| file.id == pane.active_file_id);
            if let Some(active) = active
                && active.cwd != "~"
            {
                result.insert(
                    pane.id.clone(),
                    path_key(active.project_cwd.as_deref().unwrap_or(&active.cwd)),
                );
            }
        }
    }
    result
}

/// `paneBelongsToProject`.
fn pane_belongs_to_project(
    pane_id: &str,
    target: &str,
    pane_by_id: &HashMap<String, String>,
) -> bool {
    pane_by_id
        .get(pane_id)
        .is_some_and(|project| !project.is_empty() && same_project_path(project, target))
}

/// `paneForProjectInTab`.
fn pane_for_project_in_tab(
    tab: &WorkspaceTab,
    target: &str,
    pane_by_id: &HashMap<String, String>,
) -> Option<String> {
    let shows = |id: &str| pane_by_id.get(id).map(String::as_str) == Some(target);
    leaf_ids(&tab.layout)
        .into_iter()
        .find(|id| shows(id))
        .or_else(|| {
            tab.editor_panes
                .iter()
                .find(|pane| shows(&pane.id))
                .map(|pane| pane.id.clone())
        })
        .or_else(|| {
            tab.terminal_panes
                .iter()
                .find(|pane| shows(&pane.id))
                .map(|pane| pane.id.clone())
        })
}

/// `tabContainsPane`.
fn tab_contains_pane(tab: &WorkspaceTab, pane_id: &str) -> bool {
    has_leaf(&tab.layout, pane_id)
        || tab.editor_panes.iter().any(|pane| pane.id == pane_id)
        || tab.terminal_panes.iter().any(|pane| pane.id == pane_id)
}

/// `reconcileProjectReturn`: keep the remembered panes that still show their
/// project, map remembered tab ids to their focused pane, and record the
/// active tab's focused pane. Returns an equal copy of `memory` when nothing
/// changed.
pub fn reconcile_project_return<S: SessionRef>(
    memory: &ProjectReturnMemory,
    tabs: &[WorkspaceTab],
    sessions: &[S],
    active_tab_id: &str,
) -> ProjectReturnMemory {
    let active_tab = tabs.iter().find(|tab| tab.id == active_tab_id);
    let by_pane = pane_projects(tabs, sessions);
    let mut by_tab: HashMap<&str, &str> = HashMap::new();
    for tab in tabs {
        by_tab.insert(&tab.id, &tab.focused_id);
    }

    let mut next = ProjectReturnMemory::new();
    for (project, saved) in memory.iter() {
        if by_pane.get(saved).map(String::as_str) == Some(project) {
            next.set(project, saved);
            continue;
        }
        let Some(focused) = by_tab.get(saved).filter(|focused| !focused.is_empty()) else {
            continue;
        };
        if pane_belongs_to_project(focused, project, &by_pane) {
            next.set(project, *focused);
        }
    }

    let active_pane_id = active_tab
        .map(|tab| tab.focused_id.as_str())
        .filter(|id| !id.is_empty());
    if let Some(active_pane_id) = active_pane_id
        && let Some(active_project) = by_pane
            .get(active_pane_id)
            .filter(|project| !project.is_empty())
    {
        next.set(active_project.clone(), active_pane_id);
    }

    if next.len() == memory.len()
        && next
            .iter()
            .all(|(project, id)| memory.get(project) == Some(id))
    {
        return memory.clone();
    }
    next
}

/// `planProjectReturn`: what to show when the user switches to `project_path`.
pub fn plan_project_return(
    memory: &ProjectReturnMemory,
    tabs: &[WorkspaceTab],
    sessions: &[Session],
    active_tab_id: &str,
    project_path: &str,
) -> ProjectReturnDecision {
    let target = path_key(project_path);
    let by_pane = pane_projects(tabs, sessions);
    let active = tabs.iter().find(|tab| tab.id == active_tab_id);

    if let Some(active) = active
        && !active.focused_id.is_empty()
        && pane_belongs_to_project(&active.focused_id, &target, &by_pane)
    {
        return ProjectReturnDecision::Keep;
    }

    if let Some(remembered) = memory.get(&target).filter(|id| !id.is_empty()) {
        let remembered_tab = tabs.iter().find(|tab| tab_contains_pane(tab, remembered));
        if let Some(remembered_tab) = remembered_tab
            && pane_belongs_to_project(remembered, &target, &by_pane)
        {
            return ProjectReturnDecision::Activate {
                tab_id: remembered_tab.id.clone(),
                pane_id: Some(remembered.to_string()),
            };
        }
    }

    for tab in tabs {
        if let Some(pane_id) = pane_for_project_in_tab(tab, &target, &by_pane) {
            return ProjectReturnDecision::Activate {
                tab_id: tab.id.clone(),
                pane_id: Some(pane_id),
            };
        }
    }

    let current = active
        .filter(|active| !active.focused_id.is_empty())
        .and_then(|active| {
            sessions
                .iter()
                .find(|session| session.id == active.focused_id)
        });
    match current {
        Some(current) if is_blank_session(Some(current)) => ProjectReturnDecision::ReuseBlank {
            session_id: current.id.clone(),
        },
        _ => ProjectReturnDecision::Create,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{EditorPane, SplitDir, new_file_tab, new_tab, split_pane};
    use monocode_core::HarnessId;
    use monocode_core::block::Block;

    fn chat(id: &str, cwd: &str, with_turn: bool) -> Session {
        let mut session = Session::blank(id, HarnessId::Cursor, "", cwd);
        if with_turn {
            session
                .blocks
                .push(Block::new("u1", BlockRole::User, "hello"));
        }
        session
    }

    fn tab(id: &str, session_id: &str) -> WorkspaceTab {
        WorkspaceTab {
            id: id.into(),
            ..new_tab(session_id)
        }
    }

    #[test]
    fn keeps_the_active_pane_when_it_already_shows_the_project() {
        let tabs = [tab("t1", "a")];
        let sessions = [chat("a", "/alpha", true)];
        assert_eq!(
            plan_project_return(
                &ProjectReturnMemory::new(),
                &tabs,
                &sessions,
                "t1",
                "/alpha/"
            ),
            ProjectReturnDecision::Keep
        );
    }

    #[test]
    fn activates_the_remembered_pane_then_any_pane_of_the_project() {
        let row = WorkspaceTab {
            layout: split_pane(&new_tab("b1").layout, "b1", SplitDir::Right, "b2"),
            ..tab("t2", "b1")
        };
        let tabs = [tab("t1", "a"), row];
        let sessions = [
            chat("a", "/alpha", true),
            chat("b1", "/beta", true),
            chat("b2", "/beta", true),
        ];
        let memory: ProjectReturnMemory = [("/beta", "b2")].into_iter().collect();
        assert_eq!(
            plan_project_return(&memory, &tabs, &sessions, "t1", "/beta"),
            ProjectReturnDecision::Activate {
                tab_id: "t2".into(),
                pane_id: Some("b2".into()),
            }
        );
        assert_eq!(
            plan_project_return(&ProjectReturnMemory::new(), &tabs, &sessions, "t1", "/beta"),
            ProjectReturnDecision::Activate {
                tab_id: "t2".into(),
                pane_id: Some("b1".into()),
            }
        );
    }

    #[test]
    fn reuses_a_blank_session_or_creates_one() {
        let tabs = [tab("t1", "blank")];
        assert_eq!(
            plan_project_return(
                &ProjectReturnMemory::new(),
                &tabs,
                &[chat("blank", "~", false)],
                "t1",
                "/gamma"
            ),
            ProjectReturnDecision::ReuseBlank {
                session_id: "blank".into()
            }
        );
        assert_eq!(
            plan_project_return(
                &ProjectReturnMemory::new(),
                &tabs,
                &[chat("blank", "~", true)],
                "t1",
                "/gamma"
            ),
            ProjectReturnDecision::Create
        );
    }

    #[test]
    fn reconcile_maps_tab_ids_to_their_focused_pane_and_records_the_active_pane() {
        let file = new_file_tab("/beta/readme.md", "/beta", false, None, None);
        let editor = WorkspaceTab {
            layout: crate::layout::leaf("e1"),
            focused_id: "e1".into(),
            editor_panes: vec![EditorPane::new("e1", vec![file.clone()], file.id.clone())],
            ..tab("t2", "e1")
        };
        let tabs = [tab("t1", "a"), editor];
        let sessions = [chat("a", "/alpha", true)];
        let memory: ProjectReturnMemory = [("/beta", "t2"), ("/gone", "x")].into_iter().collect();
        let next = reconcile_project_return(&memory, &tabs, &sessions, "t1");
        assert_eq!(
            next.iter().collect::<Vec<_>>(),
            vec![("/beta", "e1"), ("/alpha", "a")]
        );
        let stable: ProjectReturnMemory = [("/alpha", "a")].into_iter().collect();
        assert_eq!(
            reconcile_project_return(&stable, &tabs, &sessions, "t1"),
            stable
        );
    }

    #[test]
    fn memory_keeps_insertion_order_and_updates_in_place() {
        let mut memory = ProjectReturnMemory::new();
        memory.set("/a", "1");
        memory.set("/b", "2");
        memory.set("/a", "3");
        assert_eq!(
            memory.iter().collect::<Vec<_>>(),
            vec![("/a", "3"), ("/b", "2")]
        );
        assert!(memory.delete("/a"));
        assert!(!memory.has("/a"));
    }
}
