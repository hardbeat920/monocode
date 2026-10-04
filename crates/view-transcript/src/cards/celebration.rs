//! One-shot bursts inside a freshly sent prompt bubble. Port of
//! src/features/sessions/ui/turnCelebration.ts, MonocodeSparkles.tsx,
//! PlanStepsBurst.tsx, and their rules in src/styles/index.css
//! (`.monocode-sparkles`, `.plan-steps`, and the keyframes they play).
//!
//! An /operator turn blooms with an amber halo, a sheen sweeps across it,
//! and sparkles rise from a glow at its base. A Plan turn plays the same
//! bloom in yellow while numbered marks rise and tick off.
//!
//! CSS animates each sparkle with transforms. GPUI has no transforms on
//! elements, so one canvas paints every particle at its keyframe values for
//! the current time. Under reduced motion (`App::reduce_motion`) nothing
//! draws, like `display: none` in the stylesheet.

use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;
use std::time::{Duration, Instant};

use gpui::{
    App, Background, Bounds, BoxShadow, Corners, Font, FontWeight, Global, Hsla, IntoElement,
    ParentElement as _, PathBuilder, Pixels, Point, RenderOnce, SharedString, Styled as _,
    TextAlign, TextRun, Window, canvas, div, fill, linear_color_stop, linear_gradient, point, px,
    size,
};
use monocode_ui::color::{hex, rgba8, with_alpha};
use monocode_ui::theme::CubicBezier;
use monocode_ui::{Theme, u};

/// `FRESH_MS`: only a turn sent moments ago celebrates.
pub const FRESH_MS: i64 = 4000;
/// Both bursts: the last mark's delay plus its flight, with fade slack.
pub const CELEBRATE_MS: u64 = 3200;

/// `shouldCelebrateTurn`.
pub fn should_celebrate_turn(
    block_id: &str,
    started_at: Option<i64>,
    now: i64,
    celebrated: &HashSet<String>,
) -> bool {
    let Some(started_at) = started_at else {
        return false;
    };
    if celebrated.contains(block_id) {
        return false;
    }
    let age = now - started_at;
    (0..FRESH_MS).contains(&age)
}

/// The `celebrated` set and when each burst started. Remounts (tab
/// switches, a row scrolled away and back) must not replay a burst.
#[derive(Default)]
pub struct Celebrations {
    celebrated: HashSet<String>,
    started: HashMap<String, Instant>,
}

impl Global for Celebrations {}

impl Celebrations {
    /// `useTurnCelebration`: how far into its burst the turn is, or `None`
    /// when it should not show one. The first call for a fresh turn starts
    /// the burst.
    pub fn progress(
        block_id: &str,
        started_at: Option<i64>,
        now: i64,
        duration: Duration,
        cx: &mut App,
    ) -> Option<Duration> {
        let state = cx.default_global::<Celebrations>();
        if let Some(start) = state.started.get(block_id) {
            let elapsed = start.elapsed();
            return (elapsed < duration).then_some(elapsed);
        }
        if !should_celebrate_turn(block_id, started_at, now, &state.celebrated) {
            return None;
        }
        state.celebrated.insert(block_id.to_string());
        state.started.insert(block_id.to_string(), Instant::now());
        Some(Duration::ZERO)
    }
}

/// `planStepCount`: roomy bubbles get more steps; a one-word prompt still
/// gets a short plan.
pub fn plan_step_count(width: f32) -> usize {
    ((width / 64.).floor() as i64).clamp(2, 6) as usize
}

/// A small deterministic generator, seeded from the block id so redraws
/// keep the same particles (`useState(makeSparkles)`).
pub(crate) struct Random(u64);

impl Random {
    pub(crate) fn new(seed: &str) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in seed.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        Self(hash | 1)
    }

    /// `Math.random()`: uniform in 0..1.
    pub(crate) fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// One rising particle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sparkle {
    pub star: bool,
    /// Left edge of its column, in percent of the bubble width.
    pub x: f32,
    pub size: f32,
    pub delay: f32,
    pub duration: f32,
    pub drift: f32,
    pub spin: f32,
}

const SPARKLE_COUNT: usize = 18;

