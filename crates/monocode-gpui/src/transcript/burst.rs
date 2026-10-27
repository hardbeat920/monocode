//! MonocodeSparkles, PlanStepsBurst and OrchestratorConstellation: the one-shot
//! burst inside a freshly sent /operator, Plan or Orchestrator prompt bubble.
//! Timings, colors and keyframes follow index.css; CSS's radial gradient and
//! rounded `overflow: hidden` are approximated with masked linear gradients and
//! a coverage fade at the bubble's corners.
use gpui::{
    App, Bounds, BoxShadow, ContentMask, Corners, FontWeight, Hsla, PathBuilder, Pixels, Point,
    TextAlign, TextRun, Window, fill, linear_color_stop, linear_gradient, point, px, size,
};
use std::{hash::BuildHasher, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Operator,
    Plan,
    Orchestrator,
}

impl Kind {
    pub(super) fn from_settings(settings: &monocode_core::agent::Settings) -> Option<Self> {
        if settings.orchestrator {
            Some(Self::Orchestrator)
        } else if settings.plan {
            Some(Self::Plan)
        } else if settings.operator {
            Some(Self::Operator)
        } else {
            None
        }
    }
    /// Each component's CELEBRATE_MS.
    fn duration_ms(self) -> f32 {
        match self {
            Self::Orchestrator => 3100.,
            Self::Operator | Self::Plan => 3200.,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Burst {
    pub(super) prompt: usize,
    kind: Kind,
    born: Instant,
    /// Every frame rebuilds the same random layout from this.
    seed: u64,
}

impl Burst {
    pub(super) fn new(prompt: usize, kind: Kind) -> Self {
        Self {
            prompt,
            kind,
            born: Instant::now(),
            seed: std::collections::hash_map::RandomState::new().hash_one(prompt),
        }
    }

    pub(super) fn active(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.born).as_secs_f32() * 1000. < self.kind.duration_ms()
    }

    pub(super) fn paint(
        &self,
        bounds: Bounds<Pixels>,
        corners: Corners<Pixels>,
        now: Instant,
        window: &mut Window,
        cx: &mut App,
    ) {
        let t = now.saturating_duration_since(self.born).as_secs_f32() * 1000.;
        let colors = Palette::new(self.kind, crate::theme::is_light());
        let bubble = Bubble { bounds, corners };
        paint_halo(&bubble, t, colors.halo_ring, colors.halo_glow, window);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            paint_sheen(&bubble, t, &colors.sheen, window);
            let mut rng = Rng(self.seed | 1);
            match self.kind {
                Kind::Operator => {
                    paint_ember_glow(&bubble, t, colors.ember_glow, window);
                    paint_sparkles(&bubble, t, &colors, &mut rng, window);
                }
                Kind::Plan => {
                    paint_ember_glow(&bubble, t, colors.ember_glow, window);
                    paint_plan_steps(&bubble, t, &colors, &mut rng, window, cx);
                }
                Kind::Orchestrator => {
                    paint_network(&bubble, t, &colors, &mut rng, window);
                    paint_ember_glow(&bubble, t, colors.ember_glow, window);
                }
            }
        });
    }
}

pub(crate) struct Bubble {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) corners: Corners<Pixels>,
}

impl Bubble {
    fn width(&self) -> f32 {
        f32::from(self.bounds.size.width)
    }
    fn height(&self) -> f32 {
        f32::from(self.bounds.size.height)
    }
    fn origin(&self) -> (f32, f32) {
        (
            f32::from(self.bounds.origin.x),
            f32::from(self.bounds.origin.y),
        )
    }
    /// How much of a dot of radius `r` at bubble-relative (x, y) survives the
    /// rounded clip, so marks fade at the corners instead of overhanging them.
    fn coverage(&self, x: f32, y: f32, r: f32) -> f32 {
        let (w, h) = (self.width(), self.height());
        let radius = f32::from(match (x < w * 0.5, y < h * 0.5) {
            (true, true) => self.corners.top_left,
            (false, true) => self.corners.top_right,
            (true, false) => self.corners.bottom_left,
            (false, false) => self.corners.bottom_right,
        });
        let qx = (x - w * 0.5).abs() - (w * 0.5 - radius);
        let qy = (y - h * 0.5).abs() - (h * 0.5 - radius);
        let outside = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - radius;
        ((r - outside) / (2. * r.max(0.5))).clamp(0., 1.)
    }
}

