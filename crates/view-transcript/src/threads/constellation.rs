//! Port of src/features/sessions/ui/OrchestratorConstellation.tsx and the
//! `shouldCelebrateTurn` half of turnCelebration.ts: the one-shot burst
//! inside a freshly sent Orchestrator turn.
//!
//! The lead lights up at the base of the bubble, edges race out to a few
//! agents that pop in and hand off again, and then the whole network rises
//! past the bubble's top edge. The SVG lines and CSS keyframes of
//! index.css (`.orchestrator-*`, `turn-mode-*`) become one canvas that
//! paints the frame for the elapsed time.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, BorderStyle, Bounds, Context, Corners, Edges, Global, Hsla, IntoElement,
    ParentElement as _, PathBuilder, Pixels, Point, Render, Styled as _, Task, WeakEntity, Window,
    canvas, div, fill, linear_color_stop, linear_gradient, point, px, quad, size,
};
use monocode_ui::Theme;
use monocode_ui::color::with_alpha;
use monocode_ui::theme::CubicBezier;

use super::parts::{Random, now_ms, paint_outer_glow};
use super::style::{ModeColors, orchestrator_colors};

/// The hub must make it beyond the bubble's top edge before cleanup.
pub const CELEBRATE_MS: i64 = 3100;
const FIRST_EDGE_MS: f32 = 280.;
const EDGE_GAP_MS: f32 = 80.;
const EDGE_DRAW_MS: f32 = 320.;
const CHILD_DRAW_MS: f32 = 220.;
const INSET: f32 = 8.;
/// `FRESH_MS` in turnCelebration.ts: only a turn sent moments ago celebrates.
const FRESH_MS: i64 = 4000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

/// `ConstellationNode`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstellationNode {
    pub x: f32,
    pub y: f32,
    /// Where its edge starts: the hub, or the node it branched from.
    pub from: Pt,
    /// When its edge starts drawing, in ms.
    pub delay: f32,
    pub draw: f32,
    pub child: bool,
}

/// `Constellation`.
#[derive(Clone, Debug, PartialEq)]
pub struct Constellation {
    pub hub: Pt,
    pub nodes: Vec<ConstellationNode>,
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    max.min(min.max(value))
}

