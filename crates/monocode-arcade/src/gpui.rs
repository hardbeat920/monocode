//! Two-game arcade presentation with injected host styling, icons and clock.
//!
//! The host must call `set_visible(false, ...)` before hiding/unmounting a
//! retained view. This releases input/focus and stops renewing animation demand.
//! Remount the same entity with `set_visible(true, ...)` to resume its board
//! without catching up hidden time. A view belongs to one window at a time.
use crate::{
    controller::{Configuration, Controller},
    registry::{step_slider, GameId, GAMES, SLIDE_HOLD_MS},
    types::{Cell, Mode, Sprite, SpriteKind},
};
use gpui::{
    actions, canvas, div, fill, point, prelude::*, px, size, AnyElement, App, Bounds, Context,
    FocusHandle, Focusable, Hsla, KeyBinding, Render, Role, SharedString, WeakFocusHandle, Window,
};
use monocode_mascots::gpui::{paint_mask, AnimationClock, PixelPolicy};
use std::{cell::RefCell, rc::Rc, time::Duration};

const TICK: Duration = Duration::from_millis(33);
const CELL: f32 = 7.;

actions!(arcade, [ReleaseControl]);

/// Register once before mounting arcade views.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", ReleaseControl, Some("Arcade"))]);
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub foreground: Hsla,
    pub surface: Hsla,
    pub mono_font: SharedString,
}
impl Style {
    fn ink(&self, alpha: f32) -> Hsla {
        let mut ink = self.foreground;
        ink.a *= alpha;
        ink
    }
}

/// The host retains provider identity, branding, assets and fallback policy.
pub type ProviderIcon = Rc<dyn Fn(&str, f32, &mut Window, &mut App) -> AnyElement>;

#[derive(Clone)]
pub struct Services {
    pub clock: Rc<dyn AnimationClock>,
    pub provider_icon: ProviderIcon,
}

pub struct ArcadeView {
    controller: Rc<RefCell<Controller>>,
    services: Services,
    style: Style,
    focus: FocusHandle,
    previous: Option<WeakFocusHandle>,
    fallback: Option<WeakFocusHandle>,
    visible: bool,
    playing: bool,
    mode: Mode,
    dimensions: Option<(usize, usize)>,
    last: Option<Duration>,
    selected: GameId,
    switched: Option<Duration>,
    slide_direction: i32,
}

impl ArcadeView {
    pub fn new(
        providers: Vec<String>,
        seed: u32,
        style: Style,
        services: Services,
        fallback: Option<WeakFocusHandle>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            controller: Rc::new(RefCell::new(Controller::new(providers, seed))),
            services,
            style,
            focus: cx.focus_handle(),
            previous: None,
            fallback,
            visible: true,
            playing: false,
            mode: Mode::Mid,
            dimensions: None,
            last: None,
            selected: GameId::Pacman,
            switched: None,
            slide_direction: 1,
        }
    }

    pub fn select_game(&mut self, game: GameId, cx: &mut Context<Self>) {
        self.selected = game;
        self.switched = Some(self.services.clock.elapsed(cx));
        cx.notify();
    }

    pub fn configure_style(&mut self, style: Style, cx: &mut Context<Self>) {
        if self.style != style {
            self.style = style;
            cx.notify();
        }
    }

    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
    }

    pub fn take_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        if !self.focus.is_focused(window) {
            self.previous = window.focused(cx).map(|handle| handle.downgrade());
        }
        self.playing = true;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn release_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.playing = false;
        // Release now, even if the host immediately unmounts us and no render
        // follows. Both games retain their original release/reset semantics.
        self.controller.borrow_mut().release_control();
        self.switched = Some(self.services.clock.elapsed(cx));
        self.restore_focus(window, cx);
        cx.notify();
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.previous.take();
        // Closing a background view must not steal focus from a newer target.
        if self.focus.is_focused(window) {
            if let Some(target) = previous
                .and_then(|handle| handle.upgrade())
                .or_else(|| self.fallback.as_ref().and_then(WeakFocusHandle::upgrade))
            {
                window.focus(&target, cx);
            } else {
                window.blur(cx);
            }
        }
    }

    pub fn set_visible(&mut self, visible: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        if !visible {
            // Hiding pauses the board; only an explicit release resets a game.
            self.restore_focus(window, cx);
        }
        self.visible = visible;
        if visible && self.playing {
            self.take_control(window, cx);
        }
        self.dimensions = None; // Remeasure on remount, even under reduced motion.
        self.last = None; // Hidden wall time is never replayed as simulation dt.
        self.switched = None;
        cx.notify();
    }

    fn configure_frame(&self) {
        if let Some((cols, rows)) = self.dimensions {
            self.controller.borrow_mut().configure(Configuration {
                cols,
                rows,
                playing: self.playing,
                game: self.selected,
                mode: self.mode,
            });
        }
    }

    fn update_frame(&mut self, now: Duration, reduced: bool) {
        if self.dimensions.is_none() {
            return;
        }
        let elapsed = self.last.map(|last| now.saturating_sub(last));
        let switched = *self.switched.get_or_insert(now);
        if !self.playing && now.saturating_sub(switched) > Duration::from_millis(SLIDE_HOLD_MS) {
            let (index, direction) =
                step_slider(self.selected.index(), self.slide_direction, GAMES.len());
            self.selected = GAMES[index].id;
            self.slide_direction = direction;
            self.switched = Some(now);
        }
        self.configure_frame();
        if reduced && !self.playing {
            self.controller.borrow_mut().reveal();
        }
        // Sample each clock slot once. Measuring 33ms from the last paint can
        // skip a whole clock wakeup when display timing varies slightly.
        if self.last.is_none_or(|last| {
            now.as_nanos() / TICK.as_nanos() != last.as_nanos() / TICK.as_nanos()
        }) {
            // First draw/remount only configures: even step(0) can mutate a
            // board. Hidden time and reduced-motion idle time are not replayed.
            if let Some(elapsed) = elapsed.filter(|_| !reduced || self.playing) {
                let dt = f64::from(elapsed.as_secs_f32().min(0.1) * 1000.);
                self.controller.borrow_mut().step(dt);
            }
            self.last = Some(now);
        }
    }
}

