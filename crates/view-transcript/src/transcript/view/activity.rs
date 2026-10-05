//! Turn items and the activity trail: `renderItem`, `ActivityPhases`,
//! `ActivityPhaseGroup`, `ActivityRow`, and the one-line thinking, note,
//! status, and interjection rows from AgentTranscript.tsx.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use monocode_core::Block;
use monocode_core::block::InterjectionSeverity;
use monocode_core::transcript::activity::{
    ActivityPhase, activity_phase_title, build_activity_phases, is_prose_block, is_subagent_block,
    is_thinking_block, needs_approval, prose_summary,
};
use monocode_core::transcript::monocode_call::monocode_work_summary;
use monocode_core::transcript::{BlockRef, TurnItem};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::tooltip;
use monocode_ui::{Theme, u};

use crate::transcript::model::plan::{ItemView, Placement, Row};
use crate::transcript::model::turn::{headline_has_more, interjection_chrome};

use super::parts::{
    chevron, monocode_mark, phase_icon, phase_step, pulse, rail_branch, rail_spine,
};
use super::shimmer::shimmer;
use super::style::{LIVE_WINDOW_MAX_HEIGHT, MarkdownVariant, TextSizes as _};
use super::{MarkdownSlot, TranscriptView, eid};

impl TranscriptView {
    /// One turn item in its place: plain, on the open fold's rail, or parked
    /// under the fold line.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_item(
        &mut self,
        row: &Row,
        item: &TurnItem,
        index: usize,
        view: ItemView,
        placement: Placement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = index;
        let prose = matches!(placement, Placement::FoldBody { prose: true, .. });
        let variant = if prose {
            MarkdownVariant::FoldProse
        } else {
            MarkdownVariant::Normal
        };
        let content = match (item, view) {
            (TurnItem::Subagents(blocks), ItemView::Subagents { live }) => {
                self.render_subagent_stack(&row.key, blocks, live, false, cx)
            }
            (TurnItem::Activity(_), ItemView::InitialThinking { live }) => {
                self.render_initial_thinking(&row.key, live, cx)
            }
            (TurnItem::Activity(blocks), ItemView::Activity { done }) => {
                self.render_activity_phases(&row.key, blocks, done, true, cx)
            }
            (
                TurnItem::Block(block),
                ItemView::Block {
                    under_line,
                    can_edit,
                    editing,
                },
            ) => self.render_block(
                row, block, under_line, can_edit, editing, false, variant, window, cx,
            ),
            // A view always matches its item; draw the blocks plainly if not.
            (item, _) => self.render_activity_phases(&row.key, item.blocks(), true, true, cx),
        };
        let theme = Theme::of(cx);
        let search = row.search_current;
        let frame = match placement {
            Placement::Plain | Placement::FoldSubagents => div().pb(u(4.)).child(content),
            // `.zen-fold-rail`: the spine at 23px, which is under the chevron.
            Placement::FoldBody { tail, .. } => div()
                .relative()
                .pl(u(20.))
                .pb(u(4.))
                .child(rail_spine(23., tail, theme))
                .child(rail_branch(23., theme))
                .child(content),
        };
        frame
            .when(search, |el| el.rounded(u(6.)).bg(theme.accent(0.08)))
            .into_any_element()
    }

    /// `InitialThinking`: reasoning before the first response arrives.
    fn render_initial_thinking(
        &mut self,
        key: &str,
        live: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx);
        let label: AnyElement = if live {
            shimmer(
                eid(key, "thinking"),
                "Thinking\u{2026}",
                Duration::from_millis(1600),
                theme,
            )
            .into_any_element()
        } else {
            div().child("Thinking\u{2026}").into_any_element()
        };
        div()
            .min_w_0()
            .px(u(16.))
            .pt(u(12.))
            .pb(u(4.))
            .font_family(theme.fonts.sans.clone())
            .text_sm_ui()
            .text_color(theme.content(0.5))
            .child(label)
            .into_any_element()
    }

    /// `ActivityPhases`: the turn's work as phases. The phase the agent is
    /// in stays open; the others fold back to their header.
    pub(super) fn render_activity_phases(
        &mut self,
        key: &str,
        blocks: &[BlockRef],
        done: bool,
        padded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let phases = build_activity_phases(blocks);
        let count = phases.len();
        let mut column = div()
            .flex()
            .flex_col()
            .min_w_0()
            .gap(u(4.))
            .when(padded, |el| el.px(u(16.)));
        for (index, phase) in phases.iter().enumerate() {
            let active = !done && index + 1 == count;
            column = column.child(self.render_phase(key, phase, active, cx));
        }
        column.into_any_element()
    }

    /// `ActivityPhaseGroup`: a header the group hangs off and its steps on a
    /// rail. It opens while it is the live group and folds when the agent
    /// moves on, until the reader clicks it. A step waiting on approval keeps
    /// it open.
    fn render_phase(
        &mut self,
        key: &str,
        phase: &ActivityPhase,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let override_key = format!("phase:{}", phase.id);
        let override_open = self.toggles.get(&override_key).copied();
        let waiting = phase.steps.iter().any(|block| needs_approval(block));
        let open = waiting || override_open.unwrap_or(active);
        let title = activity_phase_title(phase, active);
        let monocode_phase = monocode_work_summary(&phase.steps, active).is_some();
        let headline = phase
            .headline
            .as_ref()
            .filter(|headline| override_open == Some(true) && headline_has_more(Some(headline)));
        let inert = phase.steps.is_empty() && !headline_has_more(phase.headline.as_deref());
        let theme = Theme::of(cx).clone();
        let part = |name: &str| eid(key, &format!("{name}:{}", phase.id));

        // A lone call the agent never introduced is not a group.
        if phase.headline.is_none() && phase.steps.len() == 1 {
            let step = phase.steps[0].clone();
            let row = self.render_activity_row(key, &step, active, cx);
            let theme = Theme::of(cx);
            return div()
                .flex()
                .items_start()
                .gap(u(6.))
                .min_w_0()
                .when(!monocode_phase, |el| {
                    el.when_some(phase_icon(phase.kind, theme), |el, icon| {
                        el.child(icon.mt(u(7.)))
                    })
                })
                .child(div().flex_1().min_w_0().child(row))
                .into_any_element();
        }

        let label: AnyElement = if active {
            div()
                .flex_1()
                .min_w_0()
                .font_family(theme.fonts.sans.clone())
                .text_sm_ui()
                .child(shimmer(
                    part("title"),
                    title.clone(),
                    Duration::from_millis(1600),
                    &theme,
                ))
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.fonts.sans.clone())
                .text_sm_ui()
                .text_color(theme.content(0.5))
                .group_hover("phase", |s| s.text_color(theme.content(0.8)))
                .child(title.clone())
                .into_any_element()
        };

        if inert {
            return div()
                .flex()
                .items_center()
                .gap(u(6.))
                .py(u(4.))
                .min_w_0()
                .when_some(phase_icon(phase.kind, &theme), |el, icon| el.child(icon))
                .child(label)
                .into_any_element();
        }

        let icon_box = div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .group_hover("phase", |s| s.opacity(0.))
                    .map(|el| {
                        if monocode_phase {
                            el.child(monocode_mark(14.))
                        } else {
                            el.when_some(phase_icon(phase.kind, &theme), |el, icon| el.child(icon))
                        }
                    }),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .opacity(0.)
                    .group_hover("phase", |s| s.opacity(1.))
                    .child(chevron(open, theme.content(0.45))),
            );
        let toggle_key = override_key.clone();
        let header = div()
            .id(part("header"))
            .group("phase")
            .flex()
            .items_center()
            .gap(u(6.))
            .w_full()
            .min_w_0()
            .py(u(4.))
            .cursor_pointer()
            .tooltip(tooltip(if open {
                format!("Hide the steps for {title}")
            } else {
                format!("Show the steps for {title}")
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggles.insert(toggle_key.clone(), !open);
                cx.notify();
            }))
            .child(icon_box)
            .child(label);

        let mut column = div().flex().flex_col().min_w_0().child(header);
        if open {
            let mut steps = div().flex().flex_col().min_w_0();
            let count = phase.steps.len() + usize::from(headline.is_some());
            let mut at = 0;
            if let Some(headline) = headline {
                let markdown = self.markdown_view(
                    &headline.id,
                    MarkdownSlot::Prose,
                    &headline.text,
                    false,
                    MarkdownVariant::Normal,
                    cx,
                );
                if headline.role == monocode_core::BlockRole::Reasoning {
                    markdown.update(cx, |view, cx| view.set_reasoning(true, cx));
                }
                at += 1;
                steps = steps.child(phase_step(
                    at == count,
                    &theme,
                    div().py(u(4.)).child(markdown),
                ));
            }
            for step in &phase.steps {
                at += 1;
                let row = self.render_activity_row(key, step, active, cx);
                steps = steps.child(phase_step(at == count, &theme, row));
            }
            if active {
                // The live phase is a short window pinned to its newest step.
                let scroll_key = format!("{key}/{}", phase.id);
                let handle = self.scroll_handle(&scroll_key);
                let unpin_key = format!("unpin:{scroll_key}");
                if !self.toggled(&unpin_key, false) {
                    handle.scroll_to_bottom();
                }
                let wheel_handle = handle.clone();
                column = column.child(
                    div()
                        .id(part("live"))
                        .max_h(u(LIVE_WINDOW_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .track_scroll(&handle)
                        .on_scroll_wheel(cx.listener(
                            move |this, event: &ScrollWheelEvent, window, cx| {
                                let delta = event.delta.pixel_delta(window.line_height()).y;
                                let at_bottom = wheel_handle.offset().y
                                    <= -wheel_handle.max_offset().y + px(2.);
                                // Only a wheel away from the end unpins; reaching it again re-pins.
                                if delta > px(0.) {
                                    this.toggles.insert(unpin_key.clone(), true);
                                } else if at_bottom {
                                    this.toggles.remove(&unpin_key);
                                }
                                cx.notify();
                            },
                        ))
                        .child(steps),
                );
            } else {
                column = column.child(steps);
            }
        }
        column.into_any_element()
    }

    /// `ActivityRow`: one step of the agent's work. In a phase the rail is
    /// the bullet, so rows drop their own leading icon.
    pub(super) fn render_activity_row(
        &mut self,
        key: &str,
        block: &BlockRef,
        live: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if is_thinking_block(block) {
            return self.render_thinking_row(key, block, cx);
        }
        if block.interjection.is_some() {
            return self.render_interjection_row(key, block, cx);
        }
        if block.role == monocode_core::BlockRole::System {
            return self.render_status_row(block, cx);
        }
        if is_prose_block(block) {
            return self.render_note_row(key, block, cx);
        }
        // Only a settled turn routes a delegated run here; it is the same row.
        if is_subagent_block(block) {
            return self.render_subagent_row(key, block, live, cx);
        }
        self.render_activity_tool_row(key, block, live, true, cx)
    }

    /// `ActivityStatusRow`: one muted line, nothing to open.
    fn render_status_row(&mut self, block: &Block, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx);
        div()
            .id(gpui::ElementId::Name(format!("status:{}", block.id).into()))
            .flex()
            .items_center()
            .gap(u(6.))
            .py(u(4.))
            .min_w_0()
            .tooltip(tooltip(block.text.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.fonts.sans.clone())
                    .text_sm_ui()
                    .text_color(theme.content(0.5))
                    .child(monocode_core::js::trim(&block.text).to_string()),
            )
            .into_any_element()
    }

    /// `ActivityThinkingRow`: a thought, one line until opened.
    fn render_thinking_row(
        &mut self,
        key: &str,
        block: &BlockRef,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let toggle = format!("think:{}", block.id);
        let open = self.toggled(&toggle, false);
        let summary = prose_summary(&block.text);
        let text = if summary.is_empty() {
            "Thinking".to_string()
        } else {
            summary
        };
        let theme = Theme::of(cx).clone();
        let label = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .font_family(theme.fonts.sans.clone())
            .text_sm_ui()
            .text_color(theme.content(0.5))
            .group_hover("think", |s| s.text_color(theme.content(0.75)))
            .child(text.clone());
        let label = if block.is_streaming() {
            pulse(eid(key, &format!("pulse:{}", block.id)), label)
        } else {
            label.into_any_element()
        };
        let toggle_key = toggle.clone();
        let mut column = div().flex().flex_col().min_w_0().child(
            div()
                .id(eid(key, &toggle))
                .group("think")
                .flex()
                .items_center()
                .gap(u(6.))
                .py(u(4.))
                .min_w_0()
                .cursor_pointer()
                .tooltip(tooltip(if open {
                    "Hide thinking".to_string()
                } else {
                    format!("Show thinking: {text}")
                }))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.toggle(toggle_key.clone(), false, cx)),
                )
                .child(label),
        );
        if open {
            let markdown = self.markdown_view(
                &block.id,
                MarkdownSlot::Reasoning,
                &block.text,
                block.is_streaming(),
                MarkdownVariant::Normal,
                cx,
            );
            column = column.child(div().min_w_0().pb(u(8.)).child(markdown));
        }
        column.into_any_element()
    }

    /// `ActivityNoteRow`: a line the agent wrote mid-run, one line until opened.
    fn render_note_row(
        &mut self,
        key: &str,
        block: &BlockRef,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let toggle = format!("note:{}", block.id);
        let open = self.toggled(&toggle, false);
        let text = prose_summary(&block.text);
        let theme = Theme::of(cx).clone();
        let toggle_key = toggle.clone();
        let mut column = div().flex().flex_col().min_w_0().child(
            div()
                .id(eid(key, &toggle))
                .group("note")
                .flex()
                .items_center()
                .gap(u(6.))
                .py(u(4.))
                .min_w_0()
                .cursor_pointer()
                .on_click(
                    cx.listener(move |this, _, _, cx| this.toggle(toggle_key.clone(), false, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(theme.fonts.sans.clone())
                        .text_sm_ui()
                        .text_color(theme.content(0.7))
                        .group_hover("note", |s| s.text_color(theme.colors.content))
                        .child(text),
                ),
        );
        if open {
            let markdown = self.markdown_view(
                &block.id,
                MarkdownSlot::Prose,
                &block.text,
                block.is_streaming(),
                MarkdownVariant::Normal,
                cx,
            );
            column = column.child(div().min_w_0().pb(u(8.)).child(markdown));
        }
        column.into_any_element()
    }

    /// `ActivityInterjectionRow`: where a note came from and what it said,
    /// opening onto the whole note.
    fn render_interjection_row(
        &mut self,
        key: &str,
        block: &BlockRef,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(meta) = &block.interjection else {
            return div().into_any_element();
        };
        let chrome = interjection_chrome(meta);
        let summary = prose_summary(&block.text);
        let toggle = format!("interject:{}", block.id);
        let open = self.toggled(&toggle, false);
        let theme = Theme::of(cx).clone();
        let severity_color = severity_color(chrome.severity, &theme);
        let label = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_baseline()
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(theme.fonts.sans.clone())
            .text_sm_ui()
            .child(
                div()
                    .flex_none()
                    .text_color(theme.content(0.55))
                    .child(chrome.label.clone()),
            )
            .when_some(chrome.severity_text, |el, severity| {
                el.child(
                    div()
                        .flex_none()
                        .pl(u(4.))
                        .text_px(11.)
                        .text_color(severity_color)
                        .child(severity),
                )
            })
            .when(!summary.is_empty(), |el| {
                el.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.content(0.5))
                        .group_hover("interject", |s| s.text_color(theme.content(0.75)))
                        .child(format!(" \u{b7} {summary}")),
                )
            });
        if monocode_core::js::trim(&block.text).is_empty() {
            return div()
                .flex()
                .items_center()
                .gap(u(6.))
                .py(u(4.))
                .min_w_0()
                .child(label)
                .into_any_element();
        }
        let toggle_key = toggle.clone();
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .child(
                div()
                    .id(eid(key, &toggle))
                    .group("interject")
                    .flex()
                    .items_center()
                    .gap(u(6.))
                    .py(u(4.))
                    .min_w_0()
                    .cursor_pointer()
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.toggle(toggle_key.clone(), false, cx)
                        }),
                    )
                    .child(label),
            )
            .when(open, |el| {
                el.child(div().min_w_0().pb(u(8.)).child(interjection_body(
                    &block.text,
                    &theme,
                    None,
                )))
            })
            .into_any_element()
    }
}

/// `text-red-400`, `text-amber-400`, or `text-content/55` by severity.
pub(super) fn severity_color(severity: Option<InterjectionSeverity>, theme: &Theme) -> gpui::Hsla {
    match severity {
        Some(InterjectionSeverity::Blocker) => theme.colors.danger,
        Some(InterjectionSeverity::Concern) => theme.colors.warning,
        _ => theme.content(0.55),
    }
}

/// `INTERJECTION_BODY`: the advisory text under an interjection.
pub(super) fn interjection_body(
    text: &str,
    theme: &Theme,
    clamp: Option<usize>,
) -> impl IntoElement {
    div()
        .min_w_0()
        .font_family(theme.fonts.sans.clone())
        .text_size(u(12.5))
        .line_height(u(20.))
        .text_color(theme.content(0.7))
        .when_some(clamp, |el, lines| el.line_clamp(lines))
        .child(text.to_string())
}
