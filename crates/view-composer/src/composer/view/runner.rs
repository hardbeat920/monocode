//! Port of src/features/sessions/ui/ComposerRunner.tsx: the project's pixel
//! mascot patrols the composer's top ledge while a turn is live, grabs
//! coins, bonks the jump-to-latest chevron once, and hops off when the turn
//! ends. The motion math is `model::runner`.
//!
//! The React version read layout every 100ms and drove the sprite with a
//! `requestAnimationFrame` loop. Here the entity requests a frame while it
//! is live and reads the composer box bounds the last frame recorded.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    App, Bounds, ContentMask, Context, Hsla, IntoElement, ParentElement as _, Pixels, Render,
    Styled as _, WeakEntity, Window, canvas, deferred, div, fill, point, px, size,
};
use monocode_ui::color::{hex, with_alpha};

use super::super::model::mascots::{
    COIN_EDGE, COIN_FACE, Mascot, STAR_EDGE, STAR_FACE, mascot_rects, project_mascot,
};
use super::super::model::paths::project_name;
use super::super::model::runner::{
    COIN_HOVER, COIN_SIZE, COLLECT_POP_MS, COLLECT_POP_PX, Coin, EXIT_MS, EXIT_SINK, Facing,
    Obstacle, RUNNER_INSET, RUNNER_SIZE, RUNNER_SPEED_PX, Rect, RunnerTrack, STAR_SIZE,
    coin_collected, exit_jump_y, hits_chevron, jump_height, next_coin_delay, obstacle_from_rects,
    pick_coin_x, pose_at, recoil_along, runner_track, scale_track_x, sprite_clip_bottom,
    step_along, stun_done, stun_shake, stun_stars,
};
use super::colors::{COIN, STAR};
use super::{Composer, ComposerProps};

/// Shared bounds the composer records each frame for the runner.
#[derive(Clone, Default)]
pub struct RunnerGeometry {
    /// The composer box.
    pub r#box: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// A control stacked on the box (the queue card), whose top edge the
    /// runner prefers.
    pub ledge: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The jump-to-latest chevron, set by the transcript's owner.
    pub obstacle: Rc<Cell<Option<Bounds<Pixels>>>>,
}

struct LiveCoin {
    coin: Coin,
    collected_at: Option<f32>,
}

/// The sprite, coins, and stars to paint this frame, in window pixels.
#[derive(Default)]
struct Frame {
    sprite: Option<(Bounds<Pixels>, Facing, f32, bool)>,
    coins: Vec<(Bounds<Pixels>, f32)>,
    stars: Vec<(Bounds<Pixels>, f32)>,
}

pub struct ComposerRunner {
    composer: WeakEntity<Composer>,
    pub(crate) geometry: RunnerGeometry,
    mascot: Mascot,
    color: Hsla,
    busy: bool,
    enabled: bool,
    reduced: bool,
    started: Instant,
    last: f32,
    along: f32,
    facing: Facing,
    prev_width: f32,
    coin_id: u32,
    next_coin_at: f32,
    exiting: bool,
    exit_at: f32,
    frozen_x: f32,
    frozen_facing: Facing,
    finished: bool,
    stunning: bool,
    stun_at: f32,
    hit_along: f32,
    hit_facing: Facing,
    learned: bool,
    coins: Vec<LiveCoin>,
    seed: u64,
}

impl ComposerRunner {
    pub fn new(
        composer: WeakEntity<Composer>,
        geometry: RunnerGeometry,
        props: &ComposerProps,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let project = project_name(&props.cwd);
        let mut this = Self {
            composer,
            geometry,
            mascot: project_mascot(&project, props.runner_mascot.as_deref()),
            color: props
                .runner_color
                .unwrap_or_else(|| monocode_ui::Theme::of(cx).colors.content),
            busy: props.busy,
            enabled: props.enabled,
            reduced: props.reduced_motion,
            started: Instant::now(),
            last: 0.,
            along: 0.,
            facing: 1,
            prev_width: 0.,
            coin_id: 0,
            next_coin_at: 0.,
            exiting: false,
            exit_at: 0.,
            frozen_x: 0.,
            frozen_facing: 1,
            finished: false,
            stunning: false,
            stun_at: 0.,
            hit_along: 0.,
            hit_facing: 1,
            learned: props.reduced_motion,
            coins: Vec::new(),
            seed: 0x9e37_79b9_7f4a_7c15,
        };
        let mut random = this.random();
        this.next_coin_at = next_coin_delay(true, &mut random);
        this
    }