impl Focusable for ArcadeView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ArcadeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        let now = self.services.clock.elapsed(cx);
        let reduced = cx.reduce_motion();
        self.update_frame(now, reduced);
        self.services.clock.request(self.playing, window, cx);
        let playing = self.playing;
        let mode = self.mode;
        let controller = self.controller.clone();
        let style = self.style.clone();
        let measured = cx.entity().downgrade();
        let mut root = div()
            .id("empty-session-arcade")
            .debug_selector(|| "empty-session-arcade".into())
            .group("empty-session-arcade")
            .relative()
            .w_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .aria_label(self.selected.descriptor().play_label)
            .text_color(self.style.foreground)
            .font_family(self.style.mono_font.clone())
            .when(playing, |d| {
                d.h_full()
                    .bg(self.style.surface)
                    .occlude()
                    .key_context("Arcade")
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.take_control(window, cx);
                        }),
                    )
                    .on_action(cx.listener(|this, _: &ReleaseControl, window, cx| {
                        this.release_control(window, cx);
                        cx.stop_propagation();
                    }))
            })
            .when(!playing, |d| d.h(px(192.)))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let dimensions = grid_size(bounds);
                        let _ = measured.update(cx, |view, cx| {
                            if view.dimensions != Some(dimensions) {
                                view.dimensions = Some(dimensions);
                                // Populate the shared paint buffer in this frame,
                                // even if reduced motion schedules no next tick.
                                // update_frame deduplicates time advancement.
                                view.update_frame(now, reduced);
                                // Flush layout-dependent overlays after this draw,
                                // when the cached entity's paint is no longer being
                                // marked clean by the current frame.
                                cx.defer_in(window, |_, _, cx| cx.notify());
                            }
                        });
                    },
                    move |bounds, _, window, _| {
                        let controller = controller.borrow();
                        let frame = controller.frame();
                        if frame.cols == 0 {
                            return;
                        }
                        let h = bounds.size.height.as_f32();
                        let opacity = |y: f32| {
                            if playing {
                                1.
                            } else {
                                (1. - ((y / h - 0.65) / 0.35).clamp(0., 1.)).max(0.)
                            }
                        };
                        // Cells within each pass never overlap (6px on a 7px
                        // grid). Reserve their ordering once, rather than run
                        // the scene's bounds-tree search for every tiny quad.
                        // Keep separate layers: the stamp overlays the grid.
                        let (background_cols, background_rows) = background_grid_size(bounds);
                        window.paint_layer(bounds, |window| {
                            for y in 0..background_rows {
                                let alpha = 0.06 * frame.fade as f32 * opacity(y as f32 * CELL);
                                if alpha > 0.005 {
                                    let ink = style.ink(alpha);
                                    for x in 0..background_cols {
                                        window.paint_quad(fill(cell_bounds(bounds, x, y), ink));
                                    }
                                }
                            }
                        });
                        let dim = frame.dim as f32;
                        window.paint_layer(bounds, |window| {
                            for (y, row) in frame.stamp.chunks_exact(frame.cols).enumerate() {
                                let row_opacity = opacity(y as f32 * CELL);
                                for (x, alpha) in row.iter().enumerate() {
                                    // Retain the old host's sparse Float32 cutoff.
                                    if *alpha <= 0.02 {
                                        continue;
                                    }
                                    window.paint_quad(fill(
                                        cell_bounds(bounds, x, y),
                                        style.ink(alpha * 0.72 * dim * row_opacity),
                                    ));
                                }
                            }
                        });
                        for sprite in frame.sprites {
                            paint_sprite(sprite, bounds, &style, dim, &opacity, window);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if !this.playing
                    || event.keystroke.modifiers.platform
                    || event.keystroke.modifiers.control
                    || event.keystroke.modifiers.alt
                {
                    return;
                }
                let (x, y) = match event.keystroke.key.as_str() {
                    "up" | "w" => (0, -1),
                    "down" | "s" => (0, 1),
                    "left" | "a" => (-1, 0),
                    "right" | "d" => (1, 0),
                    _ => return,
                };
                // Synchronize control state without stepping past this input.
                // Each key reaches the engine immediately, including reversals.
                this.configure_frame();
                this.controller.borrow_mut().steer(Cell::new(x, y));
                cx.stop_propagation();
                cx.notify();
            }));
        let controller = self.controller.borrow();
        let frame = controller.frame();
        let descriptor = self.selected.descriptor();
        let dim = frame.dim as f32;
        if let Some(pickup) = frame.pickup {
            let mark = (self.services.provider_icon)(
                pickup.provider,
                pickup.cells as f32 * CELL - 1.,
                window,
                cx,
            );
            root = root.child(
                div()
                    .absolute()
                    .left(px(pickup.x as f32 * CELL))
                    .top(px(pickup.y as f32 * CELL))
                    .opacity(pickup.alpha as f32 * 0.7 * dim)
                    .child(mark),
            );
        }
        if let Some(bubble) = frame.bubble {
            root = root.child(
                div()
                    .absolute()
                    .left(px((bubble.x as f32 * CELL).max(4.)))
                    .top(px((bubble.y as f32 * CELL - 24.).max(4.)))
                    .text_size(px(9.))
                    .text_color(self.style.ink(bubble.alpha as f32 * 0.9 * dim))
                    .child(bubble.text),
            );
        }
        if playing {
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
                    .child(div().child(format!(
                        "score {} {}",
                        frame.score,
                        if descriptor.lives {
                            "•".repeat(frame.lives as usize)
                        } else {
                            String::new()
                        }
                    )))
                    .child(div().flex().gap_1().children(
                        [(Mode::Low, "low"), (Mode::Mid, "mid"), (Mode::Hard, "hard")].map(
                            |(m, label)| {
                                div()
                                    .id(label)
                                    .role(Role::Button)
                                    .aria_label(format!("Difficulty {label}"))
                                    .px_2()
                                    .py_1()
                                    .border_1()
                                    .border_color(self.style.ink(if mode == m { 0.4 } else { 0.1 }))
                                    .child(label)
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.set_mode(m, cx)),
                                    )
                            },
                        ),
                    ))
                    .child(
                        div()
                            .id("release-arcade")
                            .debug_selector(|| "release-arcade".into())
                            .role(Role::Button)
                            .aria_label("Release game")
                            .cursor_pointer()
                            .child("esc · release")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.release_control(window, cx)),
                            ),
                    ),
            );
        } else {
            root = root.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .id("play-arcade")
                            .debug_selector(|| "play-arcade".into())
                            .role(Role::Button)
                            .aria_label(format!("Take control of {}", descriptor.label))
                            .cursor_pointer()
                            .text_size(px(11.))
                            .opacity(0.)
                            .group_hover("empty-session-arcade", |style| style.opacity(1.))
                            .child(format!("[ take control · {} ]", descriptor.label))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.take_control(window, cx)),
                            ),
                    ),
            );
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(6.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .gap_1()
                    .children(GAMES.iter().map(|game| {
                        let id = game.id;
                        let selected = id == self.selected;
                        div()
                            .id(("game", id.index()))
                            .role(Role::Button)
                            .aria_label(game.label)
                            .size_4()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(6.)).bg(self.style.ink(if selected {
                                0.45
                            } else {
                                0.15
                            })))
                            .on_click(cx.listener(move |this, _, _, cx| this.select_game(id, cx)))
                    })),
            );
        }
        root.into_any_element()
    }
}

