//! Port of `EffortTileShimmer` in ModelPicker.tsx and the
//! `.codex-effort-option` rules in ModelPicker.css: Codex's Max and Ultra
//! effort options light up with a grid of tiles that ripple out from the
//! middle, and a soft glow that pulses along the row's inside edge.
//!
//! The animation plays once, for five seconds, each time the row becomes the
//! highlighted one. With reduced motion the tiles sit still, the filled ones
//! solid, and the glow stays off, as the CSS `prefers-reduced-motion` rules
//! leave it.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, Bounds, BoxShadow, ElementId, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Rems, SharedString, Styled as _, Window, canvas, div,
    fill, point, px, size,
};
use monocode_core::HarnessId;
use monocode_core::models::ModelSetting;
use monocode_ui::color::{hex, with_alpha};
use monocode_ui::theme::CubicBezier;

use super::model_logic::is_effort_setting;

/// `data-effort-tone`: which effort option shimmers, and in what color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffortTone {
    Ultra,
    Max,
}

impl EffortTone {
    /// `--codex-effort-color`.
    pub fn color(self) -> Hsla {
        match self {
            EffortTone::Ultra => hex(0xa855f7),
            EffortTone::Max => hex(0xf4b942),
        }
    }
}

/// `effortTileTone`: only Codex's Max and Ultra reasoning options shimmer.
pub fn effort_tile_tone(
    harness: HarnessId,
    setting: &ModelSetting,
    value: &str,
) -> Option<EffortTone> {
    if harness != HarnessId::Codex || !is_effort_setting(setting) {
        return None;
    }
    match value.to_lowercase().as_str() {
        "ultra" => Some(EffortTone::Ultra),
        "max" => Some(EffortTone::Max),
        _ => None,
    }
}

pub const TILE_COLUMNS: usize = 32;
pub const TILE_ROWS: usize = 5;
pub const TILE_COUNT: usize = TILE_COLUMNS * TILE_ROWS;

/// The whole sequence, after which the tiles are gone.
const CYCLE_MS: f32 = 5000.;
/// The tiles fade out over the last 8% of the cycle.
const CLEAR_FROM_MS: f32 = CYCLE_MS * 0.92;
/// One fill pulse, played three times.
const FILL_MS: f32 = 1400.;
const FILL_REPEATS: f32 = 3.;
/// How much later a tile starts per unit of distance from the middle.
const DELAY_PER_DISTANCE_MS: f32 = 420.;
/// The tiles feather into the row over this many px at each side.
const FEATHER_X: f32 = 28.;
/// And over this fraction of the row's height at the top and bottom.
const FEATHER_Y: f32 = 0.35;
/// The grid line along each tile's right and bottom edge.
const LINE_ALPHA: f32 = 0.32;
/// Reduced motion shows the tiles still, at this opacity.
const STILL_OPACITY: f32 = 0.72;

const EASE_OUT: CubicBezier = CubicBezier(0.0, 0.0, 0.58, 1.0);
const EASE_IN_OUT: CubicBezier = CubicBezier(0.42, 0.0, 0.58, 1.0);

/// Whether tile `index` is one of the filled tiles. The pattern is fixed, so
/// every row shows the same scatter.
pub fn tile_filled(index: usize) -> bool {
    (index * 73 + index * index * 19 + 23) % 101 < 65
}

/// A tile's distance from the middle of the grid, 0 at the center and about
/// 1.41 in the corners.
pub fn tile_distance(column: usize, row: usize) -> f32 {
    let center_column = (TILE_COLUMNS as f32 - 1.) / 2.;
    let center_row = (TILE_ROWS as f32 - 1.) / 2.;
    ((column as f32 - center_column) / center_column).hypot((row as f32 - center_row) / center_row)
}

/// `codex-effort-tiles-clear`: the tiles' opacity `ms` into the cycle.
pub fn tiles_opacity(ms: f32) -> f32 {
    if ms <= CLEAR_FROM_MS {
        1.
    } else {
        (1. - (ms - CLEAR_FROM_MS) / (CYCLE_MS - CLEAR_FROM_MS)).max(0.)
    }
}

/// `codex-effort-tile-fill`: how much of the effort color a filled tile at
/// `distance` shows `ms` into the cycle. Each pulse goes clear, full, a
/// faint 18%, and clear again.
pub fn tile_fill(ms: f32, distance: f32) -> f32 {
    let local = ms - distance * DELAY_PER_DISTANCE_MS;
    if local <= 0. || local >= FILL_MS * FILL_REPEATS {
        return 0.;
    }
    let t = (local % FILL_MS) / FILL_MS;
    keyframes(&[(0., 0.), (0.26, 1.), (0.52, 0.18), (1., 0.)], t, EASE_OUT)
}

