use crate::tooltip::Tip;
use crate::{popover::AnchorSlot, theme::*};
use gpui::{Role, SharedString, div, prelude::*, px};
use monocode_core::{
    agent::Settings,
    response::{Target, TransferKind},
    transcript::Row,
};

pub(crate) enum Request {
    RemotePlan {
        id: String,
        text: String,
        build: bool,
    },
    QuoteSelection {
        text: String,
    },
    EditLastTurn {
        turn: usize,
    },
    Draft {
        send: bool,
    },
    SaveNote {
        turn: usize,
        text: String,
        selection: bool,
    },
    Target {
        turn: usize,
        kind: TransferKind,
        anchor: AnchorSlot,
    },
}
#[derive(Clone)]
pub(crate) struct Transfer {
    pub kind: TransferKind,
    pub target: Target,
    pub from: String,
    pub turn: Vec<Row>,
    pub through: Vec<Row>,
    pub cwd: String,
    pub title: String,
    pub settings: Settings,
}
pub(crate) fn button(
    turn: usize,
    action: &'static str,
    label: impl Into<SharedString>,
    glyph: &'static str,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id((action, turn))
        .role(Role::Button)
        .aria_label(label.clone())
        .tip(label)
        .group("response-action")
        .flex_shrink_0()
        .size(px(22.))
        .p(px(4.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .text_color(content(if active { 0.7 } else { 0.4 }))
        .when(active, |d| d.bg(content(0.08)))
        .hover(|s| s.bg(content(0.08)).text_color(content(0.7)))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            icon(glyph, 14.)
                .text_color(content(if active { 0.7 } else { 0.4 }))
                .group_hover("response-action", |s| s.text_color(content(0.7))),
        )
}