fn grid_size(bounds: Bounds<gpui::Pixels>) -> (usize, usize) {
    let (cols, rows) = background_grid_size(bounds);
    (cols.clamp(7, 400), rows.clamp(5, 250))
}
fn background_grid_size(bounds: Bounds<gpui::Pixels>) -> (usize, usize) {
    // Only the simulation is capped; the decoration covers the full view.
    (
        (bounds.size.width.as_f32() / CELL).ceil() as usize,
        (bounds.size.height.as_f32() / CELL).ceil() as usize,
    )
}
fn cell_bounds(bounds: Bounds<gpui::Pixels>, x: usize, y: usize) -> Bounds<gpui::Pixels> {
    Bounds::new(
        bounds.origin + point(px(x as f32 * CELL), px(y as f32 * CELL)),
        size(px(6.), px(6.)),
    )
}

fn sprite_bounds(sprite: &Sprite, bounds: Bounds<gpui::Pixels>) -> Bounds<gpui::Pixels> {
    let side = sprite.size as f32 * CELL * 0.8;
    Bounds::new(
        bounds.origin
            + point(
                px(sprite.cx as f32 * CELL + 3. - side / 2.),
                px(sprite.cy as f32 * CELL + 3. - side / 2.),
            ),
        size(px(side), px(side)),
    )
}

