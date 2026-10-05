//! View tests for the arcade background: the idle band, the take-control
//! flow, the keyboard, and the slider.

use std::time::Duration;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px, size};

use super::*;
use crate::terminal_dock::test_support::{draw, init};

fn open(cx: &mut TestAppContext) -> (Entity<TerminalGridBackground>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) =
        cx.add_window_view(|_, cx| TerminalGridBackground::with_rng(ArcadeRng::seeded(0x5eed), cx));
    cx.simulate_resize(size(px(800.), px(600.)));
    draw(cx);
    (view, cx)
}

/// Runs the timer for `ms`, frame by frame.
fn run_for(cx: &mut VisualTestContext, ms: u64) {
    for _ in 0..ms.div_ceil(FRAME_MS as u64) {
        cx.executor()
            .advance_clock(Duration::from_millis(FRAME_MS as u64));
        cx.run_until_parked();
    }
    draw(cx);
}

#[gpui::test]
fn idles_in_a_band_across_the_top(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    run_for(cx, 100);
    let band = cx.debug_bounds("arcade-band").expect("the band");
    assert_eq!(band.size.height, px(BAND_HEIGHT));
    assert_eq!(band.origin.y, px(0.));
    // The grid covers the band: 800 / 7 and 192 / 7, rounded up.
    assert_eq!(view.read_with(cx, |view, _| view.grid()), (115, 28));
    assert!(!view.read_with(cx, |view, _| view.playing()));
}

#[gpui::test]
fn takes_control_from_the_hover_button_and_covers_the_pane(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    run_for(cx, 100);
    cx.simulate_mouse_move(point(px(400.), px(60.)), None, Modifiers::none());
    run_for(cx, 300);
    let button = cx
        .debug_bounds("arcade-take-control")
        .expect("the button shows on hover");
    cx.simulate_click(button.center(), Modifiers::none());
    run_for(cx, 100);

    assert!(view.read_with(cx, |view, _| view.playing()));
    let layer = cx
        .debug_bounds("arcade-playing")
        .expect("the playing layer");
    assert_eq!(layer.size, size(px(800.), px(600.)));
    // The game now runs on the whole pane.
    assert_eq!(view.read_with(cx, |view, _| view.grid()), (115, 86));
    assert_eq!(view.read_with(cx, |view, _| view.lives()), 3);
}

#[gpui::test]
fn steers_with_the_arrow_keys_and_releases_on_escape(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    run_for(cx, 100);
    cx.update(|window, cx| view.update(cx, |view, cx| view.take_control(window, cx)));
    run_for(cx, 100);

    // Pac-man spawns stopped and only takes a heading that opens onto a
    // corridor, so try each until he eats something.
    for key in ["up", "right", "down", "left"] {
        cx.simulate_keystrokes(key);
        run_for(cx, 1000);
        if view.read_with(cx, |view, _| view.score()) > 0 {
            break;
        }
    }
    assert!(view.read_with(cx, |view, _| view.score()) > 0);

    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!view.read_with(cx, |view, _| view.playing()));
    assert_eq!(view.read_with(cx, |view, _| view.score()), 0);
    assert!(cx.debug_bounds("arcade-band").is_some());
}

#[gpui::test]
fn picks_a_mode_from_the_hud(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    run_for(cx, 100);
    cx.update(|window, cx| view.update(cx, |view, cx| view.take_control(window, cx)));
    run_for(cx, 100);
    let hard = cx
        .debug_bounds("arcade-mode-hard")
        .expect("the hard button");
    cx.simulate_click(hard.center(), Modifiers::none());
    assert_eq!(view.read_with(cx, |view, _| view.mode()), ArcadeMode::Hard);
}

#[gpui::test]
fn slides_to_the_next_game_after_the_hold(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    run_for(cx, SLIDE_HOLD_MS - 500);
    assert_eq!(view.read_with(cx, |view, _| view.slide_index()), 0);
    run_for(cx, 1000);
    assert_eq!(view.read_with(cx, |view, _| view.slide_index()), 1);
}

#[gpui::test]
fn holds_the_slide_while_the_pointer_rests_on_the_band(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    cx.simulate_mouse_move(point(px(400.), px(60.)), None, Modifiers::none());
    run_for(cx, SLIDE_HOLD_MS + 1000);
    assert_eq!(view.read_with(cx, |view, _| view.slide_index()), 0);
}

#[gpui::test]
fn jumps_to_a_game_from_its_dot(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    run_for(cx, 100);
    let dot = cx.debug_bounds("arcade-dot-snake").expect("the snake dot");
    cx.simulate_click(dot.center(), Modifiers::none());
    assert_eq!(view.read_with(cx, |view, _| view.slide_index()), 1);
}

#[gpui::test]
fn holds_a_still_frame_under_reduced_motion(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (view, cx) = open(cx);
    run_for(cx, 100);
    let frame = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let (cols, rows) = view.grid();
            let mut stamp = vec![0f32; (cols * rows) as usize];
            view.boards[0]
                .arcade
                .stamp(&mut stamp, cols as usize, rows as usize);
            (view.boards[0].arcade.fade(), stamp)
        })
    };
    let (fade, before) = frame(cx);
    assert_eq!(fade, 1.0);
    run_for(cx, 2000);
    let (_, after) = frame(cx);
    assert_eq!(before, after);
}