/// `makeSparkles`.
pub fn make_sparkles(seed: &str) -> Vec<Sparkle> {
    let mut random = Random::new(seed);
    let mut sparkles: Vec<Sparkle> = (0..SPARKLE_COUNT)
        .map(|i| {
            let star = i % 3 != 2;
            Sparkle {
                star,
                // Spread evenly with some jitter so it never reads as a grid.
                x: (((i as f32 + random.next()) / SPARKLE_COUNT as f32) * 100.).clamp(4., 96.),
                size: if star {
                    7. + random.next() * 6.
                } else {
                    2.5 + random.next() * 2.
                },
                delay: 150. + random.next() * 950.,
                duration: 1200. + random.next() * 800.,
                drift: (random.next() - 0.5) * 28.,
                spin: if random.next() < 0.5 { -1. } else { 1. } * (90. + random.next() * 120.),
            }
        })
        .collect();
    // `.sort(() => Math.random() - 0.5)`: a shuffled paint order.
    for i in (1..sparkles.len()).rev() {
        let j = (random.next() * (i + 1) as f32) as usize % (i + 1);
        sparkles.swap(i, j);
    }
    sparkles
}

const FIRST_STEP_MS: f32 = 250.;
const STEP_GAP_MS: f32 = 170.;
const STEP_RISE_MS: f32 = 1650.;
const EMBER_COUNT: usize = 6;

/// One numbered plan mark.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanStep {
    pub x: f32,
    pub delay: f32,
    pub duration: f32,
    pub drift: f32,
}

/// The marks for a bubble `count` steps wide (`stepX`, the delays).
pub fn plan_steps(count: usize) -> Vec<PlanStep> {
    (0..count)
        .map(|index| PlanStep {
            x: ((index as f32 + 0.5) / count as f32) * 100.,
            delay: FIRST_STEP_MS + index as f32 * STEP_GAP_MS,
            duration: STEP_RISE_MS,
            drift: if index % 2 == 0 { -8. } else { 8. },
        })
        .collect()
}

/// `makeEmbers`.
pub fn make_embers(seed: &str, step_span_ms: f32) -> Vec<Sparkle> {
    let mut random = Random::new(&format!("{seed}:embers"));
    (0..EMBER_COUNT)
        .map(|i| Sparkle {
            star: false,
            x: (((i as f32 + random.next()) / EMBER_COUNT as f32) * 100.).clamp(5., 95.),
            size: 2.5 + random.next() * 2.,
            delay: FIRST_STEP_MS + random.next() * (step_span_ms + 300.),
            duration: 1000. + random.next() * 600.,
            drift: (random.next() - 0.5) * 20.,
            spin: 0.,
        })
        .collect()
}

const EASE_OUT: CubicBezier = CubicBezier(0.0, 0.0, 0.58, 1.0);
const EASE_IN_OUT: CubicBezier = CubicBezier(0.42, 0.0, 0.58, 1.0);
/// `monocode-rise`'s curve.
const RISE: CubicBezier = CubicBezier(0.3, 0.6, 0.4, 1.0);
/// The sheen's curve.
const SHEEN: CubicBezier = CubicBezier(0.4, 0.0, 0.2, 1.0);

/// Where an animation with `delay` and `duration` is at `time` ms, with
/// `animation-fill-mode: both`.
pub fn phase(time: f32, delay: f32, duration: f32) -> f32 {
    ((time - delay) / duration).clamp(0., 1.)
}

/// One property's keyframes, `(offset, value)` in order, eased per segment
/// the way CSS applies `animation-timing-function` between keyframes.
pub fn keyframes(stops: &[(f32, f32)], t: f32, ease: CubicBezier) -> f32 {
    let Some(first) = stops.first() else {
        return 0.;
    };
    if t <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            let span = b.0 - a.0;
            let local = if span <= 0. { 1. } else { (t - a.0) / span };
            return a.1 + (b.1 - a.1) * ease.ease(local);
        }
    }
    stops.last().map(|stop| stop.1).unwrap_or(0.)
}

