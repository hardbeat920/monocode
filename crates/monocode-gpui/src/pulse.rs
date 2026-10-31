//! One shared clock for decorative loops: spinners, mascots, avatars and the
//! composer runner.
//!
//! A view that paints a loop leases the clock for itself. Each tick notifies
//! every leased view together, so all loops land in one frame instead of one
//! frame per spinner, and at 30Hz or less instead of the display's 120Hz. A
//! lease ends when its view was notified but did not paint a loop again, so
//! a closed, scrolled-away or hidden spinner stops costing anything, and the
//! clock parks once no lease is left. While no window is visible the clock
//! slows every loop down further; a visible window behind another app's keeps
//! the full pace, since its loops are still on screen.
use gpui::{
    AnyElement, App, Element, ElementId, EntityId, Global, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const TICK: Duration = Duration::from_millis(33);
/// A hidden app's loops tick this many times slower.
const BACKGROUND_STRIDE: u64 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Rate {
    /// About 30fps: rotations and fades that must look continuous.
    Smooth,
    /// About 15fps: small loaders.
    Half,
    /// About 7.5fps: pixel art that steps between a few poses.
    Low,
}
impl Rate {
    fn stride(self) -> u64 {
        match self {
            Rate::Smooth => 1,
            Rate::Half => 2,
            Rate::Low => 4,
        }
    }
}

type FrameCallback = Box<dyn FnOnce(&mut App)>;
struct Lease {
    rate: Rate,
    /// Painted a loop since the clock last notified it.
    painted: bool,
    on_tick: Option<FrameCallback>,
}

#[derive(Default)]
struct Clock {
    epoch: Option<Instant>,
    leases: HashMap<EntityId, Lease>,
    tick: u64,
    running: bool,
}
impl Global for Clock {}

/// Keeps the view being drawn repainting at `rate`. Call it while rendering
/// or painting a loop, every time the loop is drawn. Returns false, and
/// leases nothing, under reduced motion: draw the loop's resting frame.
pub(crate) fn request(rate: Rate, window: &Window, cx: &mut App) -> bool {
    request_inner(rate, window, cx, None)
}

/// Stream presentation uses the same coalesced 30Hz wakeup as decorative
/// loops, advancing its state before notifying rather than spawning a timer.
pub(crate) fn request_frame(
    rate: Rate,
    window: &Window,
    cx: &mut App,
    on_tick: impl FnOnce(&mut App) + 'static,
) -> bool {
    request_inner(rate, window, cx, Some(Box::new(on_tick)))
}

fn request_inner(
    rate: Rate,
    window: &Window,
    cx: &mut App,
    mut on_tick: Option<FrameCallback>,
) -> bool {
    if cx.reduce_motion() {
        return false;
    }
    let view = window.current_view();
    let clock = cx.default_global::<Clock>();
    clock
        .leases
        .entry(view)
        .and_modify(|lease| {
            lease.painted = true;
            if on_tick.is_some() {
                lease.on_tick = on_tick.take();
            }
            if lease.rate.stride() > rate.stride() {
                lease.rate = rate;
            }
        })
        .or_insert_with(|| Lease {
            rate,
            painted: true,
            on_tick,
        });
    if !clock.running {
        clock.running = true;
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if !cx.update(tick) {
                    break;
                }
            }
        })
        .detach();
    }
    true
}

/// Time on the shared clock, so loops of one period stay in step.
pub(crate) fn elapsed(cx: &mut App) -> Duration {
    let now = cx.background_executor().now();
    let clock = cx.default_global::<Clock>();
    now - *clock.epoch.get_or_insert(now)
}

/// The fraction of `period` the shared clock is into, in `0..1`.
pub(crate) fn phase(period: Duration, cx: &mut App) -> f32 {
    if period.is_zero() {
        return 0.;
    }
    let into = elapsed(cx).as_nanos() % period.as_nanos();
    into as f32 / period.as_nanos() as f32
}

#[cfg(test)]
pub(crate) fn running(cx: &App) -> bool {
    cx.try_global::<Clock>().is_some_and(|clock| clock.running)
}

fn tick(cx: &mut App) -> bool {
    let background = !app_visible(cx);
    let clock = cx.global_mut::<Clock>();
    clock.tick = clock.tick.wrapping_add(1);
    let tick = clock.tick;
    let mut due = Vec::new();
    clock.leases.retain(|view, lease| {
        let stride = lease.rate.stride() * if background { BACKGROUND_STRIDE } else { 1 };
        if !tick.is_multiple_of(stride) {
            return true;
        }
        if !lease.painted {
            return false;
        }
        lease.painted = false;
        due.push((*view, lease.on_tick.take()));
        true
    });
    if clock.leases.is_empty() {
        clock.running = false;
        return false;
    }
    for (view, callback) in due {
        if let Some(callback) = callback {
            callback(cx);
        } else {
            // A loop's frames keep its size; views can keep their measurements.
            cx.notify_paint(view);
        }
    }
    true
}

/// Whether one of the app's windows has focus.
pub(crate) fn app_active(cx: &mut App) -> bool {
    cx.windows().into_iter().any(|window| {
        window
            .update(cx, |_, window, _| window.is_window_active())
            .unwrap_or(false)
    })
}

/// Whether any window is on screen: an inactive window can still be in view.
pub(crate) fn app_visible(cx: &mut App) -> bool {
    cx.windows().into_iter().any(|window| {
        window
            .update(cx, |_, window, _| window.is_visible())
            .unwrap_or(false)
    })
}

