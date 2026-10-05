//! `WorkFoldLine` and `LiveFoldTitle` from AgentTranscript.tsx: the line a
//! turn's work folds behind, with the harness mark and the clock.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{Theme, u};

use crate::transcript::model::plan::{FoldLine, FoldTitle, Row};
use crate::transcript::model::turn::live_fold_text;

use super::parts::{chevron, harness_icon, phase_icon};
use super::shimmer::shimmer;
use super::style::{TextSizes as _, rail_color};
use super::{TranscriptView, eid};

impl TranscriptView {
    pub(super) fn render_fold_line(
        &mut self,
        row: &Row,
        line: &FoldLine,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let open = line.open;
        let expandable = line.expandable;
        let glyph: AnyElement = if open {
            // Open, the chevron stays put: it is the way back.
            chevron(true, theme.content(0.45)).into_any_element()
        } else {
            let mark: AnyElement = match line.harness {
                Some(harness) => harness_icon(harness),
                None => match phase_icon(line.kind, &theme) {
                    Some(icon) => icon.into_any_element(),
                    None => div().into_any_element(),
                },
            };
            div()
                .relative()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(expandable, |el| el.group_hover("fold", |s| s.opacity(0.)))
                        .child(mark),
                )
                .when(expandable, |el| {
                    el.child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .opacity(0.)
                            .group_hover("fold", |s| s.opacity(1.))
                            .child(chevron(false, theme.content(0.45))),
                    )
                })
                .into_any_element()
        };
        let icon_box = div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(14.))
            .child(glyph);
        let (label, background): (AnyElement, Option<String>) = match &line.title {
            FoldTitle::Live {
                started_at,
                paused,
                waiting_label,
                background,
                model_name,
            } => {
                let elapsed = self.elapsed(&row.turn_id, *started_at, *paused);
                let text = live_fold_text(
                    Some(elapsed),
                    *paused,
                    *waiting_label,
                    background,
                    model_name.as_deref(),
                );
                (
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_family(theme.fonts.sans.clone())
                        .text_sm_ui()
                        .child(shimmer(
                            eid(&row.key, "clock"),
                            text,
                            Duration::from_millis(1000),
                            &theme,
                        ))
                        .into_any_element(),
                    (!background.is_empty()).then(|| background.join("\n")),
                )
            }
            FoldTitle::Text(text) => (
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.fonts.sans.clone())
                    .text_sm_ui()
                    .text_color(theme.content(0.5))
                    .group_hover("fold", |s| s.text_color(theme.content(0.8)))
                    .child(text.clone())
                    .into_any_element(),
                None,
            ),
        };
        let turn_id = row.turn_id.clone();
        let mut line_el = div()
            .id(eid(&row.key, "fold-line"))
            .group("fold")
            .flex()
            .items_center()
            .gap(u(6.))
            .w_full()
            .min_w_0()
            .px(u(16.))
            .py(u(4.))
            .child(icon_box)
            .child(label);
        if let Some(tasks) = background {
            line_el = line_el.tooltip(tooltip(tasks));
        } else if expandable {
            line_el = line_el.tooltip(tooltip(if open {
                "Hide the work"
            } else {
                "Show the work"
            }));
        }
        if expandable {
            line_el = line_el
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_work(&turn_id, cx)));
        }
        // `.zen-fold-drop`: out of the chevron and down to the first row.
        div()
            .relative()
            .pb(u(4.))
            .child(line_el)
            .when(open, |el| {
                el.child(
                    div()
                        .absolute()
                        .left(u(23.))
                        .top(u(22.))
                        .bottom_0()
                        .w(px(1.))
                        .bg(rail_color(&theme)),
                )
            })
            .into_any_element()
    }
}
