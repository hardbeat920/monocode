//! `TranscriptBlock` from AgentTranscript.tsx: how each kind of block draws
//! on its own row, with `HandoffDivider`, `InterjectionDivider`, and simple
//! forms of TaskListPreview.tsx, PlanPreview.tsx, GeneratedImage.tsx, and
//! the orchestration result card.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, Context, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div, img, px,
};
use monocode_core::block::{HandoffStatus, PlanStatus, TaskListItem, TaskListItemStatus};
use monocode_core::plan::{plan_summary, plan_title};
use monocode_core::task_list::{legacy_task_list_from_text, task_list_progress_label};
use monocode_core::transcript::BlockRef;
use monocode_core::{Block, BlockRole};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, icon, u};

use crate::transcript::model::plan::Row;
use crate::transcript::model::turn::interjection_chrome;

use super::activity::{interjection_body, severity_color};
use super::parts::harness_icon;
use super::shimmer::shimmer;
use super::style::{MarkdownVariant, TextSizes as _, palette};
use super::{MarkdownSlot, TranscriptEvent, TranscriptView, eid};

/// `TerminalSpinner` frames, 80ms apart.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

impl TranscriptView {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_block(
        &mut self,
        row: &Row,
        block: &BlockRef,
        under_line: bool,
        can_edit: bool,
        editing: bool,
        embedded: bool,
        variant: MarkdownVariant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = row.key.as_str();
        let gutter = |el: gpui::Div| if embedded { el } else { el.px(u(16.)) };
        match block.role {
            BlockRole::User => self.render_user_message(row, block, can_edit, editing, window, cx),
            BlockRole::Image => self.render_generated_image(block, cx),
            BlockRole::Tool | BlockRole::Approval => {
                self.render_tool_call(key, block, embedded, cx)
            }
            BlockRole::Reasoning => div().into_any_element(),
            BlockRole::Tasks => match block
                .task_list
                .as_ref()
                .filter(|list| !list.items.is_empty())
            {
                Some(list) => gutter(div().py(u(4.)))
                    .child(task_list(
                        key,
                        &list.items,
                        list.explanation.as_deref(),
                        Theme::of(cx),
                    ))
                    .into_any_element(),
                None => div().into_any_element(),
            },
            BlockRole::Plan => {
                if block.orchestration.is_some() {
                    return div().into_any_element();
                }
                if let Some(items) = legacy_task_list_from_text(&block.text) {
                    return gutter(div().py(u(4.)))
                        .child(task_list(key, &items, None, Theme::of(cx)))
                        .into_any_element();
                }
                let card = self.render_plan(key, block, cx);
                gutter(div().py(u(4.))).child(card).into_any_element()
            }
            BlockRole::Handoff => self.render_handoff(key, block, cx),
            BlockRole::System => {
                if block.interjection.is_some() {
                    return self.render_interjection_divider(key, block, window, cx);
                }
                let theme = Theme::of(cx);
                gutter(div().py(u(8.)))
                    .text_color(theme.content(0.5))
                    .child(div().min_w_0().child(block.text.clone()))
                    .into_any_element()
            }
            BlockRole::Assistant => {
                if block.text.is_empty() && block.is_streaming() {
                    return div().into_any_element();
                }
                let markdown = self.markdown_view(
                    &block.id,
                    MarkdownSlot::Prose,
                    &block.text,
                    block.is_streaming(),
                    variant,
                    cx,
                );
                let theme = Theme::of(cx);
                gutter(div())
                    .min_w_0()
                    .pb(u(4.))
                    .pt(u(if under_line { 4. } else { 12. }))
                    .text_color(theme.colors.content)
                    .child(markdown)
                    .into_any_element()
            }
        }
    }

