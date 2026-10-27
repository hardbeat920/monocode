//! Charts drawn as dithered cells: bar, area, line, stat, gauge, donut,
//! heatmap and progress.
use super::{
    color,
    dither::{
        Align, CELL, Curve, Grid, Label, Motion, Scale, compact, crosshair, glow, glowing_line,
        hoverable, inked, marker, pointer, readout, swatch,
    },
};
use crate::theme::content;
use gpui::{
    AnyElement, App, Bounds, FontWeight, Hsla, IntoElement, Pixels, SharedString, Window, canvas,
    div, point, prelude::*, px,
};
use monocode_core::visual::{ChartKind, Series, Slice, ThemeColor};

/// Axis type: the shared mono caption.
fn axis() -> Label {
    Label::mono(10., content(0.45))
}

fn faint(color: Hsla, alpha: f32) -> Hsla {
    let mut c = color;
    c.a = alpha;
    c
}

/// Bars and areas fade from their top edge toward the axis.
const BAR_LEVEL: f32 = 0.75;
const BAR_FLOOR: f32 = 0.125;
const BAR_FADE: f32 = 0.7;
const MAX_BAR: f32 = 36.;
const AREA_LEVEL: f32 = 0.625;
/// Area coverage by depth below the line, as a share of the plot height.
const AREA_BANDS: [(f32, f32); 5] = [
    (0.05, 1.),
    (0.13, 0.8),
    (0.25, 0.65),
    (0.4, 0.5),
    (0.6, 0.35),
];
const AREA_REST: f32 = 0.2;

/// A value for a readout: compact, to two decimals at most.
fn reading(value: f64) -> String {
    compact(value, 0.01)
}

/// Clips a curve's points to those left of `x`, ending exactly there.
fn drawn_to(points: &[[f32; 2]], curve: &Curve, x: f32) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = points.iter().copied().filter(|p| p[0] <= x).collect();
    if x > curve.first_x() && x < curve.last_x() {
        out.push([x, curve.at(x)]);
    }
    out
}

pub(super) fn chart(
    kind: ChartKind,
    series: Vec<Series>,
    labels: Vec<String>,
    height: f32,
    motion: Motion,
    id: SharedString,
) -> AnyElement {
    let legend = (series.len() > 1).then(|| {
        div()
            .flex()
            .flex_wrap()
            .gap_x_4()
            .gap_y_1()
            .children(series.iter().map(|s| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .font_family(crate::fonts::mono())
                    .text_color(content(0.6))
                    .child(swatch(color(&s.color)))
                    .child(s.label.clone())
            }))
    });
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            hoverable(id).w_full().h(px(height)).child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                            cartesian(kind, &series, &labels, bounds, motion, window, cx)
                        })
                    },
                )
                .size_full(),
            ),
        )
        .children(legend)
        .into_any_element()
}

