//! Port of src/features/terminal/arcade/snakeArcade.ts: a snake that roams
//! the top of the empty-session grid, eating pellets and detouring for the
//! provider logos.

use std::collections::{HashSet, VecDeque};

use monocode_core::{HARNESSES, HarnessId};

use super::grid_arcade::{
    ArcadeMode, ArcadeRng, ArcadeSprite, GridArcade, LogoPickup, SpeechBubble, fade_stamp, light,
};

const BOOT_FADE_MS: f64 = 420.0;

const TICK_MS: f64 = 70.0;
const TICK_FAST_MS: f64 = 52.0;
const FAST_AT_LENGTH: usize = 10;

/// `PLAYER_TICK`: the base and fast tick for each mode.
fn player_ticks(mode: ArcadeMode) -> (f64, f64) {
    match mode {
        ArcadeMode::Low => (72.0, 54.0),
        ArcadeMode::Mid => (38.0, 26.0),
        ArcadeMode::Hard => (22.0, 16.0),
    }
}

/// How many grid cells across the logo pickup sits.
pub const LOGO_CELLS: i32 = 3;

const LOGO_FADE_MS: f64 = 500.0;
const LOGO_LIFE_MS: f64 = 12000.0;
const LOGO_GAP_MIN_MS: f64 = 4200.0;
const LOGO_GAP_MAX_MS: f64 = 10500.0;
const LOGO_GROWTH: usize = 5;

const SPEECH_FADE_MS: f64 = 260.0;
/// How long the line stays up after the snake takes a logo.
const SPEECH_HOLD_MS: f64 = 2200.0;

/// What the snake pipes up with once it has swallowed a logo.
const CHATTER: &[&str] = &[
    "HELLO THERE!",
    "GENERAL KENOBI",
    "NOM NOM NOM",
    "MINE!",
    "DIBS",
    "SNACK TIME",
    "IS THIS EDIBLE?",
    "OOH, SHINY",
    "FREE REAL ESTATE",
    "ACQUIRING TARGET",
    "BRB, EATING",
    "404: FOOD FOUND",
    "SSSSSSS",
    "TASTES LIKE TABS",
    "NEEDS MORE SALT",
    "NO TRADEMARKS HARMED",
    "SHIP IT",
    "YOINK",
    "RESOLVING DEPENDENCY",
    "CACHE MISS, SNACK HIT",
];

/// How far the snake can travel before a logo times out. Its steering is
/// greedy, not optimal, so this budgets well under the straight-line
/// distance. On a wide monitor a logo parked at the far edge would expire.
const LOGO_REACH: i32 = ((LOGO_LIFE_MS * 0.7) / TICK_MS) as i32;

const PELLET_VALUE: f64 = 1.0;
const HEAD_VALUE: f64 = 0.9;

/// How far ahead of a fresh player snake the first pellet sits.
const PLAYER_PELLET_AHEAD: i32 = 8;

fn wrap(value: i32, max: i32) -> i32 {
    if max <= 0 {
        return 0;
    }
    value.rem_euclid(max)
}

/// `Math.max(min, Math.min(max, value))`, in that order.
fn clamp(value: i32, min: i32, max: i32) -> i32 {
    min.max(max.min(value))
}

type Cell = (i32, i32);

#[derive(Debug, Clone)]
struct Logo {
    harness: HarnessId,
    x: i32,
    y: i32,
    age: f64,
}

#[derive(Debug, Clone)]
struct Speech {
    text: &'static str,
    age: f64,
}

/// `createSnakeArcade`.
pub struct SnakeArcade {
    rng: ArcadeRng,
    cols: i32,
    rows: i32,
    play_rows: i32,

    body: VecDeque<Cell>,
    dir: Cell,
    pending: Option<Cell>,
    grow: usize,
    pellet: Cell,
    occupied: HashSet<Cell>,

    logo: Option<Logo>,
    logo_timer: f64,
    speech: Option<Speech>,
    last_chatter: Option<usize>,
    tick_acc: f64,
    booted: f64,
    player: bool,
    score: i64,
    mode: ArcadeMode,
}

impl SnakeArcade {
    pub fn new(rng: ArcadeRng) -> Self {
        Self {
            rng,
            cols: 0,
            rows: 0,
            play_rows: 0,
            body: VecDeque::new(),
            dir: (1, 0),
            pending: None,
            grow: 0,
            pellet: (0, 0),
            occupied: HashSet::new(),
            logo: None,
            logo_timer: 0.0,
            speech: None,
            last_chatter: None,
            tick_acc: 0.0,
            booted: 0.0,
            player: false,
            score: 0,
            mode: ArcadeMode::Mid,
        }
    }