/// `monocode-twinkle`: opacity, scale, and the share of the spin.
pub fn twinkle(t: f32) -> (f32, f32, f32) {
    let opacity = keyframes(
        &[(0., 0.), (0.18, 1.), (0.42, 0.75), (0.62, 1.), (1., 0.)],
        t,
        EASE_IN_OUT,
    );
    let scale = keyframes(
        &[(0., 0.2), (0.18, 1.), (0.42, 0.65), (0.62, 1.1), (1., 0.3)],
        t,
        EASE_IN_OUT,
    );
    let spin = keyframes(
        &[(0., 0.), (0.18, 0.3), (0.42, 0.5), (0.62, 0.7), (1., 1.)],
        t,
        EASE_IN_OUT,
    );
    (opacity, scale, spin)
}

/// `monocode-halo` and `turn-mode-halo`: how lit the halo is, 0 to 1.
pub fn halo(time: f32) -> f32 {
    keyframes(
        &[(0., 0.), (0.25, 1.), (1., 0.)],
        phase(time, 150., 2400.),
        EASE_OUT,
    )
}

/// `monocode-ember-glow`.
pub fn ember_glow(time: f32) -> f32 {
    keyframes(
        &[(0., 0.), (0.3, 1.), (1., 0.)],
        phase(time, 100., 2200.),
        EASE_OUT,
    )
}

/// `monocode-sheen`: the band's offset in widths, from -1.2 to 1.2.
pub fn sheen_offset(time: f32) -> f32 {
    -1.2 + 2.4 * SHEEN.ease(phase(time, 300., 1100.))
}

/// `plan-step-dot`: opacity, scale, and how filled the dot is (0 is the
/// 12% tint, 1 the full color).
pub fn plan_dot(t: f32) -> (f32, f32, f32) {
    let opacity = keyframes(&[(0., 0.), (0.1, 1.), (0.85, 1.), (1., 0.)], t, EASE_OUT);
    let scale = keyframes(
        &[
            (0., 0.3),
            (0.1, 1.2),
            (0.2, 1.),
            (0.38, 1.),
            (0.44, 1.14),
            (0.52, 1.),
            (1., 0.8),
        ],
        t,
        EASE_OUT,
    );
    let filled = keyframes(&[(0., 0.), (0.34, 0.), (0.44, 1.), (1., 1.)], t, EASE_OUT);
    (opacity, scale, filled)
}

/// `plan-step-number`: opacity and scale.
pub fn plan_number(t: f32) -> (f32, f32) {
    let opacity = keyframes(&[(0., 1.), (0.34, 1.), (0.42, 0.), (1., 0.)], t, EASE_OUT);
    let scale = keyframes(&[(0., 1.), (0.34, 1.), (0.42, 0.5), (1., 0.5)], t, EASE_OUT);
    (opacity, scale)
}

/// `plan-step-check`: opacity and how much of the stroke is drawn.
pub fn plan_check(t: f32) -> (f32, f32) {
    let opacity = keyframes(&[(0., 0.), (0.36, 0.), (0.5, 1.), (1., 1.)], t, EASE_OUT);
    let offset = keyframes(&[(0., 10.), (0.36, 10.), (0.5, 0.), (1., 0.)], t, EASE_OUT);
    (opacity, 10. - offset)
}

/// Which burst to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurstKind {
    /// `MonocodeSparkles`: an /operator turn.
    Sparkles,
    /// `PlanStepsBurst`: a Plan mode turn.
    PlanSteps,
}

/// The colors a burst paints with, for the current theme.
#[derive(Debug, Clone, Copy)]
struct Palette {
    /// `--sparkle-color` or `--mode-color`.
    color: Hsla,
    /// `--sparkle-glow` or `--mode-glow`.
    glow: Hsla,
    /// `--mode-ink`: the check mark.
    ink: Hsla,
    /// The halo ring and glow at full strength.
    ring: Hsla,
    halo: Hsla,
    /// The sheen's two middle stops.
    sheen_a: Hsla,
    sheen_b: Hsla,
    /// The base glow.
    base: Hsla,
    /// The ember's bright center.
    ember_core: Hsla,
}

/// `color-mix(in srgb, color P%, transparent)`.
fn mix(color: Hsla, share: f32) -> Hsla {
    with_alpha(color, color.a * share)
}

