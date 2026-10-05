//! Port of src/features/sessions/ui/PromptOutline.tsx: a rail of short bars
//! at the transcript's right edge, one per prompt. The bar of the prompt in
//! view is lit. Hovering a bar lifts it and its neighbours like a dock and
//! shows the prompt with the head of its reply; clicking jumps there.
//!
//! The outline takes the session's blocks and where the prompts sit
//! ([`PromptOutline::set_viewport`], or [`PromptOutline::sync_with_transcript`]
//! for a [`TranscriptView`]) and reports a jump as a [`PromptOutlineEvent`].
//! [`jump_to_prompt`] scrolls a transcript there.

use std::collections::HashMap;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, Bounds, Context, ElementId, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render,
    StatefulInteractiveElement as _, Styled as _, Task, Window, canvas, div, point, px,
};
use monocode_core::transcript::BlockRef;
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::{PopoverSide, popover_at, popover_frame};
use monocode_ui::{Theme, u};

use crate::transcript::TranscriptView;
use crate::transcript::model::plan::RowKind;

use super::prompt_outline_model::{
    NEAR_END_PX, OutlineAnchor, OutlineBand, RIPPLE_SPAN, active_prompt_id, bar_lift, bar_window,
    prompt_label, prompt_preview,
};
use super::util::{BoundsMap, Transition};

const OPEN_DELAY_MS: u64 = 25;
const POPOVER_WIDTH: f32 = 288.;
const MIN_PROMPTS: usize = 2;
const BAR_HEIGHT_PX: f32 = 2.;
const BAR_WIDTH_PX: f32 = 11.;
const BAR_WIDTH_LIFTED_PX: f32 = 24.;
const BAR_OPACITY_IDLE: f32 = 0.15;
const BAR_OPACITY_LIT: f32 = 0.85;
const RIPPLE_STEP_MS: f32 = 18.;
const BAR_GAP_PX: f32 = 10.;
const BAR_GAP_MIN_PX: f32 = 1.;
const BAR_STACK_MAX_PX: f32 = 330.;
const BAR_STACK_PANE_SHARE: f32 = 0.75;
/// `@max-[58rem]:hidden`: the rail hides in a pane narrower than 58rem.
const MIN_PANE_WIDTH: f32 = 928.;
/// `transition-[width,opacity] duration-200 ease-out`.
const BAR_TRANSITION_MS: f32 = 200.;

/// `barStack`: one bar per prompt while they fit the budget. The gap
/// shrinks first; past that a window slides. Returns the window and the gap.
pub fn bar_stack(count: usize, active_index: Option<usize>, budget: f32) -> (usize, usize, f32) {
    let fit =
        (((budget + BAR_GAP_MIN_PX) / (BAR_HEIGHT_PX + BAR_GAP_MIN_PX)).floor() as usize).max(1);
    let (start, end) = bar_window(count, active_index, fit);
    let shown = end - start;
    let gap = if shown > 1 {
        ((budget - shown as f32 * BAR_HEIGHT_PX) / (shown as f32 - 1.))
            .floor()
            .clamp(BAR_GAP_MIN_PX, BAR_GAP_PX)
    } else {
        0.
    };
    (start, end, gap)
}

/// What the reader did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptOutlineEvent {
    /// Scroll the prompt's turn to the top.
    Jump { block_id: String },
}

/// One bar's animated width and opacity.
struct BarMotion {
    width: Transition,
    opacity: Transition,
}

/// `<PromptOutline blocks />`.
pub struct PromptOutline {
    blocks: Vec<BlockRef>,
    prompts: Vec<BlockRef>,
    active_id: Option<String>,
    stack_budget: f32,
    hover: Option<String>,
    open: bool,
    focus_id: Option<String>,
    pointer_inside: bool,
    focus: FocusHandle,
    bars: BoundsMap,
    motion: HashMap<String, BarMotion>,
    pane_width: std::rc::Rc<std::cell::Cell<f32>>,
    open_timer: Option<Task<()>>,
}

impl EventEmitter<PromptOutlineEvent> for PromptOutline {}

