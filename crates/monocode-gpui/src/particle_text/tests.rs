use super::*;
use gpui::{Context, Render, TestAppContext, size};

#[test]
fn particles_wait_for_the_edge_then_drift_shrink_and_expire() {
    assert_eq!(edge_at(0., 200.), -18.);
    assert_eq!(edge_at(0.55, 200.), 100.);
    assert_eq!(edge_at(1.1, 200.), 218.);
    assert_eq!(edge_at(3., 200.), 218.);
    assert!((edge_at(0.275, 200.) - (-3.25)).abs() < 0.001);
    let mut particle = Particle {
        x: 40.,
        y: 9.,
        alpha: 0.5,
        vx: 12.,
        vy: -8.,
        life: 0.85,
        born: None,
    };
    assert_eq!(particle.frame(0.3, 39.), (true, None));
    assert!(particle.born.is_none());
    let (alive, first) = particle.frame(0.4, 40.);
    assert!(alive);
    let (bounds, alpha) = first.unwrap();
    assert_eq!(bounds.origin, point(px(40.), px(9.)));
    assert_eq!(bounds.size, size(px(1.8), px(1.8)));
    assert!((alpha - 0.6).abs() < 0.0001);
    let (_, middle) = particle.frame(0.825, 200.);
    let (bounds, alpha) = middle.unwrap();
    assert_eq!(bounds.origin, point(px(50.5), px(2.)));
    assert!((f32::from(bounds.size.width) - 1.35).abs() < 0.001);
    assert!((alpha - 0.6 * 0.5_f32.powf(1.2)).abs() < 0.001);
    assert_eq!(particle.frame(1.26, 200.), (false, None));
}

#[cfg(target_os = "macos")]
struct Preview {
    text: String,
    width: f32,
    visible: bool,
}
#[cfg(target_os = "macos")]
impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .h(px(20.))
            .text_size(px(14.))
            .when(self.visible, |d| d.child(particle_text(self.text.clone())))
    }
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn mount_remount_same_title_zero_width_and_reduced_motion_do_not_replay(cx: &mut TestAppContext) {
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| Preview {
                text: "New session".into(),
                width: 220.,
                visible: true,
            })
        })
        .unwrap()
    });
    let masked = |cx: &mut TestAppContext| {
        cx.refresh().unwrap();
        window
            .update(cx, |_, window, _| {
                let hue = gpui_apple::metal_renderer::text_reveal_marker(true, 0., 18.).h;
                window
                    .painted_quads()
                    .iter()
                    .filter(|quad| quad.border_color.h == hue)
                    .count()
            })
            .unwrap()
    };
    assert_eq!(masked(cx), 0, "the first value is static");
    window.update(cx, |_, _, cx| cx.notify()).unwrap();
    assert_eq!(masked(cx), 0, "unrelated redraws never animate a title");
    window
        .update(cx, |view, _, cx| {
            view.text = "Generate matching animation".into();
            cx.notify();
        })
        .unwrap();
    assert_eq!(
        masked(cx),
        2,
        "new text and old ghost have separate opposed masks"
    );
    cx.update(|cx| cx.set_reduce_motion(true));
    assert_eq!(masked(cx), 0, "reduced motion clears active masks too");
    window
        .update(cx, |view, _, cx| {
            view.text = "Already generated".into();
            cx.notify();
        })
        .unwrap();
    assert_eq!(masked(cx), 0);
    cx.update(|cx| cx.set_reduce_motion(false));
    assert_eq!(masked(cx), 0, "turning motion back on does not replay it");
    window
        .update(cx, |view, _, cx| {
            view.width = 0.;
            view.text = "Hidden change".into();
            cx.notify();
        })
        .unwrap();
    assert_eq!(masked(cx), 0);
    window
        .update(cx, |view, _, cx| {
            view.width = 220.;
            cx.notify();
        })
        .unwrap();
    assert_eq!(
        masked(cx),
        0,
        "zero-sized changes are consumed, not deferred"
    );
    window
        .update(cx, |view, _, cx| {
            view.visible = false;
            cx.notify();
        })
        .unwrap();
    assert_eq!(masked(cx), 0);
    window
        .update(cx, |view, _, cx| {
            view.text = "Title changed while unmounted".into();
            view.visible = true;
            cx.notify();
        })
        .unwrap();
    assert_eq!(
        masked(cx),
        0,
        "a filtered/collapsed row remounts without old motion"
    );
}