/// A repeating animation on the shared clock: `build` gets the fraction of
/// `period` elapsed and returns the frame. Reduced motion draws phase zero.
pub(crate) fn looping<E: IntoElement>(
    id: impl Into<ElementId>,
    period: Duration,
    rate: Rate,
    build: impl FnOnce(f32) -> E + 'static,
) -> Looping {
    Looping {
        id: id.into(),
        period,
        rate,
        build: Some(Box::new(move |phase| build(phase).into_any_element())),
    }
}

pub(crate) struct Looping {
    id: ElementId,
    period: Duration,
    rate: Rate,
    build: Option<Box<dyn FnOnce(f32) -> AnyElement>>,
}
impl IntoElement for Looping {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Looping {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let phase = if request(self.rate, window, cx) {
            phase(self.period, cx)
        } else {
            0.
        };
        let build = self.build.take().expect("a loop lays out once");
        let mut element = build(phase);
        (element.request_layout(window, cx), element)
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        element: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        element: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext, div, prelude::*};
    use std::{cell::Cell, rc::Rc};

    struct Spinner {
        spinning: bool,
        renders: Rc<Cell<usize>>,
    }
    impl Render for Spinner {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            let spinning = self.spinning;
            div().size_4().when(spinning, |d| {
                d.child(looping(
                    "spin",
                    Duration::from_secs(1),
                    Rate::Smooth,
                    |_| div(),
                ))
            })
        }
    }

    #[gpui::test]
    fn a_loop_repaints_on_the_clock_and_parks_once_it_is_gone(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(false));
        let renders = Rc::new(Cell::new(0));
        let (view, cx) = cx.add_window_view(|_, _| Spinner {
            spinning: true,
            renders: renders.clone(),
        });
        cx.run_until_parked();
        let first = renders.get();
        cx.executor().advance_clock(TICK * 2);
        cx.run_until_parked();
        assert!(renders.get() > first, "a leased view repaints on ticks");

        view.update(cx, |view, cx| {
            view.spinning = false;
            cx.notify();
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(TICK * BACKGROUND_STRIDE as u32 * 3);
        cx.run_until_parked();
        let stopped = renders.get();
        cx.executor()
            .advance_clock(TICK * BACKGROUND_STRIDE as u32 * 3);
        cx.run_until_parked();
        assert_eq!(
            renders.get(),
            stopped,
            "an unpainted loop releases the clock"
        );
        cx.update(|_, cx| assert!(!cx.global::<Clock>().running));
    }

    #[gpui::test]
    fn a_visible_window_keeps_full_pace_and_a_hidden_one_slows(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(false));
        let renders = Rc::new(Cell::new(0));
        let (_view, cx) = cx.add_window_view(|_, _| Spinner {
            spinning: true,
            renders: renders.clone(),
        });
        cx.run_until_parked();
        // A test window is visible but never active, like one behind another app.
        let start = renders.get();
        cx.executor().advance_clock(TICK * BACKGROUND_STRIDE as u32);
        cx.run_until_parked();
        assert_eq!(renders.get() - start, BACKGROUND_STRIDE as usize);

        cx.simulate_visibility_change(gpui::WindowVisibility::Hidden);
        cx.run_until_parked();
        let hidden = renders.get();
        cx.executor()
            .advance_clock(TICK * BACKGROUND_STRIDE as u32 * 2);
        cx.run_until_parked();
        assert!(
            renders.get() - hidden <= 2,
            "a hidden app's loops slow down"
        );
    }

    #[gpui::test]
    fn reduced_motion_leases_nothing(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let renders = Rc::new(Cell::new(0));
        let (_view, cx) = cx.add_window_view(|_, _| Spinner {
            spinning: true,
            renders: renders.clone(),
        });
        cx.run_until_parked();
        let first = renders.get();
        cx.executor()
            .advance_clock(TICK * BACKGROUND_STRIDE as u32 * 3);
        cx.run_until_parked();
        assert_eq!(renders.get(), first);
    }

    #[gpui::test]
    fn stream_callbacks_share_the_clock_and_release_their_lease(cx: &mut TestAppContext) {
        struct Stream {
            active: bool,
            ticks: Rc<Cell<usize>>,
        }
        impl Render for Stream {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let active = self.active;
                let view = cx.entity().downgrade();
                div().size_4().child(gpui::canvas(
                    move |_, window, cx| {
                        if active {
                            request_frame(Rate::Smooth, window, cx, move |cx| {
                                let _ = view.update(cx, |stream, cx| {
                                    if stream.active {
                                        stream.ticks.set(stream.ticks.get() + 1);
                                        cx.notify();
                                    }
                                });
                            });
                        }
                    },
                    |_, _, _, _| {},
                ))
            }
        }
        cx.update(|cx| cx.set_reduce_motion(false));
        let ticks = Rc::new(Cell::new(0));
        let (view, cx) = cx.add_window_view(|_, _| Stream {
            active: true,
            ticks: ticks.clone(),
        });
        cx.run_until_parked();
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        assert_eq!(ticks.get(), 1);
        view.update(cx, |stream, cx| {
            stream.active = false;
            cx.notify();
        });
        cx.run_until_parked();
        cx.executor()
            .advance_clock(TICK * BACKGROUND_STRIDE as u32 * 3);
        cx.run_until_parked();
        assert_eq!(ticks.get(), 1);
        cx.update(|_, cx| assert!(!running(cx)));
    }
}