    fn compute_play_rows(&self) -> i32 {
        if self.player {
            self.rows.max(8)
        } else {
            ((f64::from(self.rows) * 0.62).floor() as i32).max(8)
        }
    }

    fn sync_occupied(&mut self) {
        self.occupied = self.body.iter().copied().collect();
    }

    fn max_length(&self) -> usize {
        clamp((f64::from(self.cols) * 0.4).floor() as i32, 16, 80) as usize
    }

    fn next_logo_delay(&self) -> f64 {
        LOGO_GAP_MIN_MS + self.rng.next() * (LOGO_GAP_MAX_MS - LOGO_GAP_MIN_MS)
    }

    /// Never the same line twice running.
    fn next_chatter(&mut self) -> &'static str {
        let mut index = self.last_chatter;
        while index == self.last_chatter && CHATTER.len() > 1 {
            index = Some((self.rng.next() * CHATTER.len() as f64).floor() as usize);
        }
        self.last_chatter = index;
        index
            .and_then(|index| CHATTER.get(index))
            .copied()
            .unwrap_or(CHATTER[0])
    }

    fn clear_logo(&mut self) {
        self.logo = None;
        self.logo_timer = self.next_logo_delay();
    }

    /// True when (x, y) falls inside the logo's footprint.
    fn on_logo(&self, x: i32, y: i32) -> bool {
        self.logo.as_ref().is_some_and(|logo| {
            x >= logo.x && x < logo.x + LOGO_CELLS && y >= logo.y && y < logo.y + LOGO_CELLS
        })
    }

    fn place_pellet(&mut self) {
        for _ in 0..60 {
            let x = (self.rng.next() * f64::from(self.cols)).floor() as i32;
            let y = (self.rng.next() * f64::from(self.play_rows)).floor() as i32;
            if self.occupied.contains(&(x, y)) || self.on_logo(x, y) {
                continue;
            }
            self.pellet = (x, y);
            return;
        }
    }

    fn spawn_snake(&mut self) {
        let y = (self.play_rows / 2).max(1);
        let x = (self.cols / 4).max(2);
        self.body = VecDeque::from([(x, y), (x - 1, y), (x - 2, y)]);
        self.dir = (1, 0);
        self.pending = None;
        self.grow = 0;
        self.sync_occupied();
        if self.player {
            let ahead = wrap(x + PLAYER_PELLET_AHEAD, self.cols);
            if !self.occupied.contains(&(ahead, y)) && !self.on_logo(ahead, y) {
                self.pellet = (ahead, y);
            } else {
                self.place_pellet();
            }
        } else {
            self.place_pellet();
        }
    }

    fn spawn_logo(&mut self) {
        let Some(&head) = self.body.front() else {
            return;
        };
        if self.cols < LOGO_CELLS + 6 || self.play_rows < LOGO_CELLS + 3 {
            return;
        }

        let index = (self.rng.next() * HARNESSES.len() as f64).floor() as usize;
        let Some(&harness) = HARNESSES.get(index) else {
            return;
        };

        // Keep it landable: far enough away to be worth a detour, close
        // enough that the snake can get there before the logo times out.
        let min_reach = LOGO_CELLS + 4;
        let max_reach = clamp(
            (f64::from(self.cols) * 0.55).floor() as i32,
            min_reach + 6,
            LOGO_REACH,
        );

        for _ in 0..60 {
            let x = 1 + (self.rng.next() * f64::from(self.cols - LOGO_CELLS - 2)).floor() as i32;
            let y =
                1 + (self.rng.next() * f64::from(self.play_rows - LOGO_CELLS - 1)).floor() as i32;
            let cx = x + LOGO_CELLS / 2;
            let cy = y + LOGO_CELLS / 2;

            let dx = (cx - head.0).abs().min(self.cols - (cx - head.0).abs());
            let reach = dx + (cy - head.1).abs();
            if reach < min_reach || reach > max_reach {
                continue;
            }

            self.logo = Some(Logo {
                harness,
                x,
                y,
                age: 0.0,
            });
            if self.on_logo(self.pellet.0, self.pellet.1) {
                self.place_pellet();
            }
            return;
        }
    }

    /// Greedy steering. A logo on the board outranks the pellet outright.
    /// That detour is the point.
    fn think(&mut self) {
        let Some(&head) = self.body.front() else {
            return;
        };

        let target = match &self.logo {
            Some(logo) => (logo.x + LOGO_CELLS / 2, logo.y + LOGO_CELLS / 2),
            None => self.pellet,
        };
        let tail = self.body.back().copied();
        let dir = self.dir;
        let options = [dir, (-dir.1, dir.0), (dir.1, -dir.0)];

        let mut best = dir;
        let mut best_score = f64::NEG_INFINITY;
        for option in options {
            if option.0 == -dir.0 && option.1 == -dir.1 {
                continue;
            }
            let nx = wrap(head.0 + option.0, self.cols);
            let ny = wrap(head.1 + option.1, self.play_rows);
            let chasing_tail = tail == Some((nx, ny));
            if self.occupied.contains(&(nx, ny)) && !chasing_tail {
                continue;
            }

            let dx = (target.0 - nx).abs().min(self.cols - (target.0 - nx).abs());
            let dy = (target.1 - ny)
                .abs()
                .min(self.play_rows - (target.1 - ny).abs());
            let score = -f64::from(dx + dy) + self.rng.next() * 0.15;
            if score > best_score {
                best_score = score;
                best = option;
            }
        }
        self.dir = best;
    }

    fn advance(&mut self) {
        if self.player {
            if let Some(pending) = self.pending
                && !(pending.0 == -self.dir.0 && pending.1 == -self.dir.1)
            {
                self.dir = pending;
            }
            self.pending = None;
        } else {
            self.think();
        }
        let Some(&head) = self.body.front() else {
            return;
        };

        let next = (
            wrap(head.0 + self.dir.0, self.cols),
            wrap(head.1 + self.dir.1, self.play_rows),
        );
        let tail = self.body.back().copied();
        let hit = self.occupied.contains(&next) && tail != Some(next);
        if hit || self.body.len() > self.max_length() {
            if self.player {
                self.score = 0;
            }
            self.spawn_snake();
            return;
        }

        self.body.push_front(next);

        if self.on_logo(next.0, next.1) {
            self.clear_logo();
            let text = self.next_chatter();
            self.speech = Some(Speech { text, age: 0.0 });
            self.grow += LOGO_GROWTH;
            if self.player {
                self.score += LOGO_GROWTH as i64;
            }
        } else if next == self.pellet {
            self.grow += 1;
            if self.player {
                self.score += 1;
            }
            self.place_pellet();
        }

        if self.grow > 0 {
            self.grow -= 1;
        } else {
            self.body.pop_back();
        }

        self.sync_occupied();
    }

    fn relayout(&mut self) {
        let (cols, play_rows) = (self.cols, self.play_rows);
        let fit = |cell: Cell| (wrap(cell.0, cols), wrap(cell.1, play_rows));
        let mut seen = HashSet::new();
        self.body = self
            .body
            .iter()
            .map(|&cell| fit(cell))
            .filter(|cell| seen.insert(*cell))
            .collect();
        if self.body.is_empty() {
            self.spawn_snake();
            return;
        }
        self.pellet = fit(self.pellet);
        if let Some(logo) = &mut self.logo {
            if cols < LOGO_CELLS || play_rows < LOGO_CELLS {
                self.logo = None;
            } else {
                logo.x = clamp(logo.x, 0, cols - LOGO_CELLS);
                logo.y = clamp(logo.y, 0, play_rows - LOGO_CELLS);
            }
        }
        self.sync_occupied();
        if self.occupied.contains(&self.pellet) || self.on_logo(self.pellet.0, self.pellet.1) {
            self.place_pellet();
        }
    }

    fn boot(&mut self) {
        self.booted = 0.0;
        self.tick_acc = 0.0;
        self.score = 0;
        self.pending = None;
        self.logo = None;
        self.speech = None;
        self.logo_timer = self.next_logo_delay();
        self.spawn_snake();
    }

    fn player_tick(&self) -> f64 {
        let (base, fast) = player_ticks(self.mode);
        if self.body.len() > FAST_AT_LENGTH {
            fast
        } else {
            base
        }
    }

    /// 0 while fading in, ramping to 1. Keeps resizes from popping.
    fn boot_alpha(&self) -> f64 {
        (self.booted / BOOT_FADE_MS).min(1.0)
    }
}

