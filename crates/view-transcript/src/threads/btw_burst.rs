//! Port of src/features/sessions/ui/BtwQuestionBurst.tsx and the
//! `.btw-burst*` rules in index.css: a one-shot burst for opening a side
//! question. Question marks drift up out of the composer into the panel as
//! it opens, while the composer box glows and a sheen crosses it.
//!
//! GPUI cannot rotate text, so the marks keep their rise, drift, scale, and
//! fade but not their wobble's tilt.

use std::time::{Duration, Instant};

use gpui::{
    BoxShadow, Context, Corners, EventEmitter, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Task, WeakEntity, Window, canvas, div, fill, linear_color_stop,
    linear_gradient, point, px, size,
};
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::theme::CubicBezier;
use monocode_ui::{Theme, u};

use super::parts::{Random, paint_outer_glow};
use super::style::burst_colors;

/// Longest mark delay plus duration, with a little slack for the fade.
pub const BURST_MS: u64 = 1900;
pub const MARK_COUNT: usize = 16;

/// `BurstRect`: a box relative to the burst's positioned parent, in px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BurstRect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// `Mark`: one question mark (`glyph`) or ember.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub glyph: bool,
    /// Percent across the box.
    pub x: f32,
    pub size: f32,
    pub delay: f32,
    pub duration: f32,
    pub drift: f32,
    pub rise: f32,
    pub tilt: f32,
}

/// `makeMarks`.
pub fn make_marks(random: &mut dyn FnMut() -> f32) -> Vec<Mark> {
    let mut marks: Vec<(f32, Mark)> = (0..MARK_COUNT)
        .map(|i| {
            let glyph = i % 4 != 3;
            // Spread across the composer with jitter so it never reads as a grid.
            let x = 95f32.min(5f32.max(((i as f32 + random()) / MARK_COUNT as f32) * 100.));
            let size = if glyph {
                11. + random() * 9.
            } else {
                4. + random() * 3.
            };
            let mark = Mark {
                glyph,
                x,
                size,
                delay: 120. + random() * 520.,
                duration: 900. + random() * 500.,
                drift: (random() - 0.5) * 36.,
                rise: 110. + random() * 130.,
                tilt: (if random() < 0.5 { -1. } else { 1. }) * (10. + random() * 18.),
            };
            (random(), mark)
        })
        .collect();
    // `.sort(() => Math.random() - 0.5)`: a shuffle.
    marks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    marks.into_iter().map(|(_, mark)| mark).collect()
}

/// The burst tells its owner when it is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BtwQuestionBurstEvent {
    /// `onDone`.
    Done,
}

/// The burst element. A new open makes a new entity.
pub struct BtwQuestionBurst {
    rect: BurstRect,
    marks: Vec<Mark>,
    started: Instant,
    frozen_ms: Option<f32>,
    _timer: Task<()>,
}

impl EventEmitter<BtwQuestionBurstEvent> for BtwQuestionBurst {}

impl BtwQuestionBurst {
    pub fn new(rect: BurstRect, cx: &mut Context<Self>) -> Self {
        let mut random = Random::from_clock();
        Self::with_marks(rect, make_marks(&mut || random.next()), cx)
    }

    pub fn with_marks(rect: BurstRect, marks: Vec<Mark>, cx: &mut Context<Self>) -> Self {
        // One burst per mount; a new open makes a new burst.
        let timer = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(BURST_MS))
                .await;
            this.update(cx, |_, cx| cx.emit(BtwQuestionBurstEvent::Done))
                .ok();
        });
        Self {
            rect,
            marks,
            started: Instant::now(),
            frozen_ms: None,
            _timer: timer,
        }
    }

    pub fn rect(&self) -> BurstRect {
        self.rect
    }

    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// Draws one fixed moment instead of the running clock, for screenshots.
    pub fn freeze_at(&mut self, ms: f32, cx: &mut Context<Self>) {
        self.frozen_ms = Some(ms);
        cx.notify();
    }
}

fn progress(ms: f32, delay: f32, duration: f32) -> f32 {
    ((ms - delay) / duration).clamp(0., 1.)
}

fn keyframes(frames: &[(f32, f32)], t: f32) -> f32 {
    let mut previous = frames[0];
    for &frame in frames {
        if t <= frame.0 {
            let span = frame.0 - previous.0;
            if span <= 0. {
                return frame.1;
            }
            return previous.1 + (frame.1 - previous.1) * (t - previous.0) / span;
        }
        previous = frame;
    }
    previous.1
}

const EASE_OUT: CubicBezier = CubicBezier(0., 0., 0.58, 1.);
const EASE_IN_OUT: CubicBezier = CubicBezier(0.42, 0., 0.58, 1.);
const SHEEN: CubicBezier = CubicBezier(0.4, 0., 0.2, 1.);
const RISE: CubicBezier = CubicBezier(0.2, 0.7, 0.3, 1.);

impl Render for BtwQuestionBurst {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ms = self
            .frozen_ms
            .unwrap_or_else(|| self.started.elapsed().as_secs_f32() * 1000.);
        if self.frozen_ms.is_none() && ms < BURST_MS as f32 {
            window.request_animation_frame();
        }
        let theme = Theme::of(cx);
        let (color, glow) = burst_colors(theme);
        let radius = theme.radius.lg;
        let rect = self.rect;