// Deliberately retains the shipped native renderer: 32-segment Pac-Man,
// transparent mask eye holes, and invisible eaten-ghost bodies (no eye dots).
fn paint_sprite(
    sprite: &Sprite,
    bounds: Bounds<gpui::Pixels>,
    style: &Style,
    dim: f32,
    opacity: &impl Fn(f32) -> f32,
    window: &mut Window,
) {
    let area = sprite_bounds(sprite, bounds);
    let center = area.center();
    let alpha = sprite.alpha as f32 * dim * opacity(sprite.cy as f32 * CELL + 3.);
    match sprite.kind {
        SpriteKind::Pacman => {
            let mut path = gpui::PathBuilder::fill();
            path.move_to(center);
            for offset in pacman_outline(sprite) {
                path.line_to(center + offset);
            }
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, style.ink(alpha * 0.92));
            }
        }
        SpriteKind::Ghost if !sprite.eyes => {
            if let Some(id) = sprite.mascot {
                paint_mask(
                    &id.mascot().frame(sprite.talk).rows,
                    area,
                    style.ink(alpha * 0.8),
                    PixelPolicy::Logical,
                    window,
                );
            }
        }
        SpriteKind::Ghost => {}
    }
}

fn pacman_outline(sprite: &Sprite) -> [gpui::Point<gpui::Pixels>; 33] {
    let radius = sprite.size as f32 * CELL * 0.8 / 2.;
    let angle = (sprite.dy as f32).atan2(sprite.dx as f32);
    let mouth = sprite.mouth as f32 * std::f32::consts::PI;
    std::array::from_fn(|i| {
        let a = angle + mouth + (std::f32::consts::TAU - 2. * mouth) * i as f32 / 32.;
        point(px(a.cos() * radius), px(a.sin() * radius))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_covers_the_view_while_simulation_dimensions_stay_capped() {
        for (width, height, background, simulation) in [
            (1., 1., (1, 1), (7, 5)),
            (700., 192., (100, 28), (100, 28)),
            (2800., 1750., (400, 250), (400, 250)),
            (2801., 1751., (401, 251), (400, 250)),
            (3500., 2100., (500, 300), (400, 250)),
        ] {
            let bounds = Bounds::new(point(px(13.), px(17.)), size(px(width), px(height)));
            assert_eq!(background_grid_size(bounds), background, "{width}x{height}");
            assert_eq!(grid_size(bounds), simulation, "{width}x{height}");
            let last = cell_bounds(bounds, background.0 - 1, background.1 - 1);
            assert!(last.left() < bounds.right() && last.top() < bounds.bottom());
            assert!(last.left() + px(CELL) >= bounds.right());
            assert!(last.top() + px(CELL) >= bounds.bottom());
        }
    }
}