    pub fn set_props(&mut self, props: &ComposerProps, cx: &mut Context<Self>) {
        self.busy = props.busy;
        self.enabled = props.enabled;
        self.mascot = project_mascot(&project_name(&props.cwd), props.runner_mascot.as_deref());
        if let Some(color) = props.runner_color {
            self.color = color;
        }
        cx.notify();
    }

    /// A tiny xorshift stand-in for `Math.random`.
    fn random(&mut self) -> impl FnMut() -> f32 + use<> {
        let mut state = self.seed;
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / (1u64 << 24) as f32
        }
    }

    fn exited(&mut self, cx: &mut Context<Self>) {
        self.finished = true;
        self.coins.clear();
        let composer = self.composer.clone();
        cx.defer(move |cx| {
            composer
                .update(cx, |composer, cx| composer.runner_exited(cx))
                .ok();
        });
    }

    /// One animation step: the body of `apply(now)`.
    fn tick(&mut self, window: &Window, cx: &mut Context<Self>) -> Frame {
        let now = self.started.elapsed().as_secs_f32() * 1000.;
        let dt = (now - self.last).min(48.);
        self.last = now;
        let mut frame = Frame::default();
        if !self.enabled {
            self.stunning = false;
            if !self.busy && !self.finished {
                self.exited(cx);
            }
            return frame;
        }
        let Some(r#box) = self.geometry.r#box.get() else {
            return frame;
        };
        let rect = |b: Bounds<Pixels>| Rect {
            left: f32::from(b.left()),
            right: f32::from(b.right()),
            top: f32::from(b.top()),
            bottom: f32::from(b.bottom()),
            width: Some(f32::from(b.size.width)),
        };
        let box_rect = rect(r#box);
        let ledge = self.geometry.ledge.get().map(rect);
        let track: RunnerTrack = runner_track(&box_rect, ledge.as_ref());
        let obstacle: Option<Obstacle> = obstacle_from_rects(
            &Rect {
                left: track.left,
                right: track.left + track.width,
                top: track.top,
                bottom: track.top + 8.,
                width: Some(track.width),
            },
            self.geometry.obstacle.get().map(rect).as_ref(),
        );
        if track.width <= 0. {
            return frame;
        }
        let inset_track = (track.width - RUNNER_INSET * 2.).max(0.);
        if self.prev_width > 0. && self.prev_width != track.width {
            let prev_inset = (self.prev_width - RUNNER_INSET * 2.).max(0.);
            self.along = scale_track_x(self.along, prev_inset, inset_track);
            self.hit_along = scale_track_x(self.hit_along, prev_inset, inset_track);
            self.frozen_x = scale_track_x(self.frozen_x, self.prev_width, track.width);
            for coin in &mut self.coins {
                coin.coin.x = scale_track_x(coin.coin.x, self.prev_width, track.width);
            }
        }
        self.prev_width = track.width;

        if self.busy {
            if self.exiting {
                self.exiting = false;
                self.learned = self.reduced;
                self.stunning = false;
            }
            self.finished = false;
            if !self.reduced && !self.stunning {
                (self.along, self.facing) =
                    step_along(self.along, self.facing, dt, inset_track, RUNNER_SPEED_PX);
            }
        } else if !self.exiting && !self.finished {
            self.exiting = true;
            self.exit_at = now;
            self.stunning = false;
            let current = pose_at(self.along, self.facing, track.width, None, &[]);
            self.frozen_x = current.x;
            self.frozen_facing = current.facing;
            for coin in &mut self.coins {
                coin.collected_at.get_or_insert(now);
            }
        }

        let sprite_at = |x: f32, y: f32, shake: (f32, f32)| {
            let left = (track.left + x - RUNNER_SIZE / 2. + shake.0).round();
            let top = (track.top - RUNNER_SIZE - y + 1. + shake.1).round();
            Bounds::new(
                point(px(left), px(top)),
                size(px(RUNNER_SIZE), px(RUNNER_SIZE)),
            )
        };
        let beat = (now / 460.) as u64 % 2 == 1;

        if self.exiting {
            let t = if self.reduced {
                1.
            } else {
                ((now - self.exit_at) / EXIT_MS).min(1.)
            };
            let y = if self.reduced {
                -EXIT_SINK
            } else {
                exit_jump_y(t)
            };
            frame.sprite = Some((
                sprite_at(self.frozen_x, y, (0., 0.)),
                self.frozen_facing,
                sprite_clip_bottom(y),
                beat,
            ));
            self.coins.retain(|coin| {
                let pop = ((now - coin.collected_at.unwrap_or(now)) / COLLECT_POP_MS).min(1.);
                pop < 1.
            });
            for coin in &self.coins {
                let pop = ((now - coin.collected_at.unwrap_or(now)) / COLLECT_POP_MS).min(1.);
                frame
                    .coins
                    .push((coin_bounds(&track, &coin.coin, 0., pop), 1. - pop));
            }
            if t >= 1. && !self.finished {
                self.exited(cx);
            }
            return frame;
        }

        if self.stunning {
            self.along = recoil_along(
                self.hit_along,
                self.hit_facing,
                now - self.stun_at,
                inset_track,
            );
            self.facing = self.hit_facing;
            if stun_done(now - self.stun_at) {
                self.learned = true;
                self.stunning = false;
            }
        }
        for coin in &mut self.coins {
            if coin.collected_at.is_none()
                && (coin.coin.x < RUNNER_INSET || coin.coin.x > track.width - RUNNER_INSET)
            {
                coin.collected_at = Some(now);
            }
        }
        let live: Vec<Coin> = if self.stunning {
            Vec::new()
        } else {
            self.coins.iter().map(|coin| coin.coin).collect()
        };
        let pose = pose_at(
            self.along,
            self.facing,
            track.width,
            if self.learned {
                obstacle.as_ref()
            } else {
                None
            },
            &live,
        );
        if !self.stunning
            && hits_chevron(pose.x, pose.y, pose.facing, obstacle.as_ref(), self.learned)
        {
            self.stunning = true;
            self.stun_at = now;
            self.hit_along = self.along;
            self.hit_facing = self.facing;
        }
        let shake = if self.stunning {
            stun_shake(now - self.stun_at)
        } else {
            (0., 0.)
        };
        if self.stunning {
            let sprite = sprite_at(pose.x, pose.y, shake);
            for star in stun_stars(now - self.stun_at) {
                frame.stars.push((
                    Bounds::new(
                        point(sprite.origin.x + px(star.dx), sprite.origin.y + px(star.dy)),
                        size(px(STAR_SIZE), px(STAR_SIZE)),
                    ),
                    star.opacity,
                ));
            }
        }
        let has_live = self.coins.iter().any(|coin| coin.collected_at.is_none());
        if !self.reduced && !self.stunning && !has_live && now >= self.next_coin_at {
            let mut random = self.random();
            match pick_coin_x(track.width, pose.x, obstacle.as_ref(), &mut random) {
                Some(x) => {
                    self.coin_id += 1;
                    self.coins.push(LiveCoin {
                        coin: Coin {
                            id: self.coin_id,
                            x,
                            height: COIN_HOVER,
                        },
                        collected_at: None,
                    });
                }
                None => self.next_coin_at = now + 2000.,
            }
        }
        let mut keep = Vec::new();
        for mut coin in std::mem::take(&mut self.coins) {
            if !self.stunning && coin.collected_at.is_none() && coin_collected(&pose, &coin.coin) {
                coin.collected_at = Some(now);
                let mut random = self.random();
                self.next_coin_at = now + next_coin_delay(false, &mut random);
            }
            let bob = if coin.collected_at.is_none() {
                (now / 180.).sin() * 2.
            } else {
                0.
            };
            let pop = coin
                .collected_at
                .map_or(0., |at| ((now - at) / COLLECT_POP_MS).min(1.));
            if pop < 1. {
                frame
                    .coins
                    .push((coin_bounds(&track, &coin.coin, bob, pop), 1. - pop));
            }
            let done = pop >= 1. && jump_height(pose.x, None, &[coin.coin]) <= 0.5;
            if !done {
                keep.push(coin);
            }
        }
        self.coins = keep;
        frame.sprite = Some((
            sprite_at(pose.x, pose.y, shake),
            pose.facing,
            0.,
            beat && !self.stunning,
        ));
        let _ = window;
        frame
    }
}

