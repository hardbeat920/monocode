//! Port of src/shared/ui/ParticleText.tsx: one line of text that, when its
//! value changes, sweeps the new value in from left to right while the old
//! one breaks into particles at the sweep's edge. The first value shows
//! without motion, and so does every change under reduced motion.
//!
//! The web version rasterizes the old text and scatters particles over its
//! inked pixels. GPUI cannot read glyph pixels back, so particles sample each
//! glyph's box instead, thinned to about the share of the box a glyph inks.
//! CSS masks the two texts with a soft gradient; here each character takes
//! the mask's alpha at its center.

use std::f32::consts::PI;
use std::time::Instant;

use gpui::{
    App, Bounds, Div, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, RenderOnce, SharedString, StyleRefinement, Styled, TextAlign, TextRun, Window, canvas,
    div, fill, point, px, size,
};

use super::celebration::Random;

/// Room around the text for particles drifting past its box.
const PAD: f32 = 20.;
/// Sampling step in px; one particle candidate per cell.
const STEP: f32 = 1.5;
/// Share of sampled cells that become particles, so the burst stays light.
const DENSITY: f32 = 0.9;
/// Share of a glyph's box its strokes cover, standing in for the web
/// version's ink test.
const INK: f32 = 0.35;
/// Time for the reveal edge to cross the text.
pub const SWEEP_MS: f32 = 1100.;
/// Softness of the reveal edge.
const EDGE: f32 = 18.;
const PARTICLE_MS: f32 = 850.;
/// The longest a particle lives.
const LONGEST_LIFE_MS: f32 = PARTICLE_MS * 1.2;

/// One particle, in px from the text's top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// How long it lives once the edge reaches it, in ms.
    pub life: f32,
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4. * t.powi(3)
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

/// Where the reveal edge is `elapsed` ms in, for text `width` px wide. It
/// starts one edge's softness before the text and ends one after it.
pub fn edge_at(elapsed: f32, width: f32) -> f32 {
    let travel = width + EDGE * 2.;
    -EDGE + travel * ease_in_out((elapsed / SWEEP_MS).clamp(0., 1.))
}

