//! The Astra scene from AstraWelcome.tsx and AstraWelcome.css: a pale gold
//! nebula, a small solar system above the composer, blooming sparkles, and
//! a shower of falling stars.

use std::f32::consts::PI;

use gpui::{Bounds, Hsla, Pixels, Point, Window, point, px};
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::theme::CubicBezier;

use super::paint::{
    EASE_IN_OUT, EASE_OUT, LINEAR, ellipse_at, ellipse_points, fill_ellipse, fill_polygon,
    keyframes, progress, radial_glow, stroke_ellipse, stroke_polyline,
};

/// `DURATION_MS`.
pub const ASTRA_DURATION_MS: f32 = 7600.0;

const WAVE: CubicBezier = CubicBezier(0.16, 1.0, 0.3, 1.0);

struct Palette {
    star: Hsla,
    bright: Hsla,
    /// Sparkles and star heads in the light theme.
    ink: Hsla,
    light: bool,
}

fn palette(light: bool) -> Palette {
    Palette {
        star: hex(0xd8c9a2),
        bright: hex(0xf5f0e4),
        ink: if light { hex(0x806c3b) } else { hex(0xf5f0e4) },
        light,
    }
}

/// Draws the scene at `elapsed` ms into the pane `bounds`.
pub fn paint(window: &mut Window, bounds: Bounds<Pixels>, elapsed: f32, light: bool) {
    let palette = palette(light);
    let t = elapsed / ASTRA_DURATION_MS;
    // `astra-welcome-fade`.
    let fade = keyframes(
        &[(0.0, 0.0), (0.08, 1.0), (0.72, 1.0), (1.0, 0.0)],
        t,
        EASE_OUT,
    );
    if fade <= 0.0 {
        return;
    }
    paint_nebula(window, bounds, t, fade, &palette);
    paint_solar_system(window, bounds, elapsed, fade, &palette);
    paint_sparkles(window, bounds, elapsed, fade, &palette);
    paint_stars(window, bounds, elapsed, fade, &palette);
}

/// `.astra-welcome-glow`.
fn paint_nebula(window: &mut Window, bounds: Bounds<Pixels>, t: f32, fade: f32, palette: &Palette) {
    let nebula = keyframes(
        &[(0.0, 0.0), (0.25, 1.0), (0.55, 1.0), (1.0, 0.0)],
        t,
        EASE_IN_OUT,
    );
    let alpha = nebula * fade;
    for (x, y, strength, stop) in [
        (0.82, 0.12, 0x26, 0.48),
        (1.0, 0.0, 0x12, 0.65),
        (0.15, 0.75, 0x14, 0.60),
    ] {
        let (center, rx, ry) = ellipse_at(bounds, x, y);
        radial_glow(
            window,
            center,
            rx,
            ry,
            palette.star,
            alpha * strength as f32 / 255.0,
            stop,
        );
    }
}