fn palette(kind: BurstKind, dark: bool) -> Palette {
    match kind {
        BurstKind::Sparkles => {
            let amber = rgba8(245, 158, 11, 1.);
            Palette {
                color: if dark { hex(0xfde68a) } else { hex(0xf59e0b) },
                glow: if dark {
                    rgba8(251, 191, 36, 0.9)
                } else {
                    rgba8(217, 119, 6, 0.55)
                },
                ink: gpui::transparent_black(),
                ring: with_alpha(amber, 0.55),
                halo: with_alpha(amber, 0.35),
                sheen_a: rgba8(254, 243, 199, 0.5),
                sheen_b: rgba8(251, 191, 36, 0.35),
                base: rgba8(251, 191, 36, 0.35),
                ember_core: hex(0xfffbeb),
            }
        }
        BurstKind::PlanSteps => {
            let (color, glow, ink) = if dark {
                (hex(0xfde047), rgba8(250, 204, 21, 0.85), hex(0x422006))
            } else {
                (hex(0xca8a04), rgba8(202, 138, 4, 0.55), gpui::white())
            };
            Palette {
                color,
                glow,
                ink,
                ring: mix(glow, 0.6),
                halo: mix(glow, 0.38),
                sheen_a: mix(color, 0.32),
                sheen_b: mix(glow, 0.28),
                base: mix(glow, 0.35),
                ember_core: hex(0xfefce8),
            }
        }
    }
}

/// `<MonocodeSparkles />` or `<PlanStepsBurst />` over a bubble. Put it last
/// among a `relative` bubble's children; it fills the bubble.
#[derive(IntoElement)]
pub struct CelebrationBurst {
    kind: BurstKind,
    block_id: String,
    started_at: Option<i64>,
    /// The bubble's corner radius in CSS px (`border-radius: inherit`).
    radius: f32,
    now: Option<i64>,
    frozen: Option<Duration>,
}

pub fn celebration_burst(
    kind: BurstKind,
    block_id: impl Into<String>,
    started_at: Option<i64>,
) -> CelebrationBurst {
    CelebrationBurst {
        kind,
        block_id: block_id.into(),
        started_at,
        radius: 12.,
        now: None,
        frozen: None,
    }
}

impl CelebrationBurst {
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// The wall clock to judge freshness by, for tests and screenshots.
    pub fn now(mut self, now: i64) -> Self {
        self.now = Some(now);
        self
    }

    /// Draw one still frame `elapsed` into the burst, for screenshots.
    pub fn frozen_at(mut self, elapsed: Duration) -> Self {
        self.frozen = Some(elapsed);
        self
    }
}

use super::util::now_ms;

impl RenderOnce for CelebrationBurst {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let empty = div().absolute().size_0();
        if cx.reduce_motion() {
            return empty.into_any_element();
        }
        let duration = Duration::from_millis(CELEBRATE_MS);
        let elapsed = match self.frozen {
            Some(elapsed) => elapsed,
            None => {
                let now = self.now.unwrap_or_else(now_ms);
                let Some(elapsed) =
                    Celebrations::progress(&self.block_id, self.started_at, now, duration, cx)
                else {
                    return empty.into_any_element();
                };
                // Redraw every frame until the burst ends.
                window.request_animation_frame();
                elapsed
            }
        };
        let time = elapsed.as_secs_f32() * 1000.;
        let dark = Theme::of(cx).is_dark();
        let colors = palette(self.kind, dark);
        let rem = window.rem_size();
        let radius = u(self.radius).to_pixels(rem);
        let kind = self.kind;
        let seed = self.block_id.clone();

        let halo_canvas = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let lit = halo(time);
                if lit <= 0.001 {
                    return;
                }
                paint_halo(bounds, radius, lit, &colors, window);
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let particles = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                paint_sheen(bounds, time, &colors, window);
                let corner = radius
                    .min(bounds.size.height / 2.)
                    .min(bounds.size.width / 2.);
                paint_base_glow(bounds, corner, time, &colors, window);
                match kind {
                    BurstKind::Sparkles => {
                        for sparkle in make_sparkles(&seed) {
                            paint_sparkle(bounds, corner, &sparkle, time, &colors, window);
                        }
                    }
                    BurstKind::PlanSteps => {
                        let count = plan_step_count(f32::from(bounds.size.width));
                        let span = (count as f32 - 1.) * STEP_GAP_MS;
                        for ember in make_embers(&seed, span) {
                            paint_sparkle(bounds, corner, &ember, time, &colors, window);
                        }
                        for (index, step) in plan_steps(count).iter().enumerate() {
                            paint_plan_step(bounds, corner, index, step, time, &colors, window, cx);
                        }
                    }
                }
            },
        )
        .size_full();

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(halo_canvas)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(radius)
                    .overflow_hidden()
                    .child(particles),
            )
            .into_any_element()
    }
}