/// When the edge reaches `x`, in ms. A particle leaves at that moment.
pub fn edge_reaches(x: f32, width: f32) -> f32 {
    if edge_at(0., width) >= x {
        return 0.;
    }
    if edge_at(SWEEP_MS, width) < x {
        return SWEEP_MS;
    }
    let (mut lo, mut hi) = (0f32, SWEEP_MS);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.;
        if edge_at(mid, width) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

/// The new text's alpha at `x`: solid left of the edge's soft band, clear
/// right of the edge.
pub fn reveal_alpha(x: f32, edge: f32) -> f32 {
    ((edge - x) / EDGE).clamp(0., 1.)
}

/// The old text's alpha at `x`: the inverse band, centered on the edge.
pub fn ghost_alpha(x: f32, edge: f32) -> f32 {
    ((x - (edge - EDGE / 2.)) / EDGE).clamp(0., 1.)
}

/// Whether the whole effect is over `elapsed` ms after the change.
pub fn sweep_done(elapsed: f32) -> bool {
    elapsed >= SWEEP_MS + LONGEST_LIFE_MS
}

/// Particles over glyph boxes: each `(left, right)` range spans one visible
/// character between `top` and `bottom`. The same `seed` gives the same
/// burst.
pub fn sample_particles(glyphs: &[(f32, f32)], top: f32, bottom: f32, seed: &str) -> Vec<Particle> {
    let mut random = Random::new(seed);
    let mut particles = Vec::new();
    for &(left, right) in glyphs {
        let mut y = top;
        while y < bottom {
            let mut x = left;
            while x < right {
                let keep = random.next() <= DENSITY * INK;
                let (spread, speed, life) = (random.next(), random.next(), random.next());
                if keep {
                    // A small, mostly forward and upward puff.
                    let angle = -PI / 2. + (spread - 0.3) * PI;
                    let speed = 6. + speed * 12.;
                    particles.push(Particle {
                        x,
                        y,
                        vx: angle.cos() * speed + 4.,
                        vy: angle.sin() * speed * 0.8,
                        life: PARTICLE_MS * (0.7 + life * 0.5),
                    });
                }
                x += STEP;
            }
            y += STEP;
        }
    }
    particles
}

/// A change in flight: the old text and when the new one arrived.
struct Sweep {
    previous: SharedString,
    started: Instant,
}

/// What a `ParticleText` remembers between frames.
struct ParticleState {
    shown: SharedString,
    sweep: Option<Sweep>,
}

/// `<ParticleText text className />`. Style it like the label it replaces;
/// it keeps the text on one line with an ellipsis. Do not give it
/// `truncate()`: its `overflow: hidden` would clip the particles.
#[derive(IntoElement)]
pub struct ParticleText {
    id: ElementId,
    text: SharedString,
    base: Div,
}

pub fn particle_text(id: impl Into<ElementId>, text: impl Into<SharedString>) -> ParticleText {
    ParticleText {
        id: id.into(),
        text: text.into(),
        base: div(),
    }
}

impl Styled for ParticleText {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for ParticleText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let text = self.text;
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| ParticleState {
            shown: text.clone(),
            sweep: None,
        });
        let reduced = cx.reduce_motion();
        let sweep = state.update(cx, |state, _| {
            if state.shown != text {
                let previous = std::mem::replace(&mut state.shown, text.clone());
                state.sweep = (!reduced).then(|| Sweep {
                    previous,
                    started: Instant::now(),
                });
            }
            let elapsed = state
                .sweep
                .as_ref()
                .map(|sweep| sweep.started.elapsed().as_secs_f32() * 1000.);
            match elapsed {
                Some(elapsed) if !reduced && !sweep_done(elapsed) => state
                    .sweep
                    .as_ref()
                    .map(|sweep| (sweep.previous.clone(), elapsed)),
                _ => {
                    state.sweep = None;
                    None
                }
            }
        });
        let label = self
            .base
            .relative()
            .min_w_0()
            .whitespace_nowrap()
            .text_ellipsis();
        let Some((previous, elapsed)) = sweep else {
            return label.child(text).into_any_element();
        };
        // Redraw every frame until the sweep ends.
        window.request_animation_frame();
        let seed = format!("{previous}\u{0}{text}");
        let new_text = text.clone();
        label
            // The real text keeps the label's size while the canvas paints.
            .child(
                div()
                    .debug_selector(|| "particle-text-sweep".into())
                    .text_color(gpui::transparent_black())
                    .child(text),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        paint_sweep(bounds, &previous, &new_text, &seed, elapsed, window, cx)
                    },
                )
                .absolute()
                .top(px(-PAD))
                .left(px(-PAD))
                .right(px(-PAD))
                .bottom(px(-PAD)),
            )
            .into_any_element()
    }
}

/// `fitText`: the text, cut with an ellipsis to fit `width`.
fn fit(text: &SharedString, width: Pixels, look: &Look, window: &Window) -> SharedString {
    if look.shape(text, window).width() <= width {
        return text.clone();
    }
    let mut end = text.len();
    while end > 0 {
        end = text.floor_char_boundary(end - 1);
        let cut: SharedString = format!("{}…", text[..end].trim_end()).into();
        if look.shape(&cut, window).width() <= width {
            return cut;
        }
    }
    "…".into()
}

/// The label's text style, read where the canvas paints.
struct Look {
    origin: gpui::Point<Pixels>,
    line_height: Pixels,
    font_size: Pixels,
    run: TextRun,
}

impl Look {
    fn shape(&self, text: &SharedString, window: &Window) -> gpui::ShapedLine {
        window.text_system().shape_line(
            text.clone(),
            self.font_size,
            &[TextRun {
                len: text.len(),
                ..self.run.clone()
            }],
            None,
        )
    }
}

/// One line of `text` with each character at `alpha(x)` of the text color.
fn paint_masked(
    text: &SharedString,
    look: &Look,
    alpha: impl Fn(f32) -> f32,
    window: &mut Window,
    cx: &mut App,
) {
    let color = look.run.color;
    let run = &look.run;
    let plain = look.shape(text, window);
    let runs: Vec<TextRun> = text
        .char_indices()
        .map(|(ix, ch)| {
            let next = ix + ch.len_utf8();
            let center = f32::from(plain.x_for_index(ix) + plain.x_for_index(next)) / 2.;
            TextRun {
                len: ch.len_utf8(),
                color: Hsla {
                    a: color.a * alpha(center),
                    ..color
                },
                ..run.clone()
            }
        })
        .collect();
    let line = window
        .text_system()
        .shape_line(text.clone(), look.font_size, &runs, None);
    line.paint(
        look.origin,
        look.line_height,
        TextAlign::Left,
        None,
        window,
        cx,
    )
    .ok();
}

