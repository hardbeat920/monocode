//! Project marks and picker ordering from the saved project model.
use gpui::{App, Entity, Rgba, Subscription};
use monocode_engine::projects::{KvAppearanceStore, Projects, ProjectsGlobal};
use monocode_layout::paths::{project_key, project_name};
use monocode_layout::tab_groups::{self, TabGroupAppearance};
use monocode_view_pages::{Listener, ProjectMark, ProjectsData};
use std::cell::RefCell;

pub struct AppProjectsData {
    projects: Entity<Projects>,
    appearance: RefCell<TabGroupAppearance>,
}
impl AppProjectsData {
    pub fn new(cx: &App) -> Option<Self> {
        Some(Self {
            projects: ProjectsGlobal::try_global(cx)?.projects.clone(),
            appearance: RefCell::new(TabGroupAppearance::new()),
        })
    }
}
impl ProjectsData for AppProjectsData {
    fn mark(&self, cwd: &str, cx: &App) -> ProjectMark {
        let projects = self.projects.read(cx);
        let mut store = KvAppearanceStore(projects.kv().clone());
        let mut appearance = self.appearance.borrow_mut();
        let key = project_key(cwd);
        let seed = project_name(cwd);
        let labels = appearance.load_tab_group_labels(&mut store);
        let colors = appearance.load_tab_group_colors(&mut store);
        let custom = appearance.load_tab_group_custom_colors(&mut store);
        let logos = appearance.load_tab_group_logos(&mut store);
        let mascots = appearance.load_tab_group_mascots(&mut store);
        let color =
            tab_groups::resolve_tab_group_color(&key, Some(&colors), Some(&custom), Some(&seed));
        ProjectMark {
            label: tab_groups::resolve_tab_group_label(&key, Some(&labels), &seed),
            seed,
            logo: tab_groups::resolve_tab_group_logo(&key, Some(&logos)),
            mascot: tab_groups::resolve_tab_group_mascot(&key, Some(&mascots)),
            color: Rgba::try_from(color.as_str()).ok().map(Into::into),
        }
    }
    fn rail_projects(&self, rail_cwd: &str, cx: &App) -> Vec<String> {
        self.projects
            .read(cx)
            .rail_items(rail_cwd)
            .into_iter()
            .map(|project| project.path)
            .collect()
    }
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.projects, move |_, cx| listener(cx))
    }
}