/// `makeConstellation`: a lead at the bottom center fans work out to a few
/// agents across the bubble, and a couple of them hand off again.
/// Everything stays inside the bubble so its clipping never cuts a node in
/// half.
pub fn make_constellation(
    width: f32,
    height: f32,
    random: &mut dyn FnMut() -> f32,
) -> Constellation {
    let max_x = INSET.max(width - INSET);
    let max_y = INSET.max(height - INSET);
    let hub = Pt {
        x: width / 2.,
        y: max_y,
    };
    let count = clamp((width / 90.).round(), 3., 6.) as usize;
    // Short single-line bubbles still leave the agents a little above the lead.
    let top = max_y.min(INSET.max(height * 0.35));
    let bottom = top.max((max_y - 10.).min(height * 0.7));
    let mut agents: Vec<ConstellationNode> = (0..count)
        .map(|i| {
            let x = clamp(
                ((i as f32 + 0.2 + random() * 0.6) / count as f32) * width,
                INSET,
                max_x,
            );
            let y = top + random() * (bottom - top);
            ConstellationNode {
                x,
                y,
                from: hub,
                delay: 0.,
                draw: EDGE_DRAW_MS,
                child: false,
            }
        })
        .collect();
    // Fan out from the center so the lead reads as reaching both ways at once.
    agents.sort_by(|a, b| {
        (a.x - hub.x)
            .abs()
            .partial_cmp(&(b.x - hub.x).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (i, agent) in agents.iter_mut().enumerate() {
        agent.delay = FIRST_EDGE_MS + i as f32 * EDGE_GAP_MS;
    }
    let children: Vec<ConstellationNode> = agents
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, parent)| {
            let direction = if parent.x < hub.x { -1. } else { 1. };
            ConstellationNode {
                x: clamp(parent.x + direction * (22. + random() * 18.), INSET, max_x),
                y: clamp(parent.y + (random() - 0.5) * 12., top, max_y),
                from: Pt {
                    x: parent.x,
                    y: parent.y,
                },
                delay: parent.delay + parent.draw + 60.,
                draw: CHILD_DRAW_MS,
                child: true,
            }
        })
        .collect();
    agents.extend(children);
    Constellation { hub, nodes: agents }
}

/// The block ids that already celebrated, so remounts (tab switches,
/// transcript windowing) never replay a burst.
#[derive(Default)]
struct CelebratedTurns(HashSet<String>);

impl Global for CelebratedTurns {}

/// `shouldCelebrateTurn` without the module set: a turn sent less than four
/// seconds ago.
pub fn is_fresh_turn(started_at: Option<i64>, now: i64) -> bool {
    let Some(started_at) = started_at else {
        return false;
    };
    let age = now - started_at;
    (0..FRESH_MS).contains(&age)
}

/// `shouldCelebrateTurn`.
pub fn should_celebrate_turn(block_id: &str, started_at: Option<i64>, now: i64, cx: &App) -> bool {
    let celebrated = cx
        .try_global::<CelebratedTurns>()
        .is_some_and(|set| set.0.contains(block_id));
    !celebrated && is_fresh_turn(started_at, now)
}

/// The constellation laid out for one bubble size.
type Layout = Rc<RefCell<Option<(gpui::Size<Pixels>, Constellation)>>>;

/// The `OrchestratorConstellation` element, as an entity that owns its
/// clock. Put it inside the user bubble, which must be `relative()`.
pub struct OrchestratorConstellation {
    active: bool,
    started: Instant,
    /// Draws one fixed moment instead of the running clock, for screenshots.
    frozen_ms: Option<f32>,
    reduced_motion: bool,
    layout: Layout,
    seed: Option<u64>,
    _done: Option<Task<()>>,
}

impl OrchestratorConstellation {
    /// `useTurnCelebration(blockId, startedAt, CELEBRATE_MS)`: active only for
    /// a turn sent moments ago that has not celebrated yet.
    pub fn new(block_id: &str, started_at: Option<i64>, cx: &mut Context<Self>) -> Self {
        let fresh = should_celebrate_turn(block_id, started_at, now_ms(), cx);
        let mut done = None;
        if fresh {
            cx.default_global::<CelebratedTurns>()
                .0
                .insert(block_id.to_string());
            done = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(CELEBRATE_MS as u64))
                    .await;
                this.update(cx, |this, cx| {
                    this.active = false;
                    cx.notify();
                })
                .ok();
            }));
        }
        Self {
            active: fresh,
            started: Instant::now(),
            frozen_ms: None,
            reduced_motion: false,
            layout: Rc::default(),
            seed: None,
            _done: done,
        }
    }

    /// A constellation frozen at `ms` into its timeline, for the gallery.
    pub fn frozen(ms: f32, seed: u64) -> Self {
        Self {
            active: true,
            started: Instant::now(),
            frozen_ms: Some(ms),
            reduced_motion: false,
            layout: Rc::default(),
            seed: Some(seed),
            _done: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// `prefers-reduced-motion` hides the burst.
    pub fn set_reduced_motion(&mut self, reduced: bool, cx: &mut Context<Self>) {
        self.reduced_motion = reduced;
        cx.notify();
    }
}

impl Render for OrchestratorConstellation {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.active || self.reduced_motion {
            return div().into_any_element();
        }
        let ms = self
            .frozen_ms
            .unwrap_or_else(|| self.started.elapsed().as_secs_f32() * 1000.);
        if self.frozen_ms.is_none() {
            window.request_animation_frame();
        }
        let theme = Theme::of(cx);
        let colors = orchestrator_colors(theme);
        let layout = self.layout.clone();
        let seed = self.seed;
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let mut slot = layout.borrow_mut();
                        let stale = slot.as_ref().is_none_or(|(size, _)| *size != bounds.size);
                        if stale {
                            let mut random = seed.map_or_else(Random::from_clock, Random::seeded);
                            let constellation = make_constellation(
                                f32::from(bounds.size.width),
                                f32::from(bounds.size.height),
                                &mut || random.next(),
                            );
                            *slot = Some((bounds.size, constellation));
                        }
                        if let Some((_, constellation)) = slot.as_ref() {
                            paint_frame(window, bounds, constellation, colors, ms);
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }
}

/// Animation progress for a keyframe run: `0..1` once `delay` has passed.
fn progress(ms: f32, delay: f32, duration: f32) -> f32 {
    ((ms - delay) / duration).clamp(0., 1.)
}

/// Linear interpolation over keyframes `(offset, value)`.
fn keyframes(frames: &[(f32, f32)], t: f32) -> f32 {
    let mut previous = frames[0];
    for &frame in frames {
        if t <= frame.0 {
            let span = frame.0 - previous.0;
            if span <= 0. {
                return frame.1;
            }
            return previous.1 + (frame.1 - previous.1) * (t - previous.0) / span;
        }
        previous = frame;
    }
    previous.1
}

const EASE_OUT: CubicBezier = CubicBezier(0., 0., 0.58, 1.);
const RISE: CubicBezier = CubicBezier(0.3, 0.6, 0.4, 1.);
const SHEEN: CubicBezier = CubicBezier(0.4, 0., 0.2, 1.);

fn alpha(color: Hsla, factor: f32) -> Hsla {
    with_alpha(color, factor)
}

/// Paints the frame `ms` into the timeline over `bounds`.
fn paint_frame(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    constellation: &Constellation,
    colors: ModeColors,
    ms: f32,
) {
    if ms >= CELEBRATE_MS as f32 {
        return;
    }
    let origin = bounds.origin;
    let at = |p: Pt, lift: f32| point(origin.x + px(p.x), origin.y + px(p.y - lift));

    // `turn-mode-halo`: a ring and glow around the bubble that peak at 25%.
    let halo = EASE_OUT.ease(progress(ms, 150., 2400.));
    let halo_strength = keyframes(&[(0., 0.), (0.25, 1.), (1., 0.)], halo);
    if halo_strength > 0. {
        // `0 0 0 1px glow/60%` and `0 0 24px 3px glow/38%`, outside the bubble.
        let glow = colors.glow;
        paint_outer_glow(
            window,
            bounds,
            px(12.),
            alpha(glow, 0.38 * halo_strength),
            3.,
            20.,
        );
        paint_outer_glow(
            window,
            bounds,
            px(12.),
            alpha(glow, 0.6 * halo_strength),
            1.,
            0.,
        );
    }

    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        // `::after`: the ember glow rising from the base, 0 to 1 at 30%.
        let glow = keyframes(
            &[(0., 0.), (0.3, 1.), (1., 0.)],
            EASE_OUT.ease(progress(ms, 100., 2200.)),
        );
        if glow > 0. {
            window.paint_quad(fill(
                bounds,
                linear_gradient(
                    0.,
                    linear_color_stop(alpha(colors.glow, 0.35 * glow), 0.),
                    linear_color_stop(alpha(colors.glow, 0.), 0.7),
                ),
            ));
        }

        // `::before`: the sheen sweeping left to right.
        let sheen = SHEEN.ease(progress(ms, 300., 1100.));
        if (300.0..1400.0).contains(&ms) {
            let width = bounds.size.width;
            let shift = width * (-1.2 + 2.4 * sheen);
            let band = |from: f32, to: f32| {
                Bounds::new(
                    point(origin.x + shift + width * from, origin.y),
                    size(width * (to - from), bounds.size.height),
                )
            };
            window.paint_quad(fill(
                band(0.3, 0.5),
                linear_gradient(
                    90.,
                    linear_color_stop(alpha(colors.color, 0.), 0.),
                    linear_color_stop(alpha(colors.color, 0.32), 1.),
                ),
            ));
            window.paint_quad(fill(
                band(0.5, 0.7),
                linear_gradient(
                    90.,
                    linear_color_stop(alpha(colors.glow, 0.28), 0.),
                    linear_color_stop(alpha(colors.glow, 0.), 1.),
                ),
            ));
        }

        // `.orchestrator-network`: everything below rises out of the
        // bubble, fading over its last 15%.
        let rise_t = RISE.ease(progress(ms, 300., 2200.));
        let lift = (f32::from(bounds.size.height) + 12.) * rise_t;
        let network_opacity = keyframes(&[(0., 1.), (0.85, 1.), (1., 0.)], rise_t);
        if network_opacity <= 0. {
            return;
        }

        for node in &constellation.nodes {
            paint_edge(window, node, colors, ms, lift, network_opacity, &at);
        }
        paint_dot(
            window,
            at(constellation.hub, lift),
            8.,
            colors,
            ms,
            120.,
            (0.3, 1.35),
            (220., 700.),
            network_opacity,
        );
        for node in &constellation.nodes {
            let start = node.delay + node.draw - 40.;
            paint_dot(
                window,
                at(
                    Pt {
                        x: node.x,
                        y: node.y,
                    },
                    lift,
                ),
                if node.child { 4.5 } else { 6. },
                colors,
                ms,
                start,
                (0.2, 1.4),
                (start, 500.),
                network_opacity,
            );
        }
    });
}

