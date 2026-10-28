//! Ordered dithering for charts: a tone in [0, 1]
//! becomes a share of inked 3px cells through a 4×4 Bayer matrix, so fills
//! read as halftone texture instead of flat translucent colour.
use crate::theme::content;
use gpui::{
    App, Bounds, BoxShadow, Corners, FontWeight, Hsla, Pixels, Point, TextRun, Window, fill, point,
    px, size,
};

/// Cell side in logical pixels.
pub(super) const CELL: f32 = 3.;
const BAYER4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// Whether the cell at (col, row) is inked for `tone`. Thresholds sit
/// halfway between steps, so a tone of 0.25 inks exactly a quarter.
pub(super) fn inked(col: i32, row: i32, tone: f32) -> bool {
    tone > (BAYER4[((row & 3) * 4 + (col & 3)) as usize] as f32 + 0.5) / 16.
}

/// Whether the interface draws dark ink on a light surface. Glows turn
/// muddy there, so they are softer.
pub(super) fn light() -> bool {
    content(1.).l < 0.5
}

/// A grid of cells from a pixel-snapped origin.
pub(super) struct Grid {
    pub origin: Point<Pixels>,
    pub cols: i32,
    pub rows: i32,
}
impl Grid {
    pub fn new(origin: Point<Pixels>, width: f32, height: f32) -> Self {
        Self {
            origin: point(px(origin.x.as_f32().round()), px(origin.y.as_f32().round())),
            cols: (width / CELL).floor().max(0.) as i32,
            rows: (height / CELL).floor().max(0.) as i32,
        }
    }
    /// The centre of a cell, relative to the origin.
    pub fn center(col: i32, row: i32) -> (f32, f32) {
        ((col as f32 + 0.5) * CELL, (row as f32 + 0.5) * CELL)
    }
    /// Paints each cell `ink` colours, one quad per horizontal run of a colour.
    pub fn paint(&self, window: &mut Window, mut ink: impl FnMut(i32, i32) -> Option<Hsla>) {
        for row in 0..self.rows {
            let mut run: Option<(i32, Hsla)> = None;
            for col in 0..=self.cols {
                let next = if col < self.cols { ink(col, row) } else { None };
                if let (Some((_, a)), Some(b)) = (run, next)
                    && a == b
                {
                    continue;
                }
                if let Some((start, color)) = run {
                    window.paint_quad(fill(
                        Bounds::new(
                            self.origin + point(px(start as f32 * CELL), px(row as f32 * CELL)),
                            size(px((col - start) as f32 * CELL), px(CELL)),
                        ),
                        color,
                    ));
                }
                run = next.map(|c| (col, c));
            }
        }
    }
}

