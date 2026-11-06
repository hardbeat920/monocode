//! The lead's subagents on the composer's ledge: each one's state at a glance,
//! and a click opens it in the pane beside the lead.
use super::*;

/// Where a subagent stands, as the strip shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SubagentState {
    Working,
    /// Waiting on an approval or an answer from the user.
    NeedsYou,
    Failed,
    Done,
    /// Planned, saved or idle: not running and not finished.
    Waiting,
}
impl SubagentState {
    /// From a summary task's status, whether its run is live, and whether
    /// its chat is waiting on the user.
    pub(crate) fn of(status: &str, live: bool, needs_input: bool) -> Self {
        if needs_input || status == "blocked" {
            Self::NeedsYou
        } else if matches!(status, "failed" | "interrupted") {
            Self::Failed
        } else if status == "running" && live {
            Self::Working
        } else if status == "completed" {
            Self::Done
        } else {
            Self::Waiting
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::NeedsYou => "needs you",
            Self::Failed => "failed",
            Self::Done => "done",
            Self::Waiting => "waiting",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Subagent {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) provider: String,
    pub(crate) state: SubagentState,
}

impl Conversation {
    pub(crate) fn set_subagents(&mut self, subagents: Vec<Subagent>, cx: &mut Context<Self>) {
        if self.subagents != subagents {
            self.subagents = subagents;
            cx.notify();
        }
    }
    pub(super) fn subagent_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.subagents.is_empty() {
            return None;
        }
        let needs_you = self
            .subagents
            .iter()
            .filter(|s| s.state == SubagentState::NeedsYou)
            .count();
        let card = composer_shelf::card("subagent-strip", self.classic_layout)
            .debug_selector(|| "subagent-strip".into())
            .role(Role::Group)
            .aria_label("Subagents")
            .h(px(34.))
            .flex()
            .items_center()
            .gap(px(14.))
            .overflow_hidden()
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(11.))
                    .text_color(content(0.45))
                    .child("Subagents"),
            )
            .children(
                self.subagents
                    .iter()
                    .enumerate()
                    .map(|(ix, subagent)| chip(ix, subagent, cx)),
            )
            .when(needs_you > 0, |d| {
                d.child(div().flex_1()).child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(gpui::rgb(AMBER))
                        .child(if needs_you == 1 {
                            "1 needs you".to_owned()
                        } else {
                            format!("{needs_you} need you")
                        }),
                )
            });
        Some(
            composer_shelf::shelf("subagents")
                .px(px(8.))
                .child(card)
                .into_any_element(),
        )
    }
}

const AMBER: u32 = 0xfbbf24;
const RED: u32 = 0xf87171;
const GREEN: u32 = 0x34d399;

pub(crate) fn mark(ix: usize, state: SubagentState) -> AnyElement {
    let dot = |color: gpui::Hsla| div().size(px(6.)).rounded_full().bg(color);
    let mark = div()
        .size(px(14.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center();
    match state {
        SubagentState::Working => mark
            .child(crate::session_mark::spinner(
                ("subagent-spinner", ix),
                crate::pulse::Rate::Half,
            ))
            .into_any_element(),
        SubagentState::NeedsYou => mark.child(dot(gpui::rgb(AMBER).into())).into_any_element(),
        SubagentState::Failed => mark.child(dot(gpui::rgb(RED).into())).into_any_element(),
        SubagentState::Done => mark
            .child(icon("check", 12.).text_color(gpui::rgb(GREEN)))
            .into_any_element(),
        SubagentState::Waiting => mark.child(dot(content(0.35))).into_any_element(),
    }
}

fn chip(ix: usize, subagent: &Subagent, cx: &mut Context<Conversation>) -> AnyElement {
    let id = subagent.id;
    div()
        .id(("subagent-chip", ix))
        .debug_selector(move || format!("subagent-chip-{id}"))
        .role(gpui::Role::Button)
        .aria_label(format!(
            "Open subagent {}, {}",
            subagent.title,
            subagent.state.label()
        ))
        .min_w_0()
        .max_w(px(200.))
        .flex()
        .items_center()
        .gap(px(6.))
        .px_1()
        .py_0p5()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(content(0.06)))
        .child(mark(ix, subagent.state))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(content(0.8))
                .child(subagent.title.clone()),
        )
        .on_click(cx.listener(move |_, _, _, cx| cx.emit(OpenThread(id))))
        .into_any_element()
}
