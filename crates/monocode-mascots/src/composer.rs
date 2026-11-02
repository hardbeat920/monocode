//! Composer animation: mechanical native-wrapper state machine and pure motion
//! helpers. Coordinates are local logical pixels, time is f64 milliseconds.
//! The host owns visibility, tick deduplication, palette and obstacle measurement.
//! No JS, GPUI, application services or JSON are needed to run this simulation.
use crate::{
    random::{Mulberry32, Random},
    MascotId,
};

mod motion;
pub use motion::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    pub now_ms: f64,
    pub width: f64,
    pub busy: bool,
    pub obstacle: Option<Obstacle>,
    pub reduced_motion: bool,
    /// None preserves the old unknown-name policy: motion/effects without art.
    pub mascot: Option<MascotId>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CoinVisual {
    pub x: f64,
    pub y: f64,
    pub alpha: f64,
}

/// Direct borrowed output. `pose` is absent only in the never-started idle case;
/// an exit retains its frozen pose even after active becomes false. In particular
/// exit `airborne` stays frozen, matching the shipped wrapper rather than being
/// recalculated from the exit height.
pub struct Frame<'a> {
    pub active: bool,
    pub pose: Option<Pose>,
    pub artwork: Option<&'static crate::Frame>,
    pub coins: &'a [CoinVisual],
    pub stars: &'a [Star],
}

pub struct Composer<R = Mulberry32> {
    random: R,
    along: f64,
    facing: Facing,
    previous_width: f64,
    last_ms: f64,
    was_busy: bool,
    exit_at: Option<f64>,
    frozen: Option<Pose>,
    learned: bool,
    stun_at: Option<f64>,
    hit_along: f64,
    hit_facing: Facing,
    next_coin_at: f64,
    coins: Vec<Coin>,
    input: Option<Input>,
    active: bool,
    pose: Option<Pose>,
    artwork: Option<&'static crate::Frame>,
    live_coins: Vec<CoinVisual>,
    stars: Vec<Star>,
}

impl Composer<Mulberry32> {
    pub fn new(seed: u32) -> Self {
        Self::with_random(Mulberry32::new(seed))
    }
}

impl<R: Random> Composer<R> {
    pub fn with_random(random: R) -> Self {
        Self {
            random,
            along: 0.,
            facing: Facing::Right,
            previous_width: 0.,
            last_ms: 0.,
            was_busy: false,
            exit_at: None,
            frozen: None,
            learned: false,
            stun_at: None,
            hit_along: 0.,
            hit_facing: Facing::Right,
            next_coin_at: f64::INFINITY,
            coins: Vec::new(),
            input: None,
            active: false,
            pose: None,
            artwork: None,
            live_coins: Vec::new(),
            stars: Vec::with_capacity(STAR_COUNT),
        }
    }