/// The CSS custom properties each burst paints with.
struct Palette {
    color: Hsla,
    glow: Hsla,
    /// A glyph's bright center: --mode-spark, or the gradient's first stop.
    spark: Hsla,
    ink: Hsla,
    halo_ring: Hsla,
    halo_glow: Hsla,
    sheen: [(f32, Hsla); 4],
    ember_glow: Hsla,
}

impl Palette {
    fn new(kind: Kind, light: bool) -> Self {
        if kind == Kind::Operator {
            let color = if light {
                hex(0xf59e0b, 1.)
            } else {
                hex(0xfde68a, 1.)
            };
            let glow = if light {
                hex(0xd97706, 0.55)
            } else {
                hex(0xfbbf24, 0.9)
            };
            return Self {
                color,
                glow,
                spark: hex(0xfffbeb, 1.),
                ink: color,
                halo_ring: hex(0xf59e0b, 0.55),
                halo_glow: hex(0xf59e0b, 0.35),
                sheen: [
                    (0.3, hex(0xfef3c7, 0.)),
                    (0.48, hex(0xfef3c7, 0.5)),
                    (0.52, hex(0xfbbf24, 0.35)),
                    (0.7, hex(0xfbbf24, 0.)),
                ],
                ember_glow: hex(0xfbbf24, 0.35),
            };
        }
        let (color, glow, spark, ink) = match (kind, light) {
            (Kind::Plan, false) => (
                hex(0xfde047, 1.),
                hex(0xfacc15, 0.85),
                hex(0xfefce8, 1.),
                hex(0x422006, 1.),
            ),
            (Kind::Plan, true) => (
                hex(0xca8a04, 1.),
                hex(0xca8a04, 0.55),
                hex(0xfefce8, 1.),
                hex(0xffffff, 1.),
            ),
            (_, false) => (
                hex(0xf0abfc, 1.),
                hex(0xe879f9, 0.85),
                hex(0xfdf4ff, 1.),
                hex(0xfdf4ff, 1.),
            ),
            (_, true) => (
                hex(0xc026d3, 1.),
                hex(0xc026d3, 0.5),
                hex(0xf5d0fe, 1.),
                hex(0xf5d0fe, 1.),
            ),
        };
        Self {
            color,
            glow,
            spark,
            ink,
            halo_ring: mix(glow, 0.6),
            halo_glow: mix(glow, 0.38),
            sheen: [
                (0.3, mix(color, 0.)),
                (0.48, mix(color, 0.32)),
                (0.52, mix(glow, 0.28)),
                (0.7, mix(glow, 0.)),
            ],
            ember_glow: mix(glow, 0.35),
        }
    }
}

fn hex(rgb: u32, a: f32) -> Hsla {
    let mut color: Hsla = gpui::rgb(rgb).into();
    color.a = a;
    color
}

/// `color-mix(in srgb, color k%, transparent)`, also used to fade by opacity.
pub(crate) fn mix(color: Hsla, k: f32) -> Hsla {
    Hsla {
        a: color.a * k.clamp(0., 1.),
        ..color
    }
}

pub(crate) type Ease = (f32, f32, f32, f32);
pub(crate) const EASE_OUT: Ease = (0., 0., 0.58, 1.);
pub(crate) const EASE_IN_OUT: Ease = (0.42, 0., 0.58, 1.);
const RISE: Ease = (0.3, 0.6, 0.4, 1.);
pub(crate) const SHEEN: Ease = (0.4, 0., 0.2, 1.);

pub(crate) fn ease(p: f32, (x1, y1, x2, y2): Ease) -> f32 {
    crate::reveal::bezier(p, x1, y1, x2, y2)
}

/// An `animation-fill-mode: both` animation's linear progress at `t` ms.
pub(crate) fn progress(t: f32, delay: f32, duration: f32) -> f32 {
    ((t - delay) / duration).clamp(0., 1.)
}

