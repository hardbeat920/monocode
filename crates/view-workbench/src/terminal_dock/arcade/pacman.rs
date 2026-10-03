//! Port of src/features/terminal/arcade/pacmanArcade.ts: pac-man on the
//! terminal grid, chased by four of the project mascots.
//!
//! Everything lives on two grids. The background grid is one cell per
//! square, which is what `stamp` paints the maze and the pellets into. On
//! top of that sits the maze lattice, `tile` cells to a side, which is what
//! pac-man and the mascots walk on. `sprites` hands their positions back in
//! fractional cells so the caller can draw them mid-step.

use monocode_core::js;
use monocode_core::{HARNESSES, HarnessId};

use super::grid_arcade::{
    ArcadeMode, ArcadeRng, ArcadeSprite, GridArcade, LogoPickup, SpeechBubble, SpriteFrame,
    SpriteKind, clamp_i, fade_stamp, light, sign,
};
use crate::panes::pixel_art::PROJECT_MASCOTS;

const BOOT_FADE_MS: f64 = 420.0;

/// Cells to a maze tile. The idle band gets a finer maze than a real game.
const IDLE_TILE: i32 = 3;
const PLAYER_TILE: i32 = 4;

/// Smallest maze worth running. Below this the surface stays a plain grid.
const MIN_TILE_COLS: i32 = 7;
const MIN_TILE_ROWS: i32 = 5;

/// ms per tile at a player-sized tile, scaled down with the tile below.
fn mode_tick(mode: ArcadeMode) -> f64 {
    match mode {
        ArcadeMode::Low => 250.0,
        ArcadeMode::Mid => 185.0,
        ArcadeMode::Hard => 135.0,
    }
}
const IDLE_TICK: f64 = 240.0;

/// Mascot pace as a fraction of pac-man's. Eyes race back to the pen.
const GHOST_SPEED: f64 = 0.92;
const FRIGHT_SPEED: f64 = 0.55;
const EYES_SPEED: f64 = 2.4;

const FRIGHT_MS: f64 = 7000.0;
/// Tail end of a fright, where the mascots blink to warn they are coming back.
const FRIGHT_FLASH_MS: f64 = 2200.0;
const FLASH_PERIOD_MS: f64 = 260.0;

const SCATTER_MS: f64 = 7000.0;
const CHASE_MS: f64 = 20000.0;

const DEATH_MS: f64 = 1500.0;
/// The idle board sits behind a pane, so it does not dwell on a loss.
const IDLE_DEATH_MS: f64 = 900.0;
const OVER_MS: f64 = 2600.0;
const CLEAR_MS: f64 = 1200.0;
const START_LIVES: i64 = 3;

const PELLET_SCORE: i64 = 10;
const ENERGIZER_SCORE: i64 = 50;
const FRUIT_SCORE: i64 = 200;
const GHOST_SCORES: [i64; 4] = [200, 400, 800, 1600];

/// What the idle brain goes out of its way for, against a pellet's 1. A
/// logo is only up for so long, so it outranks the pellet underfoot
/// outright. That detour is the point of putting one on the board.
const LOGO_WEIGHT: f64 = 60.0;
const EDIBLE_WEIGHT: f64 = 40.0;
const ENERGIZER_WEIGHT: f64 = 2.0;

const GHOST_COUNT: usize = 4;
/// Staggered starts, so all four do not pour out of the pen at once.
const GHOST_RELEASE_MS: [f64; 4] = [0.0, 1600.0, 3600.0, 6000.0];

const WALL_VALUE: f64 = 0.26;
const PELLET_VALUE: f64 = 0.78;
const ENERGIZER_VALUE: f64 = 1.0;
/// Energizers breathe, so they read as more than a fat pellet.
const BLINK_MS: f64 = 900.0;

const LOGO_FADE_MS: f64 = 500.0;
const LOGO_LIFE_MS: f64 = 12000.0;
const LOGO_GAP_MIN_MS: f64 = 4200.0;
const LOGO_GAP_MAX_MS: f64 = 10500.0;

const SPEECH_FADE_MS: f64 = 260.0;
/// How long a line stays up after whoever said it opened their mouth.
const SPEECH_HOLD_MS: f64 = 2200.0;

/// What pac-man pipes up with once a provider logo goes down.
const FRUIT_CHATTER: &[&str] = &[
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
    "TASTES LIKE TABS",
    "NEEDS MORE SALT",
    "NO TRADEMARKS HARMED",
    "SHIP IT",
    "YOINK",
    "RESOLVING DEPENDENCY",
    "CACHE MISS, SNACK HIT",
];

/// Pac-man, having turned the tables on a mascot.
const CHOMP_CHATTER: &[&str] = &[
    "GOTCHA",
    "SORRY, LITTLE GUY",
    "REVERSE UNO",
    "WHO'S CHASING NOW",
    "RESPAWN LATER",
    "TASTES LIKE PIXELS",
    "THAT'S ONE",
];

/// The mascot that just caught him.
const CAUGHT_CHATTER: &[&str] = &[
    "TAG, YOU'RE IT",
    "OUR TURN",
    "SNACK ACQUIRED",
    "MERGE CONFLICT",
    "SKILL ISSUE",
    "GOT ONE",
    "NOM",
];

const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

#[derive(Debug, Clone)]
struct Maze {
    cols: i32,
    rows: i32,
    /// 1 wall, 0 corridor.
    wall: Vec<u8>,
}

impl Maze {
    fn wrap_tx(&self, x: i32) -> i32 {
        x.rem_euclid(self.cols)
    }

    fn open(&self, x: i32, y: i32) -> bool {
        if y < 0 || y >= self.rows {
            return false;
        }
        self.wall[(y * self.cols + self.wrap_tx(x)) as usize] == 0
    }