    /// `HandoffDivider`: a rule with the provider the session moved to.
    fn render_handoff(&mut self, key: &str, block: &Block, cx: &mut Context<Self>) -> AnyElement {
        let Some(meta) = &block.handoff else {
            return div().into_any_element();
        };
        let theme = Theme::of(cx).clone();
        let preparing = meta.status == HandoffStatus::Preparing;
        let rule = || {
            div()
                .h(px(1.))
                .min_w(u(16.))
                .flex_1()
                .bg(theme.content(0.12))
        };
        let label: AnyElement = if preparing {
            div()
                .flex()
                .items_center()
                .gap(u(6.))
                .child(
                    div()
                        .w(u(14.))
                        .flex_none()
                        .text_px(11.)
                        .text_color(theme.content(0.45))
                        .with_animation(
                            eid(key, "spinner"),
                            Animation::new(Duration::from_millis(80 * SPINNER.len() as u64))
                                .repeat(),
                            |el, delta| {
                                let frame = ((delta * SPINNER.len() as f32) as usize)
                                    .min(SPINNER.len() - 1);
                                el.child(SPINNER[frame])
                            },
                        ),
                )
                .child(shimmer(
                    eid(key, "preparing"),
                    "Preparing a handoff",
                    Duration::from_millis(1400),
                    &theme,
                ))
                .into_any_element()
        } else {
            harness_icon(meta.to)
        };
        let aria = if preparing {
            format!("Preparing a handoff to {}", meta.to.title())
        } else {
            format!("Continued with {}", meta.to.title())
        };
        div()
            .px(u(16.))
            .py(u(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(12.))
                    .child(rule())
                    .child(
                        div()
                            .id(eid(key, "handoff"))
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .px(u(6.))
                            .font_family(theme.fonts.sans.clone())
                            .text_px(12.)
                            .text_color(theme.content(0.55))
                            .tooltip(tooltip(aria))
                            .child(label),
                    )
                    .child(rule()),
            )
            .into_any_element()
    }