/// One property's keyframes, eased per segment as CSS does.
pub(crate) fn frames(p: f32, stops: &[(f32, f32)], timing: Ease) -> f32 {
    let mut previous = stops[0];
    if p <= previous.0 {
        return previous.1;
    }
    for &next in &stops[1..] {
        if p <= next.0 {
            let local = ease((p - previous.0) / (next.0 - previous.0), timing);
            return previous.1 + (next.1 - previous.1) * local;
        }
        previous = next;
    }
    previous.1
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// turn-mode-halo / monocode-halo: a ring and glow bloom around the bubble.
fn paint_halo(bubble: &Bubble, t: f32, ring: Hsla, glow: Hsla, window: &mut Window) {
    let halo = Halo {
        delay: 150.,
        duration: 2400.,
        peak: 0.25,
        blur: 24.,
        spread: 3.,
    };
    halo.paint(bubble, t, ring, glow, window);
}

/// A one-shot ring-and-glow box-shadow keyframe: none, a peak, then none.
pub(crate) struct Halo {
    pub(crate) delay: f32,
    pub(crate) duration: f32,
    pub(crate) peak: f32,
    pub(crate) blur: f32,
    pub(crate) spread: f32,
}

impl Halo {
    pub(crate) fn paint(
        &self,
        bubble: &Bubble,
        t: f32,
        ring: Hsla,
        glow: Hsla,
        window: &mut Window,
    ) {
        let k = frames(
            progress(t, self.delay, self.duration),
            &[(0., 0.), (self.peak, 1.), (1., 0.)],
            EASE_OUT,
        );
        if k <= 0. {
            return;
        }
        window.paint_drop_shadows(
            bubble.bounds,
            bubble.corners,
            &[
                BoxShadow {
                    color: mix(ring, k),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(k),
                    inset: false,
                },
                BoxShadow {
                    color: mix(glow, k),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(self.blur * k),
                    spread_radius: px(self.spread * k),
                    inset: false,
                },
            ],
        );
    }
}

/// Paint the bubble's rounded shape, but only inside `mask`, with a gradient
/// running `angle` between two bubble-relative positions.
fn paint_masked(
    bubble: &Bubble,
    mask: Bounds<Pixels>,
    angle: f32,
    from: (Hsla, f32),
    to: (Hsla, f32),
    window: &mut Window,
) {
    window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
        window.paint_quad(
            fill(
                bubble.bounds,
                linear_gradient(
                    angle,
                    linear_color_stop(from.0, from.1),
                    linear_color_stop(to.0, to.1),
                ),
            )
            .corner_radii(bubble.corners),
        );
    });
}

/// The ::before sheen: a 105deg band that sweeps from -120% to 120%. GPUI's
/// gradients have two stops, so each stop pair paints in thin horizontal
/// strips that follow the band's slant.
fn paint_sheen(bubble: &Bubble, t: f32, stops: &[(f32, Hsla); 4], window: &mut Window) {
    paint_sweep(bubble, progress(t, 300., 1100.), stops, window);
}

/// A 105deg gradient band crossing the bubble at linear progress `p`.
pub(crate) fn paint_sweep(bubble: &Bubble, p: f32, stops: &[(f32, Hsla)], window: &mut Window) {
    if p <= 0. || p >= 1. {
        return;
    }
    const STRIP: f32 = 2.;
    let (w, h) = (bubble.width(), bubble.height());
    let (bx, by) = bubble.origin();
    let shift = (-1.2 + 2.4 * ease(p, SHEEN)) * w;
    // CSS angles point up at 0deg and turn clockwise; screen y points down.
    let angle = 105f32.to_radians();
    let (dx, dy) = (angle.sin(), -angle.cos());
    let length = w * dx.abs() + h * dy.abs();
    let mut y = 0.;
    while y < h {
        let bottom = (y + STRIP).min(h);
        let from_center = (y + bottom) * 0.5 - h * 0.5;
        let x = |s: f32| w * 0.5 + shift + ((s - 0.5) * length - from_center * dy) / dx;
        for pair in stops.windows(2) {
            let (x0, x1) = (x(pair[0].0), x(pair[1].0));
            if x1 <= 0. || x0 >= w {
                continue;
            }
            let mask = Bounds::new(
                point(px(bx + x0), px(by + y)),
                size(px(x1 - x0), px(bottom - y)),
            );
            paint_masked(
                bubble,
                mask,
                90.,
                (pair[0].1, x0 / w),
                (pair[1].1, x1 / w),
                window,
            );
        }
        y = bottom;
    }
}

