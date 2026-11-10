//! PixelMascot.tsx / pixelMascots.ts rendered as native quads.
use gpui::{AnyElement, Hsla, canvas, prelude::*, px, rgb, rgba};
use serde::Deserialize;
use std::sync::LazyLock;
#[derive(Deserialize)]
struct Sprite {
    name: String,
    rest: Vec<String>,
    talk: Vec<String>,
    eyes: [Vec<[u32; 2]>; 2],
}
static SPRITES: LazyLock<Vec<Sprite>> =
    LazyLock::new(|| serde_json::from_str(include_str!("sprites.json")).unwrap());
fn tone(mut c: Hsla, t: f32) -> Hsla {
    c.l = if t >= 0. {
        c.l + (1. - c.l) * t
    } else {
        c.l * (1. + t)
    };
    c
}
fn material(name: &str, x: usize, y: usize, base: Hsla) -> Hsla {
    match name {
        "ghost" if y >= 6 => tone(base, -0.08),
        "robot" => match y {
            0 => rgb(0xf0d9b1).into(),
            2 | 3 => tone(base, 0.26),
            4 => tone(base, -0.24),
            _ => base,
        },
        "cat" if (x == 2 || x == 5) && y == 1 => rgb(0xd78f8b).into(),
        "cat" if y <= 1 => tone(base, -0.03),
        "cat" if y == 5 || y == 6 => tone(base, 0.07),
        "crab" if y <= 1 => tone(base, 0.05),
        "mushroom" if y >= 5 => rgb(0xf1e3be).into(),
        "rocket" if y >= 6 => rgb(0xf9c081).into(),
        "rocket" if y == 5 => tone(base, -0.25),
        "dino" if y == 2 && x >= 5 => rgb(0xece0ae).into(),
        "frog" if y == 4 => tone(rgb(0xf5e7ad).into(), -0.03),
        _ => base,
    }
}
pub(crate) fn avatar(name: &str, color: u32, status: &str, side: f32, still: bool) -> AnyElement {
    let index = SPRITES.iter().position(|m| m.name == name).unwrap_or(1);
    let status = status.to_string();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            // "needs-you" holds still, so it must not keep the clock running.
            let working = status == "working";
            let moves = working || (!still && status != "needs-you");
            // Pixel art reads fine at a low frame rate; a working hop is faster.
            let rate = if working {
                crate::pulse::Rate::Half
            } else {
                crate::pulse::Rate::Low
            };
            let animate = moves && crate::pulse::request(rate, window, cx);
            let t = crate::pulse::elapsed(cx).as_secs_f32();
            let talk = animate && status == "working" && (t * 3.).floor() as u64 % 2 == 1;
            let sprite = &SPRITES[index];
            let rows = if talk { &sprite.talk } else { &sprite.rest };
            let unit = bounds.size.width / 16.;
            let hop = if animate {
                if status == "working" {
                    -(t * 9.).sin().abs() * 0.9
                } else if status == "needs-you" {
                    0.
                } else {
                    (t * 2.5).sin() * 0.25
                }
            } else {
                0.
            };
            let draw = |x: f32, y: f32, w: f32, h: f32, color: Hsla, window: &mut gpui::Window| {
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(
                        bounds.origin + gpui::point(unit * x, unit * (y + hop)),
                        gpui::size(unit * w, unit * h),
                    ),
                    color,
                ));
            };
            let mut base: Hsla = rgb(color).into();
            base.s *= 0.55;
            base.l += (0.82 - base.l) * 0.55;
            let filled = |x: isize, y: isize| {
                x >= 0
                    && y >= 0
                    && rows
                        .get(y as usize)
                        .and_then(|s| s.as_bytes().get(x as usize))
                        == Some(&b'#')
            };
            for depth in [true, false] {
                for (y, row) in rows.iter().enumerate() {
                    for (x, pixel) in row.bytes().enumerate() {
                        if pixel != b'#' {
                            continue;
                        }
                        let face = tone(
                            material(&sprite.name, x, y, base),
                            0.04 + (7. - y as f32) * 0.009 + (3. - x as f32) * 0.006,
                        );
                        let offset = if depth { 0.14 } else { 0. };
                        draw(
                            2. + (x as f32 + offset) * 1.5,
                            2. + (y as f32 + offset * 0.66) * 1.5,
                            1.5,
                            1.5,
                            if depth { tone(face, -0.32) } else { face },
                            window,
                        );
                        if !depth {
                            if !filled(x as isize, y as isize - 1) {
                                draw(
                                    2. + x as f32 * 1.5,
                                    2. + y as f32 * 1.5,
                                    1.5,
                                    0.12,
                                    rgba(0xffffed3d).into(),
                                    window,
                                );
                            }
                            if !filled(x as isize - 1, y as isize) {
                                draw(
                                    2. + x as f32 * 1.5,
                                    2. + y as f32 * 1.5,
                                    0.09,
                                    1.5,
                                    rgba(0xffffed21).into(),
                                    window,
                                );
                            }
                        }
                    }
                }
            }
            for &[x, y] in &sprite.eyes[usize::from(talk)] {
                let x = 2. + x as f32 * 1.5;
                let y = 2. + y as f32 * 1.5;
                let blink =
                    animate && status != "needs-you" && (t + index as f32 * 0.37) % 5.2 > 5.05;
                if blink {
                    draw(x, y + 0.825, 1.5, 0.375, rgb(0x263331).into(), window);
                } else {
                    draw(
                        x,
                        y,
                        1.5,
                        1.5,
                        rgb(if sprite.name == "rocket" {
                            0x243e51
                        } else {
                            0x263331
                        })
                        .into(),
                        window,
                    );
                    draw(
                        x + 0.225,
                        y + 0.225,
                        0.375,
                        0.375,
                        rgba(0xfffff4bf).into(),
                        window,
                    );
                }
            }
            if sprite.name == "mushroom" {
                for (x, y, w, h) in [
                    (3., 0.42, 0.6, 0.35),
                    (1.65, 1.4, 0.5, 0.38),
                    (5.5, 1.7, 0.35, 0.4),
                ] {
                    draw(
                        2. + x * 1.5,
                        2. + (y + if talk { 1. } else { 0. }) * 1.5,
                        w * 1.5,
                        h * 1.5,
                        rgba(0xfff8e2a8).into(),
                        window,
                    );
                }
            }
            if status == "needs-you" {
                draw(12.5, -2.5, 3., 3., rgb(0xffd24a).into(), window);
                draw(13.25, 0.5, 1.5, 1., rgb(0x8c8680).into(), window);
            }
        },
    )
    .size(px(side))
    .flex_shrink_0()
    .into_any_element()
}
