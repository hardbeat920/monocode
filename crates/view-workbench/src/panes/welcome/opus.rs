//! The Opus 5.5 scene from OpusWelcome.tsx and OpusWelcome.css: a clay glow
//! over the pane, and a five-line staff that rises as a loose handwritten
//! wave in the space by the composer while a ten-note phrase plays on it.

use std::f32::consts::PI;

use gpui::{Bounds, BoxShadow, Corners, Hsla, Pixels, Window, point, px, size};
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::theme::CubicBezier;

use super::ComposerBand;
use super::paint::{
    EASE_IN_OUT, EASE_OUT, ellipse_at, fill_ellipse, keyframes, progress, radial_glow,
    stroke_ellipse, stroke_polyline,
};

/// `DURATION_MS`.
pub const OPUS_DURATION_MS: f32 = 7000.0;

/// A short phrase: it climbs, hesitates, and resolves on the middle line.
const MELODY: [(f32, i32); 10] = [
    (0.06, -3),
    (0.13, -1),
    (0.2, 1),
    (0.28, 0),
    (0.36, 2),
    (0.44, 4),
    (0.52, 3),
    (0.6, 1),
    (0.68, 2),
    (0.77, 0),
];

const LINE_WRITE: CubicBezier = CubicBezier(0.55, 0.1, 0.3, 1.0);
const NOTE_PLAY: CubicBezier = CubicBezier(0.2, 0.9, 0.3, 1.2);

struct Palette {
    clay: Hsla,
    note: Hsla,
    light: bool,
}

fn palette(light: bool) -> Palette {
    if light {
        Palette {
            clay: hex(0xbd5d3a),
            note: hex(0x8a4128),
            light,
        }
    } else {
        Palette {
            clay: hex(0xd97757),
            note: hex(0xf4ede4),
            light,
        }
    }
}

/// `sceneLayout`'s staff curve: where a line `step` half-spaces from the
/// middle sits at `t` across the stage.
fn staff_y(t: f32, step: f32, height: f32) -> f32 {
    let end_y = height * 0.42;
    let rise = height * 0.86 - end_y;
    end_y
        + rise * (1.0 - t).powf(1.7)
        + (t * PI * 2.4 + 0.3).sin() * 20.0f32.min(height * 0.08) * (1.0 - t)
        - (step / 2.0) * 11.0 * (1.0 - 0.84 * t)
}

/// Draws the scene at `elapsed` ms into the pane `bounds`.
pub fn paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    composer: Option<ComposerBand>,
    elapsed: f32,
    light: bool,
) {
    let palette = palette(light);
    let t = elapsed / OPUS_DURATION_MS;
    // `opus-welcome-fade`.
    let fade = keyframes(
        &[(0.0, 0.0), (0.06, 1.0), (0.78, 1.0), (1.0, 0.0)],
        t,
        EASE_OUT,
    );
    if fade <= 0.0 {
        return;
    }
    paint_glow(window, bounds, t, fade, &palette);

    let pane_height = f32::from(bounds.size.height);
    let (top, height) = super::opus_stage(pane_height, composer);
    let (top, height) = (top.round(), height.round());
    if height <= 0.0 {
        return;
    }
    let stage = Bounds::new(
        point(bounds.origin.x, bounds.origin.y + px(top)),
        size(bounds.size.width, px(height)),
    );
    paint_motes(window, stage, elapsed, fade, &palette);
    paint_staff(window, stage, elapsed, fade, &palette);
    paint_notes(window, stage, elapsed, fade, &palette);
}

/// `.opus-welcome-glow`: three soft radial washes that breathe in and out.
fn paint_glow(window: &mut Window, bounds: Bounds<Pixels>, t: f32, fade: f32, palette: &Palette) {
    let breathe = keyframes(
        &[(0.0, 0.0), (0.22, 1.0), (0.58, 1.0), (1.0, 0.0)],
        t,
        EASE_IN_OUT,
    );
    let alpha = fade * breathe;
    if alpha <= 0.0 {
        return;
    }
    let cream = hex(0xf4ede4);
    for (x, y, color, strength, stop) in [
        (0.82, 0.14, palette.clay, 0x24, 0.46),
        (0.40, 0.18, palette.clay, 0x12, 0.60),
        (0.10, 0.85, cream, 0x0c, 0.55),
    ] {
        let (center, rx, ry) = ellipse_at(bounds, x, y);
        radial_glow(
            window,
            center,
            rx,
            ry,
            color,
            alpha * strength as f32 / 255.0,
            stop,
        );
    }
    // `box-shadow: inset 0 0 50px #d9775708`.
    window.paint_inset_shadows(
        bounds,
        Corners::default(),
        &[BoxShadow {
            color: with_alpha(palette.clay, alpha * 8.0 / 255.0),
            offset: point(px(0.), px(0.)),
            blur_radius: px(50.),
            spread_radius: px(0.),
            inset: true,
        }],
    );
}