fn speech_alpha(state: &Speech) -> f64 {
    let fade_in = (state.age / SPEECH_FADE_MS).min(1.0);
    let fade_out = (1.0 - (state.age - SPEECH_HOLD_MS) / SPEECH_FADE_MS).max(0.0);
    fade_in.min(fade_out)
}

impl GridArcade for SnakeArcade {
    fn resize(&mut self, next_cols: i32, next_rows: i32) {
        if next_cols == self.cols && next_rows == self.rows {
            return;
        }
        let keep_game = self.player && !self.body.is_empty() && self.cols > 0;
        self.cols = next_cols;
        self.rows = next_rows;
        self.play_rows = self.compute_play_rows();
        if keep_game {
            self.relayout();
        } else {
            self.boot();
        }
    }

    /// Hands the heading over to the caller. The idle brain goes quiet.
    fn take_control(&mut self) {
        if self.player {
            return;
        }
        self.player = true;
        self.play_rows = self.compute_play_rows();
        self.boot();
    }

    /// Puts the idle brain back in the seat.
    fn release_control(&mut self) {
        if !self.player {
            return;
        }
        self.player = false;
        self.play_rows = self.compute_play_rows();
        self.boot();
    }

    /// Queue the next heading. A turn back against the current direction is
    /// ignored, as in any snake: you cannot reverse into yourself.
    fn steer(&mut self, x: i32, y: i32) {
        if !self.player {
            return;
        }
        if x.abs() + y.abs() != 1 {
            return;
        }
        if x == -self.dir.0 && y == -self.dir.1 {
            return;
        }
        self.pending = Some((x, y));
    }

