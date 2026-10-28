// The prompt outline rail, ported from src/features/sessions/ui/PromptOutline.tsx
// and its model in src/features/sessions/model/promptOutline.ts.
use super::TranscriptView;
use crate::popover::{Align, Anchor, AnchorSlot, Side, popover};
use crate::theme::*;
use gpui::{Context, ListOffset, Role, Task, Window, div, prelude::*, px};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const OPEN_DELAY: Duration = Duration::from_millis(25);
const POPOVER_WIDTH: f32 = 288.;
const MIN_PROMPTS: usize = 2;
const BAR_HEIGHT: f32 = 2.;
const BAR_WIDTH: f32 = 11.;
const BAR_WIDTH_LIFTED: f32 = 24.;
const BAR_OPACITY_IDLE: f32 = 0.15;
const BAR_OPACITY_LIT: f32 = 0.85;
const RIPPLE_STEP: f32 = 0.018;
const BAR_TRANSITION: f32 = 0.2;
const BAR_GAP: f32 = 10.;
const BAR_GAP_MIN: f32 = 1.;
const BAR_STACK_MAX: f32 = 330.;
const BAR_STACK_PANE_SHARE: f32 = 0.75;
// The React rail hides below a 58rem container.
const MIN_PANE_WIDTH: f32 = 928.;
const NEAR_END: f32 = 16.;
/// How far the hover ripple reaches, in bars on each side.
const RIPPLE_SPAN: usize = 2;
const REPLY_SCAN_CHARS: usize = 2000;

/// A bar's width and opacity, eased like the rail's CSS transition.
#[derive(Clone, Copy)]
struct Tween {
    from: (f32, f32),
    to: (f32, f32),
    start: Instant,
}
impl Tween {
    fn at(&self, now: Instant) -> (f32, f32) {
        let t = now.saturating_duration_since(self.start).as_secs_f32() / BAR_TRANSITION;
        let k = crate::reveal::bezier(t.clamp(0., 1.), 0., 0., 0.58, 1.);
        (
            self.from.0 + (self.to.0 - self.from.0) * k,
            self.from.1 + (self.to.1 - self.from.1) * k,
        )
    }
    fn done(&self, now: Instant) -> bool {
        now >= self.start + Duration::from_secs_f32(BAR_TRANSITION)
    }
}

#[derive(Default)]
pub(super) struct Outline {
    /// Source indices of the prompts, in order.
    prompts: Vec<usize>,
    active: Option<usize>,
    hovered: Option<usize>,
    open: bool,
    opening: Option<Task<()>>,
    anchors: HashMap<usize, AnchorSlot>,
    tweens: HashMap<usize, Tween>,
}
impl Outline {
    /// Patches only replace or extend the source from `start`.
    pub(super) fn patched(&mut self, source: &[monocode_core::transcript::Row], start: usize) {
        self.prompts.retain(|&ix| ix < start);
        let from = self.prompts.last().map_or(0, |&ix| ix + 1).max(start);
        self.prompts.extend(
            (from..source.len()).filter(|&ix| source[ix].first && source[ix].role == "user"),
        );
    }
}