/// `.astra-solar-system`: kept small and above the composer, even in
/// splits.
fn paint_solar_system(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    elapsed: f32,
    fade: f32,
    palette: &Palette,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let size = (width * 0.10).clamp(46.0, 86.0);
    let center = point(
        bounds.origin.x + px(width * 0.82),
        bounds.origin.y + px((height * 0.15).clamp(58.0, 120.0)),
    );
    // `astra-solar-arrival`, 6s.
    let arrival = progress(elapsed, 0.0, 6000.0);
    let opacity = keyframes(
        &[(0.0, 0.0), (0.16, 0.85), (0.62, 0.6), (1.0, 0.0)],
        arrival,
        EASE_OUT,
    ) * fade;
    if opacity <= 0.0 {
        return;
    }
    let scale = keyframes(&[(0.0, 0.7), (0.16, 1.0), (1.0, 1.06)], arrival, EASE_OUT);
    let s = size * scale;
    let t = elapsed / ASTRA_DURATION_MS;

    // `.astra-solar-corona`: thin rays every 15deg, faded in a ring, turning
    // 45deg over the scene.
    let turn = keyframes(&[(0.0, 0.0), (1.0, 45.0)], t, LINEAR).to_radians();
    let corona = s * 2.8 / 2.0;
    for ray in 0..24 {
        let angle = turn + (ray as f32 * 15.0).to_radians() - PI / 2.0;
        let half = 1f32.to_radians();
        for (from, to, alpha) in [(0.12, 0.22, 0.5), (0.22, 0.40, 1.0), (0.40, 0.65, 0.45)] {
            let polygon = [
                polar(center, corona * from, angle - half),
                polar(center, corona * to, angle - half),
                polar(center, corona * to, angle + half),
                polar(center, corona * from, angle + half),
            ];
            fill_polygon(
                window,
                &polygon,
                with_alpha(palette.star, 0x38 as f32 / 255.0 * alpha * opacity),
            );
        }
    }

    // `.astra-solar-wave`: two rings that swell and fade.
    for delay in [200.0, 850.0] {
        let p = progress(elapsed, delay, 3200.0);
        let wave_opacity = keyframes(&[(0.0, 0.0), (0.15, 0.25), (1.0, 0.0)], p, WAVE) * opacity;
        let wave_scale = keyframes(&[(0.0, 0.5), (1.0, 4.5)], p, WAVE);
        if wave_opacity > 0.0 {
            let radius = s / 2.0 * wave_scale;
            stroke_ellipse(
                window,
                center,
                radius,
                radius,
                0.0,
                1.0,
                with_alpha(palette.star, wave_opacity),
            );
        }
    }

    // `.astra-solar-orbit`: tilted ellipses with a planet riding each.
    for (inset, tilt, squash, ring_alpha, reverse) in [
        (0.45, -28.0f32, 0.38, 0x35, false),
        (0.70, 48.0, 0.32, 0x30, true),
    ] {
        let radius = s * (1.0 + 2.0 * inset) / 2.0;
        let tilt = tilt.to_radians();
        let ring: Vec<Point<Pixels>> = ellipse_points(center, radius, radius * squash, tilt, 72);
        stroke_polyline(
            window,
            &ring,
            1.0,
            with_alpha(palette.star, ring_alpha as f32 / 255.0 * opacity),
            true,
        );
        let spin = keyframes(
            &[(0.0, 15.0), (1.0, 235.0)],
            if reverse { 1.0 - t } else { t },
            LINEAR,
        )
        .to_radians();
        // The planet starts at the top of the circle, turns with the span,
        // then takes the orbit's squash and tilt.
        let (x, y) = (radius * spin.sin(), -radius * spin.cos() * squash);
        let (sin, cos) = tilt.sin_cos();
        let planet = point(
            center.x + px(x * cos - y * sin),
            center.y + px(x * sin + y * cos),
        );
        radial_glow(window, planet, 8.0, 8.0, palette.star, 0.6 * opacity, 1.0);
        fill_ellipse(
            window,
            planet,
            2.0,
            2.0,
            0.0,
            with_alpha(palette.star, opacity),
        );
    }

    // `.astra-solar-core` and its halo.
    let core = s * 0.44 / 2.0;
    radial_glow(
        window,
        center,
        core * 2.3,
        core * 2.3,
        palette.star,
        0x1f as f32 / 255.0 * opacity,
        0.7,
    );
    let glow = if palette.light { 0.3 } else { 0.55 };
    radial_glow(
        window,
        center,
        core + 20.0,
        core + 20.0,
        palette.star,
        glow * opacity,
        1.0,
    );
    fill_ellipse(
        window,
        center,
        core,
        core,
        0.0,
        with_alpha(hex(0xa89560), opacity),
    );
    fill_ellipse(
        window,
        center,
        core * 0.8,
        core * 0.8,
        0.0,
        with_alpha(palette.star, opacity),
    );
    let highlight = point(center.x - px(core * 0.24), center.y - px(core * 0.3));
    radial_glow(
        window,
        highlight,
        core * 0.7,
        core * 0.7,
        palette.bright,
        opacity,
        1.0,
    );
}