/// The ::after glow, `radial-gradient(120% 70% at 50% 100%, color, transparent
/// 70%)`, painted as narrow columns of vertical gradients.
fn paint_ember_glow(bubble: &Bubble, t: f32, color: Hsla, window: &mut Window) {
    let k = frames(
        progress(t, 100., 2200.),
        &[(0., 0.), (0.3, 1.), (1., 0.)],
        EASE_OUT,
    );
    if k <= 0. {
        return;
    }
    const COLUMN: f32 = 6.;
    const SEGMENTS: usize = 4;
    let (w, h) = (bubble.width(), bubble.height());
    let (bx, by) = bubble.origin();
    let (rx, ry) = (1.2 * w, 0.7 * h);
    let alpha = |nx: f32, ny: f32| (1. - nx.hypot(ny) / 0.7).max(0.) * k;
    let mut x = 0.;
    while x < w {
        let right = (x + COLUMN).min(w);
        let nx = ((x + right) * 0.5 - w * 0.5) / rx;
        let reach = ry * (0.49 - nx * nx).max(0.).sqrt();
        for i in 0..SEGMENTS {
            let near = reach * i as f32 / SEGMENTS as f32;
            let far = reach * (i + 1) as f32 / SEGMENTS as f32;
            let (top, bottom) = (h - far, h - near);
            if bottom <= 0. {
                continue;
            }
            let mask = Bounds::new(
                point(px(bx + x), px(by + top)),
                size(px(right - x), px(bottom - top)),
            );
            paint_masked(
                bubble,
                mask,
                180.,
                (mix(color, alpha(nx, far / ry)), top / h),
                (mix(color, alpha(nx, near / ry)), bottom / h),
                window,
            );
        }
        x = right;
    }
}

/// monocode-rise: a column the bubble's height drifts up past the top edge.
fn rise(t: f32, delay: f32, duration: f32, h: f32, drift: f32) -> (f32, f32) {
    let p = ease(progress(t, delay, duration), RISE);
    (drift * p, -(h + 6.) * p)
}

/// monocode-twinkle's opacity, scale and rotation (as a fraction of --spin).
fn twinkle(p: f32) -> (f32, f32, f32) {
    let stops = [0., 0.18, 0.42, 0.62, 1.];
    let at = |values: [f32; 5]| {
        let pairs: Vec<_> = stops.iter().copied().zip(values).collect();
        frames(p, &pairs, EASE_IN_OUT)
    };
    (
        at([0., 1., 0.75, 1., 0.]),
        at([0.2, 1., 0.65, 1.1, 0.3]),
        at([0., 0.3, 0.5, 0.7, 1.]),
    )
}

pub(crate) fn paint_dot(center: Point<Pixels>, diameter: f32, color: Hsla, window: &mut Window) {
    let r = diameter * 0.5;
    window.paint_quad(
        fill(
            Bounds::new(
                center - point(px(r), px(r)),
                size(px(diameter), px(diameter)),
            ),
            color,
        )
        .corner_radii(px(r)),
    );
}

pub(crate) fn paint_glow(
    center: Point<Pixels>,
    diameter: f32,
    blur: f32,
    color: Hsla,
    window: &mut Window,
) {
    let r = diameter * 0.5;
    window.paint_drop_shadows(
        Bounds::new(
            center - point(px(r), px(r)),
            size(px(diameter), px(diameter)),
        ),
        Corners::all(px(r)),
        &[BoxShadow {
            color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(blur),
            spread_radius: px(0.),
            inset: false,
        }],
    );
}

/// `radial-gradient(circle, spark, color 70%)` on a small round glyph.
pub(crate) fn paint_orb(
    center: Point<Pixels>,
    diameter: f32,
    colors: (Hsla, Hsla),
    window: &mut Window,
) {
    paint_dot(center, diameter, colors.1, window);
    paint_dot(center, diameter * 0.55, mix(colors.0, 0.8), window);
}