/// `.orchestrator-edge` and `.orchestrator-pulse` for one node.
fn paint_edge(
    window: &mut Window,
    node: &ConstellationNode,
    colors: ModeColors,
    ms: f32,
    lift: f32,
    opacity: f32,
    at: &dyn Fn(Pt, f32) -> Point<Pixels>,
) {
    let t = progress(ms, node.delay, node.draw);
    if t <= 0. {
        return;
    }
    let drawn = RISE.ease(t);
    let from = node.from;
    let to = Pt {
        x: node.x,
        y: node.y,
    };
    let length = ((to.x - from.x).powi(2) + (to.y - from.y).powi(2)).sqrt();
    if length <= 0. {
        return;
    }
    let along = |distance: f32| {
        let k = (distance / length).clamp(0., 1.);
        Pt {
            x: from.x + (to.x - from.x) * k,
            y: from.y + (to.y - from.y) * k,
        }
    };
    let mut edge = PathBuilder::stroke(px(1.25));
    edge.move_to(at(from, lift));
    edge.line_to(at(along(length * drawn), lift));
    if let Ok(path) = edge.build() {
        window.paint_path(path, alpha(colors.color, 0.6 * opacity));
    }
    // The bright head rides the tip while the edge draws, then fades.
    let pulse_opacity = keyframes(&[(0., 1.), (0.85, 1.), (1., 0.)], drawn) * opacity;
    if pulse_opacity <= 0. {
        return;
    }
    let start = -6. + drawn * (length + 6.);
    let (a, b) = (start.max(0.), (start + 6.).min(length));
    if b <= a {
        return;
    }
    let mut glow = PathBuilder::stroke(px(5.));
    glow.move_to(at(along(a), lift));
    glow.line_to(at(along(b), lift));
    if let Ok(path) = glow.build() {
        window.paint_path(path, alpha(colors.glow, 0.35 * pulse_opacity));
    }
    let mut pulse = PathBuilder::stroke(px(2.5));
    pulse.move_to(at(along(a), lift));
    pulse.line_to(at(along(b), lift));
    if let Ok(path) = pulse.build() {
        window.paint_path(path, alpha(colors.spark, pulse_opacity));
    }
}

