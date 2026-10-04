//! The installed update card in the full project rail.

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use monocode_ui::widgets::icon_button;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

pub fn card(version: &str, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    let version = version.to_owned();
    gpui::div()
        .flex()
        .items_start()
        .gap(u(6.))
        .rounded(u(8.))
        .bg(theme.content(0.12))
        .child(
            gpui::div()
                .id("installed-update-open")
                .flex()
                .flex_1()
                .min_w_0()
                .items_start()
                .gap(u(8.))
                .p(u(8.))
                .cursor_pointer()
                .child(
                    icon(IconName::ArrowDownCircle)
                        .size(u(16.))
                        .text_color(theme.colors.accent),
                )
                .child(
                    gpui::div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .child(
                            gpui::div()
                                .text_px(12.)
                                .medium()
                                .truncate()
                                .child(format!("Updated to {version}")),
                        )
                        .child(
                            gpui::div()
                                .text_px(11.)
                                .text_color(theme.content(0.5))
                                .child("What's new"),
                        ),
                )
                .on_click(move |_, window, cx| super::whats_new::open(&version, window, cx)),
        )
        .child(
            icon_button("installed-update-dismiss", IconName::X)
                .size(24.)
                .icon_size(12.)
                .tooltip("Dismiss update notification")
                .on_click(|_, _, cx| super::sidebar_update::dismiss(cx)),
        )
        .into_any_element()
}
