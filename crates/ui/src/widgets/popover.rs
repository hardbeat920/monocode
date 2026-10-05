//! The popover frame from src/shared/ui/Popover.tsx: a 12px rounded glass
//! frame (`rounded-xl border border-content/10 shadow-xl` over a blurred
//! backdrop) whose content plays `.popover-open`: 170ms, fading in while it
//! slides 8px from the anchor side.
//!
//! GPUI has no transforms on divs, so the open animation drops the CSS
//! `scale(0.94)` and keeps the fade and the slide.

use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, App, ElementId, IntoElement, ParentElement,
    Pixels, Point, RenderOnce, Styled as _, Window, anchored, deferred, div, px,
};

use crate::styled::glass_backdrop;
use crate::{Theme, u};

/// Which side of the anchor the popover opens on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PopoverSide {
    #[default]
    Bottom,
    Top,
    Left,
    Right,
}

/// Popover.tsx's default gap between anchor and popover, in CSS px.
pub const POPOVER_GAP: f32 = 6.0;
/// Popover.tsx's default viewport padding, in CSS px.
pub const POPOVER_PADDING: f32 = 8.0;

#[derive(IntoElement)]
pub struct PopoverFrame {
    id: ElementId,
    side: PopoverSide,
    width: Option<f32>,
    max_height: Option<f32>,
    animate: bool,
    children: Vec<AnyElement>,
}

/// A glass popover frame. `id` keys the open animation: a new id replays it.
pub fn popover_frame(id: impl Into<ElementId>) -> PopoverFrame {
    PopoverFrame {
        id: id.into(),
        side: PopoverSide::Bottom,
        width: None,
        max_height: None,
        animate: true,
        children: Vec::new(),
    }
}

impl PopoverFrame {
    pub fn side(mut self, side: PopoverSide) -> Self {
        self.side = side;
        self
    }

    /// Width in CSS px.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Maximum height in CSS px.
    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    /// Turns the open animation off, for screenshots and reduced motion.
    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }
}

impl ParentElement for PopoverFrame {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for PopoverFrame {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let motion = theme.motion;
        let radius = theme.radius.xl;
        let mut frame = div()
            .relative()
            .flex_none()
            .rounded(u(radius))
            .border_1()
            .border_color(theme.colors.popover_border)
            .shadow_xl()
            .overflow_hidden()
            .text_color(theme.colors.content)
            .child(glass_backdrop(radius, 24., theme.colors.popover_backdrop));
        if let Some(width) = self.width {
            frame = frame.w(u(width));
        }
        let mut content = div().relative().flex().flex_col();
        if let Some(height) = self.max_height {
            content = content.max_h(u(height));
        }
        let content = content.children(self.children);
        if !self.animate {
            return frame.child(content);
        }
        let side = self.side;
        let lift = motion.popover_lift;
        let animated = content.with_animation(
            self.id,
            Animation::new(motion.popover_open).with_easing(motion.popover_ease.easing()),
            move |el, t| {
                let offset = u(lift * (1.0 - t));
                let el = el.opacity(t);
                match side {
                    PopoverSide::Bottom => el.top(-offset),
                    PopoverSide::Top => el.top(offset),
                    PopoverSide::Right => el.left(-offset),
                    PopoverSide::Left => el.left(offset),
                }
            },
        );
        frame.child(animated)
    }
}

/// Places `content` at a window point in the popover layer, kept inside the
/// window by `POPOVER_PADDING`. `anchor` is the popover corner that sits on
/// the point.
pub fn popover_at(
    position: Point<Pixels>,
    anchor: Anchor,
    content: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    let layer = Theme::of(cx).layer.popover;
    deferred(
        anchored()
            .position(position)
            .anchor(anchor)
            .snap_to_window_with_margin(px(POPOVER_PADDING))
            .child(content),
    )
    .with_priority(layer)
}
