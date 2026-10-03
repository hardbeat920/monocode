//! Small pieces the transcript rows share: phase icons, chevrons, the rails
//! work hangs off, the spinning ring of a running call, and the pulse of a
//! thought that is still streaming.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, ElementId, Hsla, Image, ImageFormat, IntoElement,
    ParentElement as _, Styled as _, Transformation, div, img, percentage, px,
};
use monocode_core::HarnessId;
use monocode_ui::{IconName, ProviderLogo, Theme, icon, provider_logo, u};

use crate::transcript::model::ActivityPhaseKind;

use super::style::rail_color;

/// `/monocode.png`, the app mark on MonoCode CLI rows.
static MONOCODE_MARK: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../assets/monocode.png").to_vec(),
    ))
});

/// `MonoCodeMark`.
pub fn monocode_mark(size: f32) -> impl IntoElement {
    img(MONOCODE_MARK.clone()).flex_none().size(u(size))
}

/// `ActivityPhaseIcon`: what a group was for, at a glance. A thought has
/// no icon.
pub fn phase_icon(kind: ActivityPhaseKind, theme: &Theme) -> Option<gpui::Svg> {
    let name = match kind {
        ActivityPhaseKind::Edit => IconName::PenLine,
        ActivityPhaseKind::Research => IconName::Search,
        ActivityPhaseKind::Run => IconName::Terminal,
        ActivityPhaseKind::Agent => IconName::Bot,
        ActivityPhaseKind::Think => return None,
        ActivityPhaseKind::Other => IconName::Wrench,
        ActivityPhaseKind::Note => IconName::Minus,
    };
    Some(icon(name).size(u(14.)).text_color(theme.content(0.45)))
}

/// `HarnessIcon` at 14px.
pub fn harness_icon(harness: HarnessId) -> AnyElement {
    match ProviderLogo::from_id(harness.as_str()) {
        Some(logo) => provider_logo(logo).size(14.).into_any_element(),
        None => div().size(u(14.)).into_any_element(),
    }
}

/// A chevron that points down when `open` (`rotate-90` on ChevronRight).
pub fn chevron(open: bool, color: Hsla) -> gpui::Svg {
    icon(if open {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    })
    .size(u(14.))
    .text_color(color)
}

/// `CircleDashed` that turns slowly while the call runs (`.zen-tool-spin`).
pub fn pending_ring(id: impl Into<ElementId>, color: Hsla, spin: bool) -> AnyElement {
    let ring = icon(IconName::CircleDashed).size(u(14.)).text_color(color);
    if !spin {
        return ring.into_any_element();
    }
    ring.with_animation(
        id,
        Animation::new(Duration::from_millis(3600)).repeat(),
        |ring, delta| ring.with_transformation(Transformation::rotate(percentage(delta))),
    )
    .into_any_element()
}

/// `.zen-thinking-pulse`: opacity breathing between 35% and 90%.
pub fn pulse<E: IntoElement + gpui::Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
) -> AnyElement {
    element
        .with_animation(
            id,
            Animation::new(Duration::from_millis(1800)).repeat(),
            |el, delta| {
                let wave = 1. - (delta * 2. - 1.).abs();
                el.opacity(0.35 + 0.55 * wave)
            },
        )
        .into_any_element()
}

/// The curve a step branches off a spine on: a quarter circle from the
/// spine at `left` into the row (`::after` on `.zen-phase-step` and
/// `.zen-fold-rail`).
pub fn rail_branch(left: f32, theme: &Theme) -> impl IntoElement {
    div()
        .absolute()
        .left(u(left))
        .top(u(6.))
        .size(u(8.))
        .border_l(px(1.))
        .border_b(px(1.))
        .border_color(rail_color(theme))
        .rounded_bl(u(8.))
}

/// The spine down a group (`::before`). The last row ends it at its branch.
pub fn rail_spine(left: f32, last: bool, theme: &Theme) -> impl IntoElement {
    let spine = div()
        .absolute()
        .left(u(left))
        .top_0()
        .w(px(1.))
        .bg(rail_color(theme));
    if last {
        spine.h(u(7.))
    } else {
        spine.bottom_0()
    }
}

/// `.zen-phase-step`: a step on a phase's rail, 20px in.
pub fn phase_step(last: bool, theme: &Theme, child: impl IntoElement) -> impl IntoElement {
    div()
        .relative()
        .pl(u(20.))
        .child(rail_spine(6., last, theme))
        .child(rail_branch(6., theme))
        .child(child)
}