    /// The old runnerFrame call order, including the retained state that was in
    /// the bundler rather than composerRunner.ts. Repeated calls are real steps;
    /// the view must not use `advance` for same-tick repaint/configuration.
    pub fn advance(&mut self, input: Input) -> Frame<'_> {
        Self::validate(input);
        let dt = (input.now_ms - self.last_ms).clamp(0., 80.);
        self.last_ms = input.now_ms;
        self.apply_configuration(input);
        let pose = if input.busy && self.exit_at.is_none() {
            Some(self.simulate(input, dt))
        } else {
            None
        };
        self.render(input, pose);
        self.frame()
    }

    /// Geometry, appearance, motion preference and busy edges are processed now,
    /// without a physics step, collision, spawn, collection or cleanup pass.
    /// Busy transitions still consume their original coin-delay draw exactly once.
    pub fn configure(&mut self, input: Input) -> Frame<'_> {
        Self::validate(input);
        if self.input == Some(input) {
            return self.frame();
        }
        self.apply_configuration(input);
        self.render(input, None);
        self.frame()
    }

    /// Host suspension/remount rebases elapsed time without advancing simulation.
    /// Absolute exit/stun/effect timestamps remain under explicit host policy.
    pub fn rebase_time(&mut self, now_ms: f64) {
        assert!(
            now_ms.is_finite() && now_ms >= 0.,
            "time must be finite and nonnegative"
        );
        self.last_ms = now_ms;
    }

    pub fn frame(&self) -> Frame<'_> {
        Frame {
            active: self.active,
            pose: self.pose,
            artwork: self.artwork,
            coins: &self.live_coins,
            stars: &self.stars,
        }
    }

    fn validate(input: Input) {
        assert!(
            input.now_ms.is_finite() && input.now_ms >= 0.,
            "time must be finite and nonnegative"
        );
        assert!(
            input.width.is_finite() && input.width >= 0.,
            "width must be finite and nonnegative"
        );
        if let Some(o) = input.obstacle {
            assert!(
                o.left.is_finite() && o.right.is_finite() && o.height.is_finite(),
                "obstacle must be finite"
            );
        }
    }

    fn apply_configuration(&mut self, input: Input) {
        let track = (input.width - RUNNER_INSET * 2.).max(0.);
        if self.previous_width != 0. && self.previous_width != input.width {
            let old_track = (self.previous_width - RUNNER_INSET * 2.).max(0.);
            self.along = scale_track_x(self.along, old_track, track);
            self.hit_along = scale_track_x(self.hit_along, old_track, track);
            for coin in &mut self.coins {
                coin.x = scale_track_x(coin.x, self.previous_width, input.width);
            }
        }
        self.previous_width = input.width;
        if input.busy && !self.was_busy {
            self.exit_at = None;
            self.stun_at = None;
            self.learned = input.reduced_motion;
            self.next_coin_at = input.now_ms + next_coin_delay(true, &mut self.random);
            self.coins.clear();
        }
        if !input.busy && self.was_busy {
            self.exit_at = Some(input.now_ms);
            self.frozen = Some(pose_at(self.along, self.facing, input.width, None, &[]));
            self.stun_at = None;
            for coin in &mut self.coins {
                if coin.collected_at.is_none() {
                    coin.collected_at = Some(input.now_ms);
                }
            }
        }
        self.was_busy = input.busy;
        self.input = Some(input);
    }

    fn simulate(&mut self, input: Input, dt: f64) -> Pose {
        let track = (input.width - RUNNER_INSET * 2.).max(0.);
        if !input.reduced_motion && self.stun_at.is_none() {
            (self.along, self.facing) =
                step_along(self.along, self.facing, dt, track, RUNNER_SPEED_PX);
        }
        if let Some(stun_at) = self.stun_at {
            self.along = recoil_along(
                self.hit_along,
                self.hit_facing,
                input.now_ms - stun_at,
                track,
            );
            self.facing = self.hit_facing;
            if stun_done(input.now_ms - stun_at) {
                self.learned = true;
                self.stun_at = None;
            }
        }
        let pose = self.current_pose(input);
        if self.stun_at.is_none() && hits_chevron(pose, input.obstacle, self.learned) {
            self.stun_at = Some(input.now_ms);
            self.hit_along = self.along;
            self.hit_facing = self.facing;
        }
        if !input.reduced_motion
            && self.stun_at.is_none()
            && !self.coins.iter().any(|c| c.collected_at.is_none())
            && input.now_ms >= self.next_coin_at
        {
            if let Some(x) = pick_coin_x(input.width, pose.x, input.obstacle, &mut self.random) {
                self.coins.push(Coin {
                    x,
                    height: COIN_HOVER,
                    collected_at: None,
                });
            } else {
                self.next_coin_at = input.now_ms + 2000.;
            }
        }
        // Generate output before retaining/removing landing-arc records, exactly
        // as the wrapper does. A newly spawned coin doesn't affect this pose.
        self.live_coins.clear();
        for coin in &mut self.coins {
            if self.stun_at.is_none() && coin.collected_at.is_none() && coin_collected(pose, *coin)
            {
                coin.collected_at = Some(input.now_ms);
                self.next_coin_at = input.now_ms + next_coin_delay(false, &mut self.random);
            }
            if let Some(visual) = coin_visual(*coin, input.now_ms) {
                self.live_coins.push(visual);
            }
        }
        self.coins.retain(|coin| {
            coin.collected_at
                .is_none_or(|at| input.now_ms - at < COLLECT_POP_MS)
                || jump_height(pose.x, None, std::slice::from_ref(coin)) > 0.5
        });
        pose
    }

    fn current_pose(&self, input: Input) -> Pose {
        pose_at(
            self.along,
            self.facing,
            input.width,
            if self.learned { input.obstacle } else { None },
            if self.stun_at.is_none() {
                &self.coins
            } else {
                &[]
            },
        )
    }

    fn render(&mut self, input: Input, simulated_pose: Option<Pose>) {
        self.stars.clear();
        if !input.busy && self.exit_at.is_none() {
            self.active = false;
            self.pose = None;
            self.artwork = None;
            self.live_coins.clear();
            return;
        }
        if let Some(at) = self.exit_at {
            let t = if input.reduced_motion {
                1.
            } else {
                ((input.now_ms - at) / EXIT_MS).min(1.)
            };
            let mut pose = self.frozen.expect("an exit has a frozen pose");
            pose.y = exit_jump_y(t);
            self.pose = Some(pose);
            self.active = t < 1.;
            self.artwork = input.mascot.map(|id| &id.mascot().rest);
            self.live_coins.clear();
            return;
        }
        let mut pose = simulated_pose.unwrap_or_else(|| self.current_pose(input));
        if simulated_pose.is_none() {
            self.live_coins.clear();
            self.live_coins.extend(
                self.coins
                    .iter()
                    .filter_map(|coin| coin_visual(*coin, input.now_ms)),
            );
        }
        // configure recomputes its pose; advance must retain the pre-spawn and
        // pre-cleanup pose used by the mechanical wrapper in simulate().
        let shake = self
            .stun_at
            .map_or((0., 0.), |at| stun_shake(input.now_ms - at));
        pose.x += shake.0;
        pose.y -= shake.1;
        self.pose = Some(pose);
        self.active = true;
        let talk = !input.reduced_motion && (input.now_ms / 180.).floor() % 2. != 0.;
        self.artwork = input.mascot.map(|id| id.mascot().frame(talk));
        if let Some(at) = self.stun_at {
            write_stun_stars(input.now_ms - at, &mut self.stars);
        }
    }
}

fn coin_visual(coin: Coin, now_ms: f64) -> Option<CoinVisual> {
    let pop = coin
        .collected_at
        .map_or(0., |at| ((now_ms - at) / COLLECT_POP_MS).min(1.));
    (pop < 1.).then(|| CoinVisual {
        x: coin.x,
        y: coin.height
            + if coin.collected_at.is_none() {
                (now_ms / 180.).sin() * 2.
            } else {
                0.
            }
            + COLLECT_POP_PX * pop,
        alpha: 1. - pop,
    })
}