/// `codex-effort-edge-ripple`: the glow's strength `ms` into the cycle, 0
/// at rest and 1 at each of its three peaks.
pub fn edge_glow(ms: f32) -> f32 {
    let t = ms / CYCLE_MS;
    keyframes(
        &[
            (0.12, 0.),
            (0.19, 1.),
            (0.26, 0.),
            (0.40, 0.),
            (0.47, 1.),
            (0.54, 0.),
            (0.68, 0.),
            (0.75, 1.),
            (0.82, 0.),
        ],
        t,
        EASE_IN_OUT,
    )
}

/// The tile mask: fades at both sides and at the top and bottom, multiplied
/// so the corners round off. `x` and `y` are offsets into a `width` by
/// `height` row.
pub fn feather(x: f32, y: f32, width: f32, height: f32) -> f32 {
    let across = (x.min(width - x) / FEATHER_X).clamp(0., 1.);
    let down = if height <= 0. {
        0.
    } else {
        let y = y / height;
        (y.min(1. - y) / FEATHER_Y).clamp(0., 1.)
    };
    across * down
}

/// A value between keyframes, each segment eased with `easing`, as CSS
/// applies `animation-timing-function` per keyframe. Outside the first and
/// last keyframes the value holds.
fn keyframes(frames: &[(f32, f32)], t: f32, easing: CubicBezier) -> f32 {
    let Some(&(first_at, first)) = frames.first() else {
        return 0.;
    };
    if t <= first_at {
        return first;
    }
    for pair in frames.windows(2) {
        let ((from_at, from), (to_at, to)) = (pair[0], pair[1]);
        if t <= to_at {
            let span = (to_at - from_at).max(f32::EPSILON);
            let eased = easing.ease((t - from_at) / span);
            return from + (to - from) * eased;
        }
    }
    frames.last().map_or(0., |&(_, value)| value)
}

/// One frame of the tiles: `ms` into the cycle, or `None` for the still
/// frame reduced motion shows.
fn paint_tiles(bounds: Bounds<Pixels>, tone: EffortTone, ms: Option<f32>, window: &mut Window) {
    let color = tone.color();
    let opacity = ms.map_or(STILL_OPACITY, tiles_opacity);
    if opacity <= 0. {
        return;
    }
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let tile_w = width / TILE_COLUMNS as f32;
    let tile_h = height / TILE_ROWS as f32;
    for index in 0..TILE_COUNT {
        let (column, row) = (index % TILE_COLUMNS, index / TILE_COLUMNS);
        let x = column as f32 * tile_w;
        let y = row as f32 * tile_h;
        let mask = feather(x + tile_w / 2., y + tile_h / 2., width, height) * opacity;
        if mask <= 0. {
            continue;
        }
        let origin = bounds.origin + point(px(x), px(y));
        if tile_filled(index) {
            let lit = ms.map_or(1., |ms| tile_fill(ms, tile_distance(column, row)));
            if lit > 0. {
                window.paint_quad(fill(
                    Bounds::new(origin, size(px(tile_w), px(tile_h))),
                    with_alpha(color, lit * mask),
                ));
            }
        }
        // `box-shadow: inset -1px -1px 0`: a line on the right and bottom.
        let line = with_alpha(color, LINE_ALPHA * mask);
        window.paint_quad(fill(
            Bounds::new(
                origin + point(px(tile_w - 1.), px(0.)),
                size(px(1.), px(tile_h)),
            ),
            line,
        ));
        window.paint_quad(fill(
            Bounds::new(
                origin + point(px(0.), px(tile_h - 1.)),
                size(px(tile_w - 1.), px(1.)),
            ),
            line,
        ));
    }
}

/// `<EffortTileShimmer />`: the tile layer. Put it first in a `relative`
/// row so the row's label paints over it, and render it only while the row
/// is highlighted, so each highlight plays the sequence from the start.
pub fn effort_tiles(id: SharedString, tone: EffortTone, reduced: bool) -> impl IntoElement {
    let selector = id.clone();
    let layer = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .debug_selector(move || selector.to_string());
    if reduced {
        return layer
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| paint_tiles(bounds, tone, None, window),
                )
                .size_full(),
            )
            .into_any_element();
    }
    layer
        .with_animation(
            ElementId::Name(id),
            Animation::new(Duration::from_millis(CYCLE_MS as u64)),
            move |layer, delta| {
                let ms = delta * CYCLE_MS;
                layer.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| paint_tiles(bounds, tone, Some(ms), window),
                    )
                    .size_full(),
                )
            },
        )
        .into_any_element()
}