fn cartesian(
    kind: ChartKind,
    series: &[Series],
    labels: &[String],
    bounds: Bounds<Pixels>,
    motion: Motion,
    window: &mut Window,
    cx: &mut App,
) {
    let bars = matches!(kind, ChartKind::Bar);
    let count = series
        .iter()
        .map(|s| s.values.len())
        .max()
        .unwrap_or(0)
        .max(labels.len());
    if count == 0 {
        return;
    }
    let values = series.iter().flat_map(|s| s.values.iter().copied());
    let scale = Scale::nice(values, !matches!(kind, ChartKind::Line));
    let ticks = scale.ticks();
    let tick_labels: Vec<String> = ticks.iter().map(|t| scale.label(*t)).collect();
    let longest = tick_labels.iter().map(|l| l.len()).max().unwrap_or(1) as f32;
    let width = bounds.size.width.as_f32();
    let height = bounds.size.height.as_f32();
    let left = (longest * 6.1).ceil() + 10.;
    // Room above the plot for the highlighted bar's value.
    let top = if bars { 16. } else { 8. };
    let right = width - if bars { 0. } else { 6. };
    let grid = Grid::new(
        bounds.origin + point(px(left), px(top)),
        right - left,
        height - top - 22.,
    );
    let plot_w = grid.cols as f32 * CELL;
    let plot_h = grid.rows as f32 * CELL;
    let origin = grid.origin;
    let y_of = |v: f64| scale.depth(v) * plot_h;
    let band = plot_w / count as f32;
    let x_of = |i: usize| {
        if bars {
            band * (i as f32 + 0.5)
        } else if count <= 1 {
            plot_w / 2.
        } else {
            i as f32 * plot_w / (count - 1) as f32
        }
    };

    // Grid lines and ticks.
    for (tick, label) in ticks.iter().zip(&tick_labels) {
        let y = y_of(*tick).round();
        window.paint_quad(gpui::fill(
            Bounds::new(
                origin + point(px(0.), px(y)),
                gpui::size(px(plot_w), px(1.)),
            ),
            content(0.07),
        ));
        axis().paint(
            label,
            origin + point(px(-10.), px(y)),
            Align::Right,
            window,
            cx,
        );
    }
    let stride = (count as f32 / (plot_w / 72.).floor().max(1.))
        .ceil()
        .max(1.) as usize;
    for (i, label) in labels.iter().enumerate().step_by(stride) {
        let align = if bars {
            Align::Center
        } else if i == 0 {
            Align::Left
        } else if i + 1 == count {
            Align::Right
        } else {
            Align::Center
        };
        let label = if label.chars().count() > 12 {
            format!("{}…", label.chars().take(11).collect::<String>())
        } else {
            label.clone()
        };
        axis().paint(
            &label,
            origin + point(px(x_of(i)), px(plot_h + 13.)),
            align,
            window,
            cx,
        );
    }

    let hover = pointer(bounds, window)
        .map(|(x, y)| (x - (origin.x - bounds.origin.x).as_f32(), y))
        .filter(|(x, _)| (0. ..=plot_w).contains(x));
    let title = |i: usize| {
        labels
            .get(i)
            .cloned()
            .unwrap_or_else(|| format!("#{}", i + 1))
    };
    let rows = |i: usize| -> Vec<(Hsla, String, String)> {
        series
            .iter()
            .filter_map(|s| {
                let v = s.values.get(i)?;
                Some((color(&s.color), s.label.clone(), reading(*v)))
            })
            .collect()
    };
    if bars {
        let hovered = hover.map(|(x, _)| ((x / band) as usize).min(count - 1));
        if let Some(i) = hovered {
            // A soft column behind the hovered category.
            window.paint_quad(gpui::fill(
                Bounds::new(
                    origin + point(px(band * i as f32), px(0.)),
                    gpui::size(px(band), px(plot_h)),
                ),
                content(0.04),
            ));
        }
        bar_marks(
            series, count, &scale, &grid, band, hovered, motion, window, cx,
        );
        if let Some(i) = hovered {
            let peak = series
                .iter()
                .filter_map(|s| s.values.get(i))
                .copied()
                .fold(f64::MIN, f64::max);
            readout(
                &title(i),
                &rows(i),
                origin
                    + point(
                        px(band * (i as f32 + 0.5)),
                        px(y_of(peak).min(plot_h - 20.)),
                    ),
                bounds,
                window,
                cx,
            );
        }
        return;
    }
    let points: Vec<Vec<[f32; 2]>> = series
        .iter()
        .map(|s| {
            s.values
                .iter()
                .enumerate()
                .map(|(i, v)| [x_of(i), y_of(*v)])
                .collect()
        })
        .collect();
    let curves: Vec<Curve> = points.iter().map(|p| Curve::new(p)).collect();
    // Lines draw in from the left; areas fill in behind them.
    let reach = motion.at(0., 900., window, cx) * plot_w;
    if matches!(kind, ChartKind::Area) {
        let baseline = y_of(scale.min.max(0.));
        let colors: Vec<Hsla> = series.iter().map(|s| color(&s.color)).collect();
        // A wash of each colour under its area stands in for bloom.
        for (curve, color) in curves.iter().zip(&colors) {
            let mut area: Vec<[f32; 2]> = curve
                .samples(4.)
                .into_iter()
                .filter(|p| p[0] <= reach)
                .collect();
            if let (Some(first), Some(last)) = (area.first().copied(), area.last().copied()) {
                area.push([last[0], baseline]);
                area.push([first[0], baseline]);
                let wash = faint(*color, if super::dither::light() { 0.04 } else { 0.07 });
                super::paint_path(&area, origin, 1., 0., true, wash, window);
            }
        }
        grid.paint(window, |col, row| {
            let (x, y) = Grid::center(col, row);
            if y > baseline || x > reach {
                return None;
            }
            // Later series sit in front, as stacked paper would.
            let (index, curve) = curves
                .iter()
                .enumerate()
                .rev()
                .find(|(_, c)| x >= c.first_x() && x <= c.last_x() && y >= c.at(x))?;
            let depth = (y - curve.at(x)) / plot_h.max(1.);
            let lead = (2. * CELL / plot_h.max(1.)).max(AREA_BANDS[0].0);
            let share = if depth < lead {
                1.
            } else {
                AREA_BANDS
                    .iter()
                    .find(|(d, _)| depth < *d)
                    .map_or(AREA_REST, |(_, s)| *s)
            };
            let tone = (AREA_LEVEL * share * 16.).round() / 16.;
            inked(col, row, tone).then_some(colors[index])
        });
    }
    let hovered = hover.map(|(x, _)| {
        if count <= 1 {
            0
        } else {
            ((x / plot_w) * (count - 1) as f32).round() as usize
        }
    });
    if let Some(i) = hovered {
        crosshair(origin, x_of(i), plot_h, window);
    }
    for (curve, (s, points)) in curves.iter().zip(series.iter().zip(&points)) {
        let c = color(&s.color);
        let samples = drawn_to(&curve.samples(2.), curve, reach);
        glowing_line(&samples, origin, 2., c, window);
        // The end marker rides the head of the line as it draws in.
        let head = if reach >= curve.last_x() {
            points.last().copied()
        } else {
            samples.last().copied()
        };
        if hovered.is_none()
            && let Some(head) = head
        {
            marker(
                window,
                origin + point(px(head[0]), px(head[1])),
                2. * CELL,
                c,
            );
        }
        if let Some(p) = hovered.and_then(|i| points.get(i)) {
            marker(window, origin + point(px(p[0]), px(p[1])), 2. * CELL, c);
        }
    }
    if let Some(i) = hovered {
        let top = points
            .iter()
            .filter_map(|p| p.get(i))
            .map(|p| p[1])
            .fold(plot_h, f32::min);
        readout(
            &title(i),
            &rows(i),
            origin + point(px(x_of(i)), px(top)),
            bounds,
            window,
            cx,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn bar_marks(
    series: &[Series],
    count: usize,
    scale: &Scale,
    grid: &Grid,
    band: f32,
    hovered: Option<usize>,
    motion: Motion,
    window: &mut Window,
    cx: &mut App,
) {
    let plot_h = grid.rows as f32 * CELL;
    let n = series.len().max(1);
    let width_px = ((band * 0.7 - CELL * (n - 1) as f32) / n as f32).min(MAX_BAR);
    let wide = (width_px / CELL).round().max(1.) as i32;
    let group = wide * n as i32 + (n as i32 - 1);
    let row_of = |v: f64| ((scale.depth(v) * plot_h) / CELL).round() as i32;
    let zero = row_of(scale.min.max(0.).min(scale.max));
    let last = count - 1;
    let fade = (grid.rows as f32 * BAR_FADE).max(1.);
    // Each column's bar, if any: (top row, bottom row, series, category).
    let mut columns: Vec<Option<(i32, i32, usize, usize)>> = vec![None; grid.cols.max(0) as usize];
    let mut last_top = None;
    let mut settled = true;
    for i in 0..count {
        // Each group rises from the axis, a beat after the one before.
        let rise = motion.at((i as f32 * 30.).min(360.), 700., window, cx);
        settled &= rise >= 1.;
        let start = ((band * (i as f32 + 0.5)) / CELL - group as f32 / 2.).round() as i32;
        for (j, s) in series.iter().enumerate() {
            let Some(value) = s.values.get(i) else {
                continue;
            };
            let end = zero + ((row_of(*value) - zero) as f32 * rise).round() as i32;
            let (top, bottom) = (end.min(zero), end.max(zero));
            let first = start + j as i32 * (wide + 1);
            for col in first..first + wide {
                if let Some(slot) = columns.get_mut(col as usize) {
                    *slot = Some((top, bottom, j, i));
                }
            }
            if i == last {
                last_top = Some(top);
            }
        }
    }
    let colors: Vec<Hsla> = series.iter().map(|s| color(&s.color)).collect();
    grid.paint(window, |col, row| {
        let (top, bottom, j, i) = columns.get(col as usize).copied().flatten()?;
        if row < top || row >= bottom {
            return None;
        }
        let depth = (row - top) as f32;
        let mut tone = BAR_LEVEL - (BAR_LEVEL - BAR_FLOOR) * (depth / fade).min(1.);
        // The hovered group lights up: a denser dither, never solid.
        if hovered == Some(i) {
            tone = 0.5 + tone * 0.5;
        }
        (row == top || inked(col, row, tone)).then_some(colors[j])
    });
    // With one series, the latest value is written over its bar once it has
    // risen. A group's value would be ambiguous.
    if let ([s], Some(top)) = (series, last_top)
        && settled
        && hovered.is_none()
        && let Some(value) = s.values.get(last)
    {
        Label::mono(10., content(0.9)).paint(
            &compact(*value, scale.step / 10.),
            grid.origin + point(px(band * (last as f32 + 0.5)), px(top as f32 * CELL - 9.)),
            Align::Center,
            window,
            cx,
        );
    }
}

/// The sparkline under a stat: a line over bands that step lighter downward.
pub(super) fn trend(values: Vec<f64>, tint: Hsla, motion: Motion) -> AnyElement {
    const HEIGHT: f32 = 56.;
    const LEVELS: [f32; 4] = [10. / 16., 6. / 16., 3. / 16., 1. / 16.];
    const DEPTH: f32 = 7.;
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            if values.len() < 2 {
                return;
            }
            let grid = Grid::new(bounds.origin, bounds.size.width.as_f32(), HEIGHT);
            let width = grid.cols as f32 * CELL;
            let height = grid.rows as f32 * CELL;
            let low = values.iter().copied().fold(f64::MAX, f64::min);
            let high = values.iter().copied().fold(f64::MIN, f64::max);
            let range = if high > low { high - low } else { 1. };
            let (top, floor) = (6., height * 0.3);
            let points: Vec<[f32; 2]> = values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    [
                        i as f32 / (values.len() - 1) as f32 * width,
                        top + (1. - ((v - low) / range) as f32) * (height - top - floor),
                    ]
                })
                .collect();
            let curve = Curve::new(&points);
            let reach = motion.at(150., 900., window, cx) * width;
            grid.paint(window, |col, row| {
                let (x, y) = Grid::center(col, row);
                let depth = y - curve.at(x);
                if depth < 0. || x > reach {
                    return None;
                }
                let tone = LEVELS[((depth / DEPTH) as usize).min(LEVELS.len() - 1)];
                inked(col, row, tone).then_some(tint)
            });
            let samples = drawn_to(&curve.samples(2.), &curve, reach);
            glowing_line(&samples, grid.origin, 1.5, tint, window);
            if let Some(end) = samples.last() {
                marker(
                    window,
                    grid.origin + point(px(end[0].min(width - CELL)), px(end[1])),
                    2. * CELL,
                    tint,
                );
            }
        },
    )
    .w_full()
    .h(px(HEIGHT))
    .into_any_element()
}

