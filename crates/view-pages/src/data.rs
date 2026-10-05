//! What every page shares: async data calls, change listeners, and the
//! project appearance (label, logo, mascot, color) that cards and the
//! project picker draw.
//!
//! The React pages read `tabGroups.ts` and `recents.ts` from localStorage.
//! Here they reach both through [`ProjectsData`], which the engine's projects
//! package implements. [`StaticProjects`] is a fixed list for the gallery
//! and the tests.

use std::collections::HashMap;

use gpui::{App, Hsla, Subscription, Task};
use monocode_layout::paths::{project_key, project_name, same_project_path};

use crate::format::looks_like_project;

/// A data call that finishes later. Errors are the messages the UI shows.
pub type DataTask<T> = Task<Result<T, String>>;

/// A change listener. Implementations call it after their own update, so it
/// may read the data again.
pub type Listener = Box<dyn Fn(&mut App)>;

/// How a project looks in a card or a picker row: `resolveTabGroupLabel`,
/// `resolveTabGroupLogo`, `resolveTabGroupMascot`, and
/// `resolveTabGroupColor` for one project path.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectMark {
    /// The group label, else the folder name.
    pub label: String,
    /// `projectName(cwd)`: the seed the mascot hash and default color use.
    pub seed: String,
    /// A logo image path, when the user picked one.
    pub logo: Option<String>,
    /// An explicit mascot pick.
    pub mascot: Option<String>,
    /// The group color. `None` draws in the surrounding text color.
    pub color: Option<Hsla>,
}

impl ProjectMark {
    /// A mark with no saved appearance: the folder name and a hashed mascot.
    pub fn plain(cwd: &str) -> Self {
        let seed = project_name(cwd);
        Self {
            label: seed.clone(),
            seed,
            logo: None,
            mascot: None,
            color: None,
        }
    }
}

/// The projects a page can show and pick.
pub trait ProjectsData: 'static {
    /// The appearance of `cwd`.
    fn mark(&self, cwd: &str, cx: &App) -> ProjectMark;
    /// `projectRailItems(recents, rail_cwd)`: the rail's projects in rail
    /// order, pinned first, as the project picker offers them.
    fn rail_projects(&self, rail_cwd: &str, cx: &App) -> Vec<String>;
    /// `subscribeProjectPathsChanged` and the logo change event.
    fn subscribe(&self, _listener: Listener, _cx: &mut App) -> Subscription {
        Subscription::new(|| {})
    }
}

/// A fixed project list with optional saved appearance.
#[derive(Debug, Clone, Default)]
pub struct StaticProjects {
    pub projects: Vec<String>,
    pub marks: HashMap<String, ProjectMark>,
}

impl StaticProjects {
    pub fn new(projects: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            projects: projects.into_iter().map(Into::into).collect(),
            marks: HashMap::new(),
        }
    }

    pub fn with_mark(mut self, cwd: &str, mark: ProjectMark) -> Self {
        self.marks.insert(project_key(cwd), mark);
        self
    }
}

impl ProjectsData for StaticProjects {
    fn mark(&self, cwd: &str, _: &App) -> ProjectMark {
        self.marks
            .get(&project_key(cwd))
            .cloned()
            .unwrap_or_else(|| ProjectMark::plain(cwd))
    }

    fn rail_projects(&self, rail_cwd: &str, _: &App) -> Vec<String> {
        let mut projects: Vec<String> = self
            .projects
            .iter()
            .filter(|path| looks_like_project(path))
            .cloned()
            .collect();
        if looks_like_project(rail_cwd)
            && !projects
                .iter()
                .any(|path| same_project_path(path, rail_cwd))
        {
            projects.push(rail_cwd.to_string());
        }
        projects
    }
}
