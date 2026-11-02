//! The composer's submit button as one morphing control. Empty, it is a
//! small raised square holding its glyph; with something to send it grows
//! into a wider button that names what will happen (Send, Queue, Steer, Save draft),
//! and while a turn runs it settles back into a square with a stop mark.
//! When the action changes, say a held modifier turns Queue into Steer, the
//! glyph and label roll to the new one, as the title tabs roll their pane.
//! The surface is the tab tiles' language: a gradient that is lighter at
//! the top, a bright rim, a hairline edge and a lift beneath it.
use crate::theme::*;
use gpui::{
    AnyElement, Corners, Hsla, MouseButton, Role, SharedString, Window, canvas, div, prelude::*, px,
};
use std::time::{Duration, Instant};

const HEIGHT: f32 = 26.;
/// The same soft corner square or pill: it widens, it never turns round.
const RADIUS: f32 = 7.;
/// The collapsed square, and the glyph's slot inside the pill.
const SQUARE: f32 = 26.;
const GLYPH: f32 = 14.;
const LABEL_GAP: f32 = 6.;
const LABEL_SIZE: f32 = 12.;
const PAD_RIGHT: f32 = 10.;
const MORPH: Duration = Duration::from_millis(320);
const ROLL: Duration = Duration::from_millis(240);

/// What the button offers right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Face {
    pub label: &'static str,
    pub glyph: &'static str,
    /// A stop square instead of the glyph.
    pub stop: bool,
    /// Grown into a pill that shows the label.
    pub expanded: bool,
}

impl Face {
    /// Where the action sits in the roll, so moving to a later one rises
    /// and moving back drops.
    fn rank(&self) -> i32 {
        match (self.stop, self.label) {
            (true, _) => 4,
            (_, "Queue") => 1,
            (_, "Steer") => 2,
            (_, "Save draft") => 3,
            _ => 0,
        }
    }
    fn same_line(&self, other: &Face) -> bool {
        self.label == other.label && self.glyph == other.glyph && self.stop == other.stop
    }
}

/// The button's motion between renders.
#[derive(Default)]
pub(crate) struct Morph {
    shown: Option<Face>,
    width: Option<(f32, f32, Instant)>,
    roll: Option<(Face, f32, Instant)>,
}

/// One render's worth of the morph.
pub(crate) struct Frame {
    width: f32,
    /// How grown the pill is, 0 square to 1 full label.
    open: f32,
    /// The line rolling out, its direction and the eased progress.
    roll: Option<(Face, f32, f32)>,
    pub moving: bool,
}

fn ease(start: Instant, length: Duration, now: Instant) -> f32 {
    let t = now.saturating_duration_since(start).as_secs_f32() / length.as_secs_f32();
    crate::reveal::bezier(t.clamp(0., 1.), 0.22, 1., 0.36, 1.)
}

/// The pill's width for `label`, measured in the window's font.
pub(crate) fn expanded_width(label: &str, window: &Window) -> f32 {
    let style = window.text_style();
    let run = gpui::TextRun {
        len: label.len(),
        font: style.font(),
        color: content(1.),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(label.to_owned()),
        px(LABEL_SIZE),
        &[run],
        None,
    );
    // The glyph's square already holds its own inset and the gap.
    SQUARE + f32::from(line.width) + PAD_RIGHT
}

impl Morph {
    /// Steps toward `face`, whose pill is `target` wide when expanded.
    pub(crate) fn frame(&mut self, face: Face, target: f32, animate: bool) -> Frame {
        let now = Instant::now();
        let to = if face.expanded { target } else { SQUARE };
        let previous = self.shown.replace(face);
        // Width: ease from wherever it is now toward the new size.
        let current = self.width.map_or(to, |(from, goal, start)| {
            from + (goal - from) * ease(start, MORPH, now)
        });
        match self.width {
            Some((_, goal, _)) if (goal - to).abs() < 0.5 => {}
            _ => self.width = Some((if animate { current } else { to }, to, now)),
        }
        // The line rolls when the action changes, not when it only grows.
        if let Some(previous) = previous.filter(|p| animate && !p.same_line(&face)) {
            let dir = if face.rank() >= previous.rank() {
                1.
            } else {
                -1.
            };
            self.roll = Some((previous, dir, now));
        }
        let (from, goal, start) = self.width.unwrap_or((to, to, now));
        let grow = ease(start, MORPH, now);
        let width = from + (goal - from) * grow;
        let roll = self.roll.and_then(|(face, dir, start)| {
            let p = ease(start, ROLL, now);
            (p < 1.).then_some((face, dir, p))
        });
        if roll.is_none() {
            self.roll = None;
        }
        let span = (target - SQUARE).max(1.);
        Frame {
            width,
            open: ((width - SQUARE) / span).clamp(0., 1.),
            roll,
            moving: grow < 1. || roll.is_some(),
        }
    }
}

