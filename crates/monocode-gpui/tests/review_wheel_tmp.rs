use gpui::{
    Context, FollowMode, IntoElement, ListAlignment, ListOffset, ListState, Render, ScrollDelta,
    ScrollWheelEvent, TestAppContext, Window, div, list, point, prelude::*, px, size,
};

struct Rows(ListState);
impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        list(self.0.clone(), |_, _, _| {
            div().h(px(20.)).w_full().into_any()
        })
        .w_full()
        .h_full()
    }
}

fn draw(cx: &mut gpui::VisualTestContext, state: &ListState) {
    cx.draw(point(px(0.), px(0.)), size(px(100.), px(100.)), |_, cx| {
        cx.new(|_| Rows(state.clone())).into_any_element()
    });
}

#[gpui::test]
fn pending_wheel_must_not_cancel_jump_to_bottom(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let state = ListState::new(50, ListAlignment::Bottom, px(10.)).measure_all();
    state.scroll_to(ListOffset { item_ix: 20, offset_in_item: px(0.) });
    draw(cx, &state);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Lines(point(0., 3.)),
        ..Default::default()
    });
    state.set_follow_mode(FollowMode::Tail);
    state.scroll_to_end();
    draw(cx, &state);
    std::thread::sleep(std::time::Duration::from_millis(5));
    cx.update(|window, cx| { window.simulate_next_frame(cx); });
    assert!(state.is_following_tail(), "old wheel input cancelled the jump to bottom");
}

#[gpui::test]
fn pending_wheel_must_not_survive_reset(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let state = ListState::new(50, ListAlignment::Top, px(10.)).measure_all();
    state.scroll_to(ListOffset { item_ix: 20, offset_in_item: px(0.) });
    draw(cx, &state);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Lines(point(0., -3.)),
        ..Default::default()
    });
    state.reset(50);
    draw(cx, &state);
    std::thread::sleep(std::time::Duration::from_millis(5));
    cx.update(|window, cx| { window.simulate_next_frame(cx); });
    assert_eq!(state.logical_scroll_top().offset_in_item, px(0.));
    assert_eq!(state.logical_scroll_top().item_ix, 0);
}

#[gpui::test]
fn wheel_glide_must_preserve_input_distance(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let state = ListState::new(50, ListAlignment::Top, px(10.)).measure_all();
    draw(cx, &state);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Lines(point(0., -10.)),
        ..Default::default()
    });
    for _ in 0..35 {
        std::thread::sleep(std::time::Duration::from_millis(16));
        cx.update(|window, cx| { window.simulate_next_frame(cx); });
    }
    assert_eq!(state.logical_scroll_top().item_ix, 10);
}
