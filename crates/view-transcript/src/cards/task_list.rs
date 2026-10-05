//! Port of src/features/sessions/ui/TaskListPreview.tsx: the agent's task
//! list with a progress pill and one state mark per item.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ElementId, Hsla, IntoElement,
    ParentElement as _, RenderOnce, SharedString, Styled as _, Transformation, Window, div,
    percentage, px,
};
use monocode_core::block::{TaskListItem, TaskListItemStatus};
use monocode_core::task_list::task_list_progress_label;
use monocode_ui::styled::UiStyled as _;
use monocode_ui::{IconName, Theme, icon, u};

use super::style;

/// `<TaskListPreview items explanation />`.
#[derive(IntoElement)]
pub struct TaskListPreview {
    id: ElementId,
    items: Vec<TaskListItem>,
    explanation: Option<SharedString>,
}

pub fn task_list_preview(id: impl Into<ElementId>, items: Vec<TaskListItem>) -> TaskListPreview {
    TaskListPreview {
        id: id.into(),
        items,
        explanation: None,
    }
}

impl TaskListPreview {
    pub fn explanation(mut self, explanation: Option<impl Into<SharedString>>) -> Self {
        self.explanation = explanation
            .map(Into::into)
            .filter(|text: &SharedString| !text.is_empty());
        self
    }
}

/// The text color and strike-through of an item in `status`.
pub fn item_ink(status: TaskListItemStatus, theme: &Theme) -> (Hsla, Option<Hsla>) {
    match status {
        TaskListItemStatus::Completed => (theme.content(0.4), Some(theme.content(0.25))),
        TaskListItemStatus::Cancelled => (theme.content(0.35), Some(theme.content(0.2))),
        TaskListItemStatus::InProgress => (theme.content(0.85), None),
        TaskListItemStatus::Pending => (theme.content(0.6), None),
    }
}

impl RenderOnce for TaskListPreview {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let header = div()
            .flex()
            .items_start()
            .gap(u(8.))
            .border_b(px(1.))
            .border_color(theme.colors.stroke)
            .px(u(10.))
            .py(u(8.))
            .child(
                icon(IconName::ListEnd)
                    .mt(u(2.))
                    .size(u(16.))
                    .flex_none()
                    .text_color(theme.content(0.45)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(u(12.))
                            .child(
                                div()
                                    .font_family(theme.fonts.mono.clone())
                                    .text_px(12.)
                                    .line_height(u(16.))
                                    .medium()
                                    .text_color(theme.content(0.85))
                                    .child("Tasks"),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .rounded_full()
                                    .bg(theme.content(0.07))
                                    .px(u(8.))
                                    .py(u(2.))
                                    .font_family(theme.fonts.mono.clone())
                                    .text_px(10.)
                                    .line_height(u(14.))
                                    .text_color(theme.content(0.5))
                                    .child(task_list_progress_label(&self.items)),
                            ),
                    )
                    .when_some(self.explanation.clone(), |el, explanation| {
                        el.child(
                            div()
                                .mt(u(2.))
                                .line_clamp(2)
                                .font_family(theme.fonts.sans.clone())
                                .text_px(11.5)
                                .line_height(u(16.))
                                .text_color(theme.content(0.5))
                                .child(explanation),
                        )
                    }),
            );
        let mut list = div().py(u(4.)).flex().flex_col();
        for (index, item) in self.items.iter().enumerate() {
            let (ink, strike) = item_ink(item.status, &theme);
            let key = item
                .id
                .clone()
                .unwrap_or_else(|| format!("{index}:{}", item.text));
            list = list.child(
                div()
                    .flex()
                    .items_start()
                    .gap(u(10.))
                    .min_w_0()
                    .px(u(10.))
                    .py(u(6.))
                    .child(task_state(
                        ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), key.into()),
                        item.status,
                        &theme,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(theme.fonts.sans.clone())
                            .text_px(12.5)
                            .line_height(u(18.))
                            .text_color(ink)
                            .when_some(strike, strike_through)
                            .child(item.text.clone()),
                    ),
            );
        }
        div()
            .mb(u(8.))
            .overflow_hidden()
            .rounded(u(10.))
            .border_1()
            .border_color(theme.content(0.1))
            .bg(theme.content(0.035))
            .child(header)
            .child(list)
    }
}

/// `line-through decoration-content/25`: a strike line in its own color.
pub fn strike_through<E: gpui::Styled>(mut el: E, color: Hsla) -> E {
    el.text_style().strikethrough = Some(gpui::StrikethroughStyle {
        thickness: px(1.),
        color: Some(color),
    });
    el
}

/// `TaskState`.
/// The loader spins with `motion-safe:animate-spin`; GPUI holds it still
/// when `App::reduce_motion` is set.
fn task_state(id: ElementId, status: TaskListItemStatus, theme: &Theme) -> AnyElement {
    let frame = div()
        .mt(px(1.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(u(16.))
        .rounded_full();
    match status {
        TaskListItemStatus::Completed => frame
            .bg(style::emerald_400(0.2))
            .child(
                icon(IconName::Check)
                    .size(u(10.))
                    .text_color(style::emerald_300()),
            )
            .into_any_element(),
        TaskListItemStatus::InProgress => {
            let loader = icon(IconName::Loader)
                .size(u(16.))
                .text_color(style::sky_300());
            frame
                .child(loader.with_animation(
                    id,
                    Animation::new(Duration::from_secs(1)).repeat(),
                    |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
                ))
                .into_any_element()
        }
        TaskListItemStatus::Cancelled => frame
            .bg(theme.content(0.08))
            .child(
                icon(IconName::Minus)
                    .size(u(10.))
                    .text_color(theme.content(0.35)),
            )
            .into_any_element(),
        TaskListItemStatus::Pending => frame
            .border_1()
            .border_color(theme.content(0.25))
            .bg(theme.content(0.02))
            .into_any_element(),
    }
}
