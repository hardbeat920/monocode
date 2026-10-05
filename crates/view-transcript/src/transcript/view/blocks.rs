//! `TranscriptBlock` from AgentTranscript.tsx: how each kind of block draws
//! on its own row, with `HandoffDivider` and `InterjectionDivider`. Task
//! lists, plans, and generated images draw through `crate::cards`; the
//! orchestration result is `crate::threads::OrchestrationPreview`.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Animation, AnimationExt as _, AnyElement, AppContext as _, Context, InteractiveElement as _,
    IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use monocode_core::block::HandoffStatus;
use monocode_core::task_list::legacy_task_list_from_text;
use monocode_core::transcript::BlockRef;
use monocode_core::{Block, BlockRole};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::tooltip;
use monocode_ui::{Theme, u};

use crate::cards::generated_image::GeneratedImage;
use crate::cards::plan_preview::{build_disabled, plan_preview};
use crate::cards::task_list::task_list_preview;
use crate::threads::{
    OrchestrationPreview, SecondOpinionButton, SecondOpinionEvent, SecondOpinionProps,
};
use crate::transcript::model::plan::Row;
use crate::transcript::model::turn::interjection_chrome;

use super::activity::{interjection_body, severity_color};
use super::parts::harness_icon;
use super::shimmer::shimmer;
use super::style::{MarkdownVariant, TextSizes as _};
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
                    .child(
                        task_list_preview(eid(key, "tasks"), list.items.clone())
                            .explanation(list.explanation.clone()),
                    )
                    .into_any_element(),
                None => div().into_any_element(),
            },
            BlockRole::Plan => {
                if block.orchestration.is_some() {
                    return div().into_any_element();
                }
                if let Some(items) = legacy_task_list_from_text(&block.text) {
                    return gutter(div().py(u(4.)))
                        .child(task_list_preview(eid(key, "tasks"), items))
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
        let busy = self.session().is_some_and(|session| session.is_busy());
        let mut card = plan_preview(eid(key, &format!("plan:{}", block.id)), block.text.clone())
            .streaming(block.is_streaming())
            .busy(busy)
            .status(block.plan.as_ref().map(|plan| plan.status));
        if self.config.can_open_plans {
            let id = block.id.clone();
            let weak = cx.entity().downgrade();
            card = card.on_open(move |_, _, cx| {
                let block_id = id.clone();
                weak.update(cx, |_, cx| cx.emit(TranscriptEvent::OpenPlan { block_id }))
                    .ok();
            });
        }
        if self.config.can_build_plans {
            let id = block.id.clone();
            let weak = cx.entity().downgrade();
            card = card.on_build(move |_, _, cx| {
                let block_id = id.clone();
                weak.update(cx, |_, cx| cx.emit(TranscriptEvent::BuildPlan { block_id }))
                    .ok();
            });
            if self.config.can_build_plan_targets
                && let Some(session) = self.session()
            {
                let props = SecondOpinionProps::build_target(
                    session.harness,
                    Some(session.model.clone()),
                    Some(session.model_settings.clone()),
                    build_disabled(
                        &block.text,
                        block.is_streaming(),
                        busy,
                        block.plan.as_ref().map(|plan| plan.status),
                    ),
                );
                let key = format!("build-plan:{}", block.id);
                let picker = if let Some((picker, _)) = self.turn_model_menus.get(&key) {
                    picker.update(cx, |picker, cx| picker.set_props(props, cx));
                    picker.clone()
                } else {
                    let source = self.model_menu_source();
                    let picker = cx.new(|cx| SecondOpinionButton::new(props, source, cx));
                    let block_id = block.id.clone();
                    let subscription =
                        cx.subscribe(&picker, move |_, _, event: &SecondOpinionEvent, cx| {
                            let SecondOpinionEvent::Pick(target) = event;
                            cx.emit(TranscriptEvent::BuildPlanWithTarget {
                                block_id: block_id.clone(),
                                target: target.clone(),
                            });
                        });
                    self.turn_model_menus
                        .insert(key, (picker.clone(), subscription));
                    picker
                };
                card = card.target_picker(picker.into());
            }
        }
        card.into_any_element()
    }

    /// The orchestration result after a finished lead turn
    /// (`data-orchestration-result`): the assignment card,
    /// `crate::threads::OrchestrationPreview`, one per proposal block.
    pub(super) fn render_proposal(
        &mut self,
        row: &Row,
        block: &BlockRef,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if block.orchestration.is_none() {
            return div().into_any_element();
        }
        let busy = self.session().is_some_and(|session| session.is_busy());
        let catalog = self.config.catalog.clone();
        let card = match self.orchestration_cards.get(&block.id).cloned() {
            Some(card) => {
                // Only pass on what changed: each setter redraws the card.
                let seen = self.orchestration_seen.get(&block.id).cloned();
                let block_changed = seen
                    .as_ref()
                    .is_none_or(|(seen, _, _)| !std::sync::Arc::ptr_eq(seen, block));
                let busy_changed = seen.as_ref().is_none_or(|(_, seen, _)| *seen != busy);
                let catalog_changed = seen
                    .as_ref()
                    .is_none_or(|(_, _, seen)| !std::sync::Arc::ptr_eq(seen, &catalog));
                card.update(cx, |card, cx| {
                    if block_changed {
                        card.set_block(block, window, cx);
                    }
                    if busy_changed {
                        card.set_busy(busy, cx);
                    }
                    if catalog_changed {
                        card.set_catalog(catalog.clone(), cx);
                    }
                });
                self.orchestration_seen
                    .insert(block.id.clone(), (block.clone(), busy, catalog));
                card
            }
            None => {
                let (runs, actions) = self.orchestration_providers();
                let seen_catalog = catalog.clone();
                let card = cx.new(|cx| {
                    let mut card = OrchestrationPreview::new(block, runs, actions, window, cx);
                    card.set_busy(busy, cx);
                    card.set_catalog(catalog, cx);
                    card
                });
                self.orchestration_seen
                    .insert(block.id.clone(), (block.clone(), busy, seen_catalog));
                self.orchestration_cards
                    .insert(block.id.clone(), card.clone());
                card
            }
        };
        div()
            .id(eid(&row.key, "proposal"))
            .px(u(16.))
            .pt(u(4.))
            .pb(u(8.))
            .child(card)
            .into_any_element()
    }

    /// `GeneratedImage`: the image the agent produced, read from its path.
    fn render_generated_image(&mut self, block: &Block, cx: &mut Context<Self>) -> AnyElement {
        let Some(image) = &block.image else {
            return div().into_any_element();
        };
        let card = match self.image_cards.get(&block.id) {
            Some(card) => {
                let card = card.clone();
                let meta = image.clone();
                card.update(cx, |card, cx| card.set_image(meta, cx));
                card
            }
            None => {
                let meta = image.clone();
                let card = cx.new(|cx| GeneratedImage::new(meta, cx));
                self.image_cards.insert(block.id.clone(), card.clone());
                card
            }
        };
        div().min_w_0().child(card).into_any_element()
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
