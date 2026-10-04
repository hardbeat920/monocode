//! Views the shell embeds but other modules build. Boot code fills these at
//! startup ([`install`]), and the shell reads them when it renders.
//! Factories cache their entities per window, so calling one on every frame
//! returns the same view.
//!
//! The views find the window's `Workspace` through [`window_workspace_for`],
//! which reads the window registry the shell sets once the boot
//! restore finishes. Until then the sidebar tab and page factories return
//! `None`, and the workspace view draws a spinner and picks the workspace up
//! when it appears.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{AnyView, App, Entity, Global, Window, WindowId};
use monocode_app::bridge::ActiveWorkspace;
use monocode_engine::workspace::Workspace;

use crate::shell::SidebarTab;

/// A full-page view that replaces the main column (`*ViewOpen` in App.tsx).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Search,
    Inbox,
    Notes,
    Automations,
    Settings,
}

/// Builds or returns a cached view for a key, or `None` when there is none.
pub type ViewFactory<K> = Rc<dyn Fn(K, &mut Window, &mut App) -> Option<AnyView>>;

/// Builds or returns the cached workspace area (the pane tree) for a window.
pub type WorkspaceFactory = Rc<dyn Fn(&mut Window, &mut App) -> AnyView>;

/// The shell's slots. Unset slots draw the shell's own placeholder.
#[derive(Clone, Default)]
pub struct AppSlots {
    /// Content for every sidebar tab except Sessions, which the shell draws.
    pub sidebar_tab: Option<ViewFactory<SidebarTab>>,
    /// Full-page views.
    pub page: Option<ViewFactory<Page>>,
    /// The workspace area: tabs' pane trees with session, file, and terminal panes.
    pub workspace: Option<WorkspaceFactory>,
}

impl Global for AppSlots {}

impl AppSlots {
    pub fn get(cx: &App) -> AppSlots {
        cx.try_global::<AppSlots>().cloned().unwrap_or_default()
    }
}

/// Fill the slots with the app's views. Call once after boot.
pub fn install(cx: &mut App) {
    crate::adapters::search::SearchReveals::init(cx);
    cx.set_global(AppSlots {
        sidebar_tab: Some(Rc::new(crate::panes::sidebar_tab_view)),
        page: Some(Rc::new(crate::pages::page_view)),
        workspace: Some(Rc::new(crate::panes::workspace_view)),
    });
}

/// The window's workspace, once the shell restored it.
pub fn window_workspace(cx: &App) -> Option<Entity<Workspace>> {
    ActiveWorkspace::get(cx).and_then(|weak| weak.upgrade())
}

#[derive(Default)]
struct WindowWorkspaces(HashMap<WindowId, Entity<Workspace>>);
impl Global for WindowWorkspaces {}

pub fn register_workspace(workspace: Entity<Workspace>, window: &Window, cx: &mut App) {
    cx.default_global::<WindowWorkspaces>()
        .0
        .insert(window.window_handle().window_id(), workspace);
    crate::shell::windows::sync_window_visibility(window, cx);
}

pub fn window_workspace_for(window: &Window, cx: &App) -> Option<Entity<Workspace>> {
    cx.try_global::<WindowWorkspaces>()?
        .0
        .get(&window.window_handle().window_id())
        .cloned()
}

