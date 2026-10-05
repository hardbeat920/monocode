//! Delegated runs: `SubagentStack`, `SubagentRow`, and `SubagentPanel` from
//! AgentTranscript.tsx. A row is a mascot, a name, and how far the agent
//! got; clicking it opens the agent's own trail underneath.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, relative,
};
use monocode_core::transcript::BlockRef;
use monocode_core::transcript::activity::{
    ToolCallState, subagent_brief, subagent_model_name, subagent_name, subagent_report,
    tool_call_state,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{Theme, u};

use crate::transcript::model::turn::{agent_step_block, subagent_status_line};

use super::mascot::mascot;
use super::parts::{chevron, phase_step};
use super::shimmer::shimmer;
use super::style::{MarkdownVariant, TextSizes as _};
use super::{MarkdownSlot, TranscriptView, eid};

impl TranscriptView {
    /// `SubagentStack`: one row per delegated run.
    pub(super) fn render_subagent_stack(
        &mut self,
        key: &str,
        blocks: &[BlockRef],
        live: bool,
        embedded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut column = div()
            .flex()
            .flex_col()
            .min_w_0()
            .when(!embedded, |el| el.px(u(16.)));
        for block in blocks {
            column = column.child(self.render_subagent_row(key, block, live, cx));
        }
        column.into_any_element()
    }

    /// `SubagentRow` and `SubagentPanel`. A run that died opens itself onto
    /// the provider's reason; a click takes over from there.
    pub(super) fn render_subagent_row(
        &mut self,
        key: &str,
        block: &BlockRef,
        live: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = tool_call_state(block);
        let toggle = format!("agent:{}", block.id);
        let open = self.toggled(&toggle, state == ToolCallState::Rejected);
        let name = subagent_name(block);
        let brief = subagent_brief(block);
        let model = subagent_model_name(block, &self.config.catalog);
        let active = live && state == ToolCallState::Pending;
        let steps = block
            .agent_run
            .as_ref()
            .map(|run| run.steps.clone())
            .unwrap_or_default();
        let status = subagent_status_line(block, &steps);
        let report = subagent_report(block);
        let failed = state == ToolCallState::Rejected;
        let theme = Theme::of(cx).clone();

        let name_el: AnyElement = if active {
            div()
                .flex_1()
                .min_w_0()
                .font_family(theme.fonts.sans.clone())
                .text_sm_ui()
                .child(shimmer(
                    eid(key, &format!("agent-name:{}", block.id)),
                    name.clone(),
                    Duration::from_millis(1600),
                    &theme,
                ))
                .into_any_element()
        } else {
            let ink = if failed {
                theme.colors.danger
            } else {
                theme.content(0.75)
            };
            let hover = if failed {
                theme.colors.danger
            } else {
                theme.colors.content
            };
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.fonts.sans.clone())
                .text_sm_ui()
                .text_color(ink)
                .group_hover("agent", move |s| s.text_color(hover))
                .child(name.clone())
                .into_any_element()
        };
        let meta = (model.is_some() || !status.is_empty()).then(|| {
            div()
                .flex()
                .flex_none()
                .min_w_0()
                .max_w(relative(0.55))
                .items_center()
                .gap(u(8.))
                .font_family(theme.fonts.sans.clone())
                .text_px_l5(12.)
                .text_color(theme.content(0.4))
                .when_some(model.clone(), |el, model| {
                    el.child(div().min_w_0().truncate().child(model))
                })
                .when(!status.is_empty(), |el| {
                    el.child(div().flex_none().child(status.clone()))
                })
        });
        let label = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .child(name_el)
            .children(meta);
        let mascot_color = match state {
            ToolCallState::Rejected => theme.colors.danger,
            ToolCallState::Pending => theme.content(0.7),
            ToolCallState::Accepted => theme.content(0.45),
        };
        let mascot_el = mascot(
            eid(key, &format!("mascot:{}", block.id)),
            &name,
            mascot_color,
            active,
        );

        // A run without a step has nothing to open into. The row keeps its
        // place, so the chevron arriving later moves nothing.
        if steps.is_empty() && report.is_none() {
            return div()
                .id(eid(key, &format!("agent:{}", block.id)))
                .flex()
                .items_center()
                .gap(u(8.))
                .min_w_0()
                .mx(u(-6.))
                .px(u(6.))
                .py(u(4.))
                .tooltip(tooltip(brief))
                .child(mascot_el)
                .child(label)
                .child(div().flex_none().size(u(14.)))
                .into_any_element();
        }

        let wash = theme.content(0.08);
        let toggle_key = toggle.clone();
        let default_open = state == ToolCallState::Rejected;
        let header =
            div()
                .id(eid(key, &format!("agent:{}", block.id)))
                .group("agent")
                .flex()
                .items_center()
                .gap(u(8.))
                .w_full()
                .min_w_0()
                .mx(u(-6.))
                .px(u(6.))
                .py(u(4.))
                .rounded(u(6.))
                .cursor_pointer()
                .hover(move |s| s.bg(wash))
                .when(open, |el| el.bg(wash))
                .tooltip(tooltip(brief))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle(toggle_key.clone(), default_open, cx)
                }))
                .child(mascot_el)
                .child(label)
                .child(chevron(open, theme.content(0.35)));
        let mut column = div().flex().flex_col().min_w_0().child(header);
        if open {
            // The run's trail as transcript blocks, grouped the way the main
            // trail is. No scroll window of its own: each live phase keeps one.
            let step_blocks: Vec<BlockRef> = steps
                .iter()
                .map(|step| Arc::new(agent_step_block(step)))
                .collect();
            let phases = self.render_activity_phases(
                &format!("{key}/{}", block.id),
                &step_blocks,
                !active,
                false,
                cx,
            );
            let mut body = div().flex().flex_col().min_w_0().pb(u(4.)).child(phases);
            if let Some(report) = report {
                let content: AnyElement = if failed {
                    div()
                        .min_w_0()
                        .font_family(theme.fonts.mono.clone())
                        .text_px_l5(12.)
                        .text_color(gpui::Hsla {
                            a: 0.8,
                            ..theme.colors.danger
                        })
                        .child(report)
                        .into_any_element()
                } else {
                    self.markdown_view(
                        &block.id,
                        MarkdownSlot::Report,
                        &report,
                        false,
                        MarkdownVariant::Normal,
                        cx,
                    )
                    .into_any_element()
                };
                body = body.child(phase_step(true, &theme, div().py(u(4.)).child(content)));
            }
            column = column.child(body);
        }
        column.into_any_element()
    }
}
