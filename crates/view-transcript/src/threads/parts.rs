//! Small pieces the thread views share.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, ElementId, Hsla, IntoElement as _,
    ParentElement as _, Pixels, SharedString, Styled as _, Window, canvas, div, fill, point, px,
    size,
};
use monocode_core::HarnessId;
use monocode_ui::color::{hsl_to_rgb, parse_hex};
use monocode_ui::{ProviderLogo, provider_logo};
use monocode_view_composer::composer::model::mascots::{self, MASCOT_GRID};

/// `HarnessIcon` at `size` CSS px.
pub fn harness_icon(harness: HarnessId, size: f32) -> AnyElement {
    match ProviderLogo::from_id(harness.as_str()) {
        Some(logo) => provider_logo(logo).size(size).into_any_element(),
        None => gpui::div().into_any_element(),
    }
}

/// `ProjectMascot`: the 8 by 8 pixel mascot in `color`. An active mascot
/// swaps to its talk frame and hops a pixel every other half beat
/// (`.mascot-active`, 460ms).
pub fn project_mascot(
    id: impl Into<ElementId>,
    project: &str,
    name: Option<&str>,
    color: Hsla,
    active: bool,
) -> AnyElement {
    let mascot = mascots::project_mascot(project, name);
    let sprite = move |talk: bool| {
        let rows = if talk { mascot.talk } else { mascot.rest };
        canvas(
            |_, _, _| {},
            move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
                let cell = bounds.size.width / MASCOT_GRID as f32;
                let lift = if talk { cell.min(px(1.)) } else { px(0.) };
                for (x, y, run) in mascots::mascot_rects(&rows) {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(
                                bounds.origin.x + cell * x as f32,
                                bounds.origin.y + cell * y as f32 - lift,
                            ),
                            size(cell * run as f32, cell),
                        ),
                        color,
                    ));
                }
            },
        )
        .size_full()
    };
    if !active {
        return div().size_full().child(sprite(false)).into_any_element();
    }
    div()
        .size_full()
        .with_animation(
            id,
            Animation::new(Duration::from_millis(460)).repeat(),
            move |el, t| el.child(sprite(t >= 0.5)),
        )
        .into_any_element()
}

/// Where a popover sits against its trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Above, centered (`side="top" align="center"`).
    TopCenter,
    /// Below, centered: a flipped `TopCenter`.
    BottomCenter,
    /// Above, left edges aligned (`side="top" align="start"`).
    TopStart,
    /// Below, left edges aligned (`side="bottom" align="start"`).
    BottomStart,
    /// To the right, top edges aligned (a flyout).
    RightStart,
    /// To the left, top edges aligned: a flipped flyout.
    LeftStart,
}

impl Placement {
    fn opposite(self) -> Self {
        match self {
            Self::TopCenter => Self::BottomCenter,
            Self::BottomCenter => Self::TopCenter,
            Self::TopStart => Self::BottomStart,
            Self::BottomStart => Self::TopStart,
            Self::RightStart => Self::LeftStart,
            Self::LeftStart => Self::RightStart,
        }
    }

    /// Room between the anchor and the window edge on this side.
    fn room(self, anchor: Bounds<Pixels>, viewport: gpui::Size<Pixels>, gap: f32) -> f32 {
        const PADDING: f32 = monocode_ui::widgets::POPOVER_PADDING;
        let (left, top) = (f32::from(anchor.origin.x), f32::from(anchor.origin.y));
        let (right, bottom) = (
            left + f32::from(anchor.size.width),
            top + f32::from(anchor.size.height),
        );
        match self {
            Self::TopCenter | Self::TopStart => top - gap - PADDING,
            Self::BottomCenter | Self::BottomStart => {
                f32::from(viewport.height) - bottom - gap - PADDING
            }
            Self::LeftStart => left - gap - PADDING,
            Self::RightStart => f32::from(viewport.width) - right - gap - PADDING,
        }
    }

    fn vertical(self) -> bool {
        !matches!(self, Self::RightStart | Self::LeftStart)
    }
}

/// `placePopover`'s side choice: keep `preferred` unless its room cannot
/// hold the popover and the other side has more. `anchor` is the trigger's
/// last painted bounds and `size` the popover's, in window px; without them
/// the preferred side stands.
pub fn flip(
    preferred: Placement,
    anchor: Option<Bounds<Pixels>>,
    size: gpui::Size<Pixels>,
    gap: f32,
    window: &Window,
) -> Placement {
    let Some(anchor) = anchor else {
        return preferred;
    };
    let gap = f32::from(monocode_ui::u(gap).to_pixels(window.rem_size()));
    let viewport = window.viewport_size();
    let needed = f32::from(if preferred.vertical() {
        size.height
    } else {
        size.width
    });
    let room = preferred.room(anchor, viewport, gap);
    let other = preferred.opposite().room(anchor, viewport, gap);
    if room >= needed || other <= room {
        preferred
    } else {
        preferred.opposite()
    }
}

