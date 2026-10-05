//! The sidebar's Inbox cards follow the selected project and provider.
use crate::adapters::inbox::InboxAdapter;
use gpui::{
    AnyView, App, AppContext as _, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, px,
};
use monocode_view_inbox::data::{InboxListData, InboxServices};
use std::rc::Rc;

struct InboxSidebar {
    list: Rc<dyn InboxListData>,
    services: Rc<dyn InboxServices>,
    _subscription: Subscription,
}
impl Render for InboxSidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = monocode_ui::Theme::of(cx);
        let state = self.list.state(cx);
        let items = self.list.visible_items("", cx);
        let list = self.list.clone();
        let mut providers = div().flex().flex_wrap().gap(px(4.));
        for source in state.visible_sources {
            let list = list.clone();
            providers = providers.child(
                div()
                    .id(format!("inbox-source-{source:?}"))
                    .px(px(7.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(10.))
                    .child(monocode_view_inbox::model::inbox_source_label(source))
                    .on_click(move |_, _, cx| list.set_source(source, cx)),
            );
        }
        let refresh = self.list.clone();
        let all = self.list.clone();
        let mut cards = div()
            .id("sidebar-inbox-cards")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if let Some(error) = state.source_error {
            cards = cards.child(
                div()
                    .p(px(12.))
                    .text_size(px(11.))
                    .text_color(theme.colors.danger)
                    .child(error),
            );
        }
        if items.is_empty() {
            cards = cards.child(
                div()
                    .p(px(12.))
                    .text_size(px(11.))
                    .text_color(theme.content(0.50))
                    .child(if state.loading {
                        "Loading inbox..."
                    } else {
                        "No inbox items for this project."
                    }),
            );
        }
        for item in items {
            let selected = item.clone();
            cards = cards.child(
                monocode_view_inbox::list::card::inbox_card(
                    item.key.clone(),
                    item,
                    false,
                    self.services.clone(),
                )
                .on_select(move |_, window, cx| {
                    crate::pages::inbox::select_item(&selected, window, cx)
                }),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .child(
                        div().flex().justify_between().child("Inbox").child(
                            div()
                                .id("sidebar-inbox-refresh")
                                .cursor_pointer()
                                .text_size(px(11.))
                                .child("Refresh")
                                .on_click(move |_, _, cx| refresh.refresh(cx)),
                        ),
                    )
                    .child(providers)
                    .child(
                        div()
                            .id("sidebar-inbox-read")
                            .cursor_pointer()
                            .text_size(px(10.))
                            .text_color(theme.content(0.50))
                            .child("Mark all as read")
                            .on_click(move |_, _, cx| all.mark_source_read(cx)),
                    ),
            )
            .child(cards)
    }
}
pub fn view(window: &mut Window, cx: &mut App) -> Option<AnyView> {
    crate::slots::cached_view("inbox-sidebar", window, cx, |window, cx| {
        let workspace = crate::slots::window_workspace_for(window, cx)?;
        let list = InboxAdapter::list(workspace.clone(), cx);
        let services = Rc::new(InboxAdapter::new(workspace, cx));
        Some(
            cx.new(|cx| {
                let weak = cx.entity().downgrade();
                let subscription = list.subscribe(
                    Box::new(move |cx| {
                        weak.update(cx, |_, cx| cx.notify()).ok();
                    }),
                    cx,
                );
                InboxSidebar {
                    list,
                    services,
                    _subscription: subscription,
                }
            })
            .into(),
        )
    })
}