/// A signed change drawn with an arrow in the good or bad colour.
pub(super) fn delta(text: &str, lower_is_better: bool) -> AnyElement {
    let down = text.starts_with('-') || text.starts_with('−');
    let tone = color(if down != lower_is_better {
        &ThemeColor::Negative
    } else {
        &ThemeColor::Positive
    });
    div()
        .flex()
        .items_center()
        .gap_1()
        .flex_shrink_0()
        .text_size(px(12.))
        .font_family(crate::fonts::mono())
        .text_color(tone)
        .child(if down { "▼" } else { "▲" })
        .child(text.to_owned())
        .into_any_element()
}

/// Splits a leading signed change off a stat's detail: "+12.4% vs last
/// month" becomes ("+12.4%", "vs last month").
pub(super) fn split_delta(detail: &str) -> Option<(&str, &str)> {
    let mut chars = detail.chars();
    let sign = chars.next()?;
    if !matches!(sign, '+' | '-' | '−') || !chars.next()?.is_ascii_digit() {
        return None;
    }
    let (delta, rest) = detail.split_once(' ').unwrap_or((detail, ""));
    Some((delta, rest.trim()))
}

/// A radial dial of dithered blocks.
pub(super) fn gauge(
    label: String,
    value: f64,
    min: f64,
    max: f64,
    unit: Option<String>,
    token: ThemeColor,
    motion: Motion,
) -> AnyElement {
    const SEGMENTS: usize = 36;
    const SPAN: f32 = 270.;
    const START: f32 = -135.;
    let ratio = ((value - min) / (max - min)).clamp(0., 1.) as f32;
    let format = move |value: f64| match unit.as_deref() {
        Some("%") => format!("{}%", compact(value, 1.)),
        Some(unit) => format!("{} {unit}", compact(value, 0.1)),
        None => compact(value, 0.1),
    };
    let size = 220f32;
    let outer = size / 2. - 1. - CELL;
    let height = size / 2. + (outer + CELL) * std::f32::consts::FRAC_1_SQRT_2 + 18.;
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let tint = color(&token);
            let size = size.min(bounds.size.width.as_f32());
            let left = ((bounds.size.width.as_f32() - size) / 2.).round();
            let grid = Grid::new(bounds.origin + point(px(left), px(0.)), size, height);
            let center = (size / 2., size / 2.);
            let outer = size / 2. - 1. - CELL;
            let thickness = ((size * 0.12) / CELL).round().max(2.) * CELL;
            let inner = outer - thickness;
            let step = SPAN / SEGMENTS as f32;
            let middle = outer - thickness / 2.;
            let gap = (2. / middle).to_degrees();
            // The blocks fill in one by one as the reading counts up.
            let grow = motion.at(0., 1000., window, cx);
            let filled = ratio * grow * SEGMENTS as f32;
            let reading = format(min + (value - min) * grow as f64);
            let active = (filled - 1e-6).ceil().max(0.) as usize;
            let track = faint(tint, 0.14);
            grid.paint(window, |col, row| {
                let (x, y) = Grid::center(col, row);
                let (dx, dy) = (x - center.0, y - center.1);
                let r = (dx * dx + dy * dy).sqrt();
                if r < inner || r > outer + CELL {
                    return None;
                }
                let angle = dx.atan2(-dy).to_degrees();
                let along = angle - START;
                if !(0. ..SPAN).contains(&along) {
                    return None;
                }
                let index = (along / step) as usize;
                let within = along - index as f32 * step;
                if within < gap / 2. || within > step - gap / 2. {
                    return None;
                }
                let head = active > 0 && index == active - 1;
                // The block at the value steps out a cell, like a needle.
                if r > outer && !head {
                    return None;
                }
                if index >= active {
                    return Some(track);
                }
                if head {
                    let part = (filled - index as f32).clamp(0., 1.);
                    if within > gap / 2. + (step - gap) * part.max(0.15) {
                        return (r <= outer).then_some(track);
                    }
                    return Some(tint);
                }
                let tone = (1. - (active - 1 - index) as f32 / 16.).max(9. / 16.);
                inked(col, row, tone).then_some(tint)
            });
            let at = |dx: f32, dy: f32| grid.origin + point(px(center.0 + dx), px(center.1 + dy));
            Label::sans((size * 0.15).round(), content(1.), FontWeight::SEMIBOLD).paint(
                &reading,
                at(0., -4.),
                Align::Center,
                window,
                cx,
            );
            Label::sans(12., content(0.6), FontWeight::NORMAL).paint(
                &label,
                at(0., size * 0.1 + 6.),
                Align::Center,
                window,
                cx,
            );
            let ends = middle * std::f32::consts::FRAC_1_SQRT_2;
            for (dx, value) in [(-ends, min), (ends, max)] {
                axis().paint(
                    &compact(value, 1.),
                    at(dx, ends + thickness / 2. + 12.),
                    Align::Center,
                    window,
                    cx,
                );
            }
        },
    )
    .w_full()
    .h(px(height))
    .into_any_element()
}

