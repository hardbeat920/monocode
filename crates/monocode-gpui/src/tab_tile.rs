//! The frosted icon tile that stands for a tab or a session: a small raised
//! glass rectangle, a touch wider than tall, holding one mark. A split tab
//! stacks one tile per pane, the focused one in front and the rest tucked
//! behind it, each peeking out a little further right.
use crate::theme::*;
use gpui::{AnyElement, Corners, Div, Hsla, canvas, div, prelude::*, px};

/// What a tile is made of.
#[derive(Clone, Copy)]
pub(crate) enum Face {
    /// Frosted glass over whatever is behind it.
    Glass,
    /// A solid brand colour, top to bottom, under a white mark.
    Fill(Hsla, Hsla),
}

impl Face {
    /// A provider's brand tile, or glass for a multicolour logo.
    pub(crate) fn provider(provider: &str) -> Self {
        provider_tile(provider).map_or(Self::Glass, |(top, bottom)| Self::Fill(top, bottom))
    }
}

pub(crate) struct TileSize {
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

/// Title-bar tabs.
pub(crate) const TAB_TILE: TileSize = TileSize {
    w: 24.,
    h: 20.,
    radius: 5.,
};

/// Between a tab's tile and its title.
pub(crate) const TAB_TILE_GAP: f32 = 8.;
/// Below this width a tab shows only its tile, centred.
pub(crate) const NARROW_TAB: f32 = 40.;

/// How far each tile behind sticks out past the one in front.
const PEEK: f32 = 4.;
/// How much smaller each tile behind is, on each axis.
const SHRINK: f32 = 2.;
/// Tiles drawn for one stack; further panes fold into the count.
pub(crate) const STACK_MAX: usize = 3;
/// The popovers' backdrop-blur-xl, scaled down to the tile.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const BLUR: f32 = 12.;

/// The left inset that matches a tile's gap above and below in a tab plate.
pub(crate) fn tab_tile_inset() -> f32 {
    (TAB_PLATE_HEIGHT - TAB_TILE.h) / 2.
}

/// The whole tile surface, painted in one pass over one rect so every layer
/// lines up: a hairline lift below it, the glass (a tint over a backdrop
/// blur of whatever sits behind, the tiles further back included), a faint
/// sheen and the border. A real drop shadow would sit under the glass and be
/// blurred into it, darkening the middle, so the lift only shows below the
/// bottom edge. Without the blur the tile paints opaque, so marks behind it
/// never bleed through.
fn surface(radius: f32, depth: usize, face: Face, lifted: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let light = is_light();
            let corners = Corners::all(px(radius));
            // A card picked up off the stack casts a deeper, softer shadow.
            if lifted > 0. {
                window.paint_drop_shadows(
                    bounds,
                    corners,
                    &[gpui::BoxShadow::new(
                        px(0.),
                        px(1. + 4. * lifted),
                        black((if light { 0.18 } else { 0.45 }) * lifted),
                    )
                    .blur_radius(px(2. + 6. * lifted))],
                );
            }
            if depth == 0 {
                let mut lift = bounds;
                lift.origin.y += px(1.);
                window.paint_quad(
                    gpui::fill(lift, black(if light { 0.1 } else { 0.3 })).corner_radii(corners),
                );
            }
            match face {
                Face::Fill(top, bottom) => window.paint_quad(
                    gpui::fill(
                        bounds,
                        gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(top, 0.),
                            gpui::linear_color_stop(bottom, 1.),
                        ),
                    )
                    .corner_radii(corners),
                ),
                Face::Glass => paint_glass(bounds, corners, depth, window),
            }
            // Brighter at the top, like light on glass.
            window.paint_quad(
                gpui::fill(
                    bounds,
                    gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(
                            white(match face {
                                Face::Fill(..) => 0.16,
                                Face::Glass if light => 0.35,
                                Face::Glass => 0.05,
                            }),
                            0.,
                        ),
                        gpui::linear_color_stop(white(0.), 1.),
                    ),
                )
                .corner_radii(corners),
            );
            window.paint_quad(
                gpui::fill(bounds, gpui::transparent_black())
                    .corner_radii(corners)
                    .border_widths(px(1.))
                    .border_color(match face {
                        // A light rim, as on an app icon.
                        Face::Fill(..) => white(0.14),
                        Face::Glass => content(if depth == 0 { 0.1 } else { 0.06 }),
                    }),
            );
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A tint over a backdrop blur, or an opaque card where there is no blur.
fn paint_glass(
    bounds: gpui::Bounds<gpui::Pixels>,
    corners: Corners<gpui::Pixels>,
    depth: usize,
    window: &mut gpui::Window,
) {
    let light = is_light();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let tint = if light {
            white(if depth == 0 { 0.72 } else { 0.6 })
        } else {
            content(if depth == 0 { 0.12 } else { 0.08 })
        };
        window.paint_quad(
            gpui::fill(bounds, tint)
                .corner_radii(corners)
                .border_color(gpui::backdrop_blur_marker(BLUR * window.scale_factor())),
        );
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (light, depth);
        window.paint_quad(gpui::fill(bounds, floating_fill()).corner_radii(corners));
    }
}