/// `.opus-mote`: small thoughts drifting up while the phrase is written.
fn paint_motes(
    window: &mut Window,
    stage: Bounds<Pixels>,
    elapsed: f32,
    fade: f32,
    palette: &Palette,
) {
    let width = f32::from(stage.size.width);
    let height = f32::from(stage.size.height);
    for index in 0..26usize {
        let x = (4 + (index * 41) % 92) as f32 / 100.0 * width;
        let y = (20 + (index * 29) % 72) as f32 / 100.0 * height;
        let size = 2.0 + (index % 4) as f32;
        let delay = (0.5 + ((index * 11) % 30) as f32 * 0.12) * 1000.0;
        let duration = (2.6 + (index % 5) as f32 * 0.4) * 1000.0;
        let drift = if index % 2 == 1 { 1.0 } else { -1.0 } * (6.0 + (index % 7) as f32 * 3.0);
        let p = progress(elapsed, delay, duration);
        let opacity = keyframes(&[(0.0, 0.0), (0.25, 0.55), (1.0, 0.0)], p, EASE_OUT) * fade;
        if opacity <= 0.0 {
            continue;
        }
        let tx = keyframes(
            &[(0.0, 0.0), (0.25, drift * 0.3), (1.0, drift)],
            p,
            EASE_OUT,
        );
        let ty = keyframes(&[(0.0, 0.0), (0.25, -10.0), (1.0, -46.0)], p, EASE_OUT);
        let scale = keyframes(&[(0.0, 0.6), (0.25, 1.0), (1.0, 0.8)], p, EASE_OUT);
        let ring = index % 5 == 0;
        let diameter = if ring { size * 2.2 } else { size };
        let center = point(
            stage.origin.x + px(x + diameter / 2.0 + tx),
            stage.origin.y + px(y + diameter / 2.0 + ty),
        );
        let radius = diameter / 2.0 * scale;
        if ring {
            stroke_ellipse(
                window,
                center,
                radius,
                radius,
                0.0,
                1.0,
                with_alpha(palette.clay, 0.6 * opacity),
            );
        } else {
            radial_glow(
                window,
                center,
                radius + 6.0,
                radius + 6.0,
                palette.clay,
                0.5 * opacity,
                1.0,
            );
            fill_ellipse(
                window,
                center,
                radius,
                radius,
                0.0,
                with_alpha(palette.clay, opacity),
            );
        }
    }
}

/// `.opus-staff-line`: each line writes itself left to right under a soft
/// pen edge, then settles away.
fn paint_staff(
    window: &mut Window,
    stage: Bounds<Pixels>,
    elapsed: f32,
    fade: f32,
    palette: &Palette,
) {
    const BUCKETS: usize = 8;
    let width = f32::from(stage.size.width);
    let height = f32::from(stage.size.height);
    let settle = keyframes(
        &[(0.0, 1.0), (0.6, 1.0), (1.0, 0.0)],
        elapsed / OPUS_DURATION_MS,
        EASE_IN_OUT,
    );
    for (index, step) in [-4.0f32, -2.0, 0.0, 2.0, 4.0].into_iter().enumerate() {
        let delay = (0.15 + index as f32 * 0.09) * 1000.0;
        let written = LINE_WRITE.ease(progress(elapsed, delay, 2300.0));
        // The mask is 250% wide and slides from 100% to 0%: opaque up to
        // `edge`, transparent 40% of the width later.
        let position = 1.0 - written;
        let edge = -1.5 * width * position + 1.05 * width;
        let soft = 0.4 * width;
        let line_alpha = if index == 2 { 0.8 } else { 0.55 } * settle * fade;
        if line_alpha <= 0.0 || edge + soft <= 0.0 {
            continue;
        }
        let points: Vec<(f32, f32)> = (0..=120)
            .map(|i| {
                let t = i as f32 / 120.0;
                (t * width, staff_y(t, step, height))
            })
            .collect();
        // Segments grouped by how much of the mask they show.
        let mut runs: Vec<(usize, Vec<gpui::Point<Pixels>>)> = Vec::new();
        for pair in points.windows(2) {
            let mid = (pair[0].0 + pair[1].0) / 2.0;
            let mask = ((edge + soft - mid) / soft).clamp(0.0, 1.0);
            let bucket = (mask * BUCKETS as f32).round() as usize;
            if bucket == 0 {
                continue;
            }
            let a = point(
                stage.origin.x + px(pair[0].0),
                stage.origin.y + px(pair[0].1),
            );
            let b = point(
                stage.origin.x + px(pair[1].0),
                stage.origin.y + px(pair[1].1),
            );
            match runs.last_mut() {
                Some((last, run)) if *last == bucket && run.last() == Some(&a) => run.push(b),
                _ => runs.push((bucket, vec![a, b])),
            }
        }
        for (bucket, run) in runs {
            let alpha = line_alpha * bucket as f32 / BUCKETS as f32;
            stroke_polyline(window, &run, 1.0, with_alpha(palette.clay, alpha), false);
        }
    }
}