/// Parts of a whole as a dithered ring with a legend.
pub(super) fn donut(
    slices: Vec<Slice>,
    label: Option<String>,
    value: Option<String>,
    motion: Motion,
    id: SharedString,
) -> AnyElement {
    const SIZE: f32 = 184.;
    const FLOOR: f32 = 3. / 16.;
    let total: f64 = slices.iter().map(|s| s.value.max(0.)).sum();
    let total_reading = value.unwrap_or_else(|| {
        let s = format!("{total:.0}");
        if total.abs() >= 1e4 {
            // 106000 → 106,000: the hole has room for the full figure.
            let digits: Vec<char> = s.chars().collect();
            let mut out = String::new();
            for (i, c) in digits.iter().enumerate() {
                if i > 0 && (digits.len() - i).is_multiple_of(3) && c.is_ascii_digit() {
                    out.push(',');
                }
                out.push(*c);
            }
            out
        } else {
            compact(total, 0.1)
        }
    });
    let legend = div()
        .flex_1()
        .min_w(px(140.))
        .max_w(px(320.))
        .flex()
        .flex_col()
        .gap_2()
        .children(slices.iter().map(|s| {
            let share = if total > 0. {
                s.value / total * 100.
            } else {
                0.
            };
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .child(swatch(color(&s.color)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(content(0.8))
                        .child(s.label.clone()),
                )
                .child(
                    div()
                        .font_family(crate::fonts::mono())
                        .text_color(content(0.5))
                        .child(format!("{} · {share:.0}%", compact(s.value, 0.1))),
                )
        }));
    let ring = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let grid = Grid::new(bounds.origin, SIZE, SIZE);
            let half = (grid.cols as f32 * CELL) / 2.;
            let outer = half - 2. * CELL;
            let inner = outer - ((SIZE * 0.17) / CELL).round() * CELL;
            // The ring sweeps round from twelve o'clock.
            let sweep = motion.at(0., 900., window, cx) * std::f32::consts::TAU;
            let angle_of = |dx: f32, dy: f32| {
                let a = dx.atan2(-dy);
                if a < 0. { a + std::f32::consts::TAU } else { a }
            };
            let mut edges = Vec::new();
            let mut running = 0.;
            for s in &slices {
                edges.push((running / total.max(1e-9)) as f32 * std::f32::consts::TAU);
                running += s.value.max(0.);
            }
            let shown = slices.iter().filter(|s| s.value > 0.).count();
            // The hovered slice stays bright and the others dim.
            let hovered = pointer(bounds, window).and_then(|(x, y)| {
                let (dx, dy) = (x - half, y - half);
                let r = (dx * dx + dy * dy).sqrt();
                (r >= inner - CELL && r <= outer + CELL)
                    .then(|| edges.iter().rposition(|e| angle_of(dx, dy) >= *e))
                    .flatten()
            });
            let colors: Vec<Hsla> = slices
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let c = color(&s.color);
                    if hovered.is_some_and(|h| h != i) {
                        faint(c, 0.25)
                    } else {
                        c
                    }
                })
                .collect();
            grid.paint(window, |col, row| {
                let (x, y) = Grid::center(col, row);
                let (dx, dy) = (x - half, y - half);
                let r = (dx * dx + dy * dy).sqrt();
                if r < inner || r > outer {
                    return None;
                }
                let angle = angle_of(dx, dy);
                if angle > sweep {
                    return None;
                }
                let index = edges.iter().rposition(|e| angle >= *e)?;
                let mut outline = r > outer - CELL || r < inner + CELL;
                if shown > 1 {
                    // Distance in cells from the nearest slice edge.
                    let near = edges
                        .iter()
                        .zip(&slices)
                        .filter(|(_, s)| s.value > 0.)
                        .map(|(e, _)| {
                            let d = (angle - e).abs();
                            let d = d.min(std::f32::consts::TAU - d);
                            if d > std::f32::consts::FRAC_PI_2 {
                                f32::MAX
                            } else {
                                r * d.sin() / CELL
                            }
                        })
                        .fold(f32::MAX, f32::min);
                    if near < 0.75 {
                        return None;
                    }
                    outline |= near < 1.75;
                }
                let depth = (outer - r) / (outer - inner);
                let tone = FLOOR + (1. - FLOOR) * (1. - depth).powf(1.4);
                (outline || inked(col, row, tone)).then_some(colors[index])
            });
            let center = grid.origin + point(px(half), px(half));
            // The middle names the hovered slice, else the whole.
            let (caption, figure, share) = match hovered.and_then(|h| slices.get(h)) {
                Some(s) => (
                    Some(s.label.clone()),
                    reading(s.value),
                    Some(format!("{:.0}%", s.value / total.max(1e-9) * 100.)),
                ),
                None => (label.clone(), total_reading.clone(), None),
            };
            if let Some(caption) = &caption {
                let caption: String = caption.to_uppercase().chars().take(18).collect();
                axis().paint(
                    &caption,
                    center - point(px(0.), px(16.)),
                    Align::Center,
                    window,
                    cx,
                );
            }
            let fit = ((inner * 2. - 16.) / (figure.chars().count().max(1) as f32 * 0.62)).min(26.);
            Label::sans(fit.floor(), content(1.), FontWeight::SEMIBOLD).paint(
                &figure,
                center + point(px(0.), px(if caption.is_some() { 4. } else { 0. })),
                Align::Center,
                window,
                cx,
            );
            if let Some(share) = share {
                axis().paint(
                    &share,
                    center + point(px(0.), px(24.)),
                    Align::Center,
                    window,
                    cx,
                );
            }
        },
    )
    .size_full();
    div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_6()
        .child(hoverable(id).size(px(SIZE)).flex_shrink_0().child(ring))
        .child(legend)
        .into_any_element()
}

