//! The full Inbox page and its routes into Settings and chat sessions.
use crate::adapters::inbox::InboxAdapter;
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, Render, Subscription, Window,
};
use monocode_app::bridge::shell::{ShellPage, ShellRequest, ShellRequests};
use monocode_view_inbox::data::{InboxSource, ListedItem};
use monocode_view_inbox::list::view::{InboxView, InboxViewConfig, InboxViewEvent};
use std::rc::Rc;

struct InboxWindow {
    view: Entity<InboxView>,
    _subscription: Subscription,
}
impl Render for InboxWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.view.clone()
    }
}
fn page_entity(window: &mut Window, cx: &mut App) -> Option<Entity<InboxWindow>> {
    crate::slots::cached_view("inbox", window, cx, |window, cx| {
        let workspace = crate::slots::window_workspace_for(window, cx)?;
        let list = InboxAdapter::list(workspace.clone(), cx);
        let services = Rc::new(InboxAdapter::new(workspace.clone(), cx));
        Some(
            cx.new(|cx| {
                let view = cx.new(|cx| {
                    InboxView::new(
                        services,
                        list,
                        InboxViewConfig {
                            beside_rail: true,
                            can_close: true,
                            can_toggle_sidebar: true,
                            can_start: true,
                            can_repair: true,
                            ..Default::default()
                        },
                        window,
                        cx,
                    )
                });
                let subscription = cx.subscribe_in(
                    &view,
                    window,
                    move |_, _, event: &InboxViewEvent, window, cx| match event {
                        InboxViewEvent::Close => {
                            ShellRequests::send(ShellRequest::ClosePage(ShellPage::Inbox), cx)
                        }
                        InboxViewEvent::ToggleSidebar => window
                            .dispatch_action(Box::new(crate::shell::keymap::ToggleSidebar), cx),
                        InboxViewEvent::OpenIntegrations(source) => {
                            open_integrations(*source, window, cx)
                        }
                        InboxViewEvent::OpenSession(id) => {
                            workspace
                                .update(cx, |w, cx| w.open_session(id, cx))
                                .detach();
                            ShellRequests::send(ShellRequest::ClosePage(ShellPage::Inbox), cx);
                        }
                    },
                );
                InboxWindow {
                    view,
                    _subscription: subscription,
                }
            })
            .into(),
        )
    })?
    .downcast()
    .ok()
}
pub fn page(window: &mut Window, cx: &mut App) -> Option<AnyView> {
    page_entity(window, cx).map(Into::into)
}
pub fn select_item(item: &ListedItem, window: &mut Window, cx: &mut App) {
    if let Some(page) = page_entity(window, cx) {
        let view = page.read(cx).view.clone();
        view.update(cx, |view, cx| view.select(item, window, cx));
    }
    ShellRequests::send(ShellRequest::OpenPage(ShellPage::Inbox), cx);
}
fn open_integrations(source: InboxSource, window: &mut Window, cx: &mut App) {
    let kv = &monocode_app::boot::AppServices::global(cx).kv;
    monocode_settings::settings_store::save_settings_section(
        kv,
        monocode_core::settings::SettingsSectionId::Inbox,
    );
    super::settings::reveal_integration(source, window, cx);
    ShellRequests::send(ShellRequest::OpenPage(ShellPage::Settings), cx);
}