/// One tile at `depth` in a stack (0 is the front); a shuffle passes
/// depths in between. Each step back is smaller; the front tile alone
/// carries the lift and the brighter glass.
fn tile(size: &TileSize, depth: f32, face: Face, mark: AnyElement) -> Div {
    tile_lifted(size, depth, face, mark, 0.)
}

/// [`tile`] partway off the stack: `lifted` 0 resting to 1 held up high.
fn tile_lifted(size: &TileSize, depth: f32, face: Face, mark: AnyElement, lifted: f32) -> Div {
    let shrink = depth * SHRINK;
    div()
        .w(px(size.w - shrink))
        .h(px(size.h - shrink))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .child(surface(
            size.radius,
            depth.max(0.).round() as usize,
            face,
            lifted,
        ))
        .child(mark)
}

/// A single mark on a tile.
pub(crate) fn mark_tile(size: &TileSize, face: Face, mark: impl IntoElement) -> Div {
    tile(size, 0., face, mark.into_any_element()).flex_shrink_0()
}

/// One pane's tile in a stack, and where it sits before and after a
/// focus change: 0 is the front, and [`STACK_MAX`] or more is out of sight.
pub(crate) struct StackTile {
    pub face: Face,
    pub mark: AnyElement,
    pub from: usize,
    pub to: usize,
}

/// A split tab's tiles, shuffled like a deck: `t` (linear, 0 to 1) of
/// the way from each tile's old depth to its new one. The card leaving the
/// front is picked up, lifted and swung out to the right, then dropped
/// behind the stack at the top of its arc; the new card slides forward and
/// grows into the front as the old one passes over it. Settled, `from`
/// equals `to`.
pub(crate) fn tile_stack(size: &TileSize, tiles: Vec<StackTile>, t: f32) -> Div {
    /// How high the picked-up card rises, and how far it swings out.
    const LIFT: f32 = 9.;
    const SWING: f32 = 12.;
    let t = t.clamp(0., 1.);
    // Ease in and out, so cards gather speed and settle.
    let travel = t * t * (3. - 2. * t);
    // The pick-up arc: up and out, peaking halfway, then down into place.
    let arc = (t * std::f32::consts::PI).sin();
    let last = (STACK_MAX - 1) as f32;
    let shown = tiles
        .iter()
        .filter(|t| t.to < STACK_MAX)
        .count()
        .clamp(1, STACK_MAX);
    // (z, depth, tile): drawn from the highest z down.
    let mut placed: Vec<(f32, f32, Div)> = tiles
        .into_iter()
        .filter(|t| t.from < STACK_MAX || t.to < STACK_MAX)
        .map(|tile| {
            let (from, to) = (tile.from as f32, tile.to as f32);
            let depth = from + (to - from) * travel;
            // A card out of sight waits behind the last one and fades.
            let opacity = (1. - (depth - last)).clamp(0., 1.);
            let depth = depth.min(last);
            let (mut dx, mut dy, mut lifted, mut grow) = (0., 0., 0., 0.);
            // Over the stack until halfway, then under it.
            let mut z = if t < 0.5 { from } else { to };
            if to > from && from == 0. {
                // The card being put back: picked up and carried over.
                dx = arc * SWING;
                dy = -arc * LIFT;
                lifted = arc;
                grow = arc * 0.8;
                if t < 0.5 {
                    z = -1.;
                }
            } else if to < from {
                // The card being dealt: drawn out from behind and forward.
                dy = arc * 1.5;
            } else {
                z = depth;
            }
            let shrink = (depth - grow) * SHRINK;
            let element = tile_lifted(size, depth - grow, tile.face, tile.mark, lifted)
                .absolute()
                .left(px(depth * PEEK + depth * SHRINK + dx - grow * SHRINK / 2.))
                .top(px(shrink / 2. + dy))
                .when(opacity < 1., |d| d.opacity(opacity));
            (opacity > 0.).then_some((z, depth, element))
        })
        // A faded tile still frosts what is behind it, so it isn't drawn.
        .flatten()
        .collect();
    placed.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.total_cmp(&a.1)));
    div()
        .relative()
        .flex_shrink_0()
        .w(px(size.w + (shown - 1) as f32 * PEEK))
        .h(px(size.h))
        .children(placed.into_iter().map(|(_, _, tile)| tile))
}