/// A grid of dithered squares, one Bayer tile each.
pub(super) fn heatmap(
    values: Vec<Vec<f64>>,
    rows: Vec<String>,
    columns: Vec<String>,
    token: ThemeColor,
    motion: Motion,
    id: SharedString,
) -> AnyElement {
    const SQUARE: i32 = 4;
    const PITCH: i32 = 5;
    let width = values.first().map_or(0, Vec::len) as i32;
    let gutter = if rows.is_empty() { 0. } else { 34. };
    let top = if columns.is_empty() { 0. } else { 18. };
    let height = top + (values.len() as i32 * PITCH * CELL as i32) as f32;
    let high = values
        .iter()
        .flatten()
        .copied()
        .fold(0f64, f64::max)
        .max(1e-9);
    let map = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let tint = color(&token);
            let track = content(0.07);
            let grid = Grid::new(
                bounds.origin + point(px(gutter), px(top)),
                bounds.size.width.as_f32() - gutter,
                height - top,
            );
            let pitch = PITCH as f32 * CELL;
            // A narrow heatmap keeps its latest columns, as calendars do.
            let fits = ((grid.cols + PITCH - SQUARE) / PITCH).max(1);
            let skip = (width - fits).max(0);
            // Each column's dither thickens in, left to right.
            let levels: Vec<f32> = (0..fits)
                .map(|c| motion.at(c as f32 * 14., 420., window, cx))
                .collect();
            let hovered = pointer(bounds, window).and_then(|(x, y)| {
                let (x, y) = (x - gutter, y - top);
                if x < 0. || y < 0. {
                    return None;
                }
                let (c, r) = ((x / pitch) as i32, (y / pitch) as i32);
                (c < fits && (r as usize) < values.len()).then_some((c, r))
            });
            grid.paint(window, |col, row| {
                let (x, y) = (col % PITCH, row % PITCH);
                if x >= SQUARE || y >= SQUARE || col / PITCH >= fits {
                    return None;
                }
                let v = *values
                    .get((row / PITCH) as usize)?
                    .get((col / PITCH + skip) as usize)?;
                if v <= 0. {
                    return Some(track);
                }
                let mut tone = (0.25 + 0.75 * (v / high) as f32) * levels[(col / PITCH) as usize];
                // The hovered square lights up, still dithered.
                if hovered == Some((col / PITCH, row / PITCH)) {
                    tone = (tone + 0.3).min(14. / 16.);
                }
                let tone = (tone * 16.).round() / 16.;
                Some(if inked(x, y, tone) { tint } else { track })
            });
            for (i, label) in rows.iter().enumerate() {
                // Every other row, as calendars label Mon, Wed, Fri.
                if rows.len() > 4 && i % 2 == 1 {
                    continue;
                }
                axis().paint(
                    label,
                    grid.origin
                        + point(
                            px(-gutter),
                            px(i as f32 * pitch + SQUARE as f32 * CELL / 2.),
                        ),
                    Align::Left,
                    window,
                    cx,
                );
            }
            // Each new label (a month over its weeks) is written where it
            // starts, when it clears the one before.
            let mut previous: Option<(&String, f32)> = None;
            for (i, label) in columns.iter().enumerate().skip(skip as usize) {
                let x = (i - skip as usize) as f32 * pitch;
                if previous.is_some_and(|(p, at)| p == label || x - at < 36.) {
                    continue;
                }
                previous = Some((label, x));
                axis().paint(
                    label,
                    grid.origin + point(px(x), px(-top / 2.)),
                    Align::Left,
                    window,
                    cx,
                );
            }
            if let Some((c, r)) = hovered
                && let Some(v) = values
                    .get(r as usize)
                    .and_then(|row| row.get((c + skip) as usize))
            {
                let name = [
                    rows.get(r as usize).cloned(),
                    columns.get((c + skip) as usize).cloned(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                let square = SQUARE as f32 * CELL;
                readout(
                    &name,
                    &[(tint, "Value".into(), reading(*v))],
                    grid.origin
                        + point(
                            px(c as f32 * pitch + square),
                            px(r as f32 * pitch + square / 2.),
                        ),
                    bounds,
                    window,
                    cx,
                );
            }
        },
    )
    .size_full();
    hoverable(id)
        .w_full()
        .h(px(height))
        .child(map)
        .into_any_element()
}