#[gpui::test]
fn interruption_uses_the_last_target_and_completion_discards_all_effects(cx: &mut TestAppContext) {
    use gpui::canvas;
    use std::{cell::RefCell, rc::Rc, time::Duration};
    struct Harness(Rc<RefCell<Option<()>>>);
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let checked = self.0.clone();
            canvas(
                move |bounds, window, cx| {
                    let style = LabelStyle {
                        width: bounds.size.width,
                        font: window.text_style().font(),
                        font_size: px(14.),
                        color: gpui::white(),
                    };
                    let now = Instant::now();
                    let mut state = State {
                        shown: "New session".into(),
                        line: fit_text("New session", &style, window),
                        style: style.clone(),
                        transition: None,
                    };
                    state.prepare("Temporary prompt", style.clone(), bounds, now, window, cx);
                    let frame = state.prepare(
                        "Generated title",
                        style.clone(),
                        bounds,
                        now + Duration::from_millis(300),
                        window,
                        cx,
                    );
                    assert_eq!(frame.previous.unwrap().0.text.as_ref(), "Temporary prompt");
                    let start = state.transition.as_ref().unwrap().start;
                    // The retained ghost keeps its original truncation during resize.
                    let narrow = LabelStyle {
                        width: px(40.),
                        ..style.clone()
                    };
                    let frame = state.prepare(
                        "Generated title",
                        narrow,
                        bounds,
                        start + Duration::from_millis(550),
                        window,
                        cx,
                    );
                    assert!(frame.line.text.ends_with('…'));
                    assert_eq!(frame.previous.unwrap().0.text.as_ref(), "Temporary prompt");
                    assert_eq!(state.transition.as_ref().unwrap().start, start);
                    let frame = state.prepare(
                        "Generated title",
                        style.clone(),
                        bounds,
                        start + Duration::from_secs(3),
                        window,
                        cx,
                    );
                    assert!(frame.previous.is_none());
                    assert!(frame.particles.is_empty());
                    assert!(state.transition.is_none());
                    let frame = state.prepare(
                        "Generated title",
                        style,
                        bounds,
                        start + Duration::from_secs(4),
                        window,
                        cx,
                    );
                    assert!(frame.previous.is_none());
                    *checked.borrow_mut() = Some(());
                },
                |_, _, _, _| {},
            )
            .w(px(220.))
            .h(px(20.))
        }
    }
    let checked = Rc::new(RefCell::new(None));
    cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| Harness(checked.clone()))
        })
        .unwrap()
    });
    cx.refresh().unwrap();
    assert!(checked.borrow().is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn real_glyph_sampling_uses_alpha_not_text_rectangles_at_both_display_scales() {
    let raster = Arc::new(gpui_apple::AppleTextSystem::new());
    let font_id = raster.font_id(&gpui::font(".AppleSystemUIFont")).unwrap();
    for scale in [1., 2.] {
        let text = "I                    I café 漢字 🙂";
        let layout = raster.layout_line(
            text,
            px(14.),
            &[gpui::FontRun {
                len: text.len(),
                font_id,
            }],
        );
        let system = gpui::WindowTextSystem::new(Arc::new(gpui::TextSystem::new(raster.clone())));
        let line = system.shape_line(
            text.into(),
            px(14.),
            &[TextRun {
                len: text.len(),
                font: gpui::font(".AppleSystemUIFont"),
                color: gpui::white(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            None,
        );
        let particles = sample_text(&line, size(px(300.), px(20.)), scale, raster.as_ref());
        assert!(!particles.is_empty());
        let second_i = f32::from(layout.x_for_index(21));
        assert!(
            particles.iter().all(|p| p.x < 8. || p.x > second_i - 3.),
            "whitespace has no particles"
        );
        assert!(
            particles.iter().any(|p| p.x > second_i),
            "fallback glyphs are sampled too"
        );
        assert!(particles.iter().all(|p| {
            p.x >= 0.
                && p.x < 300.
                && p.y >= 0.
                && p.y < 20.
                && p.x % 1.5 == 0.
                && p.y % 1.5 == 0.
                && (0.25..=1.).contains(&p.alpha)
                && (0.595..=1.02).contains(&p.life)
        }));
    }
}