/// A tab's icon on glass: app views, Search, Monos, files and diffs.
pub(crate) fn glass_tile(mark: impl IntoElement) -> Div {
    mark_tile(&TAB_TILE, Face::Glass, mark)
}

/// A file tab's tile: its type icon on glass, or the compare mark for a diff.
pub(crate) fn file_tile(name: Option<&str>) -> Div {
    glass_tile(match name {
        Some(name) => crate::file_icons::file_type_icon(name, false, false, 14.),
        None => icon("git-compare", 14.)
            .text_color(content(0.85))
            .into_any_element(),
    })
}

/// A terminal tab's tile: the agent CLI running in it on its brand colour,
/// or a white prompt on a dark tile, like a terminal app's icon.
pub(crate) fn terminal_tile(agent: Option<&str>) -> Div {
    match agent {
        Some(provider) => mark_tile(
            &TAB_TILE,
            Face::provider(provider),
            provider_glyph(provider, 14.),
        ),
        None => mark_tile(
            &TAB_TILE,
            Face::Fill(gpui::rgb(0x3A3A3C).into(), gpui::rgb(0x1C1C1E).into()),
            gpui::svg()
                .path("prompt.svg")
                .size(px(13.))
                .text_color(gpui::white()),
        ),
    }
}

/// A work phase in the transcript ("Ran 5 commands"), sized to its 20px line.
pub(crate) const ACTIVITY_TILE: TileSize = TileSize {
    w: 22.,
    h: 18.,
    radius: 5.,
};

/// A transcript work phase's tile, coloured by the kind of work like an
/// app icon: commands on a dark terminal tile, edits amber, reading and
/// searching blue, sub-agents violet, the app's own visuals on glass.
/// A work phase's tile face, its mark and the ink that suits other marks
/// on it: white on a coloured tile, the theme's ink on glass.
pub(crate) fn activity_face(glyph: &'static str) -> (Face, AnyElement, Hsla) {
    let fill = |top: u32, bottom: u32| Face::Fill(gpui::rgb(top).into(), gpui::rgb(bottom).into());
    let white = |name: &'static str, side: f32| {
        gpui::svg()
            .path(name)
            .size(px(side))
            .text_color(gpui::white())
            .into_any_element()
    };
    let ink: Hsla = gpui::white();
    match glyph {
        "terminal" => (fill(0x3A3A3C, 0x1C1C1E), white("prompt.svg", 12.), ink),
        "pencil-line" => (fill(0xF6B44B, 0xE08A12), white("pencil-line", 12.), ink),
        "search" => (fill(0x6AAEFF, 0x2F6FE4), white("search", 12.), ink),
        "bot" => (fill(0xA98BF5, 0x7246E0), white("bot", 12.), ink),
        other => (
            Face::Glass,
            crate::activity::glyph(other, 12.).into_any_element(),
            content(0.9),
        ),
    }
}

/// A tile whose mark is being pushed out to the right by `incoming` sliding
/// in from the left, `t` (eased, 0 to 1) of the way. The tile itself holds
/// still; only what sits on it moves.
pub(crate) fn push_tile(
    size: &TileSize,
    face: Face,
    outgoing: AnyElement,
    incoming: AnyElement,
    t: f32,
) -> Div {
    let slot = |mark: AnyElement, x: f32| {
        div()
            .absolute()
            .top_0()
            .left(px(x))
            .w(px(size.w))
            .h(px(size.h))
            .flex()
            .items_center()
            .justify_center()
            .child(mark)
    };
    let lane = div()
        .w(px(size.w))
        .h(px(size.h))
        .relative()
        .overflow_hidden()
        .when(t < 1., |d| d.child(slot(outgoing, t * size.w)))
        .when(t > 0., |d| d.child(slot(incoming, (t - 1.) * size.w)));
    mark_tile(size, face, lane)
}
