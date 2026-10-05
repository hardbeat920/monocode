//! `TurnDuration` and `TurnMetricsBadge` from AgentTranscript.tsx: what a
//! finished turn leaves under its answer.

use crate::threads::{SecondOpinionButton, SecondOpinionEvent, SecondOpinionProps};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div,
};
use monocode_ui::widgets::{Tooltip, tooltip};
use monocode_ui::{IconName, Theme, icon, u};

use crate::transcript::model::plan::{Row, TurnFooter};
use crate::transcript::model::turn::{
    format_clock_time, format_working_duration, turn_metrics_summary,
};

use super::parts::harness_icon;
use super::style::TextSizes as _;
use super::{TranscriptEvent, TranscriptView, eid};

impl TranscriptView {
    pub(super) fn render_footer(
        &mut self,
        row: &Row,
        footer: &TurnFooter,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let label =
            format_working_duration(Some(footer.elapsed_ms), footer.model_name.as_deref(), true);
        let dot = || {
            div()
                .flex_none()
                .size(u(3.))
                .rounded_full()
                .bg(theme.content(0.25))
        };
        let mut actions = div().flex().flex_none().items_center().gap(u(4.));
        if footer.copy_text.is_empty() {
            actions = actions.child(
                icon(IconName::Check)
                    .size(u(14.))
                    .text_color(theme.content(0.4)),
            );
        } else {
            let copy_key = format!("copy:{}", row.turn_id);
            let copied = self.flashed(&copy_key);
            let text = footer.copy_text.clone();
            actions = actions.child(
                // `-ml-1`, as an offset: a negative margin breaks the row's sizing.
                self.action_button(
                    &row.key,
                    "copy",
                    if copied {
                        IconName::Check
                    } else {
                        IconName::Copy
                    },
                    if copied { "Copied" } else { "Copy response" },
                    cx,
                )
                .relative()
                .left(u(-4.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.copy(copy_key.clone(), text.clone(), cx);
                })),
            );
            if self.config.can_save_notes {
                let save_key = format!("note:{}", row.turn_id);
                let saved = self.flashed(&save_key);
                let text = footer.copy_text.clone();
                actions = actions.child(
                    self.action_button(
                        &row.key,
                        "save",
                        if saved {
                            IconName::Check
                        } else {
                            IconName::FilePlusCorner
                        },
                        if saved {
                            "Saved to Notes"
                        } else {
                            "Save as note"
                        },
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.emit(TranscriptEvent::SaveNote { text: text.clone() });
                        this.flash(save_key.clone(), cx);
                    })),
                );
            }
        }
        if footer.harness.is_some() && self.config.can_handoff {
            actions = actions.child(self.turn_model_menu(row, footer, true, cx));
        }
        if footer.harness.is_some() && self.config.can_second_opinion {
            actions = actions.child(self.turn_model_menu(row, footer, false, cx));
        }
        if let Some(summary) =
            turn_metrics_summary(footer.metrics.as_ref(), Some(footer.elapsed_ms))
        {
            let headline = summary.headline.clone();
            let detail = summary.detail.clone();
            actions = actions.child(
                div()
                    .id(eid(&row.key, "metrics"))
                    .flex()
                    .flex_none()
                    .ml(u(3.))
                    .rounded(u(6.))
                    .p(u(4.))
                    .hover(|s| s.bg(theme.content(0.08)))
                    .tooltip(move |window, cx| {
                        let text = if detail.is_empty() {
                            headline.clone()
                        } else {
                            format!("{headline}\n{detail}")
                        };
                        Tooltip::new(text).build(window, cx)
                    })
                    .child(
                        icon(IconName::ChartBreakoutSquare)
                            .size(u(14.))
                            .text_color(theme.content(0.4)),
                    ),
            );
        }
        div()
            .flex()
            .items_center()
            .gap(u(10.))
            .w_full()
            .min_w_0()
            .px(u(16.))
            .pt(u(4.))
            .pb(u(12.))
            .font_family(theme.fonts.sans.clone())
            .text_sm_ui()
            .text_color(theme.content(0.4))
            .child(actions)
            .when(!footer.label_hidden, |el| {
                el.child(
                    div()
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(u(10.))
                        .child(dot())
                        .child(
                            div()
                                .flex()
                                .min_w_0()
                                .items_center()
                                .gap(u(6.))
                                .when_some(footer.harness, |el, harness| {
                                    el.child(harness_icon(harness))
                                })
                                .child(div().min_w_0().truncate().child(label.clone())),
                        ),
                )
            })
            .when_some(footer.completed_at, |el, at| {
                el.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(u(10.))
                        .child(dot())
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme.content(0.35))
                                .child(format_clock_time(at)),
                        ),
                )
            })
            .into_any_element()
    }

    fn turn_model_menu(
        &mut self,
        row: &Row,
        footer: &TurnFooter,
        handoff: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<SecondOpinionButton> {
        use gpui::AppContext as _;
        let kind = if handoff { "handoff" } else { "second-opinion" };
        let key = format!("{kind}:{}", row.turn_id);
        let harness = footer.harness.expect("turn has a provider");
        let mut props = if handoff {
            SecondOpinionProps::handoff(harness)
        } else {
            SecondOpinionProps::second_opinion(harness)
        };
        props.from_model = footer.from_model.clone();
        if !handoff {
            props.include_current = true;
            props.exclude_from_model = true;
        }
        if let Some((view, _)) = self.turn_model_menus.get(&key) {
            view.update(cx, |view, cx| view.set_props(props, cx));
            return view.clone();
        }
        let source = self.model_menu_source();
        let view = cx.new(|cx| SecondOpinionButton::new(props, source, cx));
        let turn_id = row.turn_id.clone();
        let subscription = cx.subscribe(&view, move |_, _, event: &SecondOpinionEvent, cx| {
            let SecondOpinionEvent::Pick(target) = event;
            cx.emit(if handoff {
                TranscriptEvent::Handoff {
                    turn_id: turn_id.clone(),
                    target: target.clone(),
                }
            } else {
                TranscriptEvent::SecondOpinion {
                    turn_id: turn_id.clone(),
                    target: target.clone(),
                }
            });
        });
        self.turn_model_menus
            .insert(key, (view.clone(), subscription));
        view
    }

    /// The quiet icon buttons on action rows (`rounded-md p-1
    /// text-content/40 hover:bg-content/8 hover:text-content/70`).
    pub(super) fn action_button(
        &self,
        key: &str,
        part: &str,
        glyph: IconName,
        label: &'static str,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::of(cx);
        let hover_ink = theme.content(0.7);
        let group = format!("action-{part}");
        div()
            .id(eid(key, part))
            .group(group.clone())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(22.))
            .rounded(u(6.))
            .cursor_pointer()
            .hover(|s| s.bg(theme.content(0.08)))
            .tooltip(tooltip(label))
            .child(
                icon(glyph)
                    .size(u(14.))
                    .text_color(theme.content(0.4))
                    .group_hover(group, move |s| s.text_color(hover_ink)),
            )
    }
}
