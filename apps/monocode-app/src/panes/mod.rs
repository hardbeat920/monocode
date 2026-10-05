//! The workspace area and the sidebar tab content: the window's pane tree
//! with session, file, terminal, diff, commit, and plan leaves, and the
//! Inbox, Explorer, and Changes tabs.

pub mod changes;
pub(crate) mod drag;
pub mod explorer;
pub mod inbox_tab;
pub mod standalone;
mod workspace;

use gpui::{AnyView, App, Window};

use crate::shell::SidebarTab;

/// `AppSlots::sidebar_tab`: the content of a sidebar tab other than
/// Sessions.
pub fn sidebar_tab_view(tab: SidebarTab, window: &mut Window, cx: &mut App) -> Option<AnyView> {
    match tab {
        SidebarTab::Sessions => None,
        SidebarTab::Inbox => inbox_tab::view(window, cx),
        SidebarTab::Files => explorer::view(window, cx),
        SidebarTab::Changes => changes::view(window, cx),
    }
}

/// `AppSlots::workspace`: the window's workspace area.
pub fn workspace_area(window: &mut Window, cx: &mut App) -> gpui::Entity<workspace::WorkspaceArea> {
    crate::slots::cached_view("workspace", window, cx, |_, cx| {
        Some(gpui::AppContext::new(cx, |_| workspace::WorkspaceArea::new()).into())
    })
    .expect("the workspace factory always builds")
    .downcast::<workspace::WorkspaceArea>()
    .expect("the workspace slot contains WorkspaceArea")
}

pub fn workspace_view(window: &mut Window, cx: &mut App) -> AnyView {
    workspace_area(window, cx).into()
}