    /// Shortest signed run from a to b, going the way the tunnel allows.
    fn span_x(&self, a: f64, b: f64) -> f64 {
        let direct = b - a;
        let around = direct - f64::from(sign(direct)) * f64::from(self.cols);
        if direct.abs() <= around.abs() {
            direct
        } else {
            around
        }
    }

    fn tile_distance(&self, ax: i32, ay: i32, bx: i32, by: i32) -> f64 {
        let dx = self.span_x(f64::from(ax), f64::from(bx));
        let dy = f64::from(by - ay);
        dx * dx + dy * dy
    }

    /// Nearest corridor to a point we would like to put something on.
    fn nearest_open(&self, x: i32, y: i32) -> (i32, i32) {
        let start = (clamp_i(x, 1, self.cols - 2), clamp_i(y, 1, self.rows - 2));
        if self.open(start.0, start.1) {
            return start;
        }
        for radius in 1..self.cols.max(self.rows) {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let nx = start.0 + dx;
                    let ny = start.1 + dy;
                    if ny < 1 || ny > self.rows - 2 {
                        continue;
                    }
                    if self.open(nx, ny) {
                        return (self.wrap_tx(nx), ny);
                    }
                }
            }
        }
        start
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Mover {
    tx: i32,
    ty: i32,
    dx: i32,
    dy: i32,
    /// 0 to 1 across the step from this tile to the next.
    progress: f64,
}

impl Mover {
    fn at(tx: i32, ty: i32) -> Self {
        Self {
            tx,
            ty,
            ..Self::default()
        }
    }

    /// Where the mover is, in fractional tiles.
    fn at_x(&self) -> f64 {
        f64::from(self.tx) + f64::from(self.dx) * self.progress
    }

    fn at_y(&self) -> f64 {
        f64::from(self.ty) + f64::from(self.dy) * self.progress
    }

    fn stopped(&self) -> bool {
        self.dx == 0 && self.dy == 0
    }