/// The colours: a fill (accent, or the inverse of the surface), its ink.
fn palette(enabled: bool) -> (Hsla, Hsla) {
    if let Some(pair) = primary_action(enabled) {
        return pair;
    }
    let light = is_light();
    let solid: fn(f32) -> Hsla = if light { black } else { white };
    let ink: fn(f32) -> Hsla = if light { white } else { black };
    (
        solid(if enabled { 1. } else { 0.3 }),
        ink(if enabled { 1. } else { 0.4 }),
    )
}

fn shade(color: Hsla, by: f32) -> Hsla {
    let mut c = color;
    c.l = (c.l + by).clamp(0., 1.);
    c
}

/// The raised surface: a soft drop and a lift below, the fill brighter at
/// the top, a light rim along the top edge and a hairline all round. The
/// glow under an enabled pill grows with it.
fn surface(fill: Hsla, enabled: bool, open: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let radius = px(RADIUS);
            let corners = Corners::all(radius);
            if enabled {
                let glow = fill.opacity(0.35 * open);
                window.paint_drop_shadows(
                    bounds,
                    corners,
                    &[
                        gpui::BoxShadow::new(px(0.), px(1.), black(0.25)).blur_radius(px(1.)),
                        gpui::BoxShadow::new(px(0.), px(3.), glow)
                            .blur_radius(px(10.))
                            .spread_radius(px(-2.)),
                    ],
                );
            }
            let (top, bottom) = if enabled {
                (shade(fill, 0.06), shade(fill, -0.07))
            } else {
                (fill, fill)
            };
            window.paint_quad(
                gpui::fill(
                    bounds,
                    gpui::linear_gradient(
                        180.,
                        gpui::linear_color_stop(top, 0.),
                        gpui::linear_color_stop(bottom, 1.),
                    ),
                )
                .corner_radii(corners),
            );
            if enabled {
                // The rim: light on the top edge, fading out by the middle.
                window.paint_quad(
                    gpui::fill(
                        bounds,
                        gpui::linear_gradient(
                            180.,
                            gpui::linear_color_stop(white(0.35), 0.),
                            gpui::linear_color_stop(white(0.), 0.5),
                        ),
                    )
                    .corner_radii(corners),
                );
            }
            window.paint_quad(
                gpui::fill(bounds, gpui::transparent_black())
                    .corner_radii(corners)
                    .border_widths(px(1.))
                    .border_color(black(if enabled { 0.18 } else { 0.08 })),
            );
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// One line of the button: its glyph (or stop square) and, as the pill
/// opens, its label.
fn line(face: Face, ink: Hsla, open: f32) -> gpui::Div {
    div()
        .h(px(HEIGHT))
        .flex()
        .items_center()
        .pl(px((SQUARE - GLYPH) / 2.))
        .gap(px(LABEL_GAP))
        .child(
            div()
                .size(px(GLYPH))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(if face.stop {
                    div()
                        .size(px(10.))
                        .rounded(px(2.))
                        .bg(ink)
                        .into_any_element()
                } else {
                    icon(face.glyph, GLYPH).text_color(ink).into_any_element()
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_size(px(LABEL_SIZE))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(ink)
                .opacity(open)
                .child(face.label),
        )
}

/// The button for `face` at `frame`.
pub(crate) fn button(
    id: &'static str,
    face: Face,
    frame: &Frame,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    let (fill, ink) = palette(enabled);
    let open = frame.open;
    let lines: Vec<AnyElement> = match frame.roll {
        // The new action rises in as the old one leaves, or drops back.
        Some((old, dir, p)) => vec![
            line(old, ink, open)
                .absolute()
                .left_0()
                .top(px(-dir * p * HEIGHT))
                .opacity(1. - p)
                .into_any_element(),
            line(face, ink, open)
                .absolute()
                .left_0()
                .top(px(dir * (1. - p) * HEIGHT))
                .opacity(p.max(0.2))
                .into_any_element(),
        ],
        None => vec![
            line(face, ink, open)
                .absolute()
                .left_0()
                .top_0()
                .into_any_element(),
        ],
    };
    div()
        .id(id)
        .debug_selector(|| id.into())
        .role(Role::Button)
        .aria_label(face.label)
        .group(id)
        .cursor_pointer()
        .flex_shrink_0()
        .relative()
        .w(px(frame.width))
        .h(px(HEIGHT))
        .child(surface(fill, enabled, open))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .overflow_hidden()
                .children(lines),
        )
        // A brighter face under the pointer.
        .when(enabled, |d| {
            d.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(RADIUS))
                    .group_hover(id, |s| s.bg(white(0.08))),
            )
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
