//! Native Snake engine. Movement order, including tail entry while growing,
//! follows snakeArcade.ts; replacement food excludes the current body.
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
        // An unlucky run of samples must not leave the eaten pellet under the
        // head. Fall back to a free cell without drawing more randomness.
        for y in 0..self.play_rows {
            for x in 0..self.cols {
                let cell = Cell::new(x, y);
                if !self.occupied.contains(&cell) && !self.on_logo(cell) {
                    self.pellet = cell;
                    return;
                }
            }
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
            // The new head is part of the growing body before food is placed.
            self.occupied.insert(next);
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
    /// Finish only the presentation fade; gameplay and its timers stay put.
    pub fn reveal(&mut self) {
        self.booted = self.booted.max(420.);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_collision_resets_the_player_but_entering_the_vacating_tail_is_safe() {
        for tail_entry in [false, true] {
            let mut game = Snake::new(vec![]);
            let mut random = crate::random::Mulberry32::new(7);
            game.resize(40, 20, &mut random);
            game.take_control(&mut random);
            game.body = [
                Cell::new(5, 5),
                Cell::new(4, 5),
                Cell::new(4, 6),
                Cell::new(5, 6),
                Cell::new(6, 6),
                Cell::new(6, 5),
            ]
            .into();
            if !tail_entry {
                game.body.push_back(Cell::new(7, 5));
            }
            game.sync_occupied();
            game.pellet = Cell::new(30, 15);
            game.score = 40;
            game.steer(1, 0);

            game.step(38., &mut random);
            if tail_entry {
                assert_eq!(game.body[0], Cell::new(6, 5));
                assert_eq!(game.body.len(), 6);
                assert_eq!(game.score(), 40);
            } else {
                assert_eq!(
                    game.body,
                    [Cell::new(10, 10), Cell::new(9, 10), Cell::new(8, 10)]
                );
                assert_eq!(game.score(), 0);
                assert_eq!(game.grow, 0);
            }
            assert_eq!(game.dir, Cell::new(1, 0));
            assert!(game.pending.is_none());
            assert_eq!(game.occupied, game.body.iter().copied().collect());
            assert!(!game.occupied.contains(&game.pellet));
            assert!(game.controlled());
        }
    }

    #[test]
    fn turns_wait_for_the_next_tick_and_invalid_input_preserves_the_buffer() {
        let mut game = Snake::new(vec![]);
        let mut random = crate::random::Mulberry32::new(7);
        game.resize(40, 20, &mut random);
        game.take_control(&mut random);
        game.steer(0, -1);
        game.steer(-1, 0); // Reversal against the current direction is ignored.
        game.steer(1, 1); // Diagonal input is ignored too.
        assert_eq!(game.pending, Some(Cell::new(0, -1)));
        game.step(37., &mut random);
        assert_eq!(game.body[0], Cell::new(10, 10));
        assert_eq!(game.dir, Cell::new(1, 0));
        game.step(1., &mut random);
        assert_eq!(game.body[0], Cell::new(10, 9));
        assert_eq!(game.dir, Cell::new(0, -1));
        assert!(game.pending.is_none());

        game.steer(-1, 0);
        game.steer(1, 0); // The latest valid turn wins; turns are not queued.
        game.steer(0, 1); // Reversal must not replace that turn.
        assert_eq!(game.pending, Some(Cell::new(1, 0)));
        game.step(38., &mut random);
        assert_eq!(game.body[0], Cell::new(11, 9));
        assert_eq!(game.dir, Cell::new(1, 0));
        assert!(game.pending.is_none());
        game.step(38., &mut random);
        assert_eq!(game.body[0], Cell::new(12, 9));
    }

    #[test]
    fn replacement_food_excludes_the_new_head_even_after_random_retries_run_out() {
        struct Candidates {
            head: Cell,
            misses: usize,
            draws: usize,
        }
        impl Random for Candidates {
            fn next_f64(&mut self) -> f64 {
                let attempt = self.draws / 2;
                let x_axis = self.draws.is_multiple_of(2);
                self.draws += 1;
                if x_axis {
                    (if attempt < self.misses {
                        f64::from(self.head.x)
                    } else {
                        0.
                    } + 0.5)
                        / 40.
                } else {
                    (f64::from(self.head.y) + 0.5) / 20.
                }
            }
        }
        for misses in [1, 60] {
            let mut game = Snake::new(vec![]);
            let mut startup = crate::random::Mulberry32::new(7);
            game.resize(40, 20, &mut startup);
            game.take_control(&mut startup);
            let head = Cell::new(game.body[0].x + 1, game.body[0].y);
            game.pellet = head;
            let mut random = Candidates {
                head,
                misses,
                draws: 0,
            };
            game.advance(&mut random);
            assert_eq!(game.score(), 1);
            assert_eq!(game.body.len(), 4);
            assert!(!game.body.contains(&game.pellet), "misses={misses}");
            assert!(!game.on_logo(game.pellet));
            assert_eq!(random.draws, if misses == 1 { 4 } else { 120 });
        }
    }
}