impl PromptOutline {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            blocks: Vec::new(),
            prompts: Vec::new(),
            active_id: None,
            stack_budget: BAR_STACK_MAX_PX,
            hover: None,
            open: false,
            focus_id: None,
            pointer_inside: false,
            focus: cx.focus_handle(),
            bars: BoundsMap::default(),
            motion: HashMap::new(),
            pane_width: std::rc::Rc::new(std::cell::Cell::new(f32::INFINITY)),
            open_timer: None,
        }
    }

    /// The session's blocks.
    pub fn set_blocks(&mut self, blocks: Vec<BlockRef>, cx: &mut Context<Self>) {
        self.prompts = super::prompt_outline_model::prompt_blocks(&blocks)
            .into_iter()
            .cloned()
            .collect();
        self.blocks = blocks;
        cx.notify();
    }

    /// `measure`: where the transcript is scrolled. `distance_to_end` is how
    /// far the scroller is from its bottom.
    pub fn set_viewport(
        &mut self,
        viewport: OutlineBand,
        anchors: &[OutlineAnchor],
        distance_to_end: f32,
        cx: &mut Context<Self>,
    ) {
        let height = viewport.bottom - viewport.top;
        // A hidden tab has a zero-size box. The rule would then pick the last prompt.
        if height <= 0. {
            return;
        }
        let budget = BAR_STACK_MAX_PX.min((height * BAR_STACK_PANE_SHARE).floor());
        let active = if distance_to_end <= NEAR_END_PX {
            self.prompts.last().map(|prompt| prompt.id.clone())
        } else {
            active_prompt_id(viewport, anchors, distance_to_end)
        };
        if budget != self.stack_budget || active != self.active_id {
            self.stack_budget = budget;
            self.active_id = active;
            cx.notify();
        }
    }

    /// Read the prompt bands from a transcript drawn in `viewport`. Prompts
    /// outside the drawn rows count as above or below it.
    pub fn sync_with_transcript(
        &mut self,
        transcript: &TranscriptView,
        viewport: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let (band, anchors, distance) = transcript_anchors(transcript, viewport);
        self.set_viewport(band, &anchors, distance, cx);
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active_id.as_deref()
    }

    pub fn hovered(&self) -> Option<&str> {
        self.hover.as_deref()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The bars on screen: (start, end, gap).
    pub fn stack(&self) -> (usize, usize, f32) {
        let active = self
            .prompts
            .iter()
            .position(|prompt| Some(prompt.id.as_str()) == self.active_id.as_deref());
        bar_stack(self.prompts.len(), active, self.stack_budget)
    }

    /// `hoverBar`: the ripple follows the pointer at once; the card waits
    /// out a pass-through.
    pub fn hover_bar(&mut self, id: &str, cx: &mut Context<Self>) {
        self.hover = Some(id.to_string());
        self.pointer_inside = true;
        cx.notify();
        if self.open || self.open_timer.is_some() {
            return;
        }
        self.open_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(OPEN_DELAY_MS))
                .await;
            this.update(cx, |this, cx| {
                this.open_timer = None;
                this.open = true;
                cx.notify();
            })
            .ok();
        }));
    }

    /// `showBar`: keyboard focus opens the card at once.
    pub fn show_bar(&mut self, id: &str, cx: &mut Context<Self>) {
        self.open_timer = None;
        self.hover = Some(id.to_string());
        self.open = true;
        cx.notify();
    }

    /// `close`.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open_timer = None;
        self.hover = None;
        self.open = false;
        cx.notify();
    }

    /// `leaveRail`: keyboard focus holds the card open after the pointer leaves.
    pub fn leave_rail(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.pointer_inside = false;
        if self.focus.is_focused(window) && self.focus_id.is_some() {
            return;
        }
        self.close(cx);
    }

    /// `jumpTo`.
    pub fn jump_to(&mut self, id: &str, cx: &mut Context<Self>) {
        cx.emit(PromptOutlineEvent::Jump {
            block_id: id.to_string(),
        });
    }

    fn tab_id(&self, bars: &[BlockRef]) -> Option<String> {
        [self.focus_id.as_deref(), self.active_id.as_deref()]
            .into_iter()
            .flatten()
            .find(|id| bars.iter().any(|bar| bar.id == *id))
            .map(str::to_string)
            .or_else(|| bars.first().map(|bar| bar.id.clone()))
    }

    /// `onKeyDown`: arrows walk the rail, Enter jumps, Escape closes.
    pub fn handle_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let (start, end, _) = self.stack();
        let bars: Vec<BlockRef> = self.prompts[start..end].to_vec();
        let Some(tab_id) = self.tab_id(&bars) else {
            return false;
        };
        let from = bars.iter().position(|bar| bar.id == tab_id).unwrap_or(0);
        match key {
            "down" | "up" => {
                let next = if key == "down" {
                    bars.get(from + 1)
                } else {
                    from.checked_sub(1).and_then(|index| bars.get(index))
                };
                if let Some(next) = next {
                    self.focus_id = Some(next.id.clone());
                    self.show_bar(&next.id.clone(), cx);
                }
                true
            }
            "enter" | "space" => {
                self.jump_to(&tab_id, cx);
                true
            }
            "escape" => {
                self.close(cx);
                true
            }
            _ => false,
        }
    }

    /// The bar's width and opacity now, heading for its targets.
    fn bar_motion(
        &mut self,
        id: &str,
        width: f32,
        opacity: f32,
        delay: f32,
        animate: bool,
    ) -> (f32, f32, bool) {
        let motion = self
            .motion
            .entry(id.to_string())
            .or_insert_with(|| BarMotion {
                width: Transition::new(width, BAR_TRANSITION_MS, gpui_ease_out()),
                opacity: Transition::new(opacity, BAR_TRANSITION_MS, gpui_ease_out()),
            });
        motion.width.set(width, delay, animate);
        motion.opacity.set(opacity, delay, animate);
        (
            motion.width.value(),
            motion.opacity.value(),
            motion.width.running() || motion.opacity.running(),
        )
    }

    fn render_popover(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.open {
            return None;
        }
        let hover = self.hover.as_deref()?;
        let preview = prompt_preview(&self.blocks, hover)?;
        let anchor = self.bars.get(hover)?;
        let gap = 10.;
        let position = point(anchor.left() - px(gap), anchor.center().y);
        let clamp = |el: gpui::Div| el.line_clamp(2).text_px(14.).leading(1.375);
        let card = popover_frame(ElementId::Name(format!("prompt-preview:{hover}").into()))
            .side(PopoverSide::Left)
            .width(POPOVER_WIDTH)
            .animate(!cx.reduce_motion())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(u(6.))
                    .p(u(12.))
                    .font_family(theme.fonts.sans.clone())
                    .child(
                        clamp(div())
                            .text_color(theme.colors.content)
                            .child(preview.title),
                    )
                    .when_some(preview.reply, |el, reply| {
                        el.child(clamp(div()).text_color(theme.content(0.45)).child(reply))
                    })
                    .when_some(preview.detail, |el, detail| {
                        el.child(
                            clamp(div())
                                .border_l(px(2.))
                                .border_color(theme.content(0.15))
                                .pl(u(12.))
                                .text_color(theme.content(0.35))
                                .child(detail),
                        )
                    }),
            );
        Some(popover_at(position, Anchor::RightCenter, card, cx).into_any_element())
    }
}