/// Views the factories built, by window and name.
#[derive(Default)]
struct WindowViews(HashMap<(WindowId, &'static str), AnyView>);

impl Global for WindowViews {}

/// The view cached under `key` for this window, built with `build` the
/// first time. A `None` from `build` is not cached, so the next call tries
/// again (for example once the workspace exists).
pub fn cached_view(
    key: &'static str,
    window: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Option<AnyView>,
) -> Option<AnyView> {
    let id = window.window_handle().window_id();
    if let Some(view) = cx
        .try_global::<WindowViews>()
        .and_then(|views| views.0.get(&(id, key)))
    {
        return Some(view.clone());
    }
    let view = build(window, cx)?;
    cx.default_global::<WindowViews>()
        .0
        .insert((id, key), view.clone());
    Some(view)
}

/// Drop every cached view of a closed window.
pub fn forget_window(id: WindowId, cx: &mut App) {
    if cx.has_global::<WindowWorkspaces>() {
        cx.global_mut::<WindowWorkspaces>().0.remove(&id);
    }
    if cx.has_global::<WindowViews>() {
        cx.global_mut::<WindowViews>()
            .0
            .retain(|(window, _), _| *window != id);
    }
}

/// A deleted conversation must disappear from every window that shows it.
pub(crate) fn forget_session_in_windows(session_id: &str, cx: &mut App) {
    let removals = cx
        .try_global::<WindowWorkspaces>()
        .map(|windows| {
            windows
                .0
                .values()
                .filter(|workspace| {
                    workspace.read(cx).tabs().iter().any(|tab| {
                        monocode_layout::leaf_ids(&tab.layout)
                            .iter()
                            .any(|id| id == session_id)
                    })
                })
                .map(|workspace| {
                    (
                        workspace.clone(),
                        workspace.read(cx).plan_session_removal(session_id, cx),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if removals.is_empty() {
        monocode_engine::runtime::Engine::sessions(cx)
            .update(cx, |sessions, cx| sessions.remove(session_id, cx));
    } else {
        for (workspace, removal) in removals {
            workspace.update(cx, |workspace, cx| {
                workspace.apply_session_removal(session_id, removal, cx)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, Context, IntoElement, Render, TestAppContext, div};
    use monocode_engine::runtime::testing::init_test_engine;
    use monocode_engine::workspace::WorkspaceConfig;

    struct TestPage;
    impl Render for TestPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn cached_pages_follow_their_own_window_and_release_on_close(cx: &mut TestAppContext) {
        init_test_engine(cx);
        let first_workspace =
            cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some("/first")), cx));
        let second_workspace =
            cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some("/second")), cx));
        let first = cx.add_window(|_, _| TestPage);
        let second = cx.add_window(|_, _| TestPage);
        let first_page = first
            .update(cx, |_, window, cx| {
                register_workspace(first_workspace.clone(), window, cx);
                assert_eq!(
                    window_workspace_for(window, cx).unwrap().entity_id(),
                    first_workspace.entity_id()
                );
                let page = cached_view("notes", window, cx, |_, cx| {
                    Some(cx.new(|_| TestPage).into())
                })
                .unwrap();
                let retained = cached_view("notes", window, cx, |_, _| {
                    panic!("the same window must retain its page")
                })
                .unwrap();
                assert_eq!(page.entity_id(), retained.entity_id());
                page
            })
            .unwrap();
        let second_page = second
            .update(cx, |_, window, cx| {
                register_workspace(second_workspace.clone(), window, cx);
                assert_eq!(
                    window_workspace_for(window, cx).unwrap().entity_id(),
                    second_workspace.entity_id()
                );
                cached_view("notes", window, cx, |_, cx| {
                    Some(cx.new(|_| TestPage).into())
                })
                .unwrap()
            })
            .unwrap();
        assert_ne!(first_page.entity_id(), second_page.entity_id());
        cx.update(|cx| forget_window(first.window_id(), cx));
        first
            .update(cx, |_, window, cx| {
                assert!(window_workspace_for(window, cx).is_none());
                let replacement = cached_view("notes", window, cx, |_, cx| {
                    Some(cx.new(|_| TestPage).into())
                })
                .unwrap();
                assert_ne!(first_page.entity_id(), replacement.entity_id());
            })
            .unwrap();
        second
            .update(cx, |_, window, cx| {
                assert_eq!(
                    cached_view("notes", window, cx, |_, _| None)
                        .unwrap()
                        .entity_id(),
                    second_page.entity_id()
                );
                assert_eq!(
                    window_workspace_for(window, cx).unwrap().entity_id(),
                    second_workspace.entity_id()
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn page_factory_retries_after_workspace_restore(cx: &mut TestAppContext) {
        init_test_engine(cx);
        let workspace = cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some("/restored")), cx));
        let window = cx.add_window(|_, _| TestPage);
        window
            .update(cx, |_, window, cx| {
                assert!(
                    cached_view("search", window, cx, |window, cx| {
                        window_workspace_for(window, cx)?;
                        Some(cx.new(|_| TestPage).into())
                    })
                    .is_none()
                );
                register_workspace(workspace, window, cx);
                assert!(
                    cached_view("search", window, cx, |window, cx| {
                        window_workspace_for(window, cx)?;
                        Some(cx.new(|_| TestPage).into())
                    })
                    .is_some()
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn deleting_a_shared_session_cleans_each_window_without_changing_other_projects(
        cx: &mut TestAppContext,
    ) {
        init_test_engine(cx);
        let first = cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some("/shared")), cx));
        let id = first.read_with(cx, |workspace, _| {
            workspace.active_tab().unwrap().focused_id.clone()
        });
        let second = cx.new(|cx| {
            let mut config = WorkspaceConfig::fresh(Some("/shared"));
            let tab = monocode_layout::new_tab(&id);
            config.active_tab_id = tab.id.clone();
            config.tabs.push(tab);
            Workspace::new(config, cx)
        });
        let unrelated = cx.new(|cx| Workspace::new(WorkspaceConfig::fresh(Some("/other")), cx));
        let unrelated_tabs = unrelated.read_with(cx, |workspace, _| workspace.tabs().to_vec());
        for workspace in [&first, &second, &unrelated] {
            let window = cx.add_window(|_, _| TestPage);
            window
                .update(cx, |_, window, cx| {
                    register_workspace(workspace.clone(), window, cx)
                })
                .unwrap();
        }
        cx.update(|cx| forget_session_in_windows(&id, cx));
        for workspace in [&first, &second] {
            workspace.read_with(cx, |workspace, cx| {
                assert!(
                    !workspace
                        .tabs()
                        .iter()
                        .any(|tab| monocode_layout::leaf_ids(&tab.layout).contains(&id))
                );
                let replacement = &workspace.active_tab().unwrap().focused_id;
                assert!(
                    monocode_engine::runtime::Engine::sessions(cx)
                        .read(cx)
                        .get(replacement)
                        .is_some()
                );
            });
        }
        assert_eq!(
            unrelated.read_with(cx, |workspace, _| workspace.tabs().to_vec()),
            unrelated_tabs
        );
        cx.read(|cx| {
            assert!(
                monocode_engine::runtime::Engine::sessions(cx)
                    .read(cx)
                    .get(&id)
                    .is_none()
            )
        });
    }
}
