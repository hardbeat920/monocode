use crate::theme::*;
use gpui::{Bounds, PathBuilder, canvas, div, fill, point, prelude::*, px, size};

/// ContextMeter.tsx: 14px, 2px stroke, 25% track, clockwise from twelve.
pub(crate) fn ring(ratio: f32) -> impl IntoElement {
    let color = if ratio >= 0.9 {
        gpui::rgb(0xf87171).into()
    } else if ratio >= 0.75 {
        gpui::rgb(0xfbbf24).into()
    } else {
        content(0.45)
    };
    progress_ring(ratio, color)
}

/// The footer's meter: a 20px ring with the percentage inside it.
pub(crate) fn labeled_ring(ratio: f32) -> impl IntoElement {
    let color = if ratio >= 0.9 {
        gpui::rgb(0xf87171).into()
    } else if ratio >= 0.75 {
        gpui::rgb(0xfbbf24).into()
    } else {
        content(0.6)
    };
    div()
        .relative()
        .size(px(20.))
        .flex_shrink_0()
        .child(ring_of(20., ratio, color))
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(8.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .font_features(crate::right_panel::tabular())
                .text_color(content(0.9))
                .child(format!("{:.0}", ratio * 100.)),
        )
}

pub(crate) fn progress_ring(ratio: f32, color: gpui::Hsla) -> impl IntoElement {
    div().size(px(14.)).child(ring_of(14., ratio, color))
}

fn ring_of(side: f32, ratio: f32, color: gpui::Hsla) -> impl IntoElement {
    let radius = side / 2. - 1.;
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let center = bounds.center();
            for (amount, alpha) in [(1., 0.25), (ratio, 1.)] {
                if amount <= 0. {
                    continue;
                }
                let mut path = PathBuilder::stroke(px(2.));
                let steps = (amount * 100.).ceil() as usize;
                for step in 0..=steps {
                    let angle = (step as f32 / steps as f32 * amount) * std::f32::consts::TAU
                        - std::f32::consts::FRAC_PI_2;
                    let p = center + point(px(radius * angle.cos()), px(radius * angle.sin()));
                    if step == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                let mut stroke: gpui::Hsla = color;
                stroke.a *= alpha;
                if amount == 1. {
                    path.close();
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, stroke);
                }
                if alpha == 1. && amount < 1. {
                    for angle in [
                        -std::f32::consts::FRAC_PI_2,
                        amount * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2,
                    ] {
                        let p = center
                            + point(px(radius * angle.cos() - 1.), px(radius * angle.sin() - 1.));
                        window.paint_quad(
                            fill(Bounds::new(p, size(px(2.), px(2.))), stroke).corner_radii(px(1.)),
                        );
                    }
                }
            }
        },
    )
    .size_full()
}