impl TranscriptView {
    /// The display rows of the prompt that starts at source index `start`.
    fn prompt_rows(&self, start: usize) -> Option<(usize, usize)> {
        let message = self
            .messages
            .binary_search_by_key(&start, |m| m.start)
            .ok()?;
        let first = self.messages[message].display_start;
        let end = self
            .messages
            .get(message + 1)
            .map_or(self.rows.len(), |m| m.display_start);
        (end > first).then_some((first, end))
    }
    /// Read after the list lays out, like the React rail's deferred measure.
    pub(super) fn update_outline(&mut self, cx: &mut Context<Self>) {
        if self.mono || !self.visible || self.viewport.size.height <= px(0.) {
            return;
        }
        let active = self.measure_active();
        if self.outline.active != active {
            self.outline.active = active;
            cx.notify();
        }
    }
    fn measure_active(&self) -> Option<usize> {
        let prompts = &self.outline.prompts;
        let last = *prompts.last()?;
        let scrolled = -f32::from(self.list.scroll_px_offset_for_scrollbar().y);
        let max = f32::from(self.list.max_offset_for_scrollbar().y);
        if self.list.is_following_tail() || max - scrolled <= NEAR_END {
            return Some(last);
        }
        let mut above = None;
        for &prompt in prompts {
            let Some((first, end)) = self.prompt_rows(prompt) else {
                continue;
            };
            if self.list.item_is_above_viewport(end - 1) == Some(true) {
                above = Some(prompt);
                continue;
            }
            // Unmeasured rows lie past the viewport, as do all later prompts.
            if self.list.item_is_below_viewport(first) != Some(false) {
                break;
            }
            return Some(prompt);
        }
        above.or(prompts.first().copied())
    }
    fn hover_prompt(&mut self, prompt: usize, cx: &mut Context<Self>) {
        self.outline.hovered = Some(prompt);
        cx.notify();
        if self.outline.open || self.outline.opening.is_some() {
            return;
        }
        // The ripple follows the pointer at once. The card waits out a pass-through.
        self.outline.opening = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(OPEN_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.outline.opening = None;
                this.outline.open = true;
                cx.notify();
            });
        }));
    }
    fn close_outline(&mut self, cx: &mut Context<Self>) {
        self.outline.opening = None;
        self.outline.hovered = None;
        self.outline.open = false;
        cx.notify();
    }
    fn jump_to_prompt(&mut self, prompt: usize, cx: &mut Context<Self>) {
        let Some((first, _)) = self.prompt_rows(prompt) else {
            return;
        };
        self.list.scroll_to(ListOffset {
            item_ix: first,
            offset_in_item: px(0.),
        });
        cx.notify();
    }
    pub(super) fn outline(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let prompts = &self.outline.prompts;
        if self.mono
            || self.document
            || !self.visible
            || prompts.len() < MIN_PROMPTS
            || f32::from(self.viewport.size.width) < MIN_PANE_WIDTH
        {
            self.outline.tweens.clear();
            return None;
        }
        let budget = BAR_STACK_MAX
            .min((f32::from(self.viewport.size.height) * BAR_STACK_PANE_SHARE).floor());
        let active_ix = self
            .outline
            .active
            .and_then(|active| prompts.iter().position(|&p| p == active));
        let (start, end, gap) = bar_stack(prompts.len(), active_ix, budget);
        let bars: Vec<usize> = prompts[start..end].to_vec();
        let hover_ix = self
            .outline
            .hovered
            .and_then(|hovered| bars.iter().position(|&p| p == hovered));
        let now = Instant::now();
        let reduced = cx.reduce_motion();
        self.outline
            .tweens
            .retain(|prompt, _| bars.contains(prompt));
        self.outline
            .anchors
            .retain(|prompt, _| bars.contains(prompt));
        let mut animating = false;
        let mut rail = div()
            .id("prompt-outline")
            .debug_selector(|| "prompt-outline".into())
            .role(Role::Toolbar)
            .aria_label("Prompts")
            .w(px(BAR_WIDTH_LIFTED))
            .flex()
            .flex_col()
            .items_end()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !hovered {
                    this.close_outline(cx);
                }
            }));
        for (index, &prompt) in bars.iter().enumerate() {
            let lift = bar_lift(index, hover_ix);
            let distance = hover_ix.map_or(0, |h| index.abs_diff(h));
            // The pointer owns the fill while it is on the rail. Off the rail,
            // the fill goes back to marking the scroll position.
            let lit = match hover_ix {
                Some(h) => index == h,
                None => Some(prompt) == self.outline.active,
            };
            let target = (
                BAR_WIDTH + (BAR_WIDTH_LIFTED - BAR_WIDTH) * lift,
                if lit {
                    BAR_OPACITY_LIT
                } else {
                    BAR_OPACITY_IDLE
                },
            );
            let tween = self.outline.tweens.entry(prompt).or_insert(Tween {
                from: target,
                to: target,
                start: now,
            });
            if tween.to != target {
                // The wave reaches the outer bars a beat after the hovered one.
                let delay = distance.min(RIPPLE_SPAN) as f32 * RIPPLE_STEP;
                *tween = if reduced {
                    Tween {
                        from: target,
                        to: target,
                        start: now,
                    }
                } else {
                    Tween {
                        from: tween.at(now),
                        to: target,
                        start: now + Duration::from_secs_f32(delay),
                    }
                };
            }
            animating |= !tween.done(now);
            let (width, opacity) = tween.at(now);
            let anchor = self.outline.anchors.entry(prompt).or_default().clone();
            rail = rail.child(
                div()
                    .id(("prompt-bar", prompt))
                    .role(Role::Button)
                    .aria_label(prompt_label(&self.message_text(prompt)))
                    .relative()
                    .flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .justify_end()
                    .h(px(BAR_HEIGHT + gap))
                    .child(anchor.probe())
                    .child(
                        div()
                            .h(px(BAR_HEIGHT))
                            .w(px(width))
                            .rounded_full()
                            .bg(content(opacity)),
                    )
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hover_prompt(prompt, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.jump_to_prompt(prompt, cx))),
            );
        }
        if animating {
            window.request_animation_frame();
        }
        if self.outline.open
            && let Some(hovered) = self.outline.hovered.filter(|_| hover_ix.is_some())
            && let Some(anchor) = self.outline.anchors.get(&hovered)
        {
            let preview = self.preview(hovered);
            let line = |text: String, alpha: f32| {
                div()
                    .text_size(px(14.))
                    .line_height(px(19.25))
                    .line_clamp(2)
                    .text_ellipsis()
                    .text_color(content(alpha))
                    .child(text)
            };
            rail = rail.child(
                popover("prompt-preview", Anchor::Slot(anchor.clone()))
                    .side(Side::Left)
                    .align(Align::Center)
                    .gap(10.)
                    .width(POPOVER_WIDTH)
                    .padding(12.)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(line(preview.title, 1.))
                            .when_some(preview.reply, |d, reply| d.child(line(reply, 0.45)))
                            .when_some(preview.detail, |d, detail| {
                                d.child(
                                    div()
                                        .border_l_2()
                                        .border_color(content(0.15))
                                        .pl_3()
                                        .child(line(detail, 0.35)),
                                )
                            }),
                    ),
            );
        }
        Some(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(16.))
                .flex()
                .items_center()
                .child(rail)
                .into_any_element(),
        )
    }
    fn message_text(&self, start: usize) -> String {
        let end = (start + 1..self.source.len())
            .find(|&ix| self.source[ix].first)
            .unwrap_or(self.source.len());
        self.source[start..end]
            .iter()
            .map(|r| r.text.as_str())
            .collect()
    }
    /// The prompt and the head of its reply, shown while a bar is hovered.
    fn preview(&self, prompt: usize) -> Preview {
        let mut reply = Vec::new();
        for ix in prompt + 1..self.source.len() {
            let row = &self.source[ix];
            if !row.first {
                continue;
            }
            if row.role == "user" {
                break;
            }
            if row.role != "assistant" {
                continue;
            }
            reply = preview_lines(&self.message_text(ix), 2);
            if !reply.is_empty() {
                break;
            }
        }
        let mut reply = reply.into_iter();
        Preview {
            title: prompt_label(&self.message_text(prompt)),
            reply: reply.next(),
            detail: reply.next(),
        }
    }
}