/// `.orchestrator-hub` and `.orchestrator-node`: a glowing dot that pops in
/// (`scale` from `pop.0` past `pop.1` back to 1) with a ring spreading out.
#[allow(clippy::too_many_arguments)]
fn paint_dot(
    window: &mut Window,
    center: Point<Pixels>,
    dot: f32,
    colors: ModeColors,
    ms: f32,
    delay: f32,
    pop: (f32, f32),
    ring: (f32, f32),
    opacity: f32,
) {
    let t = EASE_OUT.ease(progress(ms, delay, 600.));
    if ms < delay {
        return;
    }
    let shown = keyframes(&[(0., 0.), (0.3, 1.), (1., 1.)], t) * opacity;
    let scale = keyframes(&[(0., pop.0), (0.3, pop.1), (1., 1.)], t);
    let radius = dot * scale / 2.;
    let circle = |r: f32| {
        Bounds::new(
            point(center.x - px(r), center.y - px(r)),
            size(px(r * 2.), px(r * 2.)),
        )
    };
    if shown > 0. {
        window.paint_quad(
            fill(circle(radius + 4.), alpha(colors.glow, 0.25 * shown))
                .corner_radii(px(radius + 4.)),
        );
        window
            .paint_quad(fill(circle(radius), alpha(colors.color, shown)).corner_radii(px(radius)));
        window.paint_quad(
            fill(circle(radius * 0.55), alpha(colors.spark, shown)).corner_radii(px(radius * 0.55)),
        );
    }
    // `orchestrator-ring`: 0.8 to 0 opacity while it grows to 3.5 times.
    let ring_t = EASE_OUT.ease(progress(ms, ring.0, ring.1));
    if ms >= ring.0 && ring_t < 1. {
        let r = dot / 2. * (1. + 2.5 * ring_t);
        window.paint_quad(quad(
            circle(r),
            Corners::all(px(r)),
            gpui::transparent_black(),
            Edges::all(px(1.5)),
            alpha(colors.color, 0.8 * (1. - ring_t) * opacity),
            BorderStyle::Solid,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(value: f32) -> impl FnMut() -> f32 {
        move || value
    }

    #[test]
    fn celebrates_only_a_turn_sent_moments_ago() {
        let now = 1_000_000;
        assert!(is_fresh_turn(Some(now - 500), now));
        assert!(!is_fresh_turn(Some(now - 60_000), now));
        assert!(!is_fresh_turn(None, now));
    }

    #[test]
    fn keeps_every_orchestrator_node_inside_the_bubble() {
        for (width, height) in [(60., 36.), (320., 36.), (720., 140.)] {
            let mut clock = Random::seeded(width as u64);
            let mut sources: Vec<Box<dyn FnMut() -> f32>> = vec![
                Box::new(constant(0.)),
                Box::new(constant(0.999)),
                Box::new(move || clock.next()),
            ];
            for random in sources.iter_mut() {
                let Constellation { hub, nodes } =
                    make_constellation(width, height, random.as_mut());
                assert!(hub.y <= height);
                for node in nodes {
                    assert!(node.x >= 0. && node.x <= width, "{node:?}");
                    assert!(node.y >= 0. && node.y <= height, "{node:?}");
                }
            }
        }
    }

    #[test]
    fn fans_agents_out_from_the_lead_before_any_of_them_hand_off() {
        let Constellation { hub, nodes } = make_constellation(480., 80., &mut constant(0.5));
        let agents: Vec<_> = nodes.iter().filter(|node| !node.child).collect();
        let children: Vec<_> = nodes.iter().filter(|node| node.child).collect();
        assert!(agents.iter().all(|node| node.from == hub));
        assert!(!children.is_empty());
        for child in children {
            let parent = agents
                .iter()
                .find(|agent| agent.x == child.from.x && agent.y == child.from.y)
                .expect("a parent agent");
            assert!(child.delay >= parent.delay + parent.draw);
        }
    }

    #[test]
    fn interpolates_keyframes() {
        let frames = [(0., 0.), (0.3, 1.), (1., 0.)];
        assert_eq!(keyframes(&frames, 0.), 0.);
        assert!((keyframes(&frames, 0.15) - 0.5).abs() < 1e-5);
        assert_eq!(keyframes(&frames, 0.3), 1.);
        assert_eq!(keyframes(&frames, 1.), 0.);
    }
}