        // `btw-burst-halo` and `btw-burst-sheen`, painted under the marks.
        let halo = keyframes(
            &[(0., 0.), (0.3, 1.), (1., 0.)],
            EASE_OUT.ease(progress(ms, 60., 1100.)),
        );
        let sheen = SHEEN.ease(progress(ms, 140., 900.));
        let sheen_on = (140.0..1040.0).contains(&ms);
        let glow_layer = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let corners = Corners::all(u(radius).to_pixels(window.rem_size()));
                if halo > 0. {
                    // `0 0 0 1px glow` and `0 0 28px 4px glow/45%`, outside the box.
                    let corner = corners.top_left;
                    paint_outer_glow(
                        window,
                        bounds,
                        corner,
                        with_alpha(glow, 0.45 * halo),
                        4.,
                        24.,
                    );
                    paint_outer_glow(window, bounds, corner, with_alpha(glow, halo), 1., 0.);
                }
                if sheen_on {
                    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                        let width = bounds.size.width;
                        let shift = width * (-1.2 + 2.4 * sheen);
                        let band = |from: f32, to: f32| {
                            gpui::Bounds::new(
                                point(bounds.origin.x + shift + width * from, bounds.origin.y),
                                size(width * (to - from), bounds.size.height),
                            )
                        };
                        window.paint_quad(fill(
                            band(0.3, 0.5),
                            linear_gradient(
                                90.,
                                linear_color_stop(with_alpha(color, 0.), 0.),
                                linear_color_stop(with_alpha(color, 0.3), 1.),
                            ),
                        ));
                        window.paint_quad(fill(
                            band(0.5, 0.7),
                            linear_gradient(
                                90.,
                                linear_color_stop(with_alpha(color, 0.3), 0.),
                                linear_color_stop(with_alpha(color, 0.), 1.),
                            ),
                        ));
                    });
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let mut root = div()
            .absolute()
            .left(px(rect.left))
            .top(px(rect.top))
            .w(px(rect.width))
            .h(px(rect.height))
            .rounded(u(radius))
            .child(glow_layer);

        for mark in &self.marks {
            let t = progress(ms, mark.delay, mark.duration);
            if t <= 0. || t >= 1. {
                continue;
            }
            let rise = RISE.ease(t);
            let wobble = EASE_IN_OUT.ease(t);
            let opacity = keyframes(&[(0., 0.), (0.15, 1.), (0.7, 0.9), (1., 0.)], wobble);
            let scale = keyframes(
                &[(0., 0.3), (0.15, 1.15), (0.45, 0.9), (0.7, 1.), (1., 0.6)],
                wobble,
            );
            let size_px = mark.size * scale;
            // `.btw-burst-mark` sits at `bottom: 45%` and `left: var(--x)`.
            let anchor_x = rect.width * mark.x / 100. + mark.drift * rise;
            let anchor_bottom = rect.height * 0.45 + mark.rise * rise;
            let top = rect.height - anchor_bottom - size_px;
            let left = anchor_x - size_px / 2.;
            if mark.glyph {
                root = root.child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(size_px))
                        .flex()
                        .justify_center()
                        .text_size(px(size_px.max(1.)))
                        .line_height(px(size_px.max(1.)))
                        .font_weight(FontWeight::BOLD)
                        .text_color(with_alpha(color, opacity))
                        .child(SharedString::from("?")),
                );
            } else {
                root = root.child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .size(px(size_px))
                        .rounded_full()
                        .bg(with_alpha(color, opacity))
                        .shadow(vec![
                            BoxShadow::new(px(0.), px(0.), with_alpha(glow, opacity))
                                .blur_radius(px(6.)),
                        ])
                        .child(
                            div()
                                .size_full()
                                .rounded_full()
                                .bg(with_alpha(hex(0xffffff), 0.6 * opacity)),
                        ),
                );
            }
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn makes_sixteen_marks_with_one_ember_in_four() {
        let mut random = Random::seeded(3);
        let marks = make_marks(&mut || random.next());
        assert_eq!(marks.len(), MARK_COUNT);
        assert_eq!(marks.iter().filter(|mark| mark.glyph).count(), 12);
        for mark in &marks {
            assert!((5.0..=95.0).contains(&mark.x));
            assert!((120.0..640.0).contains(&mark.delay));
            assert!((900.0..1400.0).contains(&mark.duration));
        }
    }

    #[gpui::test]
    fn scatters_question_marks_over_the_composer_and_ends_itself(cx: &mut TestAppContext) {
        let done = Rc::new(RefCell::new(0));
        let sink = done.clone();
        let burst = cx.new(|cx| {
            BtwQuestionBurst::new(
                BurstRect {
                    left: 10.,
                    top: 20.,
                    width: 300.,
                    height: 80.,
                },
                cx,
            )
        });
        cx.update(|cx| {
            cx.subscribe(&burst, move |_, _: &BtwQuestionBurstEvent, _| {
                *sink.borrow_mut() += 1;
            })
            .detach();
        });
        burst.read_with(cx, |burst, _| {
            assert_eq!(burst.rect().left, 10.);
            assert_eq!(burst.rect().width, 300.);
            assert_eq!(burst.marks().len(), 16);
            assert!(burst.marks().iter().any(|mark| mark.glyph));
        });
        cx.executor().advance_clock(Duration::from_millis(2000));
        cx.run_until_parked();
        assert_eq!(*done.borrow(), 1);
    }
}