struct Preview {
    title: String,
    reply: Option<String>,
    detail: Option<String>,
}

fn prompt_label(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "Empty message".into())
}

/// Selects at most `max` prompts. The window slides to keep the active
/// prompt inside. It prefers the newest prompts.
fn bar_window(count: usize, active: Option<usize>, max: usize) -> (usize, usize) {
    if count <= max {
        return (0, count);
    }
    let newest = count - max;
    let start = active.map_or(newest, |active| active.min(newest));
    (start, start + max)
}

/// One bar per prompt while the bars fit the budget. The gap shrinks first.
/// Past that, a window slides.
fn bar_stack(count: usize, active: Option<usize>, budget: f32) -> (usize, usize, f32) {
    let fit = (((budget + BAR_GAP_MIN) / (BAR_HEIGHT + BAR_GAP_MIN)).floor() as usize).max(1);
    let (start, end) = bar_window(count, active, fit);
    let shown = end - start;
    let gap = if shown > 1 {
        ((budget - shown as f32 * BAR_HEIGHT) / (shown - 1) as f32)
            .floor()
            .clamp(BAR_GAP_MIN, BAR_GAP)
    } else {
        0.
    };
    (start, end, gap)
}

/// Dock-style magnification: 1 on the hovered bar, tapering to 0 past the ripple span.
fn bar_lift(index: usize, hover: Option<usize>) -> f32 {
    let Some(hover) = hover else {
        return 0.;
    };
    let distance = index.abs_diff(hover);
    if distance > RIPPLE_SPAN {
        return 0.;
    }
    (RIPPLE_SPAN + 1 - distance) as f32 / (RIPPLE_SPAN + 1) as f32
}