/// `.astra-sparkle`: small blooms that drift down and fade.
fn paint_sparkles(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    elapsed: f32,
    fade: f32,
    palette: &Palette,
) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    for index in 0..36usize {
        let x = (3 + (index * 37) % 94) as f32 / 100.0 * width;
        let y = (4 + (index * 23) % 86) as f32 / 100.0 * height;
        let cross = index % 6 == 0;
        let size = if cross { 9.0 } else { 1.0 + (index % 3) as f32 };
        let delay = (0.2 + ((index * 7) % 24) as f32 * 0.12) * 1000.0;
        let duration = (2.2 + (index % 4) as f32 * 0.45) * 1000.0;
        let p = progress(elapsed, delay, duration);
        let opacity = keyframes(
            &[(0.0, 0.0), (0.3, 0.7), (0.65, 0.3), (1.0, 0.0)],
            p,
            EASE_IN_OUT,
        ) * fade;
        if opacity <= 0.0 {
            continue;
        }
        let ty = keyframes(&[(0.0, 0.0), (0.3, 4.0), (1.0, 16.0)], p, EASE_IN_OUT);
        let scale = keyframes(&[(0.0, 0.6), (0.3, 1.0), (1.0, 0.7)], p, EASE_IN_OUT);
        let center = point(
            bounds.origin.x + px(x + size / 2.0),
            bounds.origin.y + px(y + size / 2.0 + ty),
        );
        let half = size / 2.0 * scale;
        if cross {
            // The clip-path star: four long points and four short ones.
            let star: Vec<Point<Pixels>> = [
                (0.0, -1.0),
                (0.22, -0.24),
                (1.0, 0.0),
                (0.22, 0.22),
                (0.0, 1.0),
                (-0.24, 0.22),
                (-1.0, 0.0),
                (-0.24, -0.24),
            ]
            .into_iter()
            .map(|(dx, dy)| point(center.x + px(dx * half), center.y + px(dy * half)))
            .collect();
            fill_polygon(window, &star, with_alpha(palette.ink, opacity * 0.85));
        } else {
            let color = if palette.light {
                palette.ink
            } else {
                palette.star
            };
            radial_glow(
                window,
                center,
                half + 7.0,
                half + 7.0,
                color,
                0.5 * opacity,
                1.0,
            );
            fill_ellipse(window, center, half, half, 0.0, with_alpha(color, opacity));
        }
    }
}

/// `.astra-star-flight`: streaks falling down and to the left.
fn paint_stars(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    elapsed: f32,
    fade: f32,
    palette: &Palette,
) {
    const SEGMENTS: usize = 8;
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let angle = -55f32.to_radians();
    let (sin, cos) = angle.sin_cos();
    for index in 0..38usize {
        let x = (8 + (index * 37) % 125) as f32 / 100.0 * width;
        let y = (-12 + ((index * 19) % 48) as i32) as f32 / 100.0 * height;
        let delay = (0.35 + index as f32 * 0.12) * 1000.0;
        let duration = (1.4 + (index % 5) as f32 * 0.22) * 1000.0;
        let long = index % 9 == 0;
        let length = if long {
            190.0
        } else {
            45.0 + ((index * 23) % 95) as f32
        };
        let thickness = if long { 2.0 } else { 1.0 };
        let brightness = if index % 3 == 0 { 0.95 } else { 0.5 };
        let p = progress(elapsed, delay, duration);
        let opacity = keyframes(
            &[
                (0.0, 0.0),
                (0.12, brightness),
                (0.7, brightness),
                (1.0, 0.0),
            ],
            p,
            LINEAR,
        ) * fade;
        if opacity <= 0.0 {
            continue;
        }
        let dx = -0.7 * height * p;
        let dy = height * p;
        let head = point(bounds.origin.x + px(x + dx), bounds.origin.y + px(y + dy));
        // The streak runs from its head along -55deg and fades out.
        let along = |distance: f32| point(head.x + px(distance * cos), head.y + px(distance * sin));
        for segment in 0..SEGMENTS {
            let from = segment as f32 / SEGMENTS as f32;
            let to = (segment + 1) as f32 / SEGMENTS as f32;
            let strength = 1.0 - (from + to) / 2.0;
            let color = if segment == 0 {
                palette.ink
            } else {
                palette.star
            };
            stroke_polyline(
                window,
                &[along(length * from), along(length * to)],
                thickness,
                with_alpha(color, opacity * strength),
                false,
            );
        }
        radial_glow(window, head, 4.0, 4.0, palette.star, opacity * 0.6, 1.0);
        fill_ellipse(
            window,
            head,
            1.5,
            1.5,
            0.0,
            with_alpha(palette.ink, opacity),
        );
    }
}

fn polar(center: Point<Pixels>, radius: f32, angle: f32) -> Point<Pixels> {
    point(
        center.x + px(radius * angle.cos()),
        center.y + px(radius * angle.sin()),
    )
}
