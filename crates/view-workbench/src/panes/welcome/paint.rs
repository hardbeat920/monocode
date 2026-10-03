//! Paint helpers for the welcome scenes: CSS keyframes with a per-segment
//! timing function, ellipses, and radial glows.
//!
//! GPUI draws two-stop linear gradients only. A `radial-gradient` from a
//! color to transparent becomes a stack of concentric ellipses whose alphas
//! add up to the same linear falloff.

use std::f32::consts::PI;

use gpui::{Bounds, Hsla, PathBuilder, Pixels, Point, Window, point, px};
use monocode_ui::color::with_alpha;
use monocode_ui::theme::CubicBezier;

/// CSS `ease`.
pub const EASE: CubicBezier = CubicBezier(0.25, 0.1, 0.25, 1.0);
/// CSS `ease-out`.
pub const EASE_OUT: CubicBezier = CubicBezier(0.0, 0.0, 0.58, 1.0);
/// CSS `ease-in-out`.
pub const EASE_IN_OUT: CubicBezier = CubicBezier(0.42, 0.0, 0.58, 1.0);
/// CSS `linear`.
pub const LINEAR: CubicBezier = CubicBezier(0.0, 0.0, 1.0, 1.0);

/// Progress of an animation with `fill-mode: both`: 0 before its delay, 1
/// after it ends.
pub fn progress(elapsed_ms: f32, delay_ms: f32, duration_ms: f32) -> f32 {
    if duration_ms <= 0.0 {
        return 1.0;
    }
    ((elapsed_ms - delay_ms) / duration_ms).clamp(0.0, 1.0)
}

/// A property's value at linear progress `t` through `stops`
/// (offset, value). The timing function applies to each segment, as CSS
/// applies `animation-timing-function` between keyframes.
pub fn keyframes(stops: &[(f32, f32)], t: f32, easing: CubicBezier) -> f32 {
    let Some(first) = stops.first() else {
        return 0.0;
    };
    if t <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            let span = (b.0 - a.0).max(f32::EPSILON);
            let local = easing.ease((t - a.0) / span);
            return a.1 + (b.1 - a.1) * local;
        }
    }
    stops.last().map_or(0.0, |last| last.1)
}

/// Points around an ellipse centered on `center`, rotated by `rotation`
/// radians.
pub fn ellipse_points(
    center: Point<Pixels>,
    rx: f32,
    ry: f32,
    rotation: f32,
    segments: usize,
) -> Vec<Point<Pixels>> {
    let (sin, cos) = rotation.sin_cos();
    (0..segments)
        .map(|index| {
            let angle = index as f32 / segments as f32 * PI * 2.0;
            let (x, y) = (rx * angle.cos(), ry * angle.sin());
            point(
                center.x + px(x * cos - y * sin),
                center.y + px(x * sin + y * cos),
            )
        })
        .collect()
}

