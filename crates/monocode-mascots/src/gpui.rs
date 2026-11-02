//! Optional drawing and scheduling contracts; the application provides its clock.
use gpui::{fill, point, size, App, Bounds, Hsla, Pixels, Window};
use std::time::Duration;

pub mod composer;

/// The legacy composer painter snaps the local top-left once (not every cell),
/// mirrors horizontally, then clips each cell at the composer rim. Kept separate
/// from Crisp's merged/device-snapped runs to preserve the native pixel policy.
#[derive(Clone, Copy, Debug)]
pub struct RimSprite {
    pub center_x: f32,
    pub feet_height: f32,
    pub side: f32,
    pub facing: crate::composer::Facing,
    pub rim_y: f32,
}

pub fn paint_rim_mask(
    rows: &[&str],
    origin: gpui::Point<Pixels>,
    sprite: RimSprite,
    color: Hsla,
    window: &mut Window,
) {
    let scale = window.scale_factor();
    let unit = sprite.side / 8.;
    let snap = |v: f32| (v * scale).round() / scale;
    let left = snap(sprite.center_x - sprite.side / 2.);
    let top = snap(sprite.rim_y - sprite.feet_height - sprite.side);
    for (y, row) in rows.iter().enumerate() {
        let py = top + y as f32 * unit;
        if py >= sprite.rim_y {
            continue;
        }
        for (x, pixel) in row.bytes().take(8).enumerate() {
            if pixel != b'#' {
                continue;
            }
            let x = if sprite.facing == crate::composer::Facing::Left {
                7 - x
            } else {
                x
            };
            window.paint_quad(fill(
                Bounds::new(
                    origin + point(gpui::px(left + x as f32 * unit), gpui::px(py)),
                    size(gpui::px(unit), gpui::px(unit.min(sprite.rim_y - py))),
                ),
                color,
            ));
        }
    }
}

/// Implemented by the host using its shared animation service. Request only
/// while the current view is visible and needs another frame; do not retain an
/// entity or spawn a component-global timer. Interactive play may animate under
/// reduced motion, unlike decorative movement.
pub trait AnimationClock {
    fn elapsed(&self, cx: &mut App) -> Duration;
    fn request(&self, interactive: bool, window: &mut Window, cx: &mut App) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub enum PixelPolicy {
    /// Existing flat project icons: individual logical-pixel cells.
    #[default]
    Logical,
    /// Transcript crispEdges: merge horizontal runs and snap every boundary.
    Crisp,
}

/// Paint a mask without filling its transparent eye holes. Callers retain their
/// own placement, hop, tint/opacity and clipping policy. Rich materials/eyes are
/// deliberately not part of this basic painter.
pub fn paint_mask(
    rows: &[&str],
    bounds: Bounds<Pixels>,
    color: Hsla,
    policy: PixelPolicy,
    window: &mut Window,
) {
    let scale = window.scale_factor();
    let unit = bounds.size.width / 8.;
    let snap = |p: Pixels| match policy {
        PixelPolicy::Logical => p,
        PixelPolicy::Crisp => (p * scale).round() / scale,
    };
    for (y, row) in rows.iter().enumerate() {
        let top = snap(bounds.origin.y + unit * y as f32);
        let bottom = snap(bounds.origin.y + unit * (y + 1) as f32);
        let bytes = row.as_bytes();
        let mut x = 0;
        while x < bytes.len() {
            if bytes[x] != b'#' {
                x += 1;
                continue;
            }
            let start = x;
            x += 1;
            if matches!(policy, PixelPolicy::Crisp) {
                while x < bytes.len() && bytes[x] == b'#' {
                    x += 1;
                }
            }
            let left = snap(bounds.origin.x + unit * start as f32);
            let right = snap(bounds.origin.x + unit * x as f32);
            let quad_size = match policy {
                PixelPolicy::Logical => size(unit, unit),
                PixelPolicy::Crisp => size(right - left, bottom - top),
            };
            window.paint_quad(fill(Bounds::new(point(left, top), quad_size), color));
        }
    }
}
