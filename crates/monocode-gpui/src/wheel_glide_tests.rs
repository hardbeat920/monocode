//! The vendored list glides through mouse-wheel notches (see
//! `WheelGlide` in gpui's list element); trackpad pixels stay immediate.
use gpui::{
    Context, IntoElement, ListAlignment, ListState, Render, ScrollDelta, ScrollWheelEvent,
    TestAppContext, Window, div, list, point, prelude::*, px, size,
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

#[gpui::test]
fn wheel_notches_glide_and_trackpad_pixels_land_at_once(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let state = ListState::new(50, ListAlignment::Top, px(10.));
    cx.draw(point(px(0.), px(0.)), size(px(100.), px(100.)), |_, cx| {
        cx.new(|_| Rows(state.clone())).into_any_element()
    });
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Lines(point(0., -3.)),
        ..Default::default()
    });
    // The notch is distance still to travel, not a jump already made.
    assert_eq!(state.logical_scroll_top().item_ix, 0);
    assert_eq!(state.logical_scroll_top().offset_in_item, px(0.));
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
        ..Default::default()
    });
    assert_eq!(state.logical_scroll_top().item_ix, 2);
}