/// Fills a closed polygon.
pub fn fill_polygon(window: &mut Window, points: &[Point<Pixels>], color: Hsla) {
    if points.len() < 3 || color.a <= 0.0 {
        return;
    }
    let mut builder = PathBuilder::fill();
    builder.add_polygon(points, true);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// Strokes a polyline, closed or open.
pub fn stroke_polyline(
    window: &mut Window,
    points: &[Point<Pixels>],
    width: f32,
    color: Hsla,
    closed: bool,
) {
    if points.len() < 2 || color.a <= 0.0 {
        return;
    }
    let mut builder = PathBuilder::stroke(px(width));
    builder.add_polygon(points, closed);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// Fills an ellipse.
pub fn fill_ellipse(
    window: &mut Window,
    center: Point<Pixels>,
    rx: f32,
    ry: f32,
    rotation: f32,
    color: Hsla,
) {
    let segments = ((rx.max(ry) * 1.5) as usize).clamp(12, 72);
    fill_polygon(
        window,
        &ellipse_points(center, rx, ry, rotation, segments),
        color,
    );
}

/// Strokes an ellipse outline.
pub fn stroke_ellipse(
    window: &mut Window,
    center: Point<Pixels>,
    rx: f32,
    ry: f32,
    rotation: f32,
    width: f32,
    color: Hsla,
) {
    let segments = ((rx.max(ry) * 1.5) as usize).clamp(16, 96);
    stroke_polyline(
        window,
        &ellipse_points(center, rx, ry, rotation, segments),
        width,
        color,
        true,
    );
}

/// A radial gradient from `color` at `alpha` in the center to transparent
/// at `stop` of the ellipse radii.
///
/// It paints one blurred rounded rect, a drop shadow with no element, so
/// the falloff is computed in floating point. Stacking many faint layers
/// instead would round each one to 8 bits and shift the hue.
pub fn radial_glow(
    window: &mut Window,
    center: Point<Pixels>,
    rx: f32,
    ry: f32,
    color: Hsla,
    alpha: f32,
    stop: f32,
) {
    if alpha <= 0.002 || rx <= 0.0 || ry <= 0.0 {
        return;
    }
    let (reach_x, reach_y) = (rx * stop, ry * stop);
    let (half_x, half_y) = (reach_x * 0.35, reach_y * 0.35);
    let sigma = 0.32 * reach_x.min(reach_y);
    // The blurred box peaks below 1 in its center; scale the color so the
    // center reaches `alpha`.
    let peak = erf(half_x / (sigma * std::f32::consts::SQRT_2))
        * erf(half_y / (sigma * std::f32::consts::SQRT_2));
    let strength = (alpha / peak.max(0.05)).min(1.0);
    let bounds = Bounds::new(
        point(center.x - px(half_x), center.y - px(half_y)),
        gpui::size(px(half_x * 2.0), px(half_y * 2.0)),
    );
    window.paint_drop_shadows(
        bounds,
        gpui::Corners::all(px(half_x.min(half_y))),
        &[gpui::BoxShadow {
            color: with_alpha(color, strength),
            offset: point(px(0.), px(0.)),
            blur_radius: px(sigma),
            spread_radius: px(0.),
            inset: false,
        }],
    );
}

/// The error function, to within 1.5e-7 (Abramowitz and Stegun 7.1.26).
fn erf(x: f32) -> f32 {
    let sign = x.signum();
    let x = x.abs() as f64;
    let t = 1.0 / (1.0 + 0.3275911 * x);
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    (sign as f64 * (1.0 - poly * (-x * x).exp())) as f32
}

/// `radial-gradient(ellipse at X% Y%, ...)`: the center and the
/// `farthest-corner` radii for a box.
pub fn ellipse_at(bounds: Bounds<Pixels>, x: f32, y: f32) -> (Point<Pixels>, f32, f32) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let cx = width * x;
    let cy = height * y;
    let rx = std::f32::consts::SQRT_2 * cx.max(width - cx);
    let ry = std::f32::consts::SQRT_2 * cy.max(height - cy);
    (
        point(bounds.origin.x + px(cx), bounds.origin.y + px(cy)),
        rx,
        ry,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_hold_the_ends_and_ease_each_segment() {
        let stops = [(0.0, 0.0), (0.06, 1.0), (0.78, 1.0), (1.0, 0.0)];
        assert_eq!(keyframes(&stops, -1.0, LINEAR), 0.0);
        assert_eq!(keyframes(&stops, 0.03, LINEAR), 0.5);
        assert_eq!(keyframes(&stops, 0.5, EASE_OUT), 1.0);
        assert_eq!(keyframes(&stops, 2.0, LINEAR), 0.0);
        assert!(keyframes(&stops, 0.03, EASE_OUT) > 0.5);
    }

    #[test]
    fn erf_matches_known_values() {
        assert!(erf(0.0).abs() < 1e-6);
        assert!((erf(1.0) - 0.842_700_8).abs() < 1e-5);
        assert!((erf(-0.5) + 0.520_499_9).abs() < 1e-5);
    }

    #[test]
    fn progress_fills_both_ways() {
        assert_eq!(progress(0.0, 500.0, 1000.0), 0.0);
        assert_eq!(progress(1000.0, 500.0, 1000.0), 0.5);
        assert_eq!(progress(9000.0, 500.0, 1000.0), 1.0);
    }
}