/// A soft bloom over `bounds`, inside as well as around it, so it shows
/// through the gaps of a dithered fill painted on top.
pub(super) fn glow(window: &mut Window, bounds: Bounds<Pixels>, color: Hsla, strength: f32) {
    let mut color = color;
    color.a = strength * if light() { 0.4 } else { 1. };
    // A drop shadow skips its own element; shrinking the element to a line
    // and spreading back out covers the whole rectangle.
    let spread = bounds.size.width.min(bounds.size.height) / 2.;
    window.paint_drop_shadows(
        Bounds::new(
            bounds.origin + point(spread, spread),
            size(
                bounds.size.width - spread * 2.,
                bounds.size.height - spread * 2.,
            ),
        ),
        Corners::default(),
        &[BoxShadow {
            color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(14.),
            spread_radius: spread,
            inset: false,
        }],
    );
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Align {
    Left,
    Center,
    Right,
}

/// Small type for axes and captions.
pub(super) struct Label {
    pub size: f32,
    pub color: Hsla,
    pub mono: bool,
    pub weight: FontWeight,
}
impl Label {
    pub fn mono(size: f32, color: Hsla) -> Self {
        Self {
            size,
            color,
            mono: true,
            weight: FontWeight::NORMAL,
        }
    }
    pub fn sans(size: f32, color: Hsla, weight: FontWeight) -> Self {
        Self {
            size,
            color,
            mono: false,
            weight,
        }
    }
    fn shape(&self, text: &str, window: &mut Window) -> gpui::ShapedLine {
        let mut font = window.text_style().font();
        if self.mono {
            font.family = crate::fonts::mono().into();
        }
        font.weight = self.weight;
        font.features = crate::right_panel::tabular();
        let run = TextRun {
            len: text.len(),
            font,
            color: self.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_line(text.to_owned().into(), px(self.size), &[run], None)
    }
    pub fn measure(&self, text: &str, window: &mut Window) -> f32 {
        if text.is_empty() {
            return 0.;
        }
        self.shape(text, window).width.as_f32()
    }
    /// Paints `text` with its vertical centre at `at.y`. Returns its width.
    pub fn paint(
        &self,
        text: &str,
        at: Point<Pixels>,
        align: Align,
        window: &mut Window,
        cx: &mut App,
    ) -> f32 {
        if text.is_empty() {
            return 0.;
        }
        let line = self.shape(text, window);
        let width = line.width.as_f32();
        let x = match align {
            Align::Left => 0.,
            Align::Center => width / 2.,
            Align::Right => width,
        };
        let height = self.size * 1.3;
        let _ = line.paint(
            at - point(px(x), px(height / 2.)),
            px(height),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
        width
    }
}

/// A visual's entrance clock: charts draw themselves in once, from when the
/// visual first appeared, unless the system asks for reduced motion.
#[derive(Clone, Copy)]
pub(super) struct Motion(pub std::time::Instant);
impl Motion {
    /// Eased progress, 0 to 1, of a step lasting `ms` that starts `delay` ms
    /// in. Asks for another frame until it settles.
    pub fn at(&self, delay: f32, ms: f32, window: &mut Window, cx: &App) -> f32 {
        if cx.reduce_motion() {
            return 1.;
        }
        let t = ((self.0.elapsed().as_secs_f32() * 1000. - delay) / ms).clamp(0., 1.);
        if t < 1. {
            window.request_animation_frame();
        }
        // A quartic ease-out.
        1. - (1. - t).powi(4)
    }
}

/// The pointer, relative to `bounds`' origin, while it is over them.
pub(super) fn pointer(bounds: Bounds<Pixels>, window: &Window) -> Option<(f32, f32)> {
    let at = window.mouse_position();
    bounds.contains(&at).then(|| {
        (
            (at.x - bounds.origin.x).as_f32(),
            (at.y - bounds.origin.y).as_f32(),
        )
    })
}

/// A chart canvas that repaints as the pointer moves over it and leaves.
pub(super) fn hoverable(id: gpui::SharedString) -> gpui::Stateful<gpui::Div> {
    use gpui::{InteractiveElement, StatefulInteractiveElement, div};
    div()
        .id(id)
        .on_mouse_move(|_, window, _| window.refresh())
        .on_hover(|_, window, _| window.refresh())
}

/// The details card next to a hovered point: a title, then one row per
/// series with its swatch, name and value. Kept inside `bounds`.
pub(super) fn readout(
    title: &str,
    rows: &[(Hsla, String, String)],
    anchor: Point<Pixels>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let head = Label::mono(10., content(0.55));
    let name = Label::sans(12., content(0.75), gpui::FontWeight::NORMAL);
    let value = Label::mono(12., content(1.));
    let pad = 10.;
    let line = 20.;
    let names = rows
        .iter()
        .map(|(_, n, _)| name.measure(n, window))
        .fold(0f32, f32::max);
    let values = rows
        .iter()
        .map(|(_, _, v)| value.measure(v, window))
        .fold(0f32, f32::max);
    let width = (pad * 2. + 16. + names + 16. + values).max(head.measure(title, window) + pad * 2.);
    let height = pad * 2. + 16. + rows.len() as f32 * line;
    let (bx, by) = (bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
    let (bw, bh) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
    let mut x = anchor.x.as_f32() + 14.;
    if x + width > bx + bw {
        x = anchor.x.as_f32() - 14. - width;
    }
    let x = x.max(bx);
    let y = (anchor.y.as_f32() - height / 2.).clamp(by, (by + bh - height).max(by));
    // The same surface as the app's popovers: frosted glass over the chart
    // in dark themes, an opaque card in light ones.
    let card = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
    crate::popover::paint_surface(card, window);
    head.paint(
        title,
        point(px(x + pad), px(y + pad + 7.)),
        Align::Left,
        window,
        cx,
    );
    for (i, (color, n, v)) in rows.iter().enumerate() {
        let cy = y + pad + 16. + (i as f32 + 0.5) * line;
        window.paint_quad(fill(
            Bounds::new(point(px(x + pad), px(cy - 4.)), size(px(8.), px(8.))),
            *color,
        ));
        name.paint(n, point(px(x + pad + 16.), px(cy)), Align::Left, window, cx);
        value.paint(
            v,
            point(px(x + width - pad), px(cy)),
            Align::Right,
            window,
            cx,
        );
    }
}

/// A dashed vertical guide at `x`, from `top` down `height` px.
pub(super) fn crosshair(origin: Point<Pixels>, x: f32, height: f32, window: &mut Window) {
    let mut y = 0.;
    while y < height {
        window.paint_quad(fill(
            Bounds::new(
                origin + point(px(x.round()), px(y)),
                size(px(1.), px((height - y).min(2.))),
            ),
            content(0.35),
        ));
        y += 5.;
    }
}

/// A y domain with round ticks covering every value, as charts want.
pub(super) struct Scale {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}
impl Scale {
    pub fn nice(values: impl IntoIterator<Item = f64>, zero: bool) -> Self {
        let (mut low, mut high) = if zero {
            (0f64, 0f64)
        } else {
            (f64::MAX, f64::MIN)
        };
        for v in values.into_iter().filter(|v| v.is_finite()) {
            low = low.min(v);
            high = high.max(v);
        }
        if low > high {
            (low, high) = (0., 1.);
        }
        let span = if high - low > 0. {
            high - low
        } else if high.abs() > 0. {
            high.abs()
        } else {
            1.
        };
        let raw = span / 4.;
        let magnitude = 10f64.powf(raw.log10().floor());
        let step = [1., 2., 2.5, 5., 10.]
            .into_iter()
            .find(|c| c * magnitude >= raw)
            .unwrap_or(10.)
            * magnitude;
        Self {
            min: (low / step).floor() * step,
            max: (high / step).ceil() * step,
            step,
        }
        .widened()
    }
    // A flat series still needs a range to sit in.
    fn widened(mut self) -> Self {
        if self.max <= self.min {
            self.max = self.min + self.step;
        }
        self
    }
    pub fn ticks(&self) -> Vec<f64> {
        let count = ((self.max - self.min) / self.step).round() as usize;
        (0..=count.min(12))
            .map(|i| self.min + self.step * i as f64)
            .collect()
    }
    /// Where `value` falls, 0 at `max` and 1 at `min`.
    pub fn depth(&self, value: f64) -> f32 {
        ((self.max - value) / (self.max - self.min)) as f32
    }
    /// A tick label: compact, with only the decimals the step needs.
    pub fn label(&self, value: f64) -> String {
        compact(value, self.step)
    }
}

/// 1.2K, 48K, 3.4M; plain numbers keep `step`'s decimals.
pub(super) fn compact(value: f64, step: f64) -> String {
    let trim = |s: String| {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s
        }
    };
    let abs = value.abs();
    if abs >= 1e9 {
        format!("{}B", trim(format!("{:.1}", value / 1e9)))
    } else if abs >= 1e6 {
        format!("{}M", trim(format!("{:.1}", value / 1e6)))
    } else if abs >= 1e4 {
        format!("{}K", trim(format!("{:.1}", value / 1e3)))
    } else {
        // As many decimals as the step has: 0.25 needs two, 0.5 one.
        let decimals = (0..4)
            .find(|d| {
                let scaled = step.abs() * 10f64.powi(*d);
                (scaled - scaled.round()).abs() < 1e-6
            })
            .unwrap_or(4) as usize;
        trim(format!("{value:.decimals$}"))
    }
}

/// A smooth curve through points that never overshoots them (monotone
/// cubic, Fritsch–Carlson), so peaks stay at the data.
pub(super) struct Curve {
    xs: Vec<f32>,
    ys: Vec<f32>,
    ms: Vec<f32>,
}
impl Curve {
    pub fn new(points: &[[f32; 2]]) -> Self {
        let xs: Vec<f32> = points.iter().map(|p| p[0]).collect();
        let ys: Vec<f32> = points.iter().map(|p| p[1]).collect();
        let n = points.len();
        let mut ms = vec![0.; n];
        if n >= 2 {
            let d: Vec<f32> = (0..n - 1)
                .map(|k| (ys[k + 1] - ys[k]) / (xs[k + 1] - xs[k]).max(1e-6))
                .collect();
            ms[0] = d[0];
            ms[n - 1] = d[n - 2];
            for k in 1..n - 1 {
                ms[k] = if d[k - 1] * d[k] <= 0. {
                    0.
                } else {
                    (d[k - 1] + d[k]) / 2.
                };
            }
            for k in 0..n - 1 {
                if d[k] == 0. {
                    ms[k] = 0.;
                    ms[k + 1] = 0.;
                    continue;
                }
                let (a, b) = (ms[k] / d[k], ms[k + 1] / d[k]);
                let s = a * a + b * b;
                if s > 9. {
                    let t = 3. / s.sqrt();
                    ms[k] = t * a * d[k];
                    ms[k + 1] = t * b * d[k];
                }
            }
        }
        Self { xs, ys, ms }
    }
    pub fn first_x(&self) -> f32 {
        self.xs.first().copied().unwrap_or(0.)
    }
    pub fn last_x(&self) -> f32 {
        self.xs.last().copied().unwrap_or(0.)
    }
    /// The curve's y at `x`, clamped to its ends.
    pub fn at(&self, x: f32) -> f32 {
        let n = self.xs.len();
        if n == 0 {
            return 0.;
        }
        if n == 1 || x <= self.xs[0] {
            return self.ys[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1];
        }
        let k = self
            .xs
            .partition_point(|&v| v <= x)
            .saturating_sub(1)
            .min(n - 2);
        let h = (self.xs[k + 1] - self.xs[k]).max(1e-6);
        let t = (x - self.xs[k]) / h;
        let (t2, t3) = (t * t, t * t * t);
        (2. * t3 - 3. * t2 + 1.) * self.ys[k]
            + (t3 - 2. * t2 + t) * h * self.ms[k]
            + (-2. * t3 + 3. * t2) * self.ys[k + 1]
            + (t3 - t2) * h * self.ms[k + 1]
    }
    /// Points along the curve every `step` px, for stroking.
    pub fn samples(&self, step: f32) -> Vec<[f32; 2]> {
        let (a, b) = (self.first_x(), self.last_x());
        if self.xs.len() < 2 {
            return self
                .xs
                .iter()
                .zip(&self.ys)
                .map(|(x, y)| [*x, *y])
                .collect();
        }
        let count = ((b - a) / step).ceil().max(1.) as usize;
        (0..=count)
            .map(|i| {
                let x = a + (b - a) * i as f32 / count as f32;
                [x, self.at(x)]
            })
            .collect()
    }
}

/// Strokes `points` (relative to `origin`) with a soft glow under the line.
pub(super) fn glowing_line(
    points: &[[f32; 2]],
    origin: Point<Pixels>,
    width: f32,
    color: Hsla,
    window: &mut Window,
) {
    let dim = if light() { 0.45 } else { 1. };
    for (spread, alpha) in [(10., 0.07), (5., 0.14)] {
        let mut halo = color;
        halo.a = alpha * dim;
        super::paint_path(points, origin, 1., width + spread, false, halo, window);
    }
    super::paint_path(points, origin, 1., width, false, color, window);
}

/// A square end marker in a faint halo of its colour.
pub(super) fn marker(window: &mut Window, center: Point<Pixels>, side: f32, color: Hsla) {
    let mut halo = color;
    halo.a = 0.25;
    for (side, color) in [(side + 6., halo), (side, color)] {
        window.paint_quad(fill(
            Bounds::new(
                center - point(px(side / 2.), px(side / 2.)),
                size(px(side), px(side)),
            ),
            color,
        ));
    }
}

/// A legend swatch: a translucent body under a solid cap, like a bar.
pub(super) fn swatch(color: Hsla) -> gpui::Div {
    use gpui::{Styled, div, prelude::*};
    let mut body = color;
    body.a = 0.4;
    div()
        .size(px(10.))
        .flex_shrink_0()
        .bg(body)
        .child(div().h(px(2.)).w_full().bg(color))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_ink_their_share_of_a_tile() {
        for steps in 0..=16 {
            let tone = steps as f32 / 16.;
            let count = (0..4)
                .flat_map(|y| (0..4).map(move |x| (x, y)))
                .filter(|&(x, y)| inked(x, y, tone))
                .count();
            assert_eq!(count, steps);
        }
    }

    #[test]
    fn nice_scales_round_their_ends_and_include_zero() {
        let scale = Scale::nice([42., 74.], true);
        assert_eq!((scale.min, scale.max, scale.step), (0., 80., 20.));
        assert_eq!(scale.ticks(), [0., 20., 40., 60., 80.]);
        let flat = Scale::nice([5., 5.], false);
        assert!(flat.max > flat.min);
        assert_eq!(compact(48_210., 1.), "48.2K");
        assert_eq!(compact(0.25, 0.25), "0.25");
        assert_eq!(compact(2_500_000., 1.), "2.5M");
    }

    #[test]
    fn monotone_curves_pass_through_points_without_overshooting() {
        let curve = Curve::new(&[[0., 10.], [10., 0.], [20., 0.], [30., 10.]]);
        assert_eq!(curve.at(10.), 0.);
        for x in 10..=20 {
            assert!(curve.at(x as f32).abs() < 1e-4, "flat between equal points");
        }
        for x in 0..=30 {
            let y = curve.at(x as f32);
            assert!((0. ..=10.).contains(&y));
        }
    }
}