/// `monocode-halo` and `turn-mode-halo`: a 1px ring and a 24px glow around
/// the bubble. A CSS box-shadow never shows under its element, and the
/// bubble is translucent, so the glow is painted through four masks that
/// leave the bubble itself out.
fn paint_halo(
    bounds: Bounds<Pixels>,
    radius: Pixels,
    lit: f32,
    colors: &Palette,
    window: &mut Window,
) {
    window.paint_quad(
        fill(bounds.dilate(px(1.)), gpui::transparent_black())
            .corner_radii(Corners::all(radius + px(1.)))
            .border_widths(px(1.))
            .border_color(mix(colors.ring, lit)),
    );
    let glow = BoxShadow {
        color: mix(colors.halo, lit),
        offset: point(px(0.), px(0.)),
        blur_radius: px(24.),
        spread_radius: px(3.),
        inset: false,
    };
    let outer = bounds.dilate(px(64.));
    let bands = [
        Bounds::from_corners(outer.origin, point(outer.right(), bounds.top())),
        Bounds::from_corners(point(outer.left(), bounds.bottom()), outer.bottom_right()),
        Bounds::from_corners(
            point(outer.left(), bounds.top()),
            point(bounds.left(), bounds.bottom()),
        ),
        Bounds::from_corners(
            point(bounds.right(), bounds.top()),
            point(outer.right(), bounds.bottom()),
        ),
    ];
    for band in bands {
        window.with_content_mask(Some(gpui::ContentMask { bounds: band }), |window| {
            window.paint_drop_shadows(bounds, Corners::all(radius), std::slice::from_ref(&glow))
        });
    }
}

/// `::before`: a bright band sweeping left to right once.
fn paint_sheen(bounds: Bounds<Pixels>, time: f32, colors: &Palette, window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let offset = sheen_offset(time);
    if offset <= -1.199 || offset >= 1.199 {
        return;
    }
    let layer_x = f32::from(bounds.origin.x) + width * offset;
    let clear = |color: Hsla| with_alpha(color, 0.);
    // `transparent 30%, a 48%, b 52%, transparent 70%`.
    let bands = [
        (0.30, 0.48, clear(colors.sheen_a), colors.sheen_a),
        (0.48, 0.52, colors.sheen_a, colors.sheen_b),
        (0.52, 0.70, colors.sheen_b, clear(colors.sheen_b)),
    ];
    for (from, to, start, end) in bands {
        let quad_bounds = Bounds::new(
            point(px(layer_x + width * from), bounds.origin.y),
            size(px(width * (to - from)), bounds.size.height),
        );
        window.paint_quad(fill(
            quad_bounds,
            linear_gradient(
                90.,
                linear_color_stop(start, 0.),
                linear_color_stop(end, 1.),
            ),
        ));
    }
}

/// `::after`: a warm glow rising from the bubble's base, then fading.
fn paint_base_glow(
    bounds: Bounds<Pixels>,
    radius: Pixels,
    time: f32,
    colors: &Palette,
    window: &mut Window,
) {
    let lit = ember_glow(time);
    if lit <= 0.001 {
        return;
    }
    // `radial-gradient(120% 70% at 50% 100%, glow, transparent 70%)`: the
    // glow is gone 49% of the height above the base. The quad keeps the
    // bubble's corners, which a rectangular clip would not.
    let color = mix(colors.base, lit);
    window.paint_quad(
        fill(
            bounds,
            linear_gradient(
                0.,
                linear_color_stop(color, 0.),
                linear_color_stop(with_alpha(color, 0.), 0.49),
            ),
        )
        .corner_radii(Corners::all(radius)),
    );
}