/// The `::after` glow: an inset glow along the row's edge that pulses three
/// times. Put it last in the row so it paints over the label, as the CSS
/// stacks it. Reduced motion leaves it off.
pub fn effort_glow(
    id: SharedString,
    tone: EffortTone,
    radius: Rems,
    reduced: bool,
) -> impl IntoElement {
    let layer = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .rounded(radius);
    if reduced {
        return layer.into_any_element();
    }
    let color = tone.color();
    layer
        .with_animation(
            ElementId::Name(id),
            Animation::new(Duration::from_millis(CYCLE_MS as u64)),
            move |layer, delta| {
                let glow = edge_glow(delta * CYCLE_MS);
                if glow <= 0. {
                    return layer;
                }
                // Opacity runs 0.3 to 0.85 while the shadow color runs from
                // clear to 55% of the effort color.
                let opacity = 0.3 + 0.55 * glow;
                layer.shadow(vec![BoxShadow {
                    color: with_alpha(color, 0.55 * glow * opacity),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(4. + 8. * glow),
                    spread_radius: px(glow),
                    inset: true,
                }])
            },
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::models::ModelSettingKind;

    fn reasoning() -> ModelSetting {
        ModelSetting {
            id: "reasoningEffort".into(),
            label: "Reasoning".into(),
            kind: ModelSettingKind::Select,
            value: "high".into(),
            options: Vec::new(),
            description: None,
        }
    }

    #[test]
    fn only_codex_max_and_ultra_effort_options_shimmer() {
        let effort = reasoning();
        assert_eq!(
            effort_tile_tone(HarnessId::Codex, &effort, "max"),
            Some(EffortTone::Max)
        );
        assert_eq!(
            effort_tile_tone(HarnessId::Codex, &effort, "Ultra"),
            Some(EffortTone::Ultra)
        );
        assert_eq!(effort_tile_tone(HarnessId::Codex, &effort, "high"), None);
        assert_eq!(effort_tile_tone(HarnessId::Claude, &effort, "max"), None);
        let tier = ModelSetting {
            id: "serviceTier".into(),
            ..reasoning()
        };
        assert_eq!(effort_tile_tone(HarnessId::Codex, &tier, "max"), None);
    }

    #[test]
    fn the_grid_has_160_tiles_and_fills_about_two_thirds() {
        assert_eq!(TILE_COUNT, 160);
        let filled = (0..TILE_COUNT).filter(|&index| tile_filled(index)).count();
        assert!((96..=112).contains(&filled), "{filled}");
    }

    #[test]
    fn tiles_ripple_out_from_the_middle_and_clear_by_the_end() {
        let middle = tile_distance(16, 2);
        let corner = tile_distance(0, 0);
        assert!(middle < 0.1 && corner > 1.3, "{middle} {corner}");
        // A middle tile is lit at the first peak while a corner tile waits.
        let peak = 0.26 * FILL_MS + middle * DELAY_PER_DISTANCE_MS;
        assert!(tile_fill(peak, middle) > 0.99);
        assert_eq!(tile_fill(peak, corner), 0.);
        // Three pulses, then nothing.
        assert!(tile_fill(2. * FILL_MS + 0.26 * FILL_MS, 0.) > 0.99);
        assert_eq!(tile_fill(FILL_MS * 3. + 1., 0.), 0.);
        assert_eq!(tiles_opacity(4000.), 1.);
        assert!((tiles_opacity(4800.) - 0.5).abs() < 1e-4);
        assert_eq!(tiles_opacity(CYCLE_MS), 0.);
    }

    #[test]
    fn the_glow_pulses_three_times_and_ends_off() {
        for peak in [0.19, 0.47, 0.75] {
            assert!((edge_glow(peak * CYCLE_MS) - 1.).abs() < 1e-4);
        }
        for rest in [0., 0.12, 0.3, 0.6, 0.9, 1.] {
            assert_eq!(edge_glow(rest * CYCLE_MS), 0.);
        }
    }

    #[test]
    fn the_mask_feathers_every_side() {
        assert_eq!(feather(0., 16., 200., 32.), 0.);
        assert_eq!(feather(100., 0., 200., 32.), 0.);
        assert_eq!(feather(100., 16., 200., 32.), 1.);
        let corner = feather(14., 5.6, 200., 32.);
        assert!((corner - 0.25).abs() < 1e-4, "{corner}");
    }
}
