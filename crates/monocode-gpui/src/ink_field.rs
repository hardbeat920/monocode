//! The empty session's canvas. The pointer stirs ink through a small smoke
//! field that drifts, curls and fades; holding charges a vortex that pulls
//! the ink into a spiral, and letting go bursts it outward as a shockwave.
//! Painted on the charts' 3px Bayer grid so it reads as the same halftone.
use crate::{session_mark::SPINNER_ROWS, visuals::dither};
use gpui::{
    Bounds, Context, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Render, Window, canvas, div, point, prelude::*, px, rgb, size,
};
use std::{cell::RefCell, f32::consts::PI, rc::Rc, time::Instant};

/// Side of a smoke cell; the dither samples the field every 3px.
const FIELD: f32 = 6.;
/// Seconds of holding that fully charge a burst.
const CHARGE: f32 = 1.2;

struct Splat {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}

struct Ripple {
    x: f32,
    y: f32,
    age: f32,
    strength: f32,
}
impl Ripple {
    fn life(&self) -> f32 {
        0.7 + 0.7 * self.strength
    }
    fn band(&self) -> f32 {
        7. + 12. * self.strength
    }
    /// The ring's radius and how strongly it still shows, once it has started.
    fn front(&self) -> Option<(f32, f32)> {
        if self.age < 0. {
            return None;
        }
        let t = (self.age / self.life()).min(1.);
        let reach = (260. + 300. * self.strength) * self.life() * 0.5;
        Some((
            reach * (1. - (1. - t).powi(3)),
            self.strength.min(1.) * (1. - t).powf(1.5),
        ))
    }
}

struct Press {
    x: f32,
    y: f32,
    held: f32,
}
impl Press {
    fn charge(&self) -> f32 {
        (self.held / CHARGE).min(1.)
    }
}

#[derive(Default)]
struct Sim {
    origin: Point<Pixels>,
    width: f32,
    height: f32,
    cols: usize,
    rows: usize,
    ink: Vec<f32>,
    vx: Vec<f32>,
    vy: Vec<f32>,
    scratch: Vec<f32>,
    splats: Vec<Splat>,
    pointer: Option<(f32, f32, Instant)>,
    press: Option<Press>,
    ripples: Vec<Ripple>,
    peak: f32,
    time: f32,
    last: Option<Instant>,
    greeted: bool,
}

impl Sim {
    fn quiet(&self) -> bool {
        self.peak < 0.01
            && self.ripples.is_empty()
            && self.press.is_none()
            && self.splats.is_empty()
    }

    fn local(&self, at: Point<Pixels>) -> (f32, f32) {
        (
            (at.x - self.origin.x).as_f32(),
            (at.y - self.origin.y).as_f32(),
        )
    }

    fn resize(&mut self, bounds: Bounds<Pixels>) {
        self.origin = bounds.origin;
        self.width = bounds.size.width.as_f32();
        self.height = bounds.size.height.as_f32();
        let cols = (self.width / FIELD).ceil() as usize + 1;
        let rows = (self.height / FIELD).ceil() as usize + 1;
        if (cols, rows) != (self.cols, self.rows) {
            self.cols = cols;
            self.rows = rows;
            for field in [&mut self.ink, &mut self.vx, &mut self.vy, &mut self.scratch] {
                field.clear();
                field.resize(cols * rows, 0.);
            }
            self.peak = 0.;
        }
        // A soft ring on arrival hints that the space is alive.
        if !self.greeted && self.width > 0. {
            self.greeted = true;
            self.ripples.push(Ripple {
                x: self.width / 2.,
                y: self.height * 0.38,
                age: 0.,
                strength: 0.2,
            });
        }
    }

