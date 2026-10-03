//! Port of src/features/projects/model/projectOpenRun.ts: what opening a set
//! of folders does, decided in one pass.
//!
//! The pane-return rules themselves are `monocode_layout::project_return`
//! (projectReturn.ts), which the layout crate already ports.

use monocode_core::Session;
use monocode_core::models::ModelEnv;
use monocode_core::session::new_session_for_project;
use monocode_layout::layout::{WorkspaceTab, new_tab};
use monocode_layout::project_return::{
    ProjectReturnDecision, ProjectReturnMemory, plan_project_return,
};

use super::recents::{looks_like_project, normalize_project_path};

/// `ProjectOpenStep`.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectOpenStep {
    Keep {
        path: String,
    },
    Activate {
        path: String,
        tab_id: String,
        pane_id: Option<String>,
    },
    ReuseBlank {
        path: String,
        session_id: String,
    },
    Create {
        path: String,
        session: Box<Session>,
        tab: Box<WorkspaceTab>,
        /// The tab the new one sits beside: the one created before it in
        /// this run.
        beside_tab_id: Option<String>,
    },
}

impl ProjectOpenStep {
    /// The folder this step opens.
    pub fn path(&self) -> &str {
        match self {
            ProjectOpenStep::Keep { path }
            | ProjectOpenStep::Activate { path, .. }
            | ProjectOpenStep::ReuseBlank { path, .. }
            | ProjectOpenStep::Create { path, .. } => path,
        }
    }
}