const STAR: [[(f32, f32); 3]; 4] = [
    [(12.9, 6.6), (17.4, 11.1), (24., 12.)],
    [(17.4, 12.9), (12.9, 17.4), (12., 24.)],
    [(11.1, 17.4), (6.6, 12.9), (0., 12.)],
    [(6.6, 11.1), (11.1, 6.6), (12., 0.)],
];

fn paint_star(center: Point<Pixels>, size: f32, turn: f32, color: Hsla, window: &mut Window) {
    let (sin, cos) = turn.to_radians().sin_cos();
    let scale = size / 24.;
    let at = |(x, y): (f32, f32)| {
        let (x, y) = ((x - 12.) * scale, (y - 12.) * scale);
        center + point(px(x * cos - y * sin), px(x * sin + y * cos))
    };
    let mut path = PathBuilder::fill();
    path.move_to(at((12., 0.)));
    for [a, b, to] in STAR {
        path.cubic_bezier_to(at(to), at(a), at(b));
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// MonocodeSparkles: eighteen stars and embers drift up and twinkle.
fn paint_sparkles(bubble: &Bubble, t: f32, colors: &Palette, rng: &mut Rng, window: &mut Window) {
    const COUNT: usize = 18;
    let (w, h) = (bubble.width(), bubble.height());
    let (bx, by) = bubble.origin();
    for i in 0..COUNT {
        let star = i % 3 != 2;
        let x = (((i as f32 + rng.next()) / COUNT as f32) * 100.).clamp(4., 96.) / 100. * w;
        let size = if star {
            7. + rng.next() * 6.
        } else {
            2.5 + rng.next() * 2.
        };
        let delay = 150. + rng.next() * 950.;
        let duration = 1200. + rng.next() * 800.;
        let drift = (rng.next() - 0.5) * 28.;
        let spin = if rng.next() < 0.5 { -1. } else { 1. } * (90. + rng.next() * 120.);
        let (opacity, scale, turn) = twinkle(progress(t, delay, duration));
        let (dx, dy) = rise(t, delay, duration, h, drift);
        let (cx, cy) = (x + dx, h + size * 0.5 + dy);
        let opacity = opacity * bubble.coverage(cx, cy, size * 0.5 * scale);
        if opacity <= 0.01 {
            continue;
        }
        let center = point(px(bx + cx), px(by + cy));
        if star {
            paint_glow(
                center,
                size * scale * 0.45,
                3.,
                mix(colors.glow, opacity),
                window,
            );
            paint_star(
                center,
                size * scale,
                spin * turn,
                mix(colors.color, opacity),
                window,
            );
        } else {
            paint_glow(center, size * scale, 3., mix(colors.glow, opacity), window);
            paint_orb(
                center,
                size * scale,
                (mix(colors.spark, opacity), mix(colors.color, opacity)),
                window,
            );
        }
    }
}

const FIRST_STEP_MS: f32 = 250.;
const STEP_GAP_MS: f32 = 170.;
const STEP_RISE_MS: f32 = 1650.;
const CHECK: [(f32, f32); 3] = [(3.2, 6.3), (5.1, 8.2), (8.8, 4.2)];

/// Roomy bubbles get more steps; a one-word prompt still gets a short plan.
pub(super) fn plan_step_count(width: f32) -> usize {
    ((width / 64.).floor() as usize).clamp(2, 6)
}

/// PlanStepsBurst: numbered marks tick off as they float up and out.
fn paint_plan_steps(
    bubble: &Bubble,
    t: f32,
    colors: &Palette,
    rng: &mut Rng,
    window: &mut Window,
    cx: &mut App,
) {
    let (w, h) = (bubble.width(), bubble.height());
    let (bx, by) = bubble.origin();
    let count = plan_step_count(w);
    let span = (count - 1) as f32 * STEP_GAP_MS;
    // Embers sit beneath the numbered marks.
    for i in 0..6 {
        let x = (((i as f32 + rng.next()) / 6.) * 100.).clamp(5., 95.) / 100. * w;
        let size = 2.5 + rng.next() * 2.;
        let delay = FIRST_STEP_MS + rng.next() * (span + 300.);
        let duration = 1000. + rng.next() * 600.;
        let drift = (rng.next() - 0.5) * 20.;
        let (opacity, scale, _) = twinkle(progress(t, delay, duration));
        let (dx, dy) = rise(t, delay, duration, h, drift);
        let (ex, ey) = (x + dx, h + size * 0.5 + dy);
        let opacity = opacity * bubble.coverage(ex, ey, size * 0.5 * scale);
        if opacity <= 0.01 {
            continue;
        }
        let center = point(px(bx + ex), px(by + ey));
        paint_glow(center, size * scale, 5., mix(colors.glow, opacity), window);
        paint_orb(
            center,
            size * scale,
            (mix(colors.spark, opacity), mix(colors.color, opacity)),
            window,
        );
    }
    let mut font = window.text_style().font();
    font.weight = FontWeight::BOLD;
    for i in 0..count {
        let delay = FIRST_STEP_MS + i as f32 * STEP_GAP_MS;
        let p = progress(t, delay, STEP_RISE_MS);
        let drift = if i % 2 == 0 { -8. } else { 8. };
        let (dx, dy) = rise(t, delay, STEP_RISE_MS, h, drift);
        let (sx, sy) = ((i as f32 + 0.5) / count as f32 * w + dx, h + 8. + dy);
        // plan-step-dot
        let opacity = frames(p, &[(0., 0.), (0.1, 1.), (0.85, 1.), (1., 0.)], EASE_OUT)
            * bubble.coverage(sx, sy, 8.);
        if opacity <= 0.01 {
            continue;
        }
        let scale = frames(
            p,
            &[
                (0., 0.3),
                (0.1, 1.2),
                (0.2, 1.),
                (0.38, 1.),
                (0.44, 1.14),
                (0.52, 1.),
                (1., 0.8),
            ],
            EASE_OUT,
        );
        let fill_alpha = frames(
            p,
            &[(0., 0.12), (0.34, 0.12), (0.44, 1.), (1., 1.)],
            EASE_OUT,
        );
        let center = point(px(bx + sx), px(by + sy));
        let diameter = 16. * scale;
        let r = diameter * 0.5;
        paint_glow(center, diameter, 8., mix(colors.glow, opacity), window);
        window.paint_quad(
            fill(
                Bounds::new(
                    center - point(px(r), px(r)),
                    size(px(diameter), px(diameter)),
                ),
                mix(colors.color, fill_alpha * opacity),
            )
            .corner_radii(px(r))
            .border_widths(px(1.5 * scale))
            .border_color(mix(colors.color, opacity)),
        );
        // plan-step-number
        let number = frames(p, &[(0., 1.), (0.34, 1.), (0.42, 0.), (1., 0.)], EASE_OUT);
        if number > 0.01 {
            let font_size = 8.5 * scale * frames(p, &[(0.34, 1.), (0.42, 0.5)], EASE_OUT);
            let text = gpui::SharedString::from((i + 1).to_string());
            let line = window.text_system().shape_line(
                text.clone(),
                px(font_size),
                &[TextRun {
                    len: text.len(),
                    font: font.clone(),
                    color: mix(colors.color, number * opacity),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            );
            let origin = center - point(line.width * 0.5, px(font_size * 0.5));
            let _ = line.paint(origin, px(font_size), TextAlign::Left, None, window, cx);
        }
        // plan-step-check: a 10px stroke that draws itself in.
        let check = frames(p, &[(0., 0.), (0.36, 0.), (0.5, 1.), (1., 1.)], EASE_OUT);
        if check > 0.01 {
            let unit = 10. / 12. * scale;
            let at = |(x, y): (f32, f32)| center + point(px((x - 6.) * unit), px((y - 6.) * unit));
            let mut left = 10. - frames(p, &[(0.36, 10.), (0.5, 0.)], EASE_OUT);
            let mut path = PathBuilder::stroke(px(1.8 * unit));
            path.move_to(at(CHECK[0]));
            for pair in CHECK.windows(2) {
                let length = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
                let k = (left / length).min(1.);
                if k <= 0. {
                    break;
                }
                path.line_to(at((
                    pair[0].0 + (pair[1].0 - pair[0].0) * k,
                    pair[0].1 + (pair[1].1 - pair[0].1) * k,
                )));
                left -= length;
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, mix(colors.ink, check * opacity));
            }
        }
    }
}

const FIRST_EDGE_MS: f32 = 280.;
const EDGE_GAP_MS: f32 = 80.;
const EDGE_DRAW_MS: f32 = 320.;
const CHILD_DRAW_MS: f32 = 220.;
const INSET: f32 = 8.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Node {
    pub(super) x: f32,
    pub(super) y: f32,
    /// Where its edge starts: the hub, or the node it branched from.
    pub(super) from: (f32, f32),
    /// When its edge starts drawing.
    pub(super) delay: f32,
    pub(super) draw: f32,
    pub(super) child: bool,
}

/// A lead at the bottom center fans work out to a few agents across the
/// bubble, and a couple of them hand off again. Everything stays inside the
/// bubble so its clipping never cuts a node in half.
pub(super) fn make_constellation(
    width: f32,
    height: f32,
    mut random: impl FnMut() -> f32,
) -> ((f32, f32), Vec<Node>) {
    let max_x = INSET.max(width - INSET);
    let max_y = INSET.max(height - INSET);
    let hub = (width / 2., max_y);
    let count = ((width / 90.).round() as usize).clamp(3, 6);
    // Short single-line bubbles still leave the agents a little above the lead.
    let top = max_y.min(INSET.max(height * 0.35));
    let bottom = top.max((max_y - 10.).min(height * 0.7));
    let mut agents: Vec<Node> = (0..count)
        .map(|i| Node {
            x: (((i as f32 + 0.2 + random() * 0.6) / count as f32) * width).clamp(INSET, max_x),
            y: top + random() * (bottom - top),
            from: hub,
            delay: 0.,
            draw: EDGE_DRAW_MS,
            child: false,
        })
        .collect();
    // Fan out from the center so the lead reads as reaching both ways at once.
    agents.sort_by(|a, b| (a.x - hub.0).abs().total_cmp(&(b.x - hub.0).abs()));
    for (i, agent) in agents.iter_mut().enumerate() {
        agent.delay = FIRST_EDGE_MS + i as f32 * EDGE_GAP_MS;
    }
    let children: Vec<Node> = agents
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, parent)| {
            let direction = if parent.x < hub.0 { -1. } else { 1. };
            Node {
                x: (parent.x + direction * (22. + random() * 18.)).clamp(INSET, max_x),
                y: (parent.y + (random() - 0.5) * 12.).clamp(top, max_y),
                from: (parent.x, parent.y),
                delay: parent.delay + parent.draw + 60.,
                draw: CHILD_DRAW_MS,
                child: true,
            }
        })
        .collect();
    agents.extend(children);
    (hub, agents)
}

fn stroke_line(
    from: Point<Pixels>,
    to: Point<Pixels>,
    width: f32,
    color: Hsla,
    window: &mut Window,
) {
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(from);
    path.line_to(to);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// orchestrator-hub / orchestrator-node with its expanding ::after ring.
fn paint_node(
    center: Point<Pixels>,
    diameter: f32,
    (t, delay, ring_delay, ring_ms): (f32, f32, f32, f32),
    fade: f32,
    colors: &Palette,
    window: &mut Window,
) {
    let p = progress(t, delay, 600.);
    let opacity = frames(p, &[(0., 0.), (0.3, 1.), (1., 1.)], EASE_OUT) * fade;
    let peak = if diameter >= 8. { 1.35 } else { 1.4 };
    let low = if diameter >= 8. { 0.3 } else { 0.2 };
    let scale = frames(p, &[(0., low), (0.3, peak), (1., 1.)], EASE_OUT);
    let ring = progress(t, ring_delay, ring_ms);
    let ring_opacity = frames(ring, &[(0., 0.8), (1., 0.)], EASE_OUT) * opacity;
    let ring_scale = scale * frames(ring, &[(0., 1.), (1., 3.5)], EASE_OUT);
    if opacity <= 0.01 {
        return;
    }
    paint_glow(
        center,
        diameter * scale,
        8.,
        mix(colors.glow, opacity),
        window,
    );
    paint_orb(
        center,
        diameter * scale,
        (mix(colors.spark, opacity), mix(colors.color, opacity)),
        window,
    );
    if ring_opacity > 0.01 {
        let d = diameter * ring_scale;
        window.paint_quad(
            fill(
                Bounds::new(center - point(px(d * 0.5), px(d * 0.5)), size(px(d), px(d))),
                gpui::transparent_black(),
            )
            .corner_radii(px(d * 0.5))
            .border_widths(px(1.5 * ring_scale))
            .border_color(mix(colors.color, ring_opacity)),
        );
    }
}

/// OrchestratorConstellation: the lead lights up at the bubble's base, edges
/// race out to agents that pop in and hand off again, then the whole network
/// rises beyond the top edge.
fn paint_network(bubble: &Bubble, t: f32, colors: &Palette, rng: &mut Rng, window: &mut Window) {
    let (w, h) = (bubble.width(), bubble.height());
    let (bx, by) = bubble.origin();
    let (hub, nodes) = make_constellation(w, h, || rng.next());
    // orchestrator-network-rise
    let p = progress(t, 300., 2200.);
    let fade = frames(p, &[(0., 1.), (0.85, 1.), (1., 0.)], RISE);
    let lift = -(h + 12.) * ease(p, RISE);
    if fade <= 0.01 {
        return;
    }
    let at = |(x, y): (f32, f32)| point(px(bx + x), px(by + y + lift));
    for node in &nodes {
        let length = (node.x - node.from.0).hypot(node.y - node.from.1);
        if length <= 0. {
            continue;
        }
        let along = |s: f32| {
            let k = s / length;
            at((
                node.from.0 + (node.x - node.from.0) * k,
                node.from.1 + (node.y - node.from.1) * k,
            ))
        };
        let q = progress(t, node.delay, node.draw);
        let drawn = length * ease(q, RISE);
        if drawn > 0.01 {
            stroke_line(
                at(node.from),
                along(drawn),
                1.25,
                mix(colors.color, 0.6 * fade),
                window,
            );
        }
        // A bright head rides the tip of each edge as it draws out.
        let offset = frames(q, &[(0., 6.), (1., -length)], RISE);
        let head = frames(q, &[(0., 1.), (0.85, 1.), (1., 0.)], RISE) * fade;
        let (start, end) = ((-offset).max(0.), (6. - offset).min(length));
        if head > 0.01 && end > start && q > 0. {
            stroke_line(
                along(start),
                along(end),
                5.,
                mix(colors.glow, 0.35 * head),
                window,
            );
            stroke_line(
                along(start),
                along(end),
                2.5,
                mix(colors.spark, head),
                window,
            );
        }
    }
    paint_node(at(hub), 8., (t, 120., 220., 700.), fade, colors, window);
    for node in &nodes {
        let delay = node.delay + node.draw - 40.;
        let diameter = if node.child { 4.5 } else { 6. };
        paint_node(
            at((node.x, node.y)),
            diameter,
            (t, delay, delay, 500.),
            fade,
            colors,
            window,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bursts_lay_out_inside_the_bubble() {
        assert_eq!(plan_step_count(40.), 2);
        assert_eq!(plan_step_count(200.), 3);
        assert_eq!(plan_step_count(900.), 6);
        let mut rng = Rng(7);
        for (w, h) in [(60., 36.), (320., 36.), (700., 120.)] {
            let (hub, nodes) = make_constellation(w, h, || rng.next());
            assert_eq!(hub, (w / 2., h - INSET));
            let agents = nodes.iter().filter(|n| !n.child).count();
            assert!((3..=6).contains(&agents));
            assert_eq!(nodes.len() - agents, agents / 2);
            for node in &nodes {
                assert!(node.x >= INSET && node.x <= w - INSET, "{node:?}");
                assert!(node.y >= INSET && node.y <= h - INSET, "{node:?}");
            }
        }
    }

    #[test]
    fn keyframes_ease_per_segment_and_hold_their_ends() {
        let stops = [(0., 0.), (0.25, 1.), (1., 0.)];
        assert_eq!(frames(0., &stops, EASE_OUT), 0.);
        assert_eq!(frames(0.25, &stops, EASE_OUT), 1.);
        assert_eq!(frames(1., &stops, EASE_OUT), 0.);
        assert!(frames(0.125, &stops, EASE_OUT) > 0.5);
        assert_eq!(progress(50., 150., 2400.), 0.);
        assert_eq!(progress(9000., 150., 2400.), 1.);
    }
}
