//! Mechanical translation of snakeArcade.ts. Ordering (including stale pellet
//! occupancy and tail entry while growing) intentionally matches the reference.
use crate::{
    random::Random,
    types::{Bubble, Cell, Mode, Pickup},
};
use std::collections::{HashSet, VecDeque};

const LOGO_CELLS: i32 = 3;
const CHATTER: [&str; 20] = [
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
#[derive(Clone, Debug)]
struct Logo {
    provider: usize,
    x: i32,
    y: i32,
    age: f64,
}
#[derive(Clone, Debug)]
struct Speech {
    line: usize,
    age: f64,
}

pub struct Snake {
    providers: Vec<String>,
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
    score: u32,
    mode: Mode,
}

impl Snake {
    pub fn new(providers: Vec<String>) -> Self {
        Self {
            providers,
            cols: 0,
            rows: 0,
            play_rows: 0,
            body: VecDeque::new(),
            dir: Cell::new(1, 0),
            pending: None,
            grow: 0,
            pellet: Cell::default(),
            occupied: HashSet::new(),
            logo: None,
            logo_timer: 0.,
            speech: None,
            last_chatter: None,
            tick_acc: 0.,
            booted: 0.,
            player: false,
            score: 0,
            mode: Mode::Mid,
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
        self.occupied.clear();
        self.occupied.extend(self.body.iter().copied());
    }
    fn max_length(&self) -> usize {
        ((f64::from(self.cols) * 0.4).floor() as usize).clamp(16, 80)
    }
    fn next_logo_delay(random: &mut dyn Random) -> f64 {
        4200. + random.next_f64() * (10500. - 4200.)
    }
    fn clear_logo(&mut self, random: &mut dyn Random) {
        self.logo = None;
        self.logo_timer = Self::next_logo_delay(random);
    }
    fn next_chatter(&mut self, random: &mut dyn Random) -> usize {
        let mut index = self.last_chatter;
        while index == self.last_chatter {
            index = Some((random.next_f64() * CHATTER.len() as f64).floor() as usize);
        }
        self.last_chatter = index;
        index.unwrap()
    }
    fn on_logo(&self, cell: Cell) -> bool {
        self.logo.as_ref().is_some_and(|l| {
            cell.x >= l.x && cell.x < l.x + LOGO_CELLS && cell.y >= l.y && cell.y < l.y + LOGO_CELLS
        })
    }
    fn place_pellet(&mut self, random: &mut dyn Random) {
        for _ in 0..60 {
            let cell = Cell::new(
                (random.next_f64() * f64::from(self.cols)).floor() as i32,
                (random.next_f64() * f64::from(self.play_rows)).floor() as i32,
            );
            if self.occupied.contains(&cell) || self.on_logo(cell) {
                continue;
            }
            self.pellet = cell;
            return;
        }
    }
    fn spawn_snake(&mut self, random: &mut dyn Random) {
        let y = (self.play_rows / 2).max(1);
        let x = (self.cols / 4).max(2);
        self.body = [Cell::new(x, y), Cell::new(x - 1, y), Cell::new(x - 2, y)].into();
        self.dir = Cell::new(1, 0);
        self.pending = None;
        self.grow = 0;
        self.sync_occupied();
        let ahead = Cell::new(wrap(x + 8, self.cols), y);
        if self.player && !self.occupied.contains(&ahead) && !self.on_logo(ahead) {
            self.pellet = ahead;
        } else {
            self.place_pellet(random);
        }
    }
    fn spawn_logo(&mut self, random: &mut dyn Random) {
        let Some(head) = self.body.front().copied() else {
            return;
        };
        if self.cols < LOGO_CELLS + 6 || self.play_rows < LOGO_CELLS + 3 {
            return;
        }
        let provider = (random.next_f64() * self.providers.len() as f64).floor() as usize;
        if provider >= self.providers.len() {
            return;
        }
        let min_reach = LOGO_CELLS + 4;
        let max_reach = ((f64::from(self.cols) * 0.55).floor() as i32).clamp(min_reach + 6, 120);
        for _ in 0..60 {
            let x = 1 + (random.next_f64() * f64::from(self.cols - LOGO_CELLS - 2)).floor() as i32;
            let y =
                1 + (random.next_f64() * f64::from(self.play_rows - LOGO_CELLS - 1)).floor() as i32;
            let cx = x + LOGO_CELLS / 2;
            let cy = y + LOGO_CELLS / 2;
            let dx = (cx - head.x).abs().min(self.cols - (cx - head.x).abs());
            let reach = dx + (cy - head.y).abs();
            if reach < min_reach || reach > max_reach {
                continue;
            }
            self.logo = Some(Logo {
                provider,
                x,
                y,
                age: 0.,
            });
            if self.on_logo(self.pellet) {
                self.place_pellet(random);
            }
            return;
        }
    }
    fn think(&mut self, random: &mut dyn Random) {
        let Some(head) = self.body.front().copied() else {
            return;
        };
        let target = self.logo.as_ref().map_or(self.pellet, |l| {
            Cell::new(l.x + LOGO_CELLS / 2, l.y + LOGO_CELLS / 2)
        });
        let tail = self.body.back().copied();
        let options = [
            self.dir,
            Cell::new(-self.dir.y, self.dir.x),
            Cell::new(self.dir.y, -self.dir.x),
        ];
        let mut best = self.dir;
        let mut best_score = f64::NEG_INFINITY;
        for option in options {
            if option.x == -self.dir.x && option.y == -self.dir.y {
                continue;
            }
            let next = Cell::new(
                wrap(head.x + option.x, self.cols),
                wrap(head.y + option.y, self.play_rows),
            );
            if self.occupied.contains(&next) && tail != Some(next) {
                continue;
            }
            let dx = (target.x - next.x)
                .abs()
                .min(self.cols - (target.x - next.x).abs());
            let dy = (target.y - next.y)
                .abs()
                .min(self.play_rows - (target.y - next.y).abs());
            let score = -f64::from(dx + dy) + random.next_f64() * 0.15;
            if score > best_score {
                best_score = score;
                best = option;
            }
        }
        self.dir = best;
    }
    fn advance(&mut self, random: &mut dyn Random) {
        if self.player {
            if let Some(pending) = self.pending.take() {
                if !(pending.x == -self.dir.x && pending.y == -self.dir.y) {
                    self.dir = pending;
                }
            }
        } else {
            self.think(random);
        }
        let Some(head) = self.body.front() else {
            return;
        };
        let next = Cell::new(
            wrap(head.x + self.dir.x, self.cols),
            wrap(head.y + self.dir.y, self.play_rows),
        );
        let hit = self.occupied.contains(&next) && self.body.back() != Some(&next);
        if hit || self.body.len() > self.max_length() {
            if self.player {
                self.score = 0;
            }
            self.spawn_snake(random);
            return;
        }
        self.body.push_front(next);
        if self.on_logo(next) {
            self.clear_logo(random);
            self.speech = Some(Speech {
                line: self.next_chatter(random),
                age: 0.,
            });
            self.grow += 5;
            if self.player {
                self.score += 5;
            }
        } else if next == self.pellet {
            self.grow += 1;
            if self.player {
                self.score += 1;
            }
            self.place_pellet(random);
        }
        if self.grow > 0 {
            self.grow -= 1;
        } else {
            self.body.pop_back();
        }
        self.sync_occupied();
    }
    fn relayout(&mut self, random: &mut dyn Random) {
        let mut seen = HashSet::new();
        self.body = self
            .body
            .iter()
            .map(|c| Cell::new(wrap(c.x, self.cols), wrap(c.y, self.play_rows)))
            .filter(|c| seen.insert(*c))
            .collect();
        if self.body.is_empty() {
            self.spawn_snake(random);
            return;
        }
        self.pellet = Cell::new(
            wrap(self.pellet.x, self.cols),
            wrap(self.pellet.y, self.play_rows),
        );
        if let Some(logo) = &mut self.logo {
            if self.cols < LOGO_CELLS || self.play_rows < LOGO_CELLS {
                self.logo = None;
            } else {
                logo.x = logo.x.clamp(0, self.cols - LOGO_CELLS);
                logo.y = logo.y.clamp(0, self.play_rows - LOGO_CELLS);
            }
        }
        self.sync_occupied();
        if self.occupied.contains(&self.pellet) || self.on_logo(self.pellet) {
            self.place_pellet(random);
        }
    }
    fn boot(&mut self, random: &mut dyn Random) {
        self.booted = 0.;
        self.tick_acc = 0.;
        self.score = 0;
        self.pending = None;
        self.logo = None;
        self.speech = None;
        self.logo_timer = Self::next_logo_delay(random);
        self.spawn_snake(random);
    }
    pub fn resize(&mut self, cols: i32, rows: i32, random: &mut dyn Random) {
        assert!(
            cols >= 0 && rows >= 0,
            "grid dimensions must be nonnegative"
        );
        if cols == self.cols && rows == self.rows {
            return;
        }
        let keep = self.player && !self.body.is_empty() && self.cols > 0;
        self.cols = cols;
        self.rows = rows;
        self.play_rows = self.compute_play_rows();
        if keep {
            self.relayout(random);
        } else {
            self.boot(random);
        }
    }
    pub fn take_control(&mut self, random: &mut dyn Random) {
        if self.player {
            return;
        }
        self.player = true;
        self.play_rows = self.compute_play_rows();
        self.boot(random);
    }
    pub fn release_control(&mut self, random: &mut dyn Random) {
        if !self.player {
            return;
        }
        self.player = false;
        self.play_rows = self.compute_play_rows();
        self.boot(random);
    }
    pub fn steer(&mut self, x: i32, y: i32) {
        if !self.player || !Cell::new(x, y).is_cardinal() || (x == -self.dir.x && y == -self.dir.y)
        {
            return;
        }
        self.pending = Some(Cell::new(x, y));
    }
    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.tick_acc = 0.;
        }
    }
    pub fn controlled(&self) -> bool {
        self.player
    }
    pub fn score(&self) -> u32 {
        self.score
    }
    pub fn lives(&self) -> u32 {
        0
    }
    pub fn game_over(&self) -> bool {
        false
    }
    pub fn fade(&self) -> f64 {
        (self.booted / 420.).min(1.)
    }
    pub fn step(&mut self, dt: f64, random: &mut dyn Random) {
        if self.cols == 0 || self.play_rows == 0 {
            return;
        }
        self.booted += dt;
        if let Some(speech) = &mut self.speech {
            speech.age += dt;
            if speech.age >= 2460. {
                self.speech = None;
            }
        }
        if let Some(logo) = &mut self.logo {
            logo.age += dt;
            if logo.age >= 12000. {
                self.clear_logo(random);
            }
        } else {
            self.logo_timer -= dt;
            if self.logo_timer <= 0. {
                self.spawn_logo(random);
                if self.logo.is_none() {
                    self.logo_timer = Self::next_logo_delay(random);
                }
            }
        }
        self.tick_acc += dt;
        let fast = self.body.len() > 10;
        let tick = if self.player {
            match (self.mode, fast) {
                (Mode::Low, false) => 72.,
                (Mode::Low, true) => 54.,
                (Mode::Mid, false) => 38.,
                (Mode::Mid, true) => 26.,
                (Mode::Hard, false) => 22.,
                (Mode::Hard, true) => 16.,
            }
        } else if fast {
            52.
        } else {
            70.
        };
        while self.tick_acc >= tick {
            self.tick_acc -= tick;
            self.advance(random);
        }
    }
    pub fn speech_bubble(&self) -> Option<Bubble> {
        let speech = self.speech.as_ref()?;
        let head = self.body.front()?;
        let alpha = (speech.age / 260.)
            .min(1.)
            .min((1. - (speech.age - 2200.) / 260.).max(0.));
        Some(Bubble {
            text: CHATTER[speech.line],
            x: f64::from(head.x),
            y: f64::from(head.y),
            alpha: alpha * self.fade(),
        })
    }
    pub fn logo_pickup(&self) -> Option<Pickup<'_>> {
        let logo = self.logo.as_ref()?;
        let alpha = (logo.age / 500.)
            .min(1.)
            .min(((12000. - logo.age) / 500.).min(1.))
            .max(0.);
        Some(Pickup {
            provider: &self.providers[logo.provider],
            x: f64::from(logo.x),
            y: f64::from(logo.y),
            cells: 3.,
            alpha: alpha * self.fade(),
        })
    }
    /// Max-compose and apply fade to the entire supplied buffer, just as the TS
    /// Float32Array contract does. Callers normally clear their reused buffer.
    pub fn stamp(&self, out: &mut [f32], cols: usize, rows: usize) {
        assert!(out.len() >= cols * rows);
        let mut light = |cell: Cell, value: f64| {
            if cell.x < 0 || cell.y < 0 || cell.x as usize >= cols || cell.y as usize >= rows {
                return;
            }
            let index = cell.y as usize * cols + cell.x as usize;
            if f64::from(out[index]) < value {
                out[index] = value as f32;
            }
        };
        for (i, cell) in self.body.iter().enumerate() {
            light(
                *cell,
                if i == 0 {
                    0.9
                } else {
                    (0.72 - i as f64 * 0.018).max(0.42)
                },
            );
        }
        light(self.pellet, 1.);
        let alpha = self.fade();
        if alpha < 0.999 {
            for value in out {
                *value = (f64::from(*value) * alpha) as f32;
            }
        }
    }
}

fn wrap(value: i32, max: i32) -> i32 {
    if max <= 0 {
        0
    } else {
        value.rem_euclid(max)
    }
}