fn coin_bounds(track: &RunnerTrack, coin: &Coin, bob: f32, pop: f32) -> Bounds<Pixels> {
    let left = (track.left + coin.x - COIN_SIZE / 2.).round();
    let top = (track.top - coin.height - COIN_SIZE / 2. - bob - COLLECT_POP_PX * pop).round();
    Bounds::new(point(px(left), px(top)), size(px(COIN_SIZE), px(COIN_SIZE)))
}

/// Paints an 8x8 grid frame into `bounds`, mirrored when facing left.
fn paint_grid(
    rows: &[&str],
    bounds: Bounds<Pixels>,
    mirror: bool,
    color: Hsla,
    window: &mut Window,
) {
    let cell = bounds.size.width / 8.;
    for (x, y, run) in mascot_rects(rows) {
        let x = if mirror { 8 - x - run } else { x };
        window.paint_quad(fill(
            Bounds::new(
                point(
                    bounds.origin.x + cell * x as f32,
                    bounds.origin.y + cell * y as f32,
                ),
                size(cell * run as f32, cell),
            ),
            color,
        ));
    }
}

impl Render for ComposerRunner {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.tick(window, cx);
        if !self.finished {
            window.request_animation_frame();
        }
        let mascot = self.mascot;
        let color = self.color;
        let shadow = with_alpha(hex(0x000000), 0.45);
        let coin_color = hex(COIN);
        let star_color = hex(STAR);
        let blink = (self.started.elapsed().as_millis() / 320) % 2 == 1;
        let painter = canvas(
            |_, _, _| {},
            move |_, _, window: &mut Window, _: &mut App| {
                for (bounds, opacity) in &frame.coins {
                    let bounds = *bounds;
                    let rows: &[&str] = if blink { &COIN_EDGE } else { &COIN_FACE };
                    let shadow_bounds = Bounds::new(
                        point(bounds.origin.x, bounds.origin.y + px(1.)),
                        bounds.size,
                    );
                    paint_grid(
                        rows,
                        shadow_bounds,
                        false,
                        Hsla {
                            a: shadow.a * opacity,
                            ..shadow
                        },
                        window,
                    );
                    paint_grid(
                        rows,
                        bounds,
                        false,
                        Hsla {
                            a: coin_color.a * opacity,
                            ..coin_color
                        },
                        window,
                    );
                }
                if let Some((bounds, facing, clip, talk)) = frame.sprite {
                    let visible = Bounds::new(
                        bounds.origin,
                        size(
                            bounds.size.width,
                            (bounds.size.height - px(clip)).max(px(0.)),
                        ),
                    );
                    let rows: &[&str] = if talk { &mascot.talk } else { &mascot.rest };
                    let hop = if talk { px(-1.) } else { px(0.) };
                    let drawn =
                        Bounds::new(point(bounds.origin.x, bounds.origin.y + hop), bounds.size);
                    window.with_content_mask(Some(ContentMask { bounds: visible }), |window| {
                        let shadow_bounds =
                            Bounds::new(point(drawn.origin.x, drawn.origin.y + px(1.)), drawn.size);
                        paint_grid(rows, shadow_bounds, facing < 0, shadow, window);
                        paint_grid(rows, drawn, facing < 0, color, window);
                    });
                }
                for (bounds, opacity) in &frame.stars {
                    let bounds = *bounds;
                    let rows: &[&str] = if blink { &STAR_EDGE } else { &STAR_FACE };
                    paint_grid(
                        rows,
                        bounds,
                        false,
                        Hsla {
                            a: star_color.a * opacity,
                            ..star_color
                        },
                        window,
                    );
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(deferred(painter).with_priority(1))
    }
}
