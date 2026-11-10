//! Dismissible app errors use the same glass card as pending approvals.
use crate::theme::*;
use gpui::{Div, FontWeight, Hsla, Role, Stateful, div, prelude::*, px, rgb};

pub(crate) fn card(id: &'static str, message: String, dismiss: impl IntoElement) -> Stateful<Div> {
    let rose: Hsla = rgb(0xf43f5e).into();
    let mut border = rose;
    border.a = 0.3;
    let mut tint = rose;
    tint.a = 0.1;
    super::surface::toast_frame()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Alert)
        .border_color(border)
        .bg(tint)
        .child(
            div()
                .px(px(14.))
                .py_3()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icon("circle-alert", 16.).text_color(rose))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Error"),
                        )
                        .child(dismiss),
                )
                .child(
                    div()
                        .id((id, 0usize))
                        .min_w_0()
                        .max_h(px(220.))
                        .overflow_y_scroll()
                        .text_size(px(12.))
                        .line_height(px(19.5))
                        .text_color(content(0.8))
                        .child(message),
                ),
        )
}