    /// Records a pointer move. Returns whether the field was asleep.
    fn moved(&mut self, at: Point<Pixels>) -> bool {
        let woke = self.quiet();
        let (x, y) = self.local(at);
        let now = Instant::now();
        if let Some((px, py, then)) = self.pointer {
            let dt = now.duration_since(then).as_secs_f32().max(1. / 240.);
            let (dx, dy) = (x - px, y - py);
            let (vx, vy) = (
                (dx / dt).clamp(-3000., 3000.),
                (dy / dt).clamp(-3000., 3000.),
            );
            let steps = (dx.hypot(dy) / 3.).ceil().clamp(1., 200.) as usize;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                self.splats.push(Splat {
                    x: px + dx * t,
                    y: py + dy * t,
                    vx,
                    vy,
                });
            }
        }
        self.pointer = Some((x, y, now));
        if let Some(press) = &mut self.press {
            press.x = x;
            press.y = y;
        }
        woke
    }

    fn pressed(&mut self, at: Point<Pixels>) -> bool {
        let woke = self.quiet();
        let (x, y) = self.local(at);
        self.press = Some(Press { x, y, held: 0. });
        woke
    }

    fn released(&mut self) {
        let Some(press) = self.press.take() else {
            return;
        };
        let charge = press.charge();
        let strength = 0.3 + 0.7 * charge;
        self.ripples.push(Ripple {
            x: press.x,
            y: press.y,
            age: 0.,
            strength,
        });
        // A charged burst echoes.
        if charge > 0.4 {
            self.ripples.push(Ripple {
                x: press.x,
                y: press.y,
                age: -0.14,
                strength: strength * 0.55,
            });
        }
    }

    fn left(&mut self) {
        self.pointer = None;
    }

    /// Visits the cells within `radius` of (x, y) with their offset and a
    /// falloff from 1 at the centre to 0 at the edge.
    fn around(
        &mut self,
        x: f32,
        y: f32,
        radius: f32,
        mut visit: impl FnMut(&mut Self, usize, f32, f32, f32),
    ) {
        let (c0, c1) = (
            ((x - radius) / FIELD).floor().max(0.) as usize,
            (((x + radius) / FIELD).ceil() as usize).min(self.cols.saturating_sub(1)),
        );
        let (r0, r1) = (
            ((y - radius) / FIELD).floor().max(0.) as usize,
            (((y + radius) / FIELD).ceil() as usize).min(self.rows.saturating_sub(1)),
        );
        for row in r0..=r1 {
            for col in c0..=c1 {
                let dx = (col as f32 + 0.5) * FIELD - x;
                let dy = (row as f32 + 0.5) * FIELD - y;
                let d = dx.hypot(dy);
                if d < radius {
                    let w = 1. - d / radius;
                    visit(self, row * self.cols + col, dx, dy, w * w);
                }
            }
        }
    }

    fn step(&mut self) {
        let now = Instant::now();
        let dt = self
            .last
            .map_or(0., |last| now.duration_since(last).as_secs_f32().min(0.05));
        self.last = Some(now);
        if self.cols == 0 || self.quiet() || dt == 0. {
            return;
        }
        self.time += dt;

        // Trails: ink thickens with speed and inherits the pointer's motion.
        for s in std::mem::take(&mut self.splats) {
            let radius = (6. + s.vx.hypot(s.vy) * 0.006).min(16.);
            self.around(s.x, s.y, radius, |sim, i, _, _, w| {
                sim.ink[i] += (1. - sim.ink[i]) * w * 0.28;
                sim.vx[i] += (s.vx * 0.45 - sim.vx[i]) * w * 0.5;
                sim.vy[i] += (s.vy * 0.45 - sim.vy[i]) * w * 0.5;
            });
        }

        // Holding spins a vortex that drinks nearby ink, shedding two arms.
        if let Some((x, y, charge)) = self.press.as_mut().map(|press| {
            press.held += dt;
            (press.x, press.y, press.charge())
        }) {
            let reach = 40. + 80. * charge;
            let spin = 140. + 320. * charge;
            let blend = (dt * 8.).min(1.);
            self.around(x, y, reach, |sim, i, dx, dy, w| {
                let d = dx.hypot(dy).max(1.);
                let (tx, ty) = (-dy / d * spin * w, dx / d * spin * w);
                let pull = 70. * w;
                sim.vx[i] += (tx - dx / d * pull - sim.vx[i]) * blend * w.sqrt();
                sim.vy[i] += (ty - dy / d * pull - sim.vy[i]) * blend * w.sqrt();
            });
            for arm in [0., PI] {
                let a = self.time * (5. + 6. * charge) + arm;
                let r = reach * 0.55;
                self.around(x + a.cos() * r, y + a.sin() * r, 7., |sim, i, _, _, w| {
                    sim.ink[i] += (1. - sim.ink[i]) * w * 0.5;
                });
            }
        }

        // Shockwaves shove the ink outward and leave a faint wake.
        let ripples = std::mem::take(&mut self.ripples);
        for ripple in &ripples {
            let Some((radius, amp)) = ripple.front() else {
                continue;
            };
            let band = ripple.band();
            self.around(
                ripple.x,
                ripple.y,
                radius + band * 2.5,
                |sim, i, dx, dy, _| {
                    let d = dx.hypot(dy).max(1.);
                    let g = (-((d - radius) / band).powi(2)).exp() * amp;
                    if g > 0.02 {
                        let push = 800. * ripple.strength * g * dt;
                        sim.vx[i] += dx / d * push;
                        sim.vy[i] += dy / d * push;
                        sim.ink[i] += (1. - sim.ink[i]) * g * 3. * dt;
                    }
                },
            );
        }
        self.ripples = ripples
            .into_iter()
            .map(|mut r| {
                r.age += dt;
                r
            })
            .filter(|r| r.age < r.life())
            .collect();

        // A slow curl keeps resting smoke wandering.
        let t = self.time;
        for row in 0..self.rows {
            for col in 0..self.cols {
                let i = row * self.cols + col;
                if self.ink[i] > 0.01 {
                    let (x, y) = (col as f32 * FIELD, row as f32 * FIELD);
                    let a = ((x * 0.021 + t * 0.37).sin()
                        + (y * 0.027 - t * 0.29).cos()
                        + ((x + y) * 0.013 + t * 0.21).sin())
                        * 1.4;
                    self.vx[i] += a.cos() * 45. * dt;
                    self.vy[i] += a.sin() * 45. * dt;
                }
            }
        }

        // Carry the ink along its velocity, then soften and fade it.
        let (cols, rows) = (self.cols, self.rows);
        for row in 0..rows {
            for col in 0..cols {
                let i = row * cols + col;
                self.scratch[i] = sample(
                    &self.ink,
                    cols,
                    rows,
                    col as f32 - self.vx[i] * dt / FIELD,
                    row as f32 - self.vy[i] * dt / FIELD,
                );
            }
        }
        let fade = (-dt * 1.6).exp();
        let calm = (-dt * 1.8).exp();
        let mut peak = 0f32;
        for row in 0..rows {
            for col in 0..cols {
                let i = row * cols + col;
                let at = |c: usize, r: usize| self.scratch[r * cols + c];
                let near = at(col.saturating_sub(1), row)
                    + at((col + 1).min(cols - 1), row)
                    + at(col, row.saturating_sub(1))
                    + at(col, (row + 1).min(rows - 1));
                let v = ((self.scratch[i] * 0.84 + near * 0.04) * fade - 0.002).clamp(0., 1.);
                self.ink[i] = v;
                peak = peak.max(v);
                self.vx[i] *= calm;
                self.vy[i] *= calm;
            }
        }
        self.peak = peak;
    }

    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        self.resize(bounds);
        self.step();
        if self.quiet() {
            return;
        }
        // The working spinner's slate, peach and pink: wisps cool, cores warm.
        let tint = |hex: u32, alpha: f32| -> Hsla {
            let mut color: Hsla = rgb(hex).into();
            color.a = alpha * if dither::light() { 1.3 } else { 1. };
            color
        };
        let [slate, peach, pink] = SPINNER_ROWS;
        let ink = [tint(slate, 0.14), tint(peach, 0.18), tint(pink, 0.24)];
        let lit_ink = [tint(slate, 0.18), tint(peach, 0.24), tint(pink, 0.3)];
        let level = |tone: f32| (tone * 3.).clamp(0., 2.) as usize;

        let rings: Vec<_> = self
            .ripples
            .iter()
            .filter_map(|r| {
                r.front()
                    .map(|(radius, amp)| (r.x, r.y, radius, r.band(), amp))
            })
            .collect();
        let orb = self.press.as_ref().map(|p| {
            let charge = p.charge();
            let breath = (self.time * 9.).sin() * 1.5;
            (p.x, p.y, 4. + 14. * charge + breath, 0.4 + 0.5 * charge)
        });
        if let Some((x, y, radius, amp)) = orb {
            let r = px(radius * 1.4);
            dither::glow(
                window,
                Bounds::new(
                    self.origin + point(px(x) - r, px(y) - r),
                    size(r * 2., r * 2.),
                ),
                tint(pink, 1.),
                0.08 * amp,
            );
        }

        let (cols, rows) = (self.cols, self.rows);
        let field = &self.ink;
        dither::Grid::new(self.origin, self.width, self.height).paint(window, |col, row| {
            let (x, y) = dither::Grid::center(col, row);
            let tone = sample(field, cols, rows, x / FIELD - 0.5, y / FIELD - 0.5);
            let mut lit = 0f32;
            for &(rx, ry, radius, band, amp) in &rings {
                let off = (x - rx).hypot(y - ry) - radius;
                if off.abs() < band * 2.5 {
                    lit = lit.max((-(off / band).powi(2)).exp() * amp);
                }
            }
            if let Some((ox, oy, radius, amp)) = orb {
                let d = (x - ox).hypot(y - oy);
                if d < radius {
                    lit = lit.max((1. - d / radius).sqrt() * amp);
                }
            }
            // Coverage stays sparse, so the halftone never fills in solid.
            if lit > tone && dither::inked(col, row, lit * 0.5) {
                Some(lit_ink[level(lit)])
            } else if dither::inked(col, row, tone * 0.55) {
                Some(ink[level(tone)])
            } else {
                None
            }
        });
    }
}