    /// `reverse`: turn on the spot, keeping the sprite where it already is.
    fn reverse(&mut self, maze: &Maze) {
        if self.stopped() {
            return;
        }
        if self.progress > 0.0 {
            self.tx = maze.wrap_tx(self.tx + self.dx);
            self.ty += self.dy;
            self.progress = 1.0 - self.progress;
        }
        self.dx = -self.dx;
        self.dy = -self.dy;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GhostState {
    Pen,
    Hunt,
    Fright,
    Eyes,
}

#[derive(Debug, Clone)]
struct Ghost {
    m: Mover,
    mascot: &'static str,
    kind: usize,
    state: GhostState,
    release_in: f64,
    scatter: (i32, i32),
    /// Tiles walked, for the two-frame shuffle.
    steps: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Speaker {
    Pac,
    Ghost(usize),
}

#[derive(Debug, Clone)]
struct Speech {
    text: &'static str,
    age: f64,
    from: Speaker,
}

#[derive(Debug, Clone)]
struct Logo {
    harness: HarnessId,
    tx: i32,
    ty: i32,
    age: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Scatter,
    Chase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Run,
    Dying,
    Clear,
    Over,
}

/// A braided maze: a perfect maze carved depth first, then opened up until
/// no dead ends are left. Dead ends are the one thing pac-man cannot
/// survive, and the extra loops give the mascots somewhere to cut you off.
fn build_maze(rng: &ArcadeRng, tile_cols: i32, tile_rows: i32) -> Maze {
    // Corridors sit on odd coordinates, walls on even, so both counts are odd.
    let cols = if tile_cols % 2 != 0 {
        tile_cols
    } else {
        tile_cols - 1
    };
    let rows = if tile_rows % 2 != 0 {
        tile_rows
    } else {
        tile_rows - 1
    };
    let mut wall = vec![1u8; (cols * rows) as usize];
    let at = |x: i32, y: i32| (y * cols + x) as usize;

    let mut stack = vec![(1, 1)];
    wall[at(1, 1)] = 0;
    while let Some(&(cx, cy)) = stack.last() {
        let mut moved = false;
        for (dx, dy) in rng.shuffled(&DIRS) {
            let nx = cx + dx * 2;
            let ny = cy + dy * 2;
            if nx < 1 || ny < 1 || nx > cols - 2 || ny > rows - 2 {
                continue;
            }
            if wall[at(nx, ny)] != 1 {
                continue;
            }
            wall[at(cx + dx, cy + dy)] = 0;
            wall[at(nx, ny)] = 0;
            stack.push((nx, ny));
            moved = true;
            break;
        }
        if !moved {
            stack.pop();
        }
    }

    // Braid: every corridor with a single way out gets a second one.
    let mut y = 1;
    while y < rows - 1 {
        let mut x = 1;
        while x < cols - 1 {
            let exits = DIRS
                .iter()
                .filter(|(dx, dy)| wall[at(x + dx, y + dy)] != 1)
                .count();
            if exits <= 1 {
                let opened = rng.shuffled(&DIRS).into_iter().find(|&(dx, dy)| {
                    wall[at(x + dx, y + dy)] == 1
                        && x + dx * 2 >= 1
                        && y + dy * 2 >= 1
                        && x + dx * 2 <= cols - 2
                        && y + dy * 2 <= rows - 2
                });
                if let Some((dx, dy)) = opened {
                    wall[at(x + dx, y + dy)] = 0;
                }
            }
            x += 2;
        }
        y += 2;
    }

    // A few more loops on top, so the place does not read as a puzzle.
    for y in 1..rows - 1 {
        for x in 1..cols - 1 {
            if wall[at(x, y)] != 1 {
                continue;
            }
            if (x % 2 == 1) == (y % 2 == 1) {
                continue;
            }
            if rng.next() > 0.12 {
                continue;
            }
            wall[at(x, y)] = 0;
        }
    }

    // The wrap-around tunnel, straight across the middle corridor row.
    let half = rows / 2;
    let tunnel = clamp_i(if half % 2 == 1 { half } else { half + 1 }, 1, rows - 2);
    for x in 0..cols {
        wall[at(x, tunnel)] = 0;
    }

    Maze { cols, rows, wall }
}

/// `createPacmanArcade`.
pub struct PacmanArcade {
    rng: ArcadeRng,
    cols: i32,
    rows: i32,
    tile: i32,
    /// Cell offset of the maze's top-left corner, keeping it centered.
    origin_x: i32,
    origin_y: i32,

    maze: Option<Maze>,
    /// 0 empty, 1 pellet, 2 energizer, one entry per maze tile.
    pellets: Vec<u8>,
    pellets_left: i64,

    pac: Mover,
    pac_home: (i32, i32),
    pending: Option<(i32, i32)>,
    chew: f64,

    ghosts: Vec<Ghost>,
    pen: (i32, i32),
    ghost_phase: Phase,
    phase_left: f64,
    fright_left: f64,
    eaten_streak: usize,

    status: Status,
    status_left: f64,

    logo: Option<Logo>,
    logo_timer: f64,
    speech: Option<Speech>,
    last_line: &'static str,

    booted: f64,
    blink: f64,
    player: bool,
    score: i64,
    lives: i64,
    mode: ArcadeMode,
}

impl PacmanArcade {
    pub fn new(rng: ArcadeRng) -> Self {
        Self {
            rng,
            cols: 0,
            rows: 0,
            tile: IDLE_TILE,
            origin_x: 0,
            origin_y: 0,
            maze: None,
            pellets: Vec::new(),
            pellets_left: 0,
            pac: Mover::default(),
            pac_home: (0, 0),
            pending: None,
            chew: 0.0,
            ghosts: Vec::new(),
            pen: (0, 0),
            ghost_phase: Phase::Scatter,
            phase_left: SCATTER_MS,
            fright_left: 0.0,
            eaten_streak: 0,
            status: Status::Run,
            status_left: 0.0,
            logo: None,
            logo_timer: 0.0,
            speech: None,
            last_line: "",
            booted: 0.0,
            blink: 0.0,
            player: false,
            score: 0,
            lives: START_LIVES,
            mode: ArcadeMode::Mid,
        }
    }

    fn wrap_tx(&self, x: i32) -> i32 {
        self.maze.as_ref().map_or(0, |maze| maze.wrap_tx(x))
    }

    fn open(&self, x: i32, y: i32) -> bool {
        self.maze.as_ref().is_some_and(|maze| maze.open(x, y))
    }

    fn span_x(&self, a: f64, b: f64) -> f64 {
        self.maze.as_ref().map_or(0.0, |maze| maze.span_x(a, b))
    }

    fn cell_x(&self, tx: f64) -> f64 {
        f64::from(self.origin_x) + tx * f64::from(self.tile)
    }

    fn cell_y(&self, ty: f64) -> f64 {
        f64::from(self.origin_y) + ty * f64::from(self.tile)
    }

    fn pac_tick(&self) -> f64 {
        let base = if self.player {
            mode_tick(self.mode)
        } else {
            IDLE_TICK
        };
        base * (f64::from(self.tile) / f64::from(PLAYER_TILE))
    }

    fn ghost_tick(&self, ghost: &Ghost) -> f64 {
        let speed = match ghost.state {
            GhostState::Eyes => EYES_SPEED,
            GhostState::Fright => FRIGHT_SPEED,
            _ => GHOST_SPEED,
        };
        self.pac_tick() / speed
    }

    fn next_logo_delay(&self) -> f64 {
        LOGO_GAP_MIN_MS + self.rng.next() * (LOGO_GAP_MAX_MS - LOGO_GAP_MIN_MS)
    }

    /// Never the same line twice running, whoever says it.
    // TODO(port): a one-line list ("LEVEL CLEAR", "GAME OVER") never enters
    // the loop, so the bubble repeats the previous line instead, as the
    // TypeScript did.
    fn say(&mut self, lines: &[&'static str], from: Speaker) {
        let mut text = self.last_line;
        while text == self.last_line && lines.len() > 1 {
            text = self.rng.pick(lines);
        }
        self.last_line = text;
        self.speech = Some(Speech {
            text,
            age: 0.0,
            from,
        });
    }

    fn clear_logo(&mut self) {
        self.logo = None;
        self.logo_timer = self.next_logo_delay();
    }

    fn scatter_corners(&self) -> Vec<(i32, i32)> {
        let Some(m) = &self.maze else {
            return vec![(0, 0)];
        };
        vec![
            (m.cols - 2, 1),
            (1, 1),
            (m.cols - 2, m.rows - 2),
            (1, m.rows - 2),
        ]
    }

    fn place_ghosts(&mut self) {
        let corners = self.scatter_corners();
        let mascots = self.rng.shuffled(&PROJECT_MASCOTS);
        let pen = self.pen;
        self.ghosts = mascots
            .iter()
            .take(GHOST_COUNT)
            .enumerate()
            .map(|(index, mascot)| Ghost {
                m: Mover::at(pen.0, pen.1),
                mascot: mascot.name,
                kind: index,
                state: GhostState::Pen,
                release_in: GHOST_RELEASE_MS.get(index).copied().unwrap_or(0.0),
                scatter: corners[index % corners.len()],
                steps: 0,
            })
            .collect();
    }

    /// Puts everyone back on their marks without touching the pellets.
    fn respawn(&mut self) {
        self.pac = Mover::at(self.pac_home.0, self.pac_home.1);
        self.pending = None;
        self.chew = 0.0;
        self.fright_left = 0.0;
        self.eaten_streak = 0;
        self.ghost_phase = Phase::Scatter;
        self.phase_left = SCATTER_MS;
        self.place_ghosts();
        self.status = Status::Run;
        self.status_left = 0.0;
    }

    fn fill_pellets(&mut self) {
        let Some(m) = self.maze.clone() else {
            return;
        };
        let mut pellets = vec![0u8; (m.cols * m.rows) as usize];
        for y in 0..m.rows {
            for x in 0..m.cols {
                if m.wall[(y * m.cols + x) as usize] == 1 {
                    continue;
                }
                pellets[(y * m.cols + x) as usize] = 1;
            }
        }
        // Nothing to hoover up in the pen or under pac-man's feet.
        for dy in -1..=1 {
            for dx in -1..=1 {
                let x = m.wrap_tx(self.pen.0 + dx);
                let y = self.pen.1 + dy;
                if y < 0 || y >= m.rows {
                    continue;
                }
                pellets[(y * m.cols + x) as usize] = 0;
            }
        }
        pellets[(self.pac_home.1 * m.cols + self.pac_home.0) as usize] = 0;

        for (cx, cy) in self.scatter_corners() {
            let spot = m.nearest_open(cx, cy);
            pellets[(spot.1 * m.cols + spot.0) as usize] = 2;
        }

        self.pellets_left = pellets.iter().filter(|value| **value != 0).count() as i64;
        self.pellets = pellets;
    }

    fn rebuild(&mut self) {
        self.tile = if self.player { PLAYER_TILE } else { IDLE_TILE };
        // Sized so the maze's own border wall falls just outside the pane on
        // all four sides: the outer corridor ring lands flush with the edge,
        // and the maze reads as running past the surface instead of sitting
        // in a margin.
        let odd_up = |count: i32| if count % 2 != 0 { count } else { count + 1 };
        let tile = f64::from(self.tile);
        let tile_cols = odd_up((f64::from(self.cols) / tile).ceil() as i32 + 2);
        let tile_rows = odd_up((f64::from(self.rows) / tile).ceil() as i32 + 2);
        if tile_cols < MIN_TILE_COLS || tile_rows < MIN_TILE_ROWS {
            self.maze = None;
            self.ghosts.clear();
            self.pellets.clear();
            self.pellets_left = 0;
            self.logo = None;
            self.speech = None;
            return;
        }

        let maze = build_maze(&self.rng, tile_cols, tile_rows);
        // One tile out on each side for the border ring, then the slack split.
        self.origin_x =
            -self.tile - (maze.cols * self.tile - 2 * self.tile - self.cols).div_euclid(2);
        self.origin_y =
            -self.tile - (maze.rows * self.tile - 2 * self.tile - self.rows).div_euclid(2);

        self.pen = maze.nearest_open(maze.cols / 2, maze.rows / 2);
        self.pac_home =
            maze.nearest_open(maze.cols / 2, (f64::from(maze.rows) * 0.78).floor() as i32);
        self.maze = Some(maze);
        self.fill_pellets();
        self.respawn();
        self.logo = None;
        self.logo_timer = self.next_logo_delay();
        self.speech = None;
    }

    fn boot(&mut self) {
        self.booted = 0.0;
        self.score = 0;
        self.lives = START_LIVES;
        self.rebuild();
    }

    /// Flood the maze, optionally treating a few tiles as walls. Returns the
    /// distance and the previous tile for every tile, and the start index.
    fn flood(
        &self,
        maze: &Maze,
        from: (i32, i32),
        blocked: Option<&[u8]>,
    ) -> (Vec<i32>, Vec<i32>, usize) {
        let size = (maze.cols * maze.rows) as usize;
        let mut dist = vec![-1i32; size];
        let mut prev = vec![-1i32; size];
        let mut queue = Vec::with_capacity(size);
        let mut head = 0;

        let start = (from.1 * maze.cols + from.0) as usize;
        dist[start] = 0;
        queue.push(start);
        while head < queue.len() {
            let at = queue[head];
            head += 1;
            let x = at as i32 % maze.cols;
            let y = at as i32 / maze.cols;
            for (dx, dy) in DIRS {
                let nx = maze.wrap_tx(x + dx);
                let ny = y + dy;
                if !maze.open(nx, ny) {
                    continue;
                }
                let next = (ny * maze.cols + nx) as usize;
                if dist[next] != -1 {
                    continue;
                }
                if blocked.is_some_and(|blocked| blocked[next] != 0) {
                    continue;
                }
                dist[next] = dist[at] + 1;
                prev[next] = at as i32;
                queue.push(next);
            }
        }
        (dist, prev, start)
    }

    /// The heading that starts the walk from `start` toward `goal`.
    fn first_step(
        &self,
        maze: &Maze,
        prev: &[i32],
        start: usize,
        goal: usize,
    ) -> Option<(i32, i32)> {
        let mut at = goal;
        while prev[at] != -1 && prev[at] != start as i32 {
            at = prev[at] as usize;
        }
        if prev[at] == -1 {
            return None;
        }
        let x = at as i32 % maze.cols;
        let y = at as i32 / maze.cols;
        let sx = start as i32 % maze.cols;
        let sy = start as i32 / maze.cols;
        Some((
            sign(maze.span_x(f64::from(sx), f64::from(x))),
            (y - sy).signum(),
        ))
    }

    /// The idle brain. It runs for pellets, detours for a logo, and only
    /// goes near a mascot once one is edible: the tiles around a hunting
    /// mascot count as walls for the search.
    fn think_pac(&mut self) {
        let Some(m) = self.maze.clone() else {
            return;
        };

        let mut danger = vec![0u8; (m.cols * m.rows) as usize];
        for ghost in &self.ghosts {
            if ghost.state != GhostState::Hunt {
                continue;
            }
            let gx = js::round(ghost.m.at_x()) as i32;
            let gy = js::round(ghost.m.at_y()) as i32;
            for dy in -2..=2i32 {
                for dx in -2..=2i32 {
                    if dx.abs() + dy.abs() > 2 {
                        continue;
                    }
                    let x = m.wrap_tx(gx + dx);
                    let y = gy + dy;
                    if y < 0 || y >= m.rows {
                        continue;
                    }
                    danger[(y * m.cols + x) as usize] = 1;
                }
            }
        }
        danger[(self.pac.ty * m.cols + self.pac.tx) as usize] = 0;

        let mut targets: Vec<(usize, f64)> = Vec::new();
        if self.fright_left > 0.0 {
            for ghost in &self.ghosts {
                if ghost.state != GhostState::Fright {
                    continue;
                }
                let x = m.wrap_tx(js::round(ghost.m.at_x()) as i32);
                let y = clamp_i(js::round(ghost.m.at_y()) as i32, 0, m.rows - 1);
                targets.push(((y * m.cols + x) as usize, EDIBLE_WEIGHT));
            }
        }
        if let Some(logo) = &self.logo {
            targets.push(((logo.ty * m.cols + logo.tx) as usize, LOGO_WEIGHT));
        }
        for (index, value) in self.pellets.iter().enumerate() {
            if *value == 0 {
                continue;
            }
            targets.push((index, if *value == 2 { ENERGIZER_WEIGHT } else { 1.0 }));
        }
        if targets.is_empty() {
            return;
        }

        let search = |blocked: Option<&[u8]>| {
            let (dist, prev, start) = self.flood(&m, (self.pac.tx, self.pac.ty), blocked);
            let mut best: Option<(usize, f64)> = None;
            for &(at, weight) in &targets {
                let steps = dist.get(at).copied().unwrap_or(-1);
                if steps < 0 {
                    continue;
                }
                let score = weight / f64::from(steps + 1);
                if best.is_none_or(|(_, best_score)| score > best_score) {
                    best = Some((at, score));
                }
            }
            let (goal, _) = best?;
            self.first_step(&m, &prev, start, goal)
        };

        let Some(heading) = search(Some(&danger)).or_else(|| search(None)) else {
            return;
        };
        if heading == (0, 0) {
            return;
        }
        self.pac.dx = heading.0;
        self.pac.dy = heading.1;
    }

    fn eat_at(&mut self, tx: i32, ty: i32) {
        let Some(m) = &self.maze else {
            return;
        };
        let at = (ty * m.cols + tx) as usize;
        let value = self.pellets.get(at).copied().unwrap_or(0);
        if value != 0 {
            self.pellets[at] = 0;
            self.pellets_left -= 1;
            self.score += if value == 2 {
                ENERGIZER_SCORE
            } else {
                PELLET_SCORE
            };
            if value == 2 {
                self.fright_left = FRIGHT_MS;
                self.eaten_streak = 0;
                let maze = self.maze.as_ref().expect("maze");
                for ghost in &mut self.ghosts {
                    if ghost.state == GhostState::Hunt {
                        ghost.state = GhostState::Fright;
                        ghost.m.reverse(maze);
                    }
                }
            }
        }

        if self
            .logo
            .as_ref()
            .is_some_and(|logo| logo.tx == tx && logo.ty == ty)
        {
            self.clear_logo();
            self.score += FRUIT_SCORE;
            self.say(FRUIT_CHATTER, Speaker::Pac);
        }

        if self.pellets_left <= 0 {
            self.status = Status::Clear;
            self.status_left = CLEAR_MS;
            self.say(&["LEVEL CLEAR"], Speaker::Pac);
        }
    }

    fn steer_pac(&mut self) {
        if self.player {
            if let Some((px, py)) = self.pending
                && self.open(self.pac.tx + px, self.pac.ty + py)
            {
                self.pac.dx = px;
                self.pac.dy = py;
                self.pending = None;
                return;
            }
            if self.open(self.pac.tx + self.pac.dx, self.pac.ty + self.pac.dy)
                && !self.pac.stopped()
            {
                return;
            }
            self.pac.dx = 0;
            self.pac.dy = 0;
            return;
        }
        self.think_pac();
        if !self.open(self.pac.tx + self.pac.dx, self.pac.ty + self.pac.dy) {
            self.pac.dx = 0;
            self.pac.dy = 0;
        }
    }

    fn target_for(&self, index: usize) -> (i32, i32) {
        let m = self.maze.as_ref().expect("maze");
        let ghost = &self.ghosts[index];
        if ghost.state == GhostState::Eyes {
            return self.pen;
        }
        if self.ghost_phase == Phase::Scatter {
            return ghost.scatter;
        }

        let px = self.pac.tx;
        let py = self.pac.ty;
        match ghost.kind {
            0 => (px, py),
            // Cuts the corner, aiming four tiles up the road.
            1 => (
                m.wrap_tx(px + self.pac.dx * 4),
                clamp_i(py + self.pac.dy * 4, 0, m.rows - 1),
            ),
            2 => {
                // Plays off the lead mascot, so the pair pincer instead of
                // queueing up.
                let ax = m.wrap_tx(px + self.pac.dx * 2);
                let ay = clamp_i(py + self.pac.dy * 2, 0, m.rows - 1);
                let Some(lead) = self.ghosts.first() else {
                    return (ax, ay);
                };
                (
                    m.wrap_tx(ax + m.span_x(f64::from(lead.m.tx), f64::from(ax)) as i32),
                    clamp_i(ay + (ay - lead.m.ty), 0, m.rows - 1),
                )
            }
            // Loses its nerve up close and heads for its corner instead.
            _ => {
                if m.tile_distance(ghost.m.tx, ghost.m.ty, px, py) > 64.0 {
                    (px, py)
                } else {
                    ghost.scatter
                }
            }
        }
    }

    fn steer_ghost(&mut self, index: usize) {
        let ghost = &self.ghosts[index];
        let options: Vec<(i32, i32)> = DIRS
            .into_iter()
            .filter(|&(dx, dy)| {
                self.open(ghost.m.tx + dx, ghost.m.ty + dy)
                    && !(dx == -ghost.m.dx && dy == -ghost.m.dy)
            })
            .collect();
        let moves: Vec<(i32, i32)> = if options.is_empty() {
            DIRS.into_iter()
                .filter(|&(dx, dy)| self.open(ghost.m.tx + dx, ghost.m.ty + dy))
                .collect()
        } else {
            options
        };
        if moves.is_empty() {
            let ghost = &mut self.ghosts[index];
            ghost.m.dx = 0;
            ghost.m.dy = 0;
            return;
        }

        if ghost.state == GhostState::Fright {
            let (dx, dy) = self.rng.pick(&moves);
            let ghost = &mut self.ghosts[index];
            ghost.m.dx = dx;
            ghost.m.dy = dy;
            return;
        }

        let target = self.target_for(index);
        let m = self.maze.as_ref().expect("maze");
        let ghost = &self.ghosts[index];
        let mut best = moves[0];
        let mut best_score = f64::INFINITY;
        for &(dx, dy) in &moves {
            let score = m.tile_distance(
                m.wrap_tx(ghost.m.tx + dx),
                ghost.m.ty + dy,
                target.0,
                target.1,
            );
            if score < best_score {
                best_score = score;
                best = (dx, dy);
            }
        }
        let ghost = &mut self.ghosts[index];
        ghost.m.dx = best.0;
        ghost.m.dy = best.1;
    }

    fn move_pac(&mut self, dt: f64) {
        self.steer_pac_if_stalled();
        if self.pac.stopped() {
            return;
        }

        let tick = self.pac_tick();
        self.pac.progress += dt / tick;
        self.chew += dt / tick;
        while self.pac.progress >= 1.0 {
            self.pac.progress -= 1.0;
            self.pac.tx = self.wrap_tx(self.pac.tx + self.pac.dx);
            self.pac.ty += self.pac.dy;
            self.eat_at(self.pac.tx, self.pac.ty);
            if self.status != Status::Run {
                self.pac.progress = 0.0;
                return;
            }
            self.steer_pac();
            if self.pac.stopped() {
                self.pac.progress = 0.0;
                break;
            }
        }
    }

    /// A stopped pac-man keeps trying the buffered turn every frame.
    fn steer_pac_if_stalled(&mut self) {
        if !self.pac.stopped() {
            return;
        }
        self.steer_pac();
    }

    fn move_ghost(&mut self, index: usize, dt: f64) {
        if self.ghosts[index].state == GhostState::Pen {
            let ghost = &mut self.ghosts[index];
            ghost.release_in -= dt;
            if ghost.release_in > 0.0 {
                return;
            }
            ghost.state = if self.fright_left > 0.0 {
                GhostState::Fright
            } else {
                GhostState::Hunt
            };
            self.steer_ghost(index);
            if self.ghosts[index].m.stopped() {
                return;
            }
        }
        if self.ghosts[index].m.stopped() {
            self.steer_ghost(index);
            if self.ghosts[index].m.stopped() {
                return;
            }
        }

        let tick = self.ghost_tick(&self.ghosts[index]);
        self.ghosts[index].m.progress += dt / tick;
        while self.ghosts[index].m.progress >= 1.0 {
            let pen = self.pen;
            let fright = self.fright_left > 0.0;
            let next_tx = self.wrap_tx(self.ghosts[index].m.tx + self.ghosts[index].m.dx);
            let ghost = &mut self.ghosts[index];
            ghost.m.progress -= 1.0;
            ghost.m.tx = next_tx;
            ghost.m.ty += ghost.m.dy;
            ghost.steps += 1;
            if ghost.state == GhostState::Eyes && ghost.m.tx == pen.0 && ghost.m.ty == pen.1 {
                ghost.state = if fright {
                    GhostState::Fright
                } else {
                    GhostState::Hunt
                };
            }
            self.steer_ghost(index);
            if self.ghosts[index].m.stopped() {
                self.ghosts[index].m.progress = 0.0;
                break;
            }
        }
    }

    fn die(&mut self) {
        self.status = Status::Dying;
        self.status_left = if self.player { DEATH_MS } else { IDLE_DEATH_MS };
        if self.player {
            self.lives = (self.lives - 1).max(0);
        }
    }

    fn collide(&mut self) {
        for index in 0..self.ghosts.len() {
            let ghost = &self.ghosts[index];
            if matches!(ghost.state, GhostState::Eyes | GhostState::Pen) {
                continue;
            }
            let dx = self.span_x(ghost.m.at_x(), self.pac.at_x()).abs();
            let dy = (ghost.m.at_y() - self.pac.at_y()).abs();
            if dx > 0.6 || dy > 0.6 {
                continue;
            }

            if ghost.state == GhostState::Fright {
                self.ghosts[index].state = GhostState::Eyes;
                self.score += GHOST_SCORES[self.eaten_streak.min(GHOST_SCORES.len() - 1)];
                self.eaten_streak += 1;
                self.say(CHOMP_CHATTER, Speaker::Pac);
                continue;
            }

            self.say(CAUGHT_CHATTER, Speaker::Ghost(index));
            self.die();
            return;
        }
    }

    fn spawn_logo(&mut self) {
        let Some(m) = self.maze.clone() else {
            return;
        };
        // Far enough to be worth a detour, close enough that pac-man can get
        // there before it times out. Walls make the real walk longer than
        // the crow flies.
        let reach = (LOGO_LIFE_MS * 0.3) / self.pac_tick();
        for _ in 0..60 {
            let x = 1 + (self.rng.next() * f64::from(m.cols - 2)).floor() as i32;
            let y = 1 + (self.rng.next() * f64::from(m.rows - 2)).floor() as i32;
            if !m.open(x, y) {
                continue;
            }
            // The maze overhangs the pane, so keep the logo where it fits whole.
            let cells = f64::from(self.tile.max(3));
            let left = self.cell_x(f64::from(x));
            let top = self.cell_y(f64::from(y));
            if left < 0.0 || left + cells > f64::from(self.cols) {
                continue;
            }
            if top < 0.0 || top + cells > f64::from(self.rows) {
                continue;
            }
            let steps = m.span_x(f64::from(self.pac.tx), f64::from(x)).abs()
                + f64::from((y - self.pac.ty).abs());
            if steps < 4.0 || steps > reach {
                continue;
            }
            self.logo = Some(Logo {
                harness: self.rng.pick(&HARNESSES),
                tx: x,
                ty: y,
                age: 0.0,
            });
            return;
        }
    }

    /// 0 while fading in, ramping to 1. Keeps resizes from popping.
    fn boot_alpha(&self) -> f64 {
        (self.booted / BOOT_FADE_MS).min(1.0)
    }

    fn speaker_cell(&self, from: Speaker) -> (f64, f64) {
        let mover = match from {
            Speaker::Pac => Some(&self.pac),
            Speaker::Ghost(index) => self.ghosts.get(index).map(|ghost| &ghost.m),
        };
        let Some(m) = mover else {
            return (0.0, 0.0);
        };
        let half = f64::from(self.tile - 1) / 2.0;
        (
            js::round(self.cell_x(m.at_x()) + half),
            js::round(self.cell_y(m.at_y()) + half),
        )
    }
}

fn speech_alpha(state: &Speech) -> f64 {
    let fade_in = (state.age / SPEECH_FADE_MS).min(1.0);
    let fade_out = (1.0 - (state.age - SPEECH_HOLD_MS) / SPEECH_FADE_MS).max(0.0);
    fade_in.min(fade_out)
}

impl GridArcade for PacmanArcade {
    fn resize(&mut self, next_cols: i32, next_rows: i32) {
        if next_cols == self.cols && next_rows == self.rows {
            return;
        }
        let keep_game = self.player && self.maze.is_some();
        self.cols = next_cols;
        self.rows = next_rows;
        if keep_game {
            self.rebuild();
        } else {
            self.boot();
        }
    }

    /// Hands pac-man over to the caller. The idle brain goes quiet.
    fn take_control(&mut self) {
        if self.player {
            return;
        }
        self.player = true;
        self.boot();
    }

    /// Puts the idle brain back in the seat.
    fn release_control(&mut self) {
        if !self.player {
            return;
        }
        self.player = false;
        self.boot();
    }

    /// Queue the next heading. Held until pac-man reaches a tile that opens
    /// that way, except for a turn on the spot, which lands immediately.
    fn steer(&mut self, x: i32, y: i32) {
        if !self.player || self.maze.is_none() {
            return;
        }
        if x.abs() + y.abs() != 1 {
            return;
        }
        if self.status != Status::Run {
            return;
        }
        if x == -self.pac.dx && y == -self.pac.dy && !self.pac.stopped() {
            let maze = self.maze.as_ref().expect("maze");
            self.pac.reverse(maze);
            self.pending = None;
            return;
        }
        self.pending = Some((x, y));
    }

    fn set_mode(&mut self, next: ArcadeMode) {
        self.mode = next;
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
        self.lives
    }

    /// True while the last life is spent and the board is sitting dark.
    fn game_over(&self) -> bool {
        self.status == Status::Over
    }

    fn step(&mut self, dt: f64) {
        self.booted += dt;
        self.blink = (self.blink + dt) % BLINK_MS;
        if self.maze.is_none() {
            return;
        }

        if let Some(speech) = &mut self.speech {
            speech.age += dt;
            if speech.age >= SPEECH_HOLD_MS + SPEECH_FADE_MS {
                self.speech = None;
            }
        }

        if self.status != Status::Run {
            self.status_left -= dt;
            if self.status_left > 0.0 {
                return;
            }
            if self.status == Status::Over {
                self.boot();
            } else if self.status == Status::Clear {
                let kept_score = self.score;
                let kept_lives = self.lives;
                self.rebuild();
                self.score = kept_score;
                self.lives = kept_lives;
            } else if self.player && self.lives <= 0 {
                self.status = Status::Over;
                self.status_left = OVER_MS;
                self.say(&["GAME OVER"], Speaker::Pac);
            } else {
                self.respawn();
            }
            return;
        }

        if self.fright_left > 0.0 {
            self.fright_left -= dt;
            if self.fright_left <= 0.0 {
                self.fright_left = 0.0;
                self.eaten_streak = 0;
                for ghost in &mut self.ghosts {
                    if ghost.state == GhostState::Fright {
                        ghost.state = GhostState::Hunt;
                    }
                }
            }
        } else {
            self.phase_left -= dt;
            if self.phase_left <= 0.0 {
                self.ghost_phase = if self.ghost_phase == Phase::Scatter {
                    Phase::Chase
                } else {
                    Phase::Scatter
                };
                self.phase_left = if self.ghost_phase == Phase::Scatter {
                    SCATTER_MS
                } else {
                    CHASE_MS
                };
                let maze = self.maze.as_ref().expect("maze");
                for ghost in &mut self.ghosts {
                    if ghost.state == GhostState::Hunt {
                        ghost.m.reverse(maze);
                    }
                }
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

        self.move_pac(dt);
        if self.status != Status::Run {
            return;
        }
        for index in 0..self.ghosts.len() {
            self.move_ghost(index, dt);
        }
        self.collide();
    }

    /// The line to float over whoever said it.
    fn speech_bubble(&self) -> Option<SpeechBubble> {
        let speech = self.speech.as_ref()?;
        self.maze.as_ref()?;
        let (x, y) = self.speaker_cell(speech.from);
        Some(SpeechBubble {
            text: speech.text,
            x,
            y,
            alpha: speech_alpha(speech) * self.boot_alpha(),
        })
    }

    /// The logo to paint over the grid, if one is on the board.
    fn logo_pickup(&self) -> Option<LogoPickup> {
        let logo = self.logo.as_ref()?;
        self.maze.as_ref()?;
        let fade_in = (logo.age / LOGO_FADE_MS).min(1.0);
        let fade_out = ((LOGO_LIFE_MS - logo.age) / LOGO_FADE_MS).min(1.0);
        let cells = self.tile.max(3);
        let inset = f64::from(self.tile - cells) / 2.0;
        Some(LogoPickup {
            harness: logo.harness,
            x: self.cell_x(f64::from(logo.tx)) + inset,
            y: self.cell_y(f64::from(logo.ty)) + inset,
            cells: f64::from(cells),
            alpha: fade_in.min(fade_out).max(0.0) * self.boot_alpha(),
        })
    }

    /// Pac-man and the mascots, ready to draw.
    fn sprites(&self) -> Vec<ArcadeSprite> {
        if self.maze.is_none() {
            return Vec::new();
        }
        let alpha = self.boot_alpha();
        let mut out = Vec::new();
        let half = f64::from(self.tile - 1) / 2.0;
        let tile = f64::from(self.tile);

        let dying = self.status == Status::Dying;
        let mouth = if dying {
            let death = if self.player { DEATH_MS } else { IDLE_DEATH_MS };
            (1.0 - self.status_left / death).clamp(0.0, 1.0)
        } else if self.status == Status::Over {
            1.0
        } else {
            0.32 * (self.chew * std::f64::consts::PI).sin().abs()
        };
        out.push(ArcadeSprite {
            kind: SpriteKind::Pacman,
            cx: self.cell_x(self.pac.at_x()) + half,
            cy: self.cell_y(self.pac.at_y()) + half,
            size: tile,
            dx: f64::from(if self.pac.dx != 0 { self.pac.dx } else { 1 }),
            dy: f64::from(self.pac.dy),
            alpha,
            mouth,
            mascot: None,
            frame: None,
            eyes: false,
        });

        if self.status == Status::Over {
            return out;
        }

        let flashing = self.fright_left > 0.0
            && self.fright_left < FRIGHT_FLASH_MS
            && (self.fright_left / FLASH_PERIOD_MS).floor() as i64 % 2 == 0;
        for ghost in &self.ghosts {
            if ghost.state == GhostState::Pen && ghost.release_in > 0.0 {
                continue;
            }
            let frightened = ghost.state == GhostState::Fright;
            let shade = if frightened && !flashing {
                0.4
            } else if dying {
                0.5
            } else {
                1.0
            };
            out.push(ArcadeSprite {
                kind: SpriteKind::Ghost,
                cx: self.cell_x(ghost.m.at_x()) + half,
                cy: self.cell_y(ghost.m.at_y()) + half,
                size: tile,
                dx: f64::from(ghost.m.dx),
                dy: f64::from(ghost.m.dy),
                alpha: alpha * shade,
                mouth: 0.0,
                mascot: Some(ghost.mascot),
                frame: Some(if ghost.steps % 2 == 0 {
                    SpriteFrame::Rest
                } else {
                    SpriteFrame::Talk
                }),
                eyes: ghost.state == GhostState::Eyes,
            });
        }
        out
    }

    /// 0 to 1 over the first paint after mount or resize.
    fn fade(&self) -> f64 {
        self.boot_alpha()
    }

    fn stamp(&self, out: &mut [f32], stamp_cols: usize, stamp_rows: usize) {
        let Some(m) = &self.maze else {
            return;
        };
        let alpha = self.boot_alpha();
        let dim = if self.status == Status::Over {
            0.35
        } else {
            1.0
        };
        let tile = self.tile;

        let pellet_offset = (tile - 1).div_euclid(2);
        for ty in 0..m.rows {
            for tx in 0..m.cols {
                let at = (ty * m.cols + tx) as usize;
                let px = self.origin_x + tx * tile;
                let py = self.origin_y + ty * tile;
                if m.wall[at] == 1 {
                    for dy in 0..tile {
                        for dx in 0..tile {
                            light(
                                out,
                                stamp_cols,
                                stamp_rows,
                                px + dx,
                                py + dy,
                                WALL_VALUE * dim,
                            );
                        }
                    }
                    continue;
                }
                let pellet = self.pellets.get(at).copied().unwrap_or(0);
                if pellet == 0 {
                    continue;
                }
                if pellet == 2 {
                    let span = (tile - 1).max(2);
                    let pulse =
                        0.72 + 0.28 * ((self.blink / BLINK_MS) * std::f64::consts::PI * 2.0).sin();
                    for dy in 0..span {
                        for dx in 0..span {
                            light(
                                out,
                                stamp_cols,
                                stamp_rows,
                                px + dx,
                                py + dy,
                                ENERGIZER_VALUE * pulse * dim,
                            );
                        }
                    }
                } else {
                    light(
                        out,
                        stamp_cols,
                        stamp_rows,
                        px + pellet_offset,
                        py + pellet_offset,
                        PELLET_VALUE * dim,
                    );
                }
            }
        }

        fade_stamp(out, alpha);
    }
}

#[cfg(test)]
#[path = "pacman_tests.rs"]
mod tests;