    /// `InterjectionDivider`: a labeled rule with the advisory text under it,
    /// clamped to two lines until expanded.
    fn render_interjection_divider(
        &mut self,
        key: &str,
        block: &Block,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(meta) = &block.interjection else {
            return div().into_any_element();
        };
        let chrome = interjection_chrome(meta);
        let theme = Theme::of(cx).clone();
        let toggle = format!("interjection-body:{}", block.id);
        let expanded = self.toggled(&toggle, false);
        let overflows = text_overflows(&block.text, 2, 12.5, self.column_width(window), window);
        let rule = || {
            div()
                .h(px(1.))
                .min_w(u(16.))
                .flex_1()
                .bg(theme.content(0.12))
        };
        let toggle_key = toggle.clone();
        div()
            .px(u(16.))
            .py(u(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(u(12.))
                    .child(rule())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(8.))
                            .px(u(6.))
                            .font_family(theme.fonts.sans.clone())
                            .text_px(12.)
                            .text_color(theme.content(0.55))
                            .child(chrome.label.clone())
                            .when_some(chrome.severity_text, |el, severity| {
                                el.child(
                                    div()
                                        .text_px(11.)
                                        .text_color(severity_color(chrome.severity, &theme))
                                        .child(severity),
                                )
                            }),
                    )
                    .child(rule()),
            )
            .when(!block.text.is_empty(), |el| {
                el.child(
                    div()
                        .mt(u(8.))
                        .px(u(8.))
                        .child(interjection_body(
                            &block.text,
                            &theme,
                            (!expanded).then_some(2),
                        ))
                        .when(overflows, |el| {
                            el.child(
                                div()
                                    .id(eid(key, &toggle))
                                    .mt(u(4.))
                                    .py(u(4.))
                                    .font_family(theme.fonts.sans.clone())
                                    .text_xs_ui()
                                    .text_color(theme.content(0.55))
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(theme.colors.content))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle(toggle_key.clone(), false, cx)
                                    }))
                                    .child(if expanded { "Show less" } else { "Show more" }),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    /// `PlanPreview`: the plan's title and summary, with Open and Build.
    fn render_plan(&mut self, key: &str, block: &Block, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let title = plan_title(&block.text);
        let summary = plan_summary(&block.text);
        let streaming = block.is_streaming();
        let status = block.plan.as_ref().map(|plan| plan.status);
        let busy = self.session().is_some_and(|session| session.is_busy());
        let build_disabled = busy
            || streaming
            || monocode_core::js::trim(&block.text).is_empty()
            || matches!(
                status,
                Some(PlanStatus::Streaming | PlanStatus::Building | PlanStatus::Built)
            );
        let build_label = match status {
            Some(PlanStatus::Building) => "Building\u{2026}",
            Some(PlanStatus::Built) => "Built",
            _ => "Build",
        };
        let can_open = self.config.can_open_plans;
        let can_build = self.config.can_build_plans;
        let id = block.id.clone();
        let title_el = div()
            .id(eid(key, &format!("plan-title:{}", block.id)))
            .truncate()
            .font_family(theme.fonts.sans.clone())
            .text_px(13.)
            .line_height(u(20.))
            .medium()
            .text_color(theme.content(0.9))
            .tooltip(tooltip(title.clone()))
            .when(can_open, |el| {
                let id = id.clone();
                el.cursor_pointer()
                    .hover(|s| s.text_color(palette::yellow_100()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(TranscriptEvent::OpenPlan {
                            block_id: id.clone(),
                        })
                    }))
            })
            .child(title);
        let actions = (can_open || can_build).then(|| {
            div()
                .mt(u(8.))
                .flex()
                .items_center()
                .justify_end()
                .gap(u(6.))
                .when(can_open, |el| {
                    let id = id.clone();
                    el.child(
                        div()
                            .id(eid(key, &format!("plan-open:{}", block.id)))
                            .flex()
                            .items_center()
                            .gap(u(4.))
                            .h(u(24.))
                            .px(u(8.))
                            .rounded(u(6.))
                            .bg(theme.content(0.08))
                            .font_family(theme.fonts.sans.clone())
                            .text_px(11.)
                            .text_color(theme.content(0.7))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.content(0.12)).text_color(theme.colors.content))
                            .tooltip(tooltip("Open in pane"))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(TranscriptEvent::OpenPlan {
                                    block_id: id.clone(),
                                })
                            }))
                            .child(
                                icon(IconName::PanelRight)
                                    .size(u(12.))
                                    .text_color(theme.content(0.7)),
                            )
                            .child("Open"),
                    )
                })
                .when(can_build, |el| {
                    let id = id.clone();
                    el.child(
                        div()
                            .id(eid(key, &format!("plan-build:{}", block.id)))
                            .flex()
                            .items_center()
                            .gap(u(4.))
                            .h(u(24.))
                            .px(u(8.))
                            .rounded(u(6.))
                            .bg(theme.colors.content)
                            .font_family(theme.fonts.sans.clone())
                            .text_px(11.)
                            .text_color(theme.colors.background_base)
                            .tooltip(tooltip("Build this plan"))
                            .when(build_disabled, |el| el.opacity(0.4))
                            .when(!build_disabled, |el| {
                                el.cursor_pointer()
                                    .hover(|s| s.bg(theme.content(0.9)))
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        cx.emit(TranscriptEvent::BuildPlan {
                                            block_id: id.clone(),
                                        })
                                    }))
                            })
                            .child(
                                icon(IconName::Play)
                                    .size(u(12.))
                                    .text_color(theme.colors.background_base),
                            )
                            .child(build_label),
                    )
                })
        });
        div()
            .mb(u(8.))
            .overflow_hidden()
            .rounded(u(12.))
            .border_1()
            .border_color(theme.content(0.1))
            .bg(theme.content(0.07))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(u(10.))
                    .px(u(12.))
                    .py(u(10.))
                    .child(
                        icon(if streaming {
                            IconName::CircleDashed
                        } else {
                            IconName::AiIdea
                        })
                        .mt(u(2.))
                        .size(u(16.))
                        .text_color(theme.content(0.4)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(title_el)
                            .when(!summary.is_empty(), |el| {
                                el.child(
                                    div()
                                        .mt(u(2.))
                                        .line_clamp(3)
                                        .font_family(theme.fonts.sans.clone())
                                        .text_px(12.)
                                        .line_height(u(18.))
                                        .text_color(theme.content(0.5))
                                        .child(summary),
                                )
                            })
                            .children(actions),
                    ),
            )
            .into_any_element()
    }

    /// The orchestration result after a finished lead turn. The full
    /// assignment card (`OrchestrationPreview`) belongs to the cards module;
    /// this shows its title and summary in the same frame.
    pub(super) fn render_proposal(
        &mut self,
        row: &Row,
        block: &BlockRef,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx);
        let Some(proposal) = &block.orchestration else {
            return div().into_any_element();
        };
        div()
            .id(eid(&row.key, "proposal"))
            .px(u(16.))
            .pt(u(4.))
            .pb(u(8.))
            .child(
                div()
                    .rounded(u(12.))
                    .border_1()
                    .border_color(theme.content(0.1))
                    .bg(theme.content(0.04))
                    .px(u(12.))
                    .py(u(10.))
                    .font_family(theme.fonts.sans.clone())
                    .child(
                        div()
                            .text_px(13.)
                            .line_height(u(20.))
                            .medium()
                            .text_color(theme.content(0.9))
                            .child(proposal.title.clone()),
                    )
                    .child(
                        div()
                            .text_px(12.)
                            .line_height(u(18.))
                            .text_color(theme.content(0.5))
                            .child(proposal.summary.clone()),
                    )
                    .child(
                        div()
                            .mt(u(6.))
                            .text_px(11.)
                            .text_color(theme.content(0.4))
                            .child(format!(
                                "{} {}",
                                proposal.tasks.len(),
                                if proposal.tasks.len() == 1 {
                                    "task"
                                } else {
                                    "tasks"
                                }
                            )),
                    ),
            )
            .into_any_element()
    }

    /// `GeneratedImage`: the image the agent produced, from its path.
    fn render_generated_image(&mut self, block: &Block, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        let Some(image) = &block.image else {
            return div().into_any_element();
        };
        let path = std::path::PathBuf::from(&image.path);
        div()
            .min_w_0()
            .px(u(16.))
            .pt(u(12.))
            .pb(u(12.))
            .child(
                div()
                    .max_w_full()
                    .overflow_hidden()
                    .rounded(u(12.))
                    .border_1()
                    .border_color(theme.content(0.1))
                    .bg(theme.content(0.05))
                    .child(img(path).max_h(u(640.)).max_w_full()),
            )
            .child(
                div()
                    .mt(u(4.))
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(u(8.))
                    .font_family(theme.fonts.sans.clone())
                    .text_px(11.)
                    .text_color(theme.content(0.45))
                    .child(div().min_w_0().truncate().child(image.name.clone()))
                    .child(format_file_size(image.size)),
            )
            .into_any_element()
    }
}