/// CSS `ease-out`.
fn gpui_ease_out() -> monocode_ui::theme::CubicBezier {
    monocode_ui::theme::Motion::EASE_OUT
}

impl Focusable for PromptOutline {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PromptOutline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pane_width = self.pane_width.clone();
        let measure = canvas(
            move |bounds, _, _| pane_width.set(f32::from(bounds.size.width)),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let root = div().absolute().top_0().left_0().size_full().child(measure);
        let narrow =
            self.pane_width.get() / window.rem_size().to_f64() as f32 * 16. < MIN_PANE_WIDTH;
        if self.prompts.len() < MIN_PROMPTS || narrow {
            return root.into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let animate = !cx.reduce_motion();
        let (start, end, gap) = self.stack();
        let bars: Vec<BlockRef> = self.prompts[start..end].to_vec();
        let hover_index = self
            .hover
            .as_ref()
            .and_then(|hover| bars.iter().position(|bar| &bar.id == hover));
        let tab_id = self.tab_id(&bars);
        let focused = self.focus.is_focused(window);
        let mut rail = div()
            .id("prompt-outline")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this.handle_key(&event.keystroke.key, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                if *hovered {
                    this.pointer_inside = true;
                } else {
                    this.leave_rail(window, cx);
                }
            }))
            .w(u(BAR_WIDTH_LIFTED_PX))
            .flex()
            .flex_col()
            .items_end();
        let mut moving = false;
        for (index, prompt) in bars.iter().enumerate() {
            let lift = bar_lift(index, hover_index);
            let distance = hover_index.map_or(0, |hover| index.abs_diff(hover));
            // The pointer owns the fill while it is on the rail. Off the rail,
            // the fill marks the scroll position.
            let lit = match hover_index {
                Some(hover) => index == hover,
                None => Some(prompt.id.as_str()) == self.active_id.as_deref(),
            };
            let delay = distance.min(RIPPLE_SPAN) as f32 * RIPPLE_STEP_MS;
            let (width, opacity, running) = self.bar_motion(
                &prompt.id,
                BAR_WIDTH_PX + (BAR_WIDTH_LIFTED_PX - BAR_WIDTH_PX) * lift,
                if lit {
                    BAR_OPACITY_LIT
                } else {
                    BAR_OPACITY_IDLE
                },
                delay,
                animate,
            );
            moving |= running;
            let id = prompt.id.clone();
            let hover_id = prompt.id.clone();
            let keyboard = focused && tab_id.as_deref() == Some(prompt.id.as_str());
            rail = rail.child(
                div()
                    .id(ElementId::Name(format!("prompt-bar:{}", prompt.id).into()))
                    .relative()
                    .flex()
                    .flex_none()
                    .w_full()
                    .items_center()
                    .justify_end()
                    .h(u(BAR_HEIGHT_PX + gap))
                    .cursor_pointer()
                    .tooltip(monocode_ui::widgets::tooltip(prompt_label(prompt)))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hover_bar(&hover_id, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.jump_to(&id, cx)))
                    .child(self.bars.track(prompt.id.clone()))
                    .child(
                        div()
                            .h(u(BAR_HEIGHT_PX))
                            .w(u(width))
                            .rounded_full()
                            .bg(theme.colors.content)
                            .opacity(opacity)
                            .when(keyboard, |el| {
                                el.shadow(vec![gpui::BoxShadow {
                                    color: theme.accent(0.6),
                                    offset: point(px(0.), px(0.)),
                                    blur_radius: px(0.),
                                    spread_radius: px(1.),
                                    inset: false,
                                }])
                            }),
                    ),
            );
        }
        if moving {
            window.request_animation_frame();
        }
        let popover = self.render_popover(&theme, cx);
        root.child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(u(16.))
                .flex()
                .items_center()
                .child(rail),
        )
        .children(popover)
        .into_any_element()
    }
}