/// The first `max` prose lines of a reply. Markers, fenced code and rules drop out.
fn preview_lines(text: &str, max: usize) -> Vec<String> {
    let head: String = text.chars().take(REPLY_SCAN_CHARS).collect();
    let mut lines = Vec::new();
    let mut fenced = false;
    for raw in head.lines() {
        let line = raw.trim();
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let plain = strip_marker(line)
            .replace("**", "")
            .replace('`', "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        // Rules, table separators and lone punctuation read as noise in a preview.
        if !plain.chars().any(char::is_alphanumeric) {
            continue;
        }
        lines.push(plain);
        if lines.len() == max {
            break;
        }
    }
    lines
}

/// Drops a leading heading, quote, bullet or ordered-list marker.
fn strip_marker(line: &str) -> &str {
    let marker = if line.starts_with(['#', '>']) {
        line.find(|c| c != '#' && c != '>').unwrap_or(line.len())
    } else if line.starts_with(['-', '*', '+']) {
        1
    } else {
        let digits = line
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(line.len());
        if digits > 0 && line[digits..].starts_with(['.', ')']) {
            digits + 1
        } else {
            0
        }
    };
    let rest = &line[marker..];
    if marker > 0 && rest.starts_with(char::is_whitespace) {
        rest.trim_start()
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::transcript::Row;

    fn row(role: &str, text: &str, first: bool) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first,
            sent_at: None,
        }
    }

    #[test]
    fn prompts_track_patches_from_their_start() {
        let mut outline = Outline::default();
        let mut source = vec![
            row("user", "one", true),
            row("assistant", "a", true),
            row("user", "two", true),
            row("user", " more", false),
        ];
        outline.patched(&source, 0);
        assert_eq!(outline.prompts, [0, 2]);
        source.truncate(2);
        source.push(row("assistant", "b", true));
        source.push(row("user", "three", true));
        outline.patched(&source, 2);
        assert_eq!(outline.prompts, [0, 3]);
    }

    #[test]
    fn labels_use_the_first_non_empty_line() {
        assert_eq!(prompt_label("\n  fix   the\tbug \nmore"), "fix the bug");
        assert_eq!(prompt_label("  \n"), "Empty message");
    }

    #[test]
    fn window_keeps_the_active_prompt_and_prefers_the_newest() {
        assert_eq!(bar_window(5, None, 10), (0, 5));
        assert_eq!(bar_window(20, None, 10), (10, 20));
        assert_eq!(bar_window(20, Some(3), 10), (3, 13));
        assert_eq!(bar_window(20, Some(18), 10), (10, 20));
    }

    #[test]
    fn stack_shrinks_the_gap_before_windowing() {
        assert_eq!(bar_stack(8, None, 330.), (0, 8, 10.));
        assert_eq!(bar_stack(100, None, 330.), (0, 100, 1.));
        assert_eq!(bar_stack(200, None, 330.), (90, 200, 1.));
    }

    #[test]
    fn lift_tapers_across_the_ripple() {
        assert_eq!(bar_lift(4, None), 0.);
        assert_eq!(bar_lift(4, Some(4)), 1.);
        assert!((bar_lift(3, Some(4)) - 2. / 3.).abs() < 1e-6);
        assert!((bar_lift(6, Some(4)) - 1. / 3.).abs() < 1e-6);
        assert_eq!(bar_lift(7, Some(4)), 0.);
    }

    #[test]
    fn preview_lines_skip_markup_and_code() {
        let text = "# Plan\n\n```rust\nlet x = 1;\n```\n---\n- **First** `step`\n2. Second\nThird";
        assert_eq!(preview_lines(text, 2), ["Plan", "First step"]);
        assert_eq!(preview_lines("> quoted\n|---|", 2), ["quoted"]);
        assert_eq!(preview_lines("#hashtag", 1), ["#hashtag"]);
    }
}
