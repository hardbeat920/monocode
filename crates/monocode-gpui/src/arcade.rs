//! React's Pac-man and Snake engines, painted with GPUI rather than a web view.
use crate::{popover::AnchorSlot, theme::*};
use gpui::{
    App, Context, FocusHandle, Focusable, Render, Role, Window, canvas, div, fill, point,
    prelude::*, px, size,
};
use serde::Deserialize;
use std::time::{Duration, Instant};
#[derive(Clone, Default, Deserialize)]
struct Frame {
    cells: Vec<(usize, f32)>,
    sprites: Vec<Sprite>,
    pickup: Option<Pickup>,
    bubble: Option<Bubble>,
    score: u32,
    lives: u32,
    dim: f32,
    fade: f32,
}
#[derive(Clone, Deserialize)]
struct Sprite {
    kind: String,
    cx: f32,
    cy: f32,
    size: f32,
    dx: f32,
    dy: f32,
    alpha: f32,
    mouth: f32,
    rows: Option<Vec<String>>,
    eyes: Option<bool>,
}
#[derive(Clone, Deserialize)]
struct Pickup {
    harness: String,
    x: f32,
    y: f32,
    cells: f32,
    alpha: f32,
}
#[derive(Clone, Deserialize)]
struct Bubble {
    text: String,
    x: f32,
    y: f32,
    alpha: f32,
}
pub(crate) struct Arcade {
    context: rquickjs::Context,
    _runtime: rquickjs::Runtime,
    frame: Frame,
    anchor: AnchorSlot,
    focus: FocusHandle,
    previous: Option<FocusHandle>,
    playing: bool,
    index: usize,
    mode: &'static str,
    heading: (i32, i32),
    last: Instant,
    switched: Instant,
}
impl Arcade {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let runtime = rquickjs::Runtime::new().expect("arcade runtime");
        runtime.set_memory_limit(8 * 1024 * 1024);
        let context = rquickjs::Context::full(&runtime).expect("arcade context");
        context
            .with(|cx| cx.eval::<(), _>(include_str!("../assets/settings-arcade.js")))
            .expect("arcade engines");
        Self {
            context,
            _runtime: runtime,
            frame: Frame::default(),
            anchor: Default::default(),
            focus: cx.focus_handle(),
            previous: None,
            playing: false,
            index: 0,
            mode: "mid",
            heading: (0, 0),
            last: Instant::now(),
            switched: Instant::now(),
        }
    }
    fn tick(&mut self, reduced: bool) {
        let now = Instant::now();
        if now.duration_since(self.last) < Duration::from_millis(33) && !self.frame.cells.is_empty()
        {
            return;
        }
        if !self.playing && now.duration_since(self.switched) > Duration::from_secs(16) {
            self.index = 1 - self.index;
            self.switched = now;
        }
        let bounds = self.anchor.get().unwrap_or_default();
        let cols = (bounds.size.width.as_f32() / 7.).ceil().clamp(7., 400.) as usize;
        let rows = (bounds.size.height.as_f32() / 7.).ceil().clamp(5., 250.) as usize;
        let dt = if reduced && !self.playing {
            0.
        } else {
            now.duration_since(self.last).as_secs_f32().min(0.1) * 1000.
        };
        self.last = now;
        let (dx, dy) = std::mem::take(&mut self.heading);
        let result = self.context.with(|cx| {
            let object: rquickjs::Object = cx.globals().get("NativeArcade")?;
            let f: rquickjs::Function = object.get("frame")?;
            f.call::<_, String>((
                cols,
                rows,
                dt,
                self.playing,
                self.index,
                self.mode,
                vec![dx, dy],
            ))
        });
        if let Ok(json) = result
            && let Ok(frame) = serde_json::from_str(&json)
        {
            self.frame = frame;
        }
    }
    fn release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.playing = false;
        self.switched = Instant::now();
        if let Some(focus) = self.previous.take() {
            window.focus(&focus, cx);
        }
        cx.notify();
    }
}
impl Focusable for Arcade {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for Arcade {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.tick(cx.reduce_motion());
        // The engine steps at most every 33ms, so repaint on the shared 30fps
        // clock. A game in play keeps running under reduced motion.
        if !crate::pulse::request(crate::pulse::Rate::Smooth, window, cx) && self.playing {
            window.request_animation_frame();
        }
        let frame = self.frame.clone();
        let playing = self.playing;
        let index = self.index;
        let mode = self.mode;
        let mut root = div()
            .id("empty-session-arcade")
            .group("empty-session-arcade")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .overflow_hidden()
            .track_focus(&self.focus)
            .when(playing, |d| d.h_full().bg(shell_fill()).occlude())
            .when(!playing, |d| d.h(px(192.)))
            .child(self.anchor.layout_probe())
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if !this.playing
                    || event.keystroke.modifiers.platform
                    || event.keystroke.modifiers.control
                    || event.keystroke.modifiers.alt
                {
                    return;
                }
                let heading = match event.keystroke.key.as_str() {
                    "up" | "w" => (0, -1),
                    "down" | "s" => (0, 1),
                    "left" | "a" => (-1, 0),
                    "right" | "d" => (1, 0),
                    "escape" => {
                        this.release(window, cx);
                        cx.stop_propagation();
                        return;
                    }
                    _ => return,
                };
                this.heading = heading;
                cx.stop_propagation();
                cx.notify();
            }))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let cols = (bounds.size.width.as_f32() / 7.).ceil().max(7.) as usize;
                        let h = bounds.size.height.as_f32();
                        let opacity = |y: f32| {
                            if playing {
                                1.
                            } else {
                                (1. - ((y / h - 0.65) / 0.35).clamp(0., 1.)).max(0.)
                            }
                        };
                        for y in 0..(h / 7.).ceil() as usize {
                            for x in 0..cols {
                                let a = 0.06 * frame.fade * opacity(y as f32 * 7.);
                                if a > 0.005 {
                                    window.paint_quad(fill(
                                        gpui::Bounds::new(
                                            bounds.origin
                                                + point(px(x as f32 * 7.), px(y as f32 * 7.)),
                                            size(px(6.), px(6.)),
                                        ),
                                        content(a),
                                    ));
                                }
                            }
                        }
                        for (index, alpha) in &frame.cells {
                            let x = index % cols;
                            let y = index / cols;
                            window.paint_quad(fill(
                                gpui::Bounds::new(
                                    bounds.origin + point(px(x as f32 * 7.), px(y as f32 * 7.)),
                                    size(px(6.), px(6.)),
                                ),
                                content(alpha * 0.72 * frame.dim * opacity(y as f32 * 7.)),
                            ));
                        }
                        for sprite in &frame.sprites {
                            let side = sprite.size * 7. * 0.8;
                            let x = sprite.cx * 7. + 3.;
                            let y = sprite.cy * 7. + 3.;
                            let alpha = sprite.alpha * frame.dim * opacity(y);
                            if sprite.kind == "pacman" {
                                let angle = sprite.dy.atan2(sprite.dx);
                                let mouth = sprite.mouth * std::f32::consts::PI;
                                let mut path = gpui::PathBuilder::fill();
                                let center = bounds.origin + point(px(x), px(y));
                                path.move_to(center);
                                for i in 0..=32 {
                                    let a = angle
                                        + mouth
                                        + (std::f32::consts::TAU - 2. * mouth) * i as f32 / 32.;
                                    path.line_to(
                                        center
                                            + point(
                                                px(a.cos() * side / 2.),
                                                px(a.sin() * side / 2.),
                                            ),
                                    );
                                }
                                path.close();
                                if let Ok(path) = path.build() {
                                    window.paint_path(path, content(alpha * 0.92));
                                }
                            } else if let Some(rows) = &sprite.rows {
                                let unit = side / 8.;
                                for (iy, row) in rows.iter().enumerate() {
                                    for (ix, pixel) in row.bytes().enumerate() {
                                        if pixel == b'#' && !sprite.eyes.unwrap_or(false) {
                                            window.paint_quad(fill(
                                                gpui::Bounds::new(
                                                    bounds.origin
                                                        + point(
                                                            px(x - side / 2. + ix as f32 * unit),
                                                            px(y - side / 2. + iy as f32 * unit),
                                                        ),
                                                    size(px(unit), px(unit)),
                                                ),
                                                content(alpha * 0.8),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
        if let Some(pickup) = &self.frame.pickup {
            root = root.child(
                div()
                    .absolute()
                    .left(px(pickup.x * 7.))
                    .top(px(pickup.y * 7.))
                    .opacity(pickup.alpha * 0.7 * self.frame.dim)
                    .child(provider_icon(&pickup.harness, pickup.cells * 7. - 1.)),
            );
        }
        if let Some(bubble) = &self.frame.bubble {
            root = root.child(
                div()
                    .absolute()
                    .left(px((bubble.x * 7.).max(4.)))
                    .top(px((bubble.y * 7. - 24.).max(4.)))
                    .text_size(px(9.))
                    .font_family(crate::fonts::mono())
                    .text_color(content(bubble.alpha * 0.9 * self.frame.dim))
                    .child(bubble.text.clone()),
            );
        }
        if playing {
            let difficulties = div()
                .flex()
                .gap_1()
                .children(["low", "mid", "hard"].map(|m| {
                    div()
                        .id(m)
                        .role(Role::Button)
                        .aria_label(format!("Difficulty {m}"))
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(content(if mode == m { 0.4 } else { 0.1 }))
                        .child(m)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.mode = m;
                            cx.notify();
                        }))
                }));
            let release = div()
                .id("release-arcade")
                .role(Role::Button)
                .aria_label("Release game")
                .cursor_pointer()
                .child("esc · release")
                .on_click(cx.listener(|this, _, window, cx| this.release(window, cx)));
            root = root.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .p_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.))
                    .font_family(crate::fonts::mono())
                    .child(format!(
                        "score {} {}",
                        self.frame.score,
                        if index == 0 {
                            "•".repeat(self.frame.lives as usize)
                        } else {
                            String::new()
                        }
                    ))
                    .child(difficulties)
                    .child(release),
            );
        } else {
            let play = div()
                .id("play-arcade")
                .role(Role::Button)
                .aria_label(if index == 0 {
                    "Take control of Pac-man"
                } else {
                    "Take control of Snake"
                })
                .opacity(0.)
                .group_hover("empty-session-arcade", |s| s.opacity(1.))
                .cursor_pointer()
                .font_family(crate::fonts::mono())
                .text_size(px(11.))
                .child(if index == 0 {
                    "[ take control · pac-man ]"
                } else {
                    "[ take control · snake ]"
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.previous = window.focused(cx);
                    this.playing = true;
                    window.focus(&this.focus, cx);
                    cx.notify();
                }));
            let games = div()
                .absolute()
                .bottom(px(6.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .gap_1()
                .children([0, 1].map(|i| {
                    div()
                        .id(("game", i))
                        .role(Role::Button)
                        .aria_label(if i == 0 { "pac-man" } else { "snake" })
                        .size_4()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(6.))
                                .bg(content(if index == i { 0.45 } else { 0.15 })),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.index = i;
                            this.switched = Instant::now();
                            cx.notify();
                        }))
                }));
            root = root
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(play),
                )
                .child(games);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use gpui::AppContext;
    #[gpui::test]
    fn original_engines_support_both_games(cx: &mut gpui::TestAppContext) {
        let view = cx.new(super::Arcade::new);
        view.update(cx, |a, _| {
            for index in [0usize, 1usize] {
                let result = a
                    .context
                    .with(|cx| {
                        let o: rquickjs::Object = cx.globals().get("NativeArcade")?;
                        let f: rquickjs::Function = o.get("frame")?;
                        f.call::<_, String>((
                            120usize,
                            50usize,
                            33f32,
                            true,
                            index,
                            "mid",
                            vec![0, 0],
                        ))
                    })
                    .unwrap();
                let frame: super::Frame = serde_json::from_str(&result).unwrap();
                assert_eq!(frame.dim, 1.);
                assert!(!frame.cells.is_empty());
            }
        });
    }
}