/// The field at fractional cell coordinates, interpolated between cells.
fn sample(field: &[f32], cols: usize, rows: usize, x: f32, y: f32) -> f32 {
    let x = x.clamp(0., cols as f32 - 1.001);
    let y = y.clamp(0., rows as f32 - 1.001);
    let (c, r) = (x as usize, y as usize);
    let (fx, fy) = (x - c as f32, y - r as f32);
    let at = |c: usize, r: usize| field[r * cols + c];
    let top = at(c, r) + (at(c + 1, r) - at(c, r)) * fx;
    let bottom = at(c, r + 1) + (at(c + 1, r + 1) - at(c, r + 1)) * fx;
    top + (bottom - top) * fy
}

pub(crate) struct InkField {
    sim: Rc<RefCell<Sim>>,
}
impl InkField {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            sim: Default::default(),
        }
    }
}
impl Render for InkField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sim = self.sim.clone();
        div()
            .id("empty-session-ink")
            .absolute()
            .inset_0()
            .overflow_hidden()
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.sim.borrow_mut().moved(event.position) {
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if this.sim.borrow_mut().pressed(event.position) {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.sim.borrow_mut().released()),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.sim.borrow_mut().released()),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, _| {
                if !hovered {
                    this.sim.borrow_mut().left();
                }
            }))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let mut sim = sim.borrow_mut();
                        sim.paint(bounds, window);
                        // Painting initializes the greeting ripple. Lease the
                        // clock afterwards, including on the very first frame.
                        if !sim.quiet() {
                            crate::pulse::request(crate::pulse::Rate::Smooth, window, cx);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn first_paint_starts_the_clock_and_a_quiet_field_releases_it(cx: &mut gpui::TestAppContext) {
        use std::time::Duration;

        cx.update(|cx| cx.set_reduce_motion(false));
        let (view, cx) = cx.add_window_view(|_, cx| InkField::new(cx));
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(view.sim.borrow().greeted);
            assert!(!view.sim.borrow().quiet());
        });
        cx.update(|_, cx| assert!(crate::pulse::running(cx)));

        view.update(cx, |view, _| {
            view.sim.borrow_mut().last = Some(Instant::now() - Duration::from_millis(33));
        });
        cx.executor().advance_clock(Duration::from_millis(33));
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert!(view.sim.borrow().time >= 0.03));

        view.update(cx, |view, cx| {
            *view.sim.borrow_mut() = Sim {
                greeted: true,
                ..Default::default()
            };
            cx.notify();
        });
        cx.executor().advance_clock(Duration::from_millis(66));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!crate::pulse::running(cx)));

        cx.simulate_mouse_down(
            point(px(100.), px(100.)),
            MouseButton::Left,
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        cx.update(|_, cx| assert!(crate::pulse::running(cx)));
    }

    #[gpui::test]
    fn reduced_motion_never_starts_the_ink_clock(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (_, cx) = cx.add_window_view(|_, cx| InkField::new(cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(!crate::pulse::running(cx)));
    }

    fn sim() -> Sim {
        let mut sim = Sim::default();
        sim.resize(Bounds::new(point(px(0.), px(0.)), size(px(600.), px(400.))));
        sim.ripples.clear();
        sim
    }

    #[test]
    fn a_trail_inks_the_field_then_fades_to_sleep() {
        let mut sim = sim();
        assert!(sim.quiet());
        sim.moved(point(px(100.), px(100.)));
        sim.moved(point(px(300.), px(120.)));
        sim.last = Some(Instant::now() - std::time::Duration::from_millis(33));
        sim.step();
        assert!(sim.peak > 0.3);
        for _ in 0..400 {
            sim.last = Some(Instant::now() - std::time::Duration::from_millis(33));
            sim.step();
        }
        assert!(sim.quiet());
    }

    #[test]
    fn a_longer_hold_bursts_harder() {
        let mut sim = sim();
        sim.pressed(point(px(300.), px(200.)));
        sim.released();
        let tap = sim.ripples[0].strength;
        sim.ripples.clear();
        sim.pressed(point(px(300.), px(200.)));
        sim.press.as_mut().unwrap().held = CHARGE;
        sim.released();
        assert!(sim.ripples[0].strength > tap);
        assert_eq!(sim.ripples.len(), 2, "a charged burst echoes");
    }
}