    fn set_mode(&mut self, next: ArcadeMode) {
        if self.mode == next {
            return;
        }
        self.mode = next;
        // Do not dump a slow-mode remainder as extra steps on a faster tick.
        self.tick_acc = 0.0;
    }

    fn mode(&self) -> ArcadeMode {
        self.mode
    }

    fn controlled(&self) -> bool {
        self.player
    }

    fn score(&self) -> i64 {
        self.score
    }

    fn lives(&self) -> i64 {
        0
    }

    fn game_over(&self) -> bool {
        false
    }

    fn step(&mut self, dt: f64) {
        if self.cols == 0 || self.play_rows == 0 {
            return;
        }
        self.booted += dt;

        if let Some(speech) = &mut self.speech {
            speech.age += dt;
            if speech.age >= SPEECH_HOLD_MS + SPEECH_FADE_MS {
                self.speech = None;
            }
        }

        if let Some(logo) = &mut self.logo {
            logo.age += dt;
            if logo.age >= LOGO_LIFE_MS {
                self.clear_logo();
            }
        } else {
            self.logo_timer -= dt;
            if self.logo_timer <= 0.0 {
                self.spawn_logo();
                // If nowhere suitable turned up, wait out another gap rather
                // than running the search again on every frame.
                if self.logo.is_none() {
                    self.logo_timer = self.next_logo_delay();
                }
            }
        }

        self.tick_acc += dt;
        let tick = if self.player {
            self.player_tick()
        } else if self.body.len() > FAST_AT_LENGTH {
            TICK_FAST_MS
        } else {
            TICK_MS
        };
        while self.tick_acc >= tick {
            self.tick_acc -= tick;
            self.advance();
        }
    }

    /// The line to float above the head, set going by swallowing a logo.
    fn speech_bubble(&self) -> Option<SpeechBubble> {
        let speech = self.speech.as_ref()?;
        let head = self.body.front()?;
        Some(SpeechBubble {
            text: speech.text,
            x: f64::from(head.0),
            y: f64::from(head.1),
            alpha: speech_alpha(speech) * self.boot_alpha(),
        })
    }

    /// The logo to paint over the grid, if one is on the board.
    fn logo_pickup(&self) -> Option<LogoPickup> {
        let logo = self.logo.as_ref()?;
        let fade_in = (logo.age / LOGO_FADE_MS).min(1.0);
        let fade_out = ((LOGO_LIFE_MS - logo.age) / LOGO_FADE_MS).min(1.0);
        Some(LogoPickup {
            harness: logo.harness,
            x: f64::from(logo.x),
            y: f64::from(logo.y),
            cells: f64::from(LOGO_CELLS),
            alpha: fade_in.min(fade_out).max(0.0) * self.boot_alpha(),
        })
    }

    /// The body is stamped onto the grid, so the snake draws no sprites.
    fn sprites(&self) -> Vec<ArcadeSprite> {
        Vec::new()
    }

    /// 0 to 1 over the first paint after mount or resize.
    fn fade(&self) -> f64 {
        self.boot_alpha()
    }

    fn stamp(&self, out: &mut [f32], stamp_cols: usize, stamp_rows: usize) {
        for (index, cell) in self.body.iter().enumerate() {
            let value = if index == 0 {
                HEAD_VALUE
            } else {
                (0.72 - index as f64 * 0.018).max(0.42)
            };
            light(out, stamp_cols, stamp_rows, cell.0, cell.1, value);
        }
        light(
            out,
            stamp_cols,
            stamp_rows,
            self.pellet.0,
            self.pellet.1,
            PELLET_VALUE,
        );

        fade_stamp(out, self.boot_alpha());
    }
}

#[cfg(test)]
#[path = "snake_tests.rs"]
mod tests;