/// `formatFileSize`.
fn format_file_size(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    if bytes < 1024. {
        return format!("{bytes:.0} B");
    }
    let units = ["KB", "MB", "GB"];
    let mut value = bytes / 1024.;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < units.len() {
        value /= 1024.;
        unit += 1;
    }
    if value >= 10. {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// Whether `text` needs more than `lines` lines in `width` (`line-clamp`
/// measurement, approximated from the font's average advance).
pub(super) fn text_overflows(
    text: &str,
    lines: usize,
    size: f32,
    width: gpui::Pixels,
    window: &Window,
) -> bool {
    let rem = window.rem_size();
    let advance = u(size * 0.55).to_pixels(rem);
    let per_line = (width / advance).floor().max(1.) as usize;
    let needed: usize = text
        .split('\n')
        .map(|line| line.chars().count().div_ceil(per_line).max(1))
        .sum();
    needed > lines
}

/// `TaskListPreview`.
fn task_list(
    key: &str,
    items: &[TaskListItem],
    explanation: Option<&str>,
    theme: &Theme,
) -> AnyElement {
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
                                .child(task_list_progress_label(items)),
                        ),
                )
                .when_some(explanation, |el, explanation| {
                    el.child(
                        div()
                            .mt(u(2.))
                            .line_clamp(2)
                            .font_family(theme.fonts.sans.clone())
                            .text_px(11.5)
                            .line_height(u(16.))
                            .text_color(theme.content(0.5))
                            .child(explanation.to_string()),
                    )
                }),
        );
    let mut list = div().py(u(4.)).flex().flex_col();
    for (index, item) in items.iter().enumerate() {
        let (ink, strike) = match item.status {
            TaskListItemStatus::Completed => (theme.content(0.4), true),
            TaskListItemStatus::Cancelled => (theme.content(0.35), true),
            TaskListItemStatus::InProgress => (theme.content(0.85), false),
            TaskListItemStatus::Pending => (theme.content(0.6), false),
        };
        list = list.child(
            div()
                .flex()
                .items_start()
                .gap(u(10.))
                .min_w_0()
                .px(u(10.))
                .py(u(6.))
                .child(task_state(
                    eid(key, &format!("task:{index}")),
                    item.status,
                    theme,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(theme.fonts.sans.clone())
                        .text_px(12.5)
                        .line_height(u(18.))
                        .text_color(ink)
                        .when(strike, |el| el.line_through())
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
        .into_any_element()
}

/// `TaskState`.
fn task_state(id: gpui::ElementId, status: TaskListItemStatus, theme: &Theme) -> AnyElement {
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
            .bg(gpui::Hsla {
                a: 0.2,
                ..theme.colors.success
            })
            .child(
                icon(IconName::Check)
                    .size(u(10.))
                    .text_color(palette::emerald_300()),
            )
            .into_any_element(),
        TaskListItemStatus::InProgress => frame
            .child(
                icon(IconName::Loader)
                    .size(u(16.))
                    .text_color(palette::sky_300())
                    .with_animation(
                        id,
                        Animation::new(Duration::from_secs(1)).repeat(),
                        |svg, delta| {
                            svg.with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                delta,
                            )))
                        },
                    ),
            )
            .into_any_element(),
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

#[cfg(test)]
mod tests {
    use super::format_file_size;

    #[test]
    fn formats_file_sizes() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(2048), "2.0 KB");
        assert_eq!(format_file_size(20 * 1024 * 1024), "20 MB");
    }
}
