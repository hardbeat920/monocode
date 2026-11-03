//! A session's provider mark, working spinner, circled done check and amber
//! attention mark, for sidebar rows and title tabs.
use crate::pulse::{self, Rate};
use crate::theme::*;
use gpui::{AnyElement, Context, ElementId, Hsla, Render, Role, Window, div, prelude::*, px, rgb};
use std::f32::consts::TAU;
use std::time::Duration;

/// One wave across the grid, as TerminalSpinner's `monocode-spinner-wave`.
pub(crate) const SPINNER_PERIOD: Duration = Duration::from_millis(1_200);
/// Slate, peach and pink, top row to bottom.
pub(crate) const SPINNER_ROWS: [u32; 3] = [0x6f84a3, 0xe8b583, 0xe47fa6];

/// The working spinner turning on the shared clock at `rate`.
pub(crate) fn spinner(id: impl Into<ElementId>, rate: Rate) -> pulse::Looping {
    pulse::looping(id, SPINNER_PERIOD, rate, spinner_at)
}

/// TerminalSpinner's 12px 3×3 grid at `fraction` of its period: a soft wave
/// washes from the top-left corner to the bottom-right, and dots off the
/// center diagonal peak dimmer.
pub(crate) fn spinner_at(fraction: f32) -> gpui::Div {
    let phase = fraction * TAU;
    div()
        .size(px(12.))
        .flex()
        .flex_col()
        .children((0..3).map(|row| {
            div().flex().children((0..3).map(move |col| {
                let wave = (0.5 - 0.5 * (phase - (col + row) as f32 * 0.9).cos()).powf(3.52);
                let peak = 0.93 * (1. - 0.65 * (col as f32 - row as f32).abs() / 2.);
                let alpha = 0.07 + peak * wave;
                let mut color: Hsla = rgb(SPINNER_ROWS[row]).into();
                color.a = alpha;
                div()
                    .size(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(2.5 * (0.89 + 0.11 * peak * wave)))
                            .rounded_full()
                            .bg(color),
                    )
            }))
        }))
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Idle,
    Working,
    Done,
    /// Waiting on an approval or a question.
    Attention,
}

/// A small cached view so animation leaves the conversation untouched.
pub(crate) struct SessionMark {
    side: f32,
    provider: String,
    state: State,
}
impl SessionMark {
    pub(crate) fn new(provider: String) -> Self {
        Self {
            side: 16.,
            provider,
            state: State::Idle,
        }
    }
    /// A title tab's mark. Tabs paint inside their own cached frame, so they
    /// can't share the sidebar's entity: a notify only repaints the cached
    /// views along the one path the entity was last drawn in.
    pub(crate) fn tab(provider: String) -> Self {
        Self {
            side: 14.,
            ..Self::new(provider)
        }
    }
    #[cfg(test)]
    pub(crate) fn state(&self) -> State {
        self.state
    }
    pub(crate) fn set_size(&mut self, side: f32, cx: &mut Context<Self>) {
        if self.side != side {
            self.side = side;
            cx.notify();
        }
    }
    pub(crate) fn set(&mut self, provider: &str, state: State, cx: &mut Context<Self>) {
        if self.state == state && self.provider == provider {
            return;
        }
        self.provider = provider.to_owned();
        self.state = state;
        cx.notify();
    }
}
impl Render for SessionMark {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let side = self.side;
        let mark: AnyElement = match self.state {
            State::Idle => provider_icon(&self.provider, side).into_any_element(),
            // Its own small view, so the clock repaints only the mark.
            State::Working => div()
                .id("session-working")
                .child(spinner("session-working-spinner", Rate::Smooth))
                .role(Role::Image)
                .aria_label("Working")
                .into_any_element(),
            // Tailwind teal-400 and amber-400, as TitleBar.tsx.
            State::Done => div()
                .id("session-done")
                .role(Role::Image)
                .aria_label("Done")
                .child(icon("check-circle-2", 14.).text_color(rgb(0x2dd4bf)))
                .into_any_element(),
            State::Attention => div()
                .id("session-attention")
                .role(Role::Image)
                .aria_label("Needs approval")
                .child(icon("alert-circle", 14.).text_color(rgb(0xfbbf24)))
                .into_any_element(),
        };
        div()
            .debug_selector(|| "session-provider-mark".into())
            .size(px(side))
            .flex()
            .items_center()
            .justify_center()
            .child(mark)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    #[gpui::test]
    fn working_mark_spins_on_the_shared_clock_and_stops_after_completion(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(false));
        let (mark, cx) = cx.add_window_view(|_, _| SessionMark::new("codex".into()));
        mark.update(cx, |mark, cx| mark.set("codex", State::Working, cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(pulse::running(cx)));
        mark.update(cx, |mark, cx| mark.set("codex", State::Done, cx));
        cx.run_until_parked();
        // Two background-paced strides: one notifies, the next finds no loop.
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!pulse::running(cx)));
    }
}