/// `.opus-note`: each note lands with a struck-key ring, then drifts on.
fn paint_notes(
    window: &mut Window,
    stage: Bounds<Pixels>,
    elapsed: f32,
    fade: f32,
    palette: &Palette,
) {
    let width = f32::from(stage.size.width);
    let height = f32::from(stage.size.height);
    for (index, (t, step)) in MELODY.iter().enumerate() {
        let x = t * width;
        let y = staff_y(*t, *step as f32, height);
        let note_size = 12.6 * (1.0 - 0.84 * t);
        let delay = (1.05 + index as f32 * 0.23) * 1000.0;
        let w = note_size * 1.45;
        let h = note_size;
        let center = point(stage.origin.x + px(x), stage.origin.y + px(y));

        // `opus-note-ring`, the `::before` ring.
        let ring_p = progress(elapsed, delay, 1100.0);
        let ring_opacity =
            keyframes(&[(0.0, 0.0), (0.2, 0.6), (1.0, 0.0)], ring_p, EASE_OUT) * fade;
        if ring_opacity > 0.0 {
            let ring_scale = keyframes(&[(0.0, 0.4), (1.0, 2.6)], ring_p, EASE_OUT);
            stroke_ellipse(
                window,
                center,
                w * 0.9 * ring_scale,
                h * 0.9 * ring_scale,
                0.0,
                1.0,
                with_alpha(palette.clay, ring_opacity),
            );
        }

        // `opus-note-play`.
        let p = progress(elapsed, delay, 3600.0);
        let opacity = keyframes(
            &[(0.0, 0.0), (0.12, 1.0), (0.6, 0.9), (1.0, 0.0)],
            p,
            NOTE_PLAY,
        ) * fade;
        if opacity <= 0.0 {
            continue;
        }
        let ty = keyframes(&[(0.0, -0.4 * h), (0.12, 0.0), (1.0, 0.0)], p, NOTE_PLAY);
        let tx = keyframes(&[(0.0, 0.0), (0.6, 0.0), (1.0, 0.18 * w)], p, NOTE_PLAY);
        let scale = keyframes(&[(0.0, 0.2), (0.12, 1.0), (1.0, 1.0)], p, NOTE_PLAY).max(0.0);
        let center = point(center.x + px(tx), center.y + px(ty));
        let (hw, hh) = (w / 2.0 * scale, h / 2.0 * scale);

        // The head's glow (`box-shadow: 0 0 6px, 0 0 14px`).
        let glow = if palette.light { 0.25 } else { 0.5 };
        radial_glow(
            window,
            center,
            hw + 10.0,
            hh + 10.0,
            palette.clay,
            glow * opacity,
            1.0,
        );
        fill_ellipse(
            window,
            center,
            hw,
            hh,
            -22f32.to_radians(),
            with_alpha(palette.note, opacity),
        );

        // The stem (`::after`): up from the right edge, or down from the left.
        let stem_w = (note_size * 0.12).max(1.0) * scale;
        let stem_h = note_size * 3.1 * scale;
        let stem_down = *step >= 1;
        let (left, top) = if stem_down {
            (center.x - px(hw), center.y - px(hh) + px(0.45 * 2.0 * hh))
        } else {
            (
                center.x + px(hw) - px(stem_w),
                center.y - px(hh) + px(0.55 * 2.0 * hh) - px(stem_h),
            )
        };
        window.paint_quad(
            gpui::fill(
                Bounds::new(point(left, top), size(px(stem_w), px(stem_h))),
                with_alpha(palette.note, 0.85 * opacity),
            )
            .corner_radii(px(1.)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_staff_rises_from_the_lower_left_and_gathers_on_the_right() {
        let height = 300.0;
        assert!(staff_y(0.0, 0.0, height) > staff_y(1.0, 0.0, height));
        let spread = |t: f32| staff_y(t, -4.0, height) - staff_y(t, 4.0, height);
        assert!(spread(0.0) > spread(1.0));
        assert!((spread(0.0) - 44.0).abs() < 1e-3);
    }
}
