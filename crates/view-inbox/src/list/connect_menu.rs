//! Port of src/features/inbox/ui/InboxConnectMenu.tsx: the "Add connection"
//! menu listing the sources that are not connected.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};
use monocode_ui::widgets::popover_frame;
use monocode_ui::{Theme, UiStyled as _, u};

use crate::data::{Action, InboxSource, ValueAction};
use crate::model::inbox_source_label;
use crate::style::{provider_mark, section_label};

const WIDTH: f32 = 188.;

/// `InboxConnectMenu`. `on_connect` runs after the menu closes.
pub fn inbox_connect_menu(
    sources: &[InboxSource],
    animate: bool,
    on_connect: ValueAction<InboxSource>,
    on_close: Action,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let dismiss = on_close.clone();
    let mut list = div()
        .flex()
        .flex_col()
        .p(u(4.))
        .on_mouse_down_out(move |_, window, cx| dismiss(window, cx))
        .child(div().pb(u(4.)).child(section_label("Not connected", cx)));
    for (index, source) in sources.iter().copied().enumerate() {
        let hover = theme.content(0.05);
        let connect = on_connect.clone();
        let close = on_close.clone();
        list = list.child(
            div()
                .id(ElementId::NamedInteger(
                    "inbox-connect".into(),
                    index as u64,
                ))
                .flex()
                .h(u(28.))
                .w_full()
                .items_center()
                .gap(u(8.))
                .rounded(u(theme.radius.lg))
                .px(u(8.))
                .text_px(theme.text.body)
                .leading(theme.leading.none)
                .text_color(theme.colors.content)
                .hover(move |s| s.bg(hover))
                .on_click(move |_, window, cx| {
                    close(window, cx);
                    connect(source, window, cx);
                })
                .child(provider_mark(source, 14., theme.colors.content))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(format!("Connect {}", inbox_source_label(source))),
                ),
        );
    }
    popover_frame("inbox-connect")
        .width(WIDTH)
        .animate(animate)
        .child(list)
        .into_any_element()
}