/// A dithered progress bar with a solid head.
pub(super) fn progress(
    label: String,
    value: f64,
    max: f64,
    detail: Option<String>,
    token: ThemeColor,
    motion: Motion,
) -> AnyElement {
    let ratio = (value / max).clamp(0., 1.) as f32;
    let bar = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let tint = color(&token);
            let track = faint(tint, 0.14);
            let grid = Grid::new(bounds.origin, bounds.size.width.as_f32(), 3. * CELL);
            let grow = motion.at(0., 800., window, cx);
            let head = (grid.cols as f32 * ratio * grow).round() as i32;
            if head > 0 {
                glow(
                    window,
                    Bounds::new(
                        grid.origin,
                        gpui::size(px(head as f32 * CELL), px(3. * CELL)),
                    ),
                    tint,
                    0.3,
                );
            }
            grid.paint(window, |col, row| {
                if col >= head {
                    return Some(track);
                }
                // A comet: sparse at the start, solid at the head.
                let tone = 0.3 + 0.7 * (col as f32 / head.max(1) as f32).powf(1.5);
                Some(if col >= head - 3 || inked(col, row, tone) {
                    tint
                } else {
                    track
                })
            });
        },
    )
    .w_full()
    .h(px(3. * CELL));
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_baseline()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .child(label),
                )
                .child(
                    div()
                        .font_family(crate::fonts::mono())
                        .text_size(px(12.))
                        .text_color(content(0.55))
                        .child(format!("{:.0}%", ratio * 100.)),
                ),
        )
        .child(bar)
        .children(detail.map(|d| div().text_size(px(12.)).text_color(content(0.55)).child(d)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    #[test]
    fn stat_details_lend_their_leading_change_to_the_delta() {
        assert_eq!(
            super::split_delta("+12.4% vs last month"),
            Some(("+12.4%", "vs last month"))
        );
        assert_eq!(super::split_delta("-3"), Some(("-3", "")));
        assert_eq!(super::split_delta("requests per day"), None);
        assert_eq!(super::split_delta("+ more"), None);
    }
}