/// `planProjectOpenRun`: every step is planned against the workspace plus
/// what earlier steps in the same run produced, so the caller can commit the
/// whole run at once. Planning each folder on its own would reuse the same
/// blank session twice, open duplicate tabs for a project already open, and
/// insert every tab beside the same anchor.
pub fn plan_project_open_run(
    env: &ModelEnv<'_>,
    memory: &ProjectReturnMemory,
    tabs: &[WorkspaceTab],
    sessions: &[Session],
    active_tab_id: &str,
    paths: &[String],
) -> Vec<ProjectOpenStep> {
    let active_tab = tabs.iter().find(|tab| tab.id == active_tab_id);
    let seed = active_tab
        .and_then(|tab| sessions.iter().find(|session| session.id == tab.focused_id))
        .or_else(|| sessions.first())
        .cloned();

    let mut steps = Vec::new();
    let mut open_tabs = tabs.to_vec();
    let mut open_sessions = sessions.to_vec();
    let mut beside_tab_id = Some(active_tab_id.to_string());
    let mut blank_reused = false;

    for path in paths {
        let normalized = normalize_project_path(path);
        if !looks_like_project(&normalized) {
            continue;
        }

        let decision = plan_project_return(
            memory,
            &open_tabs,
            &open_sessions,
            active_tab_id,
            &normalized,
        );
        match decision {
            ProjectReturnDecision::Keep => {
                steps.push(ProjectOpenStep::Keep { path: normalized });
                continue;
            }
            ProjectReturnDecision::Activate { tab_id, pane_id } => {
                steps.push(ProjectOpenStep::Activate {
                    path: normalized,
                    tab_id,
                    pane_id,
                });
                continue;
            }
            // Only the first folder can take the blank session; the rest
            // would overwrite it in turn.
            ProjectReturnDecision::ReuseBlank { session_id } if !blank_reused => {
                blank_reused = true;
                for session in &mut open_sessions {
                    if session.id == session_id {
                        session.cwd = normalized.clone();
                    }
                }
                steps.push(ProjectOpenStep::ReuseBlank {
                    path: normalized,
                    session_id,
                });
                continue;
            }
            _ => {}
        }

        let session = new_session_for_project(
            env,
            uuid::Uuid::new_v4().to_string(),
            seed.as_ref(),
            &normalized,
        );
        let tab = new_tab(&session.id);
        open_sessions.push(session.clone());
        open_tabs.push(tab.clone());
        let tab = Box::new(tab);
        let next_beside = Some(tab.id.clone());
        steps.push(ProjectOpenStep::Create {
            path: normalized,
            session: Box::new(session),
            tab,
            beside_tab_id: beside_tab_id.take(),
        });
        beside_tab_id = next_beside;
    }

    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;
    use monocode_core::block::{Block, BlockRole};
    use monocode_core::models::{HarnessAvailability, ModelCatalog, ModelPrefs};
    use monocode_core::project_providers::ProjectProviders;
    use monocode_core::session::new_session;

    struct Fixture {
        catalog: ModelCatalog,
        prefs: ModelPrefs,
        availability: HarnessAvailability,
        projects: ProjectProviders,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                catalog: ModelCatalog::new(),
                prefs: ModelPrefs::default(),
                availability: HarnessAvailability::default(),
                projects: ProjectProviders::default(),
            }
        }

        fn env(&self) -> ModelEnv<'_> {
            ModelEnv {
                catalog: &self.catalog,
                prefs: &self.prefs,
                availability: &self.availability,
                projects: &self.projects,
            }
        }

        fn chat(&self, id: &str, cwd: &str, harness: HarnessId) -> Session {
            let mut session = new_session(&self.env(), id, harness, cwd, None, None, None);
            session.blocks = vec![Block::new(format!("{id}-user"), BlockRole::User, id)];
            session
        }

        fn blank(&self, id: &str, cwd: &str) -> Session {
            new_session(&self.env(), id, HarnessId::Cursor, cwd, None, None, None)
        }
    }

    struct Workspace {
        memory: ProjectReturnMemory,
        sessions: Vec<Session>,
        tabs: Vec<WorkspaceTab>,
        active_tab_id: String,
    }

    /// Two projects open, `/beta` active; `/alpha` is first in the list.
    fn workspace(sessions: Vec<Session>) -> Workspace {
        let tabs: Vec<WorkspaceTab> = sessions
            .iter()
            .map(|session| WorkspaceTab {
                id: format!("tab-{}", session.id),
                ..new_tab(&session.id)
            })
            .collect();
        let active_tab_id = tabs.last().map(|tab| tab.id.clone()).unwrap_or_default();
        Workspace {
            memory: ProjectReturnMemory::new(),
            sessions,
            tabs,
            active_tab_id,
        }
    }

    fn default_workspace(f: &Fixture) -> Workspace {
        workspace(vec![
            f.chat("a1", "/alpha", HarnessId::Codex),
            f.chat("b1", "/beta", HarnessId::Cursor),
        ])
    }

    fn plan(f: &Fixture, state: &Workspace, paths: &[&str]) -> Vec<ProjectOpenStep> {
        let paths: Vec<String> = paths.iter().map(|path| path.to_string()).collect();
        plan_project_open_run(
            &f.env(),
            &state.memory,
            &state.tabs,
            &state.sessions,
            &state.active_tab_id,
            &paths,
        )
    }

    struct Created<'a> {
        session: &'a Session,
        tab: &'a WorkspaceTab,
        beside_tab_id: Option<&'a str>,
    }

    fn creates(steps: &[ProjectOpenStep]) -> Vec<Created<'_>> {
        steps
            .iter()
            .filter_map(|step| match step {
                ProjectOpenStep::Create {
                    session,
                    tab,
                    beside_tab_id,
                    ..
                } => Some(Created {
                    session,
                    tab,
                    beside_tab_id: beside_tab_id.as_deref(),
                }),
                _ => None,
            })
            .collect()
    }

    fn activate(path: &str, tab_id: &str, pane_id: &str) -> ProjectOpenStep {
        ProjectOpenStep::Activate {
            path: path.into(),
            tab_id: tab_id.into(),
            pane_id: Some(pane_id.into()),
        }
    }

    #[test]
    fn gives_every_folder_its_own_session_and_tab_in_selection_order() {
        let f = Fixture::new();
        let state = default_workspace(&f);
        let steps = plan(&f, &state, &["/one", "/two", "/three"]);

        let actions: Vec<(&str, bool)> = steps
            .iter()
            .map(|step| (step.path(), matches!(step, ProjectOpenStep::Create { .. })))
            .collect();
        assert_eq!(actions, [("/one", true), ("/two", true), ("/three", true)]);
        let made = creates(&steps);
        let cwds: Vec<&str> = made.iter().map(|step| step.session.cwd.as_str()).collect();
        assert_eq!(cwds, ["/one", "/two", "/three"]);
        let ids: std::collections::HashSet<&str> =
            made.iter().map(|step| step.session.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
        for step in &made {
            assert_eq!(step.tab.focused_id, step.session.id);
        }
        // Each tab is anchored to the one created before it, not the active tab.
        let anchors: Vec<Option<&str>> = made.iter().map(|step| step.beside_tab_id).collect();
        assert_eq!(
            anchors,
            [
                Some("tab-b1"),
                Some(made[0].tab.id.as_str()),
                Some(made[1].tab.id.as_str())
            ]
        );
    }

    #[test]
    fn activates_a_folder_that_is_already_open_instead_of_creating() {
        let f = Fixture::new();
        let state = default_workspace(&f);
        let steps = plan(&f, &state, &["/alpha", "/two"]);
        assert_eq!(steps[0], activate("/alpha", "tab-a1", "a1"));
        assert_eq!(creates(&steps).len(), 1);
    }

    #[test]
    fn activates_a_folder_the_run_itself_just_opened() {
        let f = Fixture::new();
        let state = default_workspace(&f);
        let steps = plan(&f, &state, &["/two", "/two"]);
        let made = creates(&steps);
        assert_eq!(made.len(), 1);
        assert_eq!(
            steps[1],
            activate("/two", &made[0].tab.id, &made[0].session.id)
        );
    }

    #[test]
    fn seeds_every_new_session_from_the_active_session_not_the_first_one() {
        let f = Fixture::new();
        let state = default_workspace(&f);
        let steps = plan(&f, &state, &["/one", "/two"]);
        let harnesses: Vec<HarnessId> = creates(&steps)
            .iter()
            .map(|step| step.session.harness)
            .collect();
        assert_eq!(harnesses, [HarnessId::Cursor, HarnessId::Cursor]);
    }

    #[test]
    fn reuses_the_active_blank_session_for_the_first_folder_only() {
        let f = Fixture::new();
        let state = workspace(vec![
            f.chat("a1", "/alpha", HarnessId::Codex),
            f.blank("b1", "~"),
        ]);
        let steps = plan(&f, &state, &["/one", "/two"]);
        assert_eq!(
            steps[0],
            ProjectOpenStep::ReuseBlank {
                path: "/one".into(),
                session_id: "b1".into()
            }
        );
        let cwds: Vec<&str> = creates(&steps)
            .iter()
            .map(|step| step.session.cwd.as_str())
            .collect();
        assert_eq!(cwds, ["/two"]);
    }

    #[test]
    fn leaves_a_single_folder_behaving_as_it_did_before_the_run() {
        let f = Fixture::new();
        let state = default_workspace(&f);

        assert_eq!(
            plan(&f, &state, &["/beta"]),
            [ProjectOpenStep::Keep {
                path: "/beta".into()
            }]
        );
        assert_eq!(
            plan(&f, &state, &["/alpha"]),
            [activate("/alpha", "tab-a1", "a1")]
        );

        let steps = plan(&f, &state, &["/one/"]);
        assert_eq!(steps.len(), 1);
        let made = creates(&steps);
        assert_eq!(steps[0].path(), "/one");
        assert_eq!(made[0].beside_tab_id, Some("tab-b1"));

        let on_blank = workspace(vec![f.blank("b1", "~")]);
        assert_eq!(
            plan(&f, &on_blank, &["/one"]),
            [ProjectOpenStep::ReuseBlank {
                path: "/one".into(),
                session_id: "b1".into()
            }]
        );
    }

    #[test]
    fn does_nothing_when_the_dialog_is_dismissed_or_the_path_is_no_project() {
        let f = Fixture::new();
        let state = default_workspace(&f);
        assert!(plan(&f, &state, &[]).is_empty());
        assert!(plan(&f, &state, &["/", "~"]).is_empty());
    }
}
