//! App-wide search with file, project, and transcript navigation.
use crate::adapters::{
    projects_data::AppProjectsData,
    search::{AppSearchData, SearchReveals, install_files},
};
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, Render, Subscription, Window,
};
use monocode_app::bridge::shell::{ShellPage, ShellRequest, ShellRequests};
use monocode_engine::{history::HistoryPackage, projects::ProjectsGlobal};
use monocode_view_pages::search::SearchView;
use std::rc::Rc;

struct SearchPage {
    view: Entity<SearchView>,
    open: bool,
    _subscription: Subscription,
}

impl Render for SearchPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.view.clone()
    }
}

pub fn page(window: &mut Window, cx: &mut App) -> Option<AnyView> {
    crate::slots::cached_view("search", window, cx, |window, cx| {
        SearchReveals::init(cx);
        install_files(cx);
        let workspace = crate::slots::window_workspace_for(window, cx)?;
        let cwd = workspace.read(cx).sidebar_cwd(cx);
        let recents = ProjectsGlobal::global(cx)
            .projects
            .read(cx)
            .recents()
            .iter()
            .map(|project| project.path.clone())
            .collect();
        let search = HistoryPackage::try_global(cx)?.search.clone();
        let data = Rc::new(AppSearchData {
            search: search.clone(),
            workspace: workspace.clone(),
        });
        let projects = Rc::new(AppProjectsData::new(cx)?);
        Some(
            cx.new(|cx| {
                let view = cx.new(|cx| {
                    SearchView::new(data, projects, window, cx).on_close(|_, cx| {
                        ShellRequests::send(ShellRequest::ClosePage(ShellPage::Search), cx)
                    })
                });
                view.update(cx, |view, cx| view.open(&cwd, recents, window, cx));
                let subscription = cx.observe_in(
                    &search,
                    window,
                    move |this: &mut SearchPage, search, window, cx| {
                        let open = search.read(cx).is_open();
                        let was_open = this.open;
                        this.open = open;
                        if open && !was_open {
                            let cwd = workspace.read(cx).sidebar_cwd(cx);
                            let recents = ProjectsGlobal::global(cx)
                                .projects
                                .read(cx)
                                .recents()
                                .iter()
                                .map(|project| project.path.clone())
                                .collect();
                            this.view
                                .update(cx, |view, cx| view.open(&cwd, recents, window, cx));
                        }
                    },
                );
                SearchPage {
                    view,
                    open: true,
                    _subscription: subscription,
                }
            })
            .into(),
        )
    })
}
