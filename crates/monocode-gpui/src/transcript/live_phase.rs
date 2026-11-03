//! ActivityPhaseGroup's bounded live window and useLivePhaseScroll.
use gpui::{
    AnyElement, Bounds, Context, Pixels, ScrollHandle, Window, canvas, div, prelude::*, px,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

#[derive(Default)]
pub(super) struct LiveScroll {
    pub handle: ScrollHandle,
    following: Rc<Cell<bool>>,
    enabled: bool,
    edges: Rc<RefCell<Edges>>,
}
impl LiveScroll {
    pub fn enable(&mut self, enabled: bool) {
        if enabled && !self.enabled {
            self.following.set(true);
        }
        self.enabled = enabled;
        if enabled && self.following.get() {
            // Applied after layout, so appends, reveals and resizes pin to the
            // newly measured bottom rather than last frame's content height.
            self.handle.scroll_to_bottom();
        }
    }
}

pub(super) fn max_height(window: &Window) -> Pixels {
    px(280.).min(window.viewport_size().height * 0.45)
}

#[derive(Default)]
struct Edges {
    from: (f32, f32),
    to: (f32, f32),
    changed: Option<Instant>,
    frame: (f32, f32),
}
impl Edges {
    fn value(&self, now: Instant) -> (f32, f32) {
        let progress = self.changed.map_or(1., |at| {
            crate::reveal::bezier(
                (now.duration_since(at).as_secs_f32() / 0.16).min(1.),
                0.,
                0.,
                0.58,
                1.,
            )
        });
        (
            self.from.0 + (self.to.0 - self.from.0) * progress,
            self.from.1 + (self.to.1 - self.from.1) * progress,
        )
    }
    fn update(&mut self, target: (f32, f32), now: Instant, reduced: bool) -> (f32, f32) {
        if self.to != target {
            self.from = self.value(now);
            self.to = target;
            self.changed = Some(now);
        }
        if reduced {
            self.from = target;
            self.to = target;
            self.changed = None;
        }
        self.value(now)
    }
}

/// GPUI deltas are negative toward the bottom. Consume only a direction with
/// room, as nestedScrollAbsorbsWheel does; reaching an edge releases the next
/// wheel to the transcript. Upward input pauses follow until the reader returns.
fn wheel(scroll: &ScrollHandle, following: &Cell<bool>, delta: Pixels) -> bool {
    let max = scroll.max_offset().y;
    let offset = scroll.offset().y;
    if max <= px(1.)
        || !((delta > px(0.) && offset < px(0.)) || (delta < px(0.) && offset > -max + px(1.)))
    {
        return false;
    }
    let y = (offset + delta).clamp(-max, px(0.));
    following.set(delta < px(0.) && -max + px(1.) >= y);
    scroll.set_offset(gpui::point(scroll.offset().x, y));
    true
}

pub(super) fn live_phase(
    body: AnyElement,
    scroll: &mut LiveScroll,
    max_height: Pixels,
    enabled: bool,
    cx: &mut Context<super::TranscriptView>,
) -> AnyElement {
    scroll.enable(enabled);
    let handle = scroll.handle.clone();
    let following = scroll.following.clone();
    let view = cx.entity_id();
    div()
        .relative()
        .w_full()
        .min_w_0()
        .max_h(max_height)
        .child(edge_mask(scroll, true, enabled))
        .child(
            div()
                .id("live-phase-scroll")
                .w_full()
                .min_w_0()
                .max_h(max_height)
                .overflow_y_scroll()
                .track_scroll(&scroll.handle)
                // The inner listener runs before GPUI's scroller. Move and consume
                // the wheel here so the outer list never receives a handled event.
                .child(
                    div()
                        .id("live-phase-content")
                        .w_full()
                        .min_w_0()
                        .on_scroll_wheel(move |event, window, cx| {
                            if enabled
                                && wheel(
                                    &handle,
                                    &following,
                                    event.delta.pixel_delta(window.line_height()).y,
                                )
                            {
                                cx.notify(view);
                                cx.stop_propagation();
                            }
                        })
                        .child(body),
                ),
        )
        .child(edge_mask(scroll, false, enabled))
        .into_any_element()
}

fn edge_mask(scroll: &LiveScroll, begin: bool, enabled: bool) -> AnyElement {
    let handle = scroll.handle.clone();
    let edges = scroll.edges.clone();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            // Read during paint, after the scroller has clamped its offset and
            // measured this frame's content. A live append can change both.
            let offset = handle.offset().y;
            let maximum = handle.max_offset().y;
            let mut edges = edges.borrow_mut();
            // Both markers must use the very same ramp for this frame.
            if begin {
                let target = if enabled {
                    (
                        if -offset > px(1.) { 24. } else { 0. },
                        if maximum + offset > px(1.) { 32. } else { 0. },
                    )
                } else {
                    (0., 0.)
                };
                let now = Instant::now();
                edges.frame = edges.update(target, now, cx.reduce_motion());
                if edges
                    .changed
                    .is_some_and(|at| now.duration_since(at).as_secs_f32() < 0.16)
                {
                    window.on_next_frame(|window, _| window.refresh());
                }
            }
            let (top, bottom) = edges.frame;
            if top > 0. || bottom > 0. {
                paint_mask(bounds, begin, top, bottom, window);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

fn paint_mask(bounds: Bounds<Pixels>, begin: bool, top: f32, bottom: f32, window: &mut Window) {
    #[cfg(target_os = "macos")]
    window.paint_layer(bounds, |window| {
        window.paint_quad(gpui::fill(bounds, gpui::black()).border_color(
            gpui_apple::metal_renderer::edge_fade_marker(
                begin,
                top * window.scale_factor(),
                bottom * window.scale_factor(),
            ),
        ));
    });
    #[cfg(not(target_os = "macos"))]
    if !begin {
        // Other backends have an opaque chat surface. Match the transcript's
        // fallback while retaining the phase's conditional edge lengths.
        window.paint_layer(bounds, |window| {
            for (at_top, height) in [(true, top), (false, bottom)] {
                let height = height.min(bounds.size.height.as_f32());
                for y in 0..height.ceil() as usize {
                    let mut color: gpui::Hsla = gpui::rgb(crate::theme::CHAT).into();
                    color.a = (1. - (y as f32 + 0.5) / height).clamp(0., 1.);
                    let origin = gpui::point(
                        bounds.left(),
                        if at_top {
                            bounds.top() + px(y as f32)
                        } else {
                            bounds.bottom() - px(y as f32 + 1.)
                        },
                    );
                    window.paint_quad(gpui::fill(
                        Bounds::new(origin, gpui::size(bounds.size.width, px(1.))),
                        color,
                    ));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Body, TranscriptView};
    use super::*;
    use gpui::{
        ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, WindowBounds, WindowOptions,
        point, size,
    };
    use monocode_core::{tool::ToolCall, transcript::Row};
    use std::time::Duration;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }
    fn tool(i: usize) -> Row {
        let mut call = ToolCall::new(&format!("read-{i}"), "Read");
        call.input = format!(r#"{{"file_path":"src/file-{i}.rs"}}"#);
        row("tool", &serde_json::to_string(&call).unwrap())
    }
    fn wheel_event(position: gpui::Point<Pixels>, delta: f32) -> ScrollWheelEvent {
        ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
            modifiers: Default::default(),
            touch_phase: TouchPhase::Moved,
        }
    }

    #[test]
    fn fade_lengths_ease_for_160ms_and_reverse_from_the_current_value() {
        let mut edges = Edges::default();
        let now = Instant::now();
        assert_eq!(edges.update((24., 32.), now, false), (0., 0.));
        let halfway = now + Duration::from_millis(80);
        let (top, bottom) = edges.value(halfway);
        assert!(top > 12. && top < 24.);
        assert!((bottom / top - 32. / 24.).abs() < 0.001);
        assert_eq!(edges.update((0., 32.), halfway, false), (top, bottom));
        assert_eq!(edges.value(halfway + Duration::from_millis(160)), (0., 32.));
        assert_eq!(edges.update((24., 0.), halfway, true), (24., 0.));
    }

    #[gpui::test]
    fn live_group_caps_height_follows_appends_and_respects_nested_wheels(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(800.), px(800.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| TranscriptView::new()),
            )
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let outer_wheels = Rc::new(Cell::new(0));
        window
            .update(visual, |view, _, cx| {
                let wheels = outer_wheels.clone();
                view.scroll_bound = true;
                view.list
                    .set_scroll_handler(move |_, _, _| wheels.set(wheels.get() + 1));
                view.streaming(true, cx);
                let mut rows = vec![row("user", "Inspect the project")];
                rows.extend((0..24).map(tool));
                view.patch(0, rows, cx);
            })
            .unwrap();
        visual.run_until_parked();
        let (position, outer) = window
            .update(visual, |view, _, _| {
                let scroll = &view.phase_scrolls[&1];
                assert_eq!(scroll.handle.bounds().size.height, px(280.));
                assert!(scroll.handle.max_offset().y > px(300.));
                assert_eq!(scroll.handle.offset().y, -scroll.handle.max_offset().y);
                assert_eq!(scroll.edges.borrow().frame, (24., 0.));
                let header = view
                    .rows
                    .iter()
                    .position(|r| matches!(r.body, Body::Phase { group: 1, .. }))
                    .unwrap();
                assert_eq!(
                    view.list.bounds_for_item(header).unwrap().size.height,
                    px(308.)
                );
                (
                    scroll.handle.bounds().center(),
                    view.list.logical_scroll_top(),
                )
            })
            .unwrap();
        visual.simulate_event(wheel_event(position, 80.));
        visual.run_until_parked();
        let paused = window
            .update(visual, |view, _, _| {
                let scroll = &view.phase_scrolls[&1];
                assert!(!scroll.following.get());
                assert_eq!(
                    scroll.handle.offset().y,
                    -scroll.handle.max_offset().y + px(80.)
                );
                assert_eq!(scroll.edges.borrow().frame, (24., 32.));
                let after = view.list.logical_scroll_top();
                assert_eq!(
                    (after.item_ix, after.offset_in_item),
                    (outer.item_ix, outer.offset_in_item)
                );
                scroll.handle.offset().y
            })
            .unwrap();
        window
            .update(visual, |view, _, cx| {
                view.patch(view.source.len(), vec![tool(24)], cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                assert_eq!(
                    view.phase_scrolls[&1].handle.offset().y,
                    paused,
                    "new calls must not move a reader who scrolled up"
                );
            })
            .unwrap();
        visual.simulate_event(wheel_event(position, -20.));
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                assert!(!view.phase_scrolls[&1].following.get())
            })
            .unwrap();
        visual.simulate_event(wheel_event(position, -10000.));
        visual.run_until_parked();
        window
            .update(visual, |view, _, cx| {
                assert!(view.phase_scrolls[&1].following.get());
                view.patch(view.source.len(), vec![tool(25)], cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                let scroll = &view.phase_scrolls[&1];
                assert_eq!(scroll.handle.offset().y, -scroll.handle.max_offset().y);
            })
            .unwrap();
        // At the top, only the bottom edge fades. The next upward wheel
        // escapes the phase and reaches the transcript's scroll handler.
        visual.simulate_event(wheel_event(position, 10000.));
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                assert_eq!(view.phase_scrolls[&1].handle.offset().y, px(0.));
                assert_eq!(view.phase_scrolls[&1].edges.borrow().frame, (0., 32.));
                assert_eq!(outer_wheels.get(), 0);
            })
            .unwrap();
        visual.simulate_event(wheel_event(position, 80.));
        visual.run_until_parked();
        assert_eq!(outer_wheels.get(), 1);
        visual.simulate_window_resize(window.into(), size(px(800.), px(400.)));
        visual.run_until_parked();
        window
            .update(visual, |view, _, cx| {
                assert_eq!(view.phase_scrolls[&1].handle.bounds().size.height, px(180.));
                view.streaming(false, cx);
                view.toggle_phase(1, true, cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |view, _, _| {
                assert!(!view.windowed_phase(1));
                assert!(
                    view.list.max_offset_for_scrollbar().y > px(300.),
                    "finished groups reopen at natural height"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn short_groups_have_no_fades_and_live_groups_collapse_from_the_capped_height(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.streaming(true, cx);
                view.patch(0, vec![row("user", "Check"), tool(0), tool(1)], cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |view, _, cx| {
                let scroll = &view.phase_scrolls[&1];
                assert_eq!(scroll.handle.max_offset().y, px(0.));
                assert_eq!(scroll.edges.borrow().frame, (0., 0.));
                view.patch(3, (2..24).map(tool).collect(), cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |view, _, cx| {
                cx.set_reduce_motion(false);
                view.streaming(false, cx);
                assert!(view.rows.iter().any(|r| r.phase_group() == Some(1)));
                let now = Instant::now();
                for motion in view.motions.values_mut() {
                    assert_eq!(motion.group, Some(1));
                    assert!(!motion.opening);
                    *motion = crate::reveal::Motion::phase(1, false, now - Duration::from_secs(25));
                    motion.duration = Duration::from_secs(100);
                }
                view.visible = false;
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        window.update(visual, |view, _, _| {
            let progress = view.motions.values().next().unwrap().value(view.frame_now).0;
            let header = view.rows.iter().position(|r| matches!(r.body, Body::Phase { group: 1, .. })).unwrap();
            let height = view.list.bounds_for_item(header).unwrap().size.height;
            assert!((height - (px(28.) + view.phase_max_height * progress)).abs() < px(1.), "collapse starts from the window, with no flash of the full tool list: {height:?}");
        }).unwrap();
    }
}