fn paint_sweep(
    bounds: Bounds<Pixels>,
    previous: &SharedString,
    text: &SharedString,
    seed: &str,
    elapsed: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let style = window.text_style();
    let rem = window.rem_size();
    let look = Look {
        origin: bounds.origin + point(px(PAD), px(PAD)),
        line_height: style.line_height_in_pixels(rem),
        font_size: style.font_size.to_pixels(rem),
        run: style.to_run(0),
    };
    let color = look.run.color;
    let origin = look.origin;
    let width = bounds.size.width - px(PAD * 2.);
    let edge = edge_at(elapsed, f32::from(width));

    let shown = fit(text, width, &look, window);
    paint_masked(&shown, &look, |x| reveal_alpha(x, edge), window, cx);
    let ghost = fit(previous, width, &look, window);
    paint_masked(&ghost, &look, |x| ghost_alpha(x, edge), window, cx);

    // Particles sit over the old text's glyphs, from cap height to baseline.
    let line = look.shape(&ghost, window);
    let ascent = f32::from(line.ascent);
    let descent = f32::from(line.descent);
    let baseline = (f32::from(look.line_height) - (ascent + descent)) / 2. + ascent;
    let glyphs: Vec<(f32, f32)> = ghost
        .char_indices()
        .filter(|(_, ch)| !ch.is_whitespace())
        .map(|(ix, ch)| {
            (
                f32::from(line.x_for_index(ix)),
                f32::from(line.x_for_index(ix + ch.len_utf8())),
            )
        })
        .collect();
    let width = f32::from(width);
    for particle in sample_particles(&glyphs, baseline - ascent * 0.72, baseline, seed) {
        let born = edge_reaches(particle.x, width);
        if elapsed < born {
            continue;
        }
        let t = ((elapsed - born) / particle.life).clamp(0., 1.);
        if t >= 1. {
            continue;
        }
        let drift = ease_out(t);
        let side = 1.8 * (1. - t * 0.5);
        window.paint_quad(fill(
            Bounds::new(
                origin
                    + point(
                        px(particle.x + particle.vx * drift),
                        px(particle.y + particle.vy * drift),
                    ),
                size(px(side), px(side)),
            ),
            Hsla {
                a: color.a * (1. - t).powf(1.2),
                ..color
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edge_crosses_the_text_in_the_sweep_time() {
        assert_eq!(edge_at(0., 200.), -EDGE);
        assert_eq!(edge_at(SWEEP_MS, 200.), 200. + EDGE);
        assert!((edge_at(SWEEP_MS / 2., 200.) - 100.).abs() < 1e-3);
        let at = edge_reaches(100., 200.);
        assert!((at - SWEEP_MS / 2.).abs() < 1., "{at}");
        assert_eq!(edge_reaches(-50., 200.), 0.);
    }

    #[test]
    fn the_new_text_fades_in_behind_the_edge_and_the_old_one_ahead_of_it() {
        assert_eq!(reveal_alpha(0., 50.), 1.);
        assert_eq!(reveal_alpha(50., 50.), 0.);
        assert_eq!(ghost_alpha(0., 50.), 0.);
        assert_eq!(ghost_alpha(100., 50.), 1.);
        assert!((ghost_alpha(50., 50.) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn particles_cover_the_glyphs_and_repeat_for_a_seed() {
        let glyphs = [(0., 8.), (10., 16.)];
        let first = sample_particles(&glyphs, 2., 12., "a");
        assert_eq!(first, sample_particles(&glyphs, 2., 12., "a"));
        assert!(!first.is_empty());
        assert!(first.iter().all(|p| {
            ((0. ..8.).contains(&p.x) || (10. ..16.).contains(&p.x))
                && (2. ..12.).contains(&p.y)
                && p.life >= PARTICLE_MS * 0.7
                && p.life <= LONGEST_LIFE_MS
        }));
        // Mostly upward and forward.
        let rising = first.iter().filter(|p| p.vy < 0.).count();
        assert!(rising * 2 > first.len());
    }

    #[test]
    fn the_effect_ends_after_the_sweep_and_the_last_particle() {
        assert!(!sweep_done(SWEEP_MS));
        assert!(sweep_done(SWEEP_MS + LONGEST_LIFE_MS));
    }
}
