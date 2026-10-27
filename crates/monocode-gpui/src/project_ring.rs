//! A project's ring on the focused layout's project rail: an arc turning
//! around its icon while a session in the project responds, and a still
//! amber ring while one waits on the user.
use crate::pulse::{self, Rate};
use crate::session_mark::State;
use crate::theme::*;
use gpui::{
    AnyElement, Context, Hsla, PathBuilder, Render, Role, Window, canvas, div, point, prelude::*,
    px, rgb,
};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

/// The ring's side, around a 28px tile.
pub(crate) const RING_SIDE: f32 = 38.;
const PERIOD: Duration = Duration::from_millis(1_100);
/// Tailwind amber-400, as the session attention mark.
const ATTENTION: u32 = 0xfbbf24;

/// Each project's state from `(project, working, attention)` per session.
/// Waiting on the user outranks working; idle projects are left out.
pub(crate) fn project_states<'a>(
    sessions: impl IntoIterator<Item = (&'a str, bool, bool)>,
) -> HashMap<String, State> {
    let mut states = HashMap::new();
    for (project, working, attention) in sessions {
        let state = if attention {
            State::Attention
        } else if working {
            State::Working
        } else {
            continue;
        };
        let entry = states.entry(project.to_owned()).or_insert(state);
        if state == State::Attention {
            *entry = State::Attention;
        }
    }
    states
}

/// A small cached view so the turning arc repaints only itself.
pub(crate) struct ProjectRing {
    state: State,
}
impl ProjectRing {
    pub(crate) fn new() -> Self {
        Self { state: State::Idle }
    }
    #[cfg(test)]
    pub(crate) fn state(&self) -> State {
        self.state
    }
    pub(crate) fn set(&mut self, state: State, cx: &mut Context<Self>) {
        if self.state != state {
            self.state = state;
            cx.notify();
        }
    }
}

/// A 2px arc of `sweep` radians from `start`, clockwise from twelve.
fn arc(start: f32, sweep: f32, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let center = bounds.center();
            let radius = RING_SIDE / 2. - 1.;
            let mut path = PathBuilder::stroke(px(2.));
            let steps = (sweep / TAU * 64.).ceil().max(2.) as usize;
            for step in 0..=steps {
                let angle = start + sweep * step as f32 / steps as f32 - FRAC_PI_2;
                let p = center + point(px(radius * angle.cos()), px(radius * angle.sin()));
                if step == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
            if sweep >= TAU {
                path.close();
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size_full()
}

impl Render for ProjectRing {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let ring: AnyElement = match self.state {
            State::Idle | State::Done => div().into_any_element(),
            // Its own small view, so the clock repaints only the ring.
            State::Working => div()
                .id("project-ring-working")
                .size_full()
                .role(Role::Image)
                .aria_label("Working")
                .child(pulse::looping(
                    "project-ring-turn",
                    PERIOD,
                    Rate::Smooth,
                    |phase| {
                        let mut track = accent_color();
                        track.a *= 0.18;
                        div()
                            .size_full()
                            .relative()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .size_full()
                                    .child(arc(0., TAU, track)),
                            )
                            .child(div().absolute().top_0().left_0().size_full().child(arc(
                                phase * TAU,
                                TAU * 0.3,
                                accent_color(),
                            )))
                    },
                ))
                .into_any_element(),
            State::Attention => div()
                .id("project-ring-attention")
                .size_full()
                .role(Role::Image)
                .aria_label("Needs attention")
                .child(arc(0., TAU, rgb(ATTENTION).into()))
                .into_any_element(),
        };
        div()
            .debug_selector(|| "project-ring".into())
            .size(px(RING_SIDE))
            .child(ring)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn attention_outranks_working_and_idle_projects_are_left_out() {
        let states = project_states([
            ("/a", true, false),
            ("/a", false, true),
            ("/b", true, false),
            ("/c", false, false),
            ("/d", false, true),
            ("/d", true, false),
        ]);
        assert!(states["/a"] == State::Attention);
        assert!(states["/b"] == State::Working);
        assert!(!states.contains_key("/c"));
        assert!(states["/d"] == State::Attention);
    }

    #[gpui::test]
    fn working_ring_turns_on_the_shared_clock_and_stops_when_idle(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(false));
        let (ring, cx) = cx.add_window_view(|_, _| ProjectRing::new());
        ring.update(cx, |ring, cx| ring.set(State::Working, cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(pulse::running(cx)));
        ring.update(cx, |ring, cx| ring.set(State::Idle, cx));
        cx.run_until_parked();
        // Two background-paced strides: one notifies, the next finds no loop.
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!pulse::running(cx)));
    }
}
