//! Port of src/features/sessions/ui/ContextMeter.tsx: a 14px ring for how
//! full the model's context window is. Hovering shows the numbers; with
//! compaction available, a click opens Compact now.

use gpui::{
    AnyElement, Bounds, Context, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    PathBuilder, Pixels, StatefulInteractiveElement as _, Styled as _, Window, canvas, div, point,
    prelude::FluentBuilder as _, px,
};
use monocode_core::context_usage::{context_ratio, context_tooltip};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::popover_frame;
use monocode_ui::{Theme, u};

use super::Composer;

const SIZE: f32 = 14.0;
const STROKE: f32 = 2.0;

/// Hover and click state.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeterState {
    pub hovered: bool,
    pub open: bool,
}

/// `ringClass`: amber, then red, as the window fills.
pub fn ring_color(ratio: f64, theme: &Theme) -> Hsla {
    if ratio >= 0.9 {
        theme.colors.danger
    } else if ratio >= 0.75 {
        theme.colors.warning
    } else {
        theme.content(0.45)
    }
}

/// `MeterRing`: a faint full circle and the filled arc from 12 o'clock.
pub fn meter_ring(ratio: f64, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let rem = window.rem_size();
            let size = u(SIZE).to_pixels(rem);
            let stroke = u(STROKE).to_pixels(rem);
            let radius = (size - stroke) / 2.;
            let center = point(
                bounds.origin.x + bounds.size.width / 2.,
                bounds.origin.y + bounds.size.height / 2.,
            );
            let arc = |from: f32, to: f32| -> Option<gpui::Path<Pixels>> {
                let steps = 48;
                let mut path = PathBuilder::stroke(stroke);
                for step in 0..=steps {
                    let t = from + (to - from) * step as f32 / steps as f32;
                    let angle = t * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                    let at = point(
                        center.x + radius * angle.cos(),
                        center.y + radius * angle.sin(),
                    );
                    if step == 0 {
                        path.move_to(at);
                    } else {
                        path.line_to(at);
                    }
                }
                path.build().ok()
            };
            if let Some(track) = arc(0., 1.) {
                window.paint_path(
                    track,
                    Hsla {
                        a: color.a * 0.25,
                        ..color
                    },
                );
            }
            let ratio = ratio.clamp(0., 1.) as f32;
            if ratio > 0.
                && let Some(fill) = arc(0., ratio)
            {
                window.paint_path(fill, color);
            }
        },
    )
    .size(u(SIZE))
    .flex_none()
}

impl Composer {
    pub(crate) fn render_context_meter(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let usage = self.props.context.as_ref()?;
        let ratio = context_ratio(Some(usage))?;
        let theme = Theme::of(cx).clone();
        let tooltip = context_tooltip(usage);
        let compactable = self.props.compact_supported && !self.props.worktree_removed;
        let actions_open = self.meter.open && compactable;
        let color = ring_color(ratio, &theme);
        let ring = div()
            .id("context-meter")
            .p(u(4.))
            .m(u(-4.))
            .rounded(u(theme.radius.sm))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.meter.hovered = *hovered;
                cx.notify();
            }))
            .when(compactable, |el| {
                el.on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.meter.open = !this.meter.open;
                    cx.notify();
                }))
            })
            .child(meter_ring(ratio, color));
        let mut root = div().relative().flex_none().child(ring);
        if self.meter.hovered || actions_open {
            let busy = self.props.busy;
            let mut body = div()
                .px(u(10.))
                .py(u(6.))
                .child(
                    div()
                        .text_px(12.)
                        .line_height(u(16.))
                        .text_color(theme.colors.content)
                        .whitespace_nowrap()
                        .child(tooltip.headline.clone()),
                )
                .child(
                    div()
                        .text_px(11.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.50))
                        .whitespace_nowrap()
                        .child(tooltip.detail.clone()),
                );
            if actions_open {
                let hover = theme.content(0.15);
                body = body.child(
                    div()
                        .id("context-compact")
                        .mt(u(6.))
                        .w_full()
                        .rounded(u(theme.radius.md))
                        .bg(theme.content(0.10))
                        .px(u(8.))
                        .py(u(4.))
                        .text_px(11.)
                        .text_center()
                        .text_color(theme.colors.content)
                        .when(busy, |el| el.opacity(0.4))
                        .when(!busy, |el| el.hover(move |style| style.bg(hover)))
                        .tooltip(monocode_ui::widgets::tooltip(if busy {
                            "Wait for the current operation to finish"
                        } else {
                            "Compact this conversation's context"
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            if busy {
                                return;
                            }
                            this.meter.open = false;
                            let host = this.host.clone();
                            host.compact_context(window, cx);
                            cx.notify();
                        }))
                        .child("Compact now"),
                );
            }
            let popover = popover_frame("context-meter-popover")
                .animate(self.props.animate)
                .child(body);
            // `align="end"`: the popover's right edge sits on the ring's.
            let gap = u(6.).to_pixels(window.rem_size());
            let slot = div().absolute().top_0().right_0().size_0().child(
                gpui::deferred(
                    gpui::anchored()
                        .anchor(gpui::Anchor::BottomRight)
                        .offset(point(px(0.), -gap))
                        .snap_to_window_with_margin(px(monocode_ui::widgets::POPOVER_PADDING))
                        .child(popover),
                )
                .with_priority(theme.layer.popover),
            );
            root = root.child(slot);
        }
        Some(root.into_any_element())
    }
}
