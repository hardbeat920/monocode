//! Application state/layout adapter for the shared two-frame mascot painter.
use crate::theme::content;
use gpui::{AnyElement, canvas, div, prelude::*, px, rgb};
use monocode_core::tool::ToolState;
use monocode_mascots::{
    CATALOG,
    gpui::{PixelPolicy, paint_mask},
    project_index,
};
use std::time::Duration;

/// A 12px sprite centered in the same 14px icon slot as the React row.
/// Activity is independent of state: a pending run can already have settled.
pub(super) fn subagent_mascot(name: &str, state: &ToolState, active: bool) -> AnyElement {
    let color = match state {
        ToolState::Failed | ToolState::Cancelled => rgb(0xf87171).into(),
        ToolState::Running => content(0.70),
        ToolState::Completed => content(0.45),
    };
    div()
        .size(px(14.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(color)
        .child(project_mascot(name, active))
        .into_any_element()
}

fn project_mascot(name: &str, active: bool) -> AnyElement {
    let index = project_index(name, None);
    let slot = div().size(px(12.)).flex_shrink_0();
    if active {
        // Reduced motion holds the first pose and requests no decorative frames.
        crate::pulse::looping(
            "subagent-mascot",
            Duration::from_millis(460),
            crate::pulse::Rate::Low,
            move |phase| slot.child(sprite(index, phase >= 0.5)),
        )
        .into_any_element()
    } else {
        slot.child(sprite(index, false)).into_any_element()
    }
}

fn sprite(index: usize, talk: bool) -> AnyElement {
    canvas(
        |_, window, _| window.text_style().color,
        move |mut bounds, color, window, _| {
            if talk {
                bounds.origin.y -= px(1.);
            }
            paint_mask(
                &CATALOG[index].frame(talk).rows,
                bounds,
                color,
                PixelPolicy::Crisp,
                window,
            );
        },
    )
    .size_full()
    .into_any_element()
}