/// The prompt bands of a transcript drawn in `viewport`: its band, one
/// anchor per prompt row, and how far it is from the bottom.
pub fn transcript_anchors(
    transcript: &TranscriptView,
    viewport: Bounds<Pixels>,
) -> (OutlineBand, Vec<OutlineAnchor>, f32) {
    let band = OutlineBand {
        top: f32::from(viewport.top()),
        bottom: f32::from(viewport.bottom()),
    };
    let rows = transcript.rows();
    let drawn: Vec<Option<Bounds<Pixels>>> = (0..rows.len())
        .map(|ix| transcript.row_bounds(ix))
        .collect();
    let first_drawn = drawn.iter().position(Option::is_some);
    let mut anchors = Vec::new();
    for (ix, row) in rows.iter().enumerate() {
        let RowKind::Item { .. } = &row.kind else {
            continue;
        };
        let Some(block) = row.first_block() else {
            continue;
        };
        if block.role != monocode_core::BlockRole::User || block.is_internal() {
            continue;
        }
        let (top, bottom) = match drawn[ix] {
            Some(bounds) => (f32::from(bounds.top()), f32::from(bounds.bottom())),
            // Rows above the drawn ones end above the viewport; the rest
            // start below it.
            None if first_drawn.is_some_and(|first| ix < first) => (band.top - 1., band.top),
            None => (band.bottom, band.bottom + 1.),
        };
        if anchors
            .iter()
            .any(|anchor: &OutlineAnchor| anchor.id == block.id)
        {
            continue;
        }
        anchors.push(OutlineAnchor {
            id: block.id.clone(),
            top,
            bottom,
        });
    }
    let distance = if transcript.is_scrolled_away() {
        f32::INFINITY
    } else {
        0.
    };
    (band, anchors, distance)
}

/// `jumpTo` against a transcript: scroll the prompt's turn to the top.
/// Unknown prompts scroll to the top of the transcript, as React did.
pub fn jump_to_prompt(
    transcript: &mut TranscriptView,
    block_id: &str,
    cx: &mut Context<TranscriptView>,
) -> bool {
    let turn = transcript
        .rows()
        .iter()
        .find(|row| {
            row.turn_id == block_id || row.first_block().is_some_and(|block| block.id == block_id)
        })
        .map(|row| row.turn_index);
    match turn {
        Some(turn) => {
            transcript.scroll_to_turn(turn, cx);
            true
        }
        None => {
            transcript.scroll_to_top(cx);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_every_bar_while_the_gap_can_shrink() {
        assert_eq!(bar_stack(3, None, 330.), (0, 3, 10.));
        // 100 bars need 2px each plus 1px gaps: 299px.
        let (start, end, gap) = bar_stack(100, None, 330.);
        assert_eq!((start, end), (0, 100));
        assert_eq!(gap, 1.);
    }

    #[test]
    fn slides_a_window_past_the_budget() {
        let (start, end, gap) = bar_stack(200, None, 330.);
        assert_eq!(end - start, 110);
        assert_eq!(end, 200);
        assert_eq!(gap, 1.);
        let (start, _, _) = bar_stack(200, Some(5), 330.);
        assert_eq!(start, 5);
        assert_eq!(bar_stack(1, None, 330.), (0, 1, 0.));
    }
}