/// Places `content` against the edge of its parent, which must be
/// `relative()`, in the popover layer `layer`. `gap` is in CSS px; a
/// negative gap overlaps, as flyouts do. It snaps to the window edge.
pub fn anchored_popover(
    placement: Placement,
    gap: f32,
    layer: usize,
    window: &Window,
    content: impl gpui::IntoElement,
) -> AnyElement {
    use gpui::{Anchor, anchored, deferred, relative};
    let gap = monocode_ui::u(gap).to_pixels(window.rem_size());
    let (corner, offset) = match placement {
        Placement::TopCenter => (Anchor::BottomCenter, point(px(0.), -gap)),
        Placement::BottomCenter => (Anchor::TopCenter, point(px(0.), gap)),
        Placement::TopStart => (Anchor::BottomLeft, point(px(0.), -gap)),
        Placement::BottomStart => (Anchor::TopLeft, point(px(0.), gap)),
        Placement::RightStart => (Anchor::TopLeft, point(gap, px(0.))),
        Placement::LeftStart => (Anchor::TopRight, point(-gap, px(0.))),
    };
    let slot = div().absolute().size_0();
    let slot = match placement {
        Placement::TopCenter => slot.top_0().left(relative(0.5)),
        Placement::BottomCenter => slot.top(relative(1.)).left(relative(0.5)),
        Placement::TopStart | Placement::LeftStart => slot.top_0().left_0(),
        Placement::BottomStart => slot.top(relative(1.)).left_0(),
        Placement::RightStart => slot.top_0().left(relative(1.)),
    };
    slot.child(
        deferred(
            anchored()
                .anchor(corner)
                .offset(offset)
                .snap_to_window_with_margin(px(monocode_ui::widgets::POPOVER_PADDING))
                .child(content),
        )
        .with_priority(layer),
    )
    .into_any_element()
}

/// A `box-shadow` glow drawn only outside `bounds`: `solid` px at full
/// `color`, then a falloff over `blur` px. GPUI's shadow primitive also
/// fills the box itself, which CSS never does, so the glow is stacked
/// rings instead.
pub fn paint_outer_glow(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    radius: Pixels,
    color: Hsla,
    solid: f32,
    blur: f32,
) {
    use gpui::{BorderStyle, Corners, Edges, quad};
    const STEP: f32 = 2.;
    let mut paint = |inset: f32, width: f32, alpha: f32| {
        if alpha <= 0.003 {
            return;
        }
        let ring = bounds.dilate(px(inset + width));
        window.paint_quad(quad(
            ring,
            Corners::all(radius + px(inset + width)),
            gpui::transparent_black(),
            Edges::all(px(width)),
            monocode_ui::color::with_alpha(color, alpha),
            BorderStyle::Solid,
        ));
    };
    if solid > 0. {
        paint(0., solid, 1.);
    }
    let rings = (blur / STEP).ceil() as usize;
    for ring in 0..rings {
        let t = (ring as f32 + 0.5) / rings as f32;
        // A gaussian-like falloff from the solid edge outward.
        let alpha = (1. - t).powi(2) * 0.6;
        paint(solid + ring as f32 * STEP, STEP, alpha);
    }
}

/// Parses the CSS colors tab groups store: `#rrggbb` and `hsl(h s% l%)`.
pub fn css_color(value: &str) -> Option<Hsla> {
    let value = value.trim();
    if value.starts_with('#') {
        return parse_hex(value);
    }
    let inner = value.strip_prefix("hsl(")?.strip_suffix(')')?;
    let parts: Vec<f64> = inner
        .split([' ', ','])
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_end_matches('%').parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    let [hue, saturation, lightness] = parts[..] else {
        return None;
    };
    Some(hsl_to_rgb(hue, saturation, lightness).to_hsla())
}

/// `"N file"` or `"N files"`, or `None` for zero or no count.
pub fn files_label(files: Option<i64>) -> Option<String> {
    let files = files.filter(|files| *files > 0)?;
    Some(format!(
        "{files} {}",
        if files == 1 { "file" } else { "files" }
    ))
}

/// An element id scoped to a key.
pub fn eid(key: &str, part: &str) -> ElementId {
    ElementId::Name(SharedString::from(format!("{key}:{part}")))
}

/// Epoch ms now, `Date.now()`.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// `Math.random()`: a small splitmix64 stream, seeded from the clock unless a
/// test fixes the seed.
#[derive(Clone, Debug)]
pub struct Random(u64);

impl Random {
    pub fn seeded(seed: u64) -> Self {
        Self(seed)
    }

    pub fn from_clock() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0);
        Self(nanos ^ COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed))
    }

    /// A value in `0.0..1.0`.
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_files() {
        assert_eq!(files_label(None), None);
        assert_eq!(files_label(Some(0)), None);
        assert_eq!(files_label(Some(1)).as_deref(), Some("1 file"));
        assert_eq!(files_label(Some(3)).as_deref(), Some("3 files"));
    }

    #[test]
    fn parses_tab_group_colors() {
        let gray = css_color("hsl(210 8% 58%)").unwrap();
        assert!((gray.l - 0.58).abs() < 0.01);
        let custom = css_color("#ff0000").unwrap();
        assert!(custom.h.abs() < 0.01 && (custom.s - 1.).abs() < 0.01);
        assert!(css_color("blue").is_none());
    }

    #[test]
    fn random_stays_in_the_unit_range() {
        let mut random = Random::seeded(7);
        for _ in 0..1000 {
            let value = random.next();
            assert!((0.0..1.0).contains(&value));
        }
    }
}