/// Whether `point` is inside the bubble's rounded outline
/// (`border-radius: inherit` with `overflow: hidden`).
fn inside_rounded(bounds: Bounds<Pixels>, radius: Pixels, point: Point<Pixels>) -> bool {
    if !bounds.contains(&point) {
        return false;
    }
    let r = f32::from(radius);
    let (x, y) = (f32::from(point.x), f32::from(point.y));
    let (left, top) = (f32::from(bounds.left()), f32::from(bounds.top()));
    let (right, bottom) = (f32::from(bounds.right()), f32::from(bounds.bottom()));
    let cx = x.clamp(left + r, right - r);
    let cy = y.clamp(top + r, bottom - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// The center of a particle `size` px across that started just below the
/// bubble's base, after rising for `progress`.
fn rise_center(
    bounds: Bounds<Pixels>,
    x_percent: f32,
    glyph: f32,
    drift: f32,
    progress: f32,
) -> Point<Pixels> {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let eased = RISE.ease(progress);
    // The column ends at the base; the glyph hangs below it (`bottom: -size`)
    // and the column rises by its own height plus 6px.
    let x = f32::from(bounds.origin.x) + width * x_percent / 100. + drift * eased;
    let y = f32::from(bounds.origin.y) + height + glyph / 2. - (height + 6.) * eased;
    point(px(x), px(y))
}

/// `.monocode-sparkle` and `.plan-step-ember`.
fn paint_sparkle(
    bounds: Bounds<Pixels>,
    corner: Pixels,
    sparkle: &Sparkle,
    time: f32,
    colors: &Palette,
    window: &mut Window,
) {
    let progress = phase(time, sparkle.delay, sparkle.duration);
    let (opacity, scale, spin) = twinkle(progress);
    if opacity <= 0.01 || time < sparkle.delay {
        return;
    }
    let center = rise_center(bounds, sparkle.x, sparkle.size, sparkle.drift, progress);
    if !inside_rounded(bounds, corner, center) {
        return;
    }
    let diameter = sparkle.size * scale;
    // `drop-shadow(0 0 3px glow)`.
    let glow_size = diameter * if sparkle.star { 0.55 } else { 1. };
    window.paint_drop_shadows(
        Bounds::centered_at(center, size(px(glow_size), px(glow_size))),
        Corners::all(px(glow_size / 2.)),
        &[BoxShadow {
            color: mix(colors.glow, opacity),
            offset: point(px(0.), px(0.)),
            blur_radius: px(if sparkle.star { 3. } else { 5. }),
            spread_radius: px(0.),
            inset: false,
        }],
    );
    if sparkle.star {
        let Some(path) = star_path(center, diameter, sparkle.spin * spin) else {
            return;
        };
        window.paint_path(path, mix(colors.color, opacity));
    } else {
        // `radial-gradient(circle, core, color 70%)`.
        let dot = Bounds::centered_at(center, size(px(diameter), px(diameter)));
        window.paint_quad(
            fill(dot, mix(colors.color, opacity)).corner_radii(Corners::all(px(diameter / 2.))),
        );
        let core = diameter * 0.45;
        window.paint_quad(
            fill(
                Bounds::centered_at(center, size(px(core), px(core))),
                mix(colors.ember_core, opacity),
            )
            .corner_radii(Corners::all(px(core / 2.))),
        );
    }
}

/// `STAR_PATH` (a four-pointed star in a 24 by 24 box), `diameter` px
/// across, turned `degrees` about its center.
fn star_path(center: Point<Pixels>, diameter: f32, degrees: f32) -> Option<gpui::Path<Pixels>> {
    let scale = diameter / 24.;
    let (sin, cos) = (degrees * PI / 180.).sin_cos();
    let map = |x: f32, y: f32| {
        let (dx, dy) = ((x - 12.) * scale, (y - 12.) * scale);
        point(
            center.x + px(dx * cos - dy * sin),
            center.y + px(dx * sin + dy * cos),
        )
    };
    let mut builder = PathBuilder::fill();
    builder.move_to(map(12., 0.));
    builder.cubic_bezier_to(map(24., 12.), map(12.9, 6.6), map(17.4, 11.1));
    builder.cubic_bezier_to(map(12., 24.), map(17.4, 12.9), map(12.9, 17.4));
    builder.cubic_bezier_to(map(0., 12.), map(11.1, 17.4), map(6.6, 12.9));
    builder.cubic_bezier_to(map(12., 0.), map(6.6, 11.1), map(11.1, 6.6));
    builder.close();
    builder.build().ok()
}

/// `.plan-step`: a numbered ring that fills and turns into a check as it
/// rises.
#[allow(clippy::too_many_arguments)]
fn paint_plan_step(
    bounds: Bounds<Pixels>,
    corner: Pixels,
    index: usize,
    step: &PlanStep,
    time: f32,
    colors: &Palette,
    window: &mut Window,
    cx: &mut App,
) {
    let progress = phase(time, step.delay, step.duration);
    let (opacity, scale, filled) = plan_dot(progress);
    if opacity <= 0.01 || time < step.delay {
        return;
    }
    const DOT: f32 = 16.;
    let center = rise_center(bounds, step.x, DOT, step.drift, progress);
    if !inside_rounded(bounds, corner, center) {
        return;
    }
    let diameter = DOT * scale;
    let dot = Bounds::centered_at(center, size(px(diameter), px(diameter)));
    // `box-shadow: 0 0 8px glow`.
    window.paint_drop_shadows(
        dot,
        Corners::all(px(diameter / 2.)),
        &[BoxShadow {
            color: mix(colors.glow, opacity),
            offset: point(px(0.), px(0.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        }],
    );
    let tint = 0.12 + 0.88 * filled;
    window.paint_quad(
        fill(dot, Background::from(mix(colors.color, tint * opacity)))
            .corner_radii(Corners::all(px(diameter / 2.)))
            .border_widths(px(1.5 * scale))
            .border_color(mix(colors.color, opacity)),
    );

    let (number_opacity, number_scale) = plan_number(progress);
    if number_opacity > 0.01 {
        let font_size = px(8.5 * scale * number_scale);
        let label: SharedString = (index + 1).to_string().into();
        let run = TextRun {
            len: label.len(),
            font: Font {
                weight: FontWeight::BOLD,
                ..gpui::font(Theme::of(cx).fonts.sans.clone())
            },
            color: mix(colors.color, opacity * number_opacity),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(label, font_size, &[run], None);
        let origin = point(center.x - line.width() / 2., center.y - font_size / 2.);
        let _ = line.paint(origin, font_size, TextAlign::Left, None, window, cx);
    }

    let (check_opacity, drawn) = plan_check(progress);
    if check_opacity > 0.01 && drawn > 0. {
        paint_check(
            center,
            scale,
            drawn,
            mix(colors.ink, opacity * check_opacity),
            window,
        );
    }
}

/// `CHECK_PATH` (`M3.2 6.3 5.1 8.2 8.8 4.2` in a 12 box drawn 10px wide),
/// stroked for its first `drawn` units, with round caps.
fn paint_check(center: Point<Pixels>, scale: f32, drawn: f32, color: Hsla, window: &mut Window) {
    let unit = 10. / 12. * scale;
    let map = |x: f32, y: f32| {
        point(
            center.x + px((x - 6.) * unit),
            center.y + px((y - 6.) * unit),
        )
    };
    let points = [(3.2, 6.3), (5.1, 8.2), (8.8, 4.2)];
    let mut left = drawn;
    let mut visible = vec![map(points[0].0, points[0].1)];
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        if left <= 0. {
            break;
        }
        let share = (left / length).min(1.);
        visible.push(map(a.0 + (b.0 - a.0) * share, a.1 + (b.1 - a.1) * share));
        left -= length;
    }
    let width = 1.8 * unit;
    let mut builder = PathBuilder::stroke(px(width));
    builder.add_polygon(&visible, false);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
    // `stroke-linecap: round` and the round join.
    for point in visible {
        window.paint_quad(
            fill(
                Bounds::centered_at(point, size(px(width), px(width))),
                color,
            )
            .corner_radii(Corners::all(px(width / 2.))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn celebrates_only_a_turn_sent_moments_ago() {
        let now = 1_000_000;
        let none = HashSet::new();
        assert!(should_celebrate_turn("fresh", Some(now - 500), now, &none));
        assert!(!should_celebrate_turn(
            "old",
            Some(now - 60_000),
            now,
            &none
        ));
        assert!(!should_celebrate_turn("unknown", None, now, &none));
        assert!(!should_celebrate_turn("future", Some(now + 10), now, &none));
        let seen: HashSet<String> = ["fresh".to_string()].into();
        assert!(!should_celebrate_turn("fresh", Some(now - 500), now, &seen));
    }

    #[test]
    fn fits_a_short_plan_into_a_narrow_bubble_and_caps_a_wide_one() {
        assert_eq!(plan_step_count(40.), 2);
        assert_eq!(plan_step_count(260.), 4);
        assert_eq!(plan_step_count(900.), 6);
    }

    #[test]
    fn sparkles_are_stable_per_turn_and_inside_the_bubble() {
        let a = make_sparkles("block-1");
        assert_eq!(a, make_sparkles("block-1"));
        assert_ne!(a, make_sparkles("block-2"));
        assert_eq!(a.len(), SPARKLE_COUNT);
        assert_eq!(a.iter().filter(|sparkle| sparkle.star).count(), 12);
        for sparkle in &a {
            assert!((4. ..=96.).contains(&sparkle.x));
            assert!((150. ..=1100.).contains(&sparkle.delay));
            assert!((1200. ..=2000.).contains(&sparkle.duration));
            assert!(sparkle.spin.abs() >= 90. && sparkle.spin.abs() <= 210.);
            // The last sparkle is gone before the burst ends.
            assert!(sparkle.delay + sparkle.duration <= CELEBRATE_MS as f32);
        }
    }

    #[test]
    fn plan_marks_space_evenly_and_finish_in_time() {
        let steps = plan_steps(4);
        assert_eq!(
            steps.iter().map(|step| step.x).collect::<Vec<_>>(),
            [12.5, 37.5, 62.5, 87.5]
        );
        assert_eq!(steps[3].delay, 250. + 3. * 170.);
        let last = steps.last().unwrap();
        assert!(last.delay + last.duration <= CELEBRATE_MS as f32);
        for ember in make_embers("b", 3. * 170.) {
            assert!((5. ..=95.).contains(&ember.x));
            assert!(ember.delay >= FIRST_STEP_MS);
        }
    }

    #[test]
    fn keyframes_hold_outside_the_range_and_ease_between_stops() {
        let stops = [(0., 0.), (0.5, 1.), (1., 0.)];
        assert_eq!(keyframes(&stops, -1., EASE_OUT), 0.);
        assert_eq!(keyframes(&stops, 0.5, EASE_OUT), 1.);
        assert_eq!(keyframes(&stops, 2., EASE_OUT), 0.);
        let quarter = keyframes(&stops, 0.25, EASE_OUT);
        // ease-out runs ahead of linear.
        assert!(quarter > 0.5 && quarter < 1.);
    }

    #[test]
    fn a_plan_mark_shows_its_number_then_its_check() {
        let (number, _) = plan_number(0.2);
        let (check, drawn) = plan_check(0.2);
        assert_eq!((number, check, drawn), (1., 0., 0.));
        let (number, _) = plan_number(0.6);
        let (check, drawn) = plan_check(0.6);
        assert_eq!((number, check, drawn), (0., 1., 10.));
        assert_eq!(plan_dot(0.).0, 0.);
        assert_eq!(plan_dot(1.).0, 0.);
        assert_eq!(plan_dot(0.6).2, 1.);
    }

    #[test]
    fn the_halo_blooms_then_fades() {
        assert_eq!(halo(0.), 0.);
        assert!((halo(150. + 600.) - 1.).abs() < 1e-4);
        assert_eq!(halo(3000.), 0.);
        assert_eq!(sheen_offset(0.), -1.2);
        assert!((sheen_offset(1400.) - 1.2).abs() < 1e-4);
    }

    #[gpui::test]
    fn a_burst_plays_once_per_turn(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let duration = Duration::from_millis(CELEBRATE_MS);
            let now = 1_000_000;
            assert_eq!(
                Celebrations::progress("t1", Some(now - 100), now, duration, cx),
                Some(Duration::ZERO)
            );
            assert!(Celebrations::progress("t1", Some(now - 100), now, duration, cx).is_some());
            assert_eq!(
                Celebrations::progress("t2", Some(now - 9_000), now, duration, cx),
                None
            );
            // A finished burst never plays again.
            assert_eq!(
                Celebrations::progress("t3", Some(now), now, Duration::ZERO, cx),
                Some(Duration::ZERO)
            );
            assert_eq!(
                Celebrations::progress("t3", Some(now), now, Duration::ZERO, cx),
                None
            );
        });
    }
}
