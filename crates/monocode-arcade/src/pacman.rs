//! Mechanical translation of pacmanArcade.ts. Variable-dt, tie/draw order,
//! transition overshoot and the existing singleton-speech defect are kept.
use crate::{
    random::Random,
    types::{Bubble, Cell, Mode, Pickup, Sprite, SpriteKind},
};
use monocode_mascots::{MascotId, CATALOG};

const DIRS: [Cell; 4] = [
    Cell::new(1, 0),
    Cell::new(-1, 0),
    Cell::new(0, 1),
    Cell::new(0, -1),
];
const FRUIT_CHATTER: [&str; 19] = [
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
const CHOMP_CHATTER: [&str; 7] = [
    "GOTCHA",
    "SORRY, LITTLE GUY",
    "REVERSE UNO",
    "WHO'S CHASING NOW",
    "RESPAWN LATER",
    "TASTES LIKE PIXELS",
    "THAT'S ONE",
];
const CAUGHT_CHATTER: [&str; 7] = [
    "TAG, YOU'RE IT",
    "OUR TURN",
    "SNACK ACQUIRED",
    "MERGE CONFLICT",
    "SKILL ISSUE",
    "GOT ONE",
    "NOM",
];

#[derive(Clone, Debug)]
struct Maze {
    cols: i32,
    rows: i32,
    wall: Vec<u8>,
}
impl Maze {
    fn at(&self, x: i32, y: i32) -> usize {
        (y * self.cols + x) as usize
    }
    fn build(tile_cols: i32, tile_rows: i32, random: &mut dyn Random) -> Self {
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
        let mut m = Self {
            cols,
            rows,
            wall: vec![1; (cols * rows) as usize],
        };
        let carve = |m: &mut Self, x, y| {
            let at = m.at(x, y);
            m.wall[at] = 0;
        };
        let mut stack = vec![Cell::new(1, 1)];
        carve(&mut m, 1, 1);
        while let Some(cur) = stack.last().copied() {
            let mut moved = false;
            for dir in shuffled(DIRS, random) {
                let nx = cur.x + dir.x * 2;
                let ny = cur.y + dir.y * 2;
                if nx < 1 || ny < 1 || nx > cols - 2 || ny > rows - 2 || m.wall[m.at(nx, ny)] != 1 {
                    continue;
                }
                carve(&mut m, cur.x + dir.x, cur.y + dir.y);
                carve(&mut m, nx, ny);
                stack.push(Cell::new(nx, ny));
                moved = true;
                break;
            }
            if !moved {
                stack.pop();
            }
        }
        for y in (1..rows - 1).step_by(2) {
            for x in (1..cols - 1).step_by(2) {
                let exits = DIRS
                    .iter()
                    .filter(|d| m.wall[m.at(x + d.x, y + d.y)] != 1)
                    .count();
                if exits > 1 {
                    continue;
                }
                let opened = shuffled(DIRS, random).into_iter().find(|d| {
                    m.wall[m.at(x + d.x, y + d.y)] == 1
                        && x + d.x * 2 >= 1
                        && y + d.y * 2 >= 1
                        && x + d.x * 2 <= cols - 2
                        && y + d.y * 2 <= rows - 2
                });
                if let Some(d) = opened {
                    carve(&mut m, x + d.x, y + d.y);
                }
            }
        }
        for y in 1..rows - 1 {
            for x in 1..cols - 1 {
                if m.wall[m.at(x, y)] != 1 || (x % 2 == 1) == (y % 2 == 1) {
                    continue;
                }
                if random.next_f64() > 0.12 {
                    continue;
                }
                carve(&mut m, x, y);
            }
        }
        let tunnel = if (rows / 2) % 2 == 1 {
            rows / 2
        } else {
            rows / 2 + 1
        };
        for x in 0..cols {
            carve(&mut m, x, tunnel.clamp(1, rows - 2));
        }
        m
    }
}
fn shuffled<T, const N: usize>(mut items: [T; N], random: &mut dyn Random) -> [T; N] {
    for i in (1..N).rev() {
        let j = (random.next_f64() * (i + 1) as f64).floor() as usize;
        items.swap(i, j);
    }
    items
}
fn js_round(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}

#[derive(Clone, Copy, Debug, Default)]
struct Mover {
    tx: i32,
    ty: i32,
    dx: i32,
    dy: i32,
    progress: f64,
}
impl Mover {
    fn at_x(self) -> f64 {
        f64::from(self.tx) + f64::from(self.dx) * self.progress
    }
    fn at_y(self) -> f64 {
        f64::from(self.ty) + f64::from(self.dy) * self.progress
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GhostState {
    Pen,
    Hunt,
    Fright,
    Eyes,
}
#[derive(Clone, Debug)]
struct Ghost {
    mover: Mover,
    mascot: MascotId,
    kind: usize,
    state: GhostState,
    release_in: f64,
    scatter: Cell,
    steps: u32,
}
#[derive(Clone, Copy, Debug)]
enum Speaker {
    Pac,
    Ghost(usize),
}
#[derive(Clone, Debug)]
struct Speech {
    text: &'static str,
    age: f64,
    from: Speaker,
}
#[derive(Clone, Debug)]
struct Logo {
    provider: usize,
    tx: i32,
    ty: i32,
    age: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Run,
    Dying,
    Clear,
    Over,
}

/// Reused search buffers. Search still visits targets and neighbors
/// in source order; only storage, not decisions or timing, changes.
#[derive(Default)]
struct AiScratch {
    danger: Vec<u8>,
    dist: Vec<i32>,
    prev: Vec<i32>,
    queue: Vec<usize>,
    targets: Vec<(usize, f64)>,
}

pub struct Pacman {
    providers: Vec<String>,
    cols: i32,
    rows: i32,
    tile: i32,
    origin_x: i32,
    origin_y: i32,
    maze: Option<Maze>,
    pellets: Vec<u8>,
    pellets_left: usize,
    pac: Mover,
    pac_home: Cell,
    pending: Option<Cell>,
    chew: f64,
    ghosts: Vec<Ghost>,
    pen: Cell,
    scatter: bool,
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
    score: u32,
    lives: u32,
    mode: Mode,
    ai: AiScratch,
}
impl Pacman {
    pub fn new(providers: Vec<String>) -> Self {
        Self {
            providers,
            cols: 0,
            rows: 0,
            tile: 3,
            origin_x: 0,
            origin_y: 0,
            maze: None,
            pellets: vec![],
            pellets_left: 0,
            pac: Mover::default(),
            pac_home: Cell::default(),
            pending: None,
            chew: 0.,
            ghosts: vec![],
            pen: Cell::default(),
            scatter: true,
            phase_left: 7000.,
            fright_left: 0.,
            eaten_streak: 0,
            status: Status::Run,
            status_left: 0.,
            logo: None,
            logo_timer: 0.,
            speech: None,
            last_line: "",
            booted: 0.,
            blink: 0.,
            player: false,
            score: 0,
            lives: 3,
            mode: Mode::Mid,
            ai: AiScratch::default(),
        }
    }
    fn wrap_tx(&self, x: i32) -> i32 {
        self.maze.as_ref().map_or(0, |m| x.rem_euclid(m.cols))
    }
    fn open(&self, x: i32, y: i32) -> bool {
        self.maze
            .as_ref()
            .is_some_and(|m| y >= 0 && y < m.rows && m.wall[m.at(self.wrap_tx(x), y)] == 0)
    }
    fn span_x(&self, a: f64, b: f64) -> f64 {
        let Some(m) = &self.maze else {
            return 0.;
        };
        let direct = b - a;
        let sign = if direct > 0. {
            1.
        } else if direct < 0. {
            -1.
        } else {
            0.
        };
        let around = direct - sign * f64::from(m.cols);
        if direct.abs() <= around.abs() {
            direct
        } else {
            around
        }
    }
    fn tile_distance(&self, a: Cell, b: Cell) -> f64 {
        let dx = self.span_x(f64::from(a.x), f64::from(b.x));
        let dy = f64::from(b.y - a.y);
        dx * dx + dy * dy
    }
    fn nearest_open(&self, x: i32, y: i32) -> Cell {
        let Some(m) = &self.maze else {
            return Cell::default();
        };
        let start = Cell::new(x.clamp(1, m.cols - 2), y.clamp(1, m.rows - 2));
        if self.open(start.x, start.y) {
            return start;
        }
        for radius in 1..m.cols.max(m.rows) {
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let nx = start.x + dx;
                    let ny = start.y + dy;
                    if ny < 1 || ny > m.rows - 2 {
                        continue;
                    }
                    if self.open(nx, ny) {
                        return Cell::new(self.wrap_tx(nx), ny);
                    }
                }
            }
        }
        start
    }
    fn cell_x(&self, tx: f64) -> f64 {
        f64::from(self.origin_x) + tx * f64::from(self.tile)
    }
    fn cell_y(&self, ty: f64) -> f64 {
        f64::from(self.origin_y) + ty * f64::from(self.tile)
    }
    fn pac_tick(&self) -> f64 {
        let tick = if self.player {
            match self.mode {
                Mode::Low => 250.,
                Mode::Mid => 185.,
                Mode::Hard => 135.,
            }
        } else {
            240.
        };
        tick * (f64::from(self.tile) / 4.)
    }
    fn next_logo_delay(random: &mut dyn Random) -> f64 {
        4200. + random.next_f64() * (10500. - 4200.)
    }
    fn say(&mut self, lines: &[&'static str], from: Speaker, random: &mut dyn Random) {
        // Keep the reference singleton bug: it repeats last_line instead of
        // selecting LEVEL CLEAR/GAME OVER. Correct only in a separate change.
        let mut text = self.last_line;
        while text == self.last_line && lines.len() > 1 {
            text = lines[(random.next_f64() * lines.len() as f64).floor() as usize];
        }
        self.last_line = text;
        self.speech = Some(Speech {
            text,
            age: 0.,
            from,
        });
    }
    fn clear_logo(&mut self, random: &mut dyn Random) {
        self.logo = None;
        self.logo_timer = Self::next_logo_delay(random);
    }
    fn scatter_corners(&self) -> Vec<Cell> {
        self.maze.as_ref().map_or_else(
            || vec![Cell::default()],
            |m| {
                vec![
                    Cell::new(m.cols - 2, 1),
                    Cell::new(1, 1),
                    Cell::new(m.cols - 2, m.rows - 2),
                    Cell::new(1, m.rows - 2),
                ]
            },
        )
    }
    fn place_ghosts(&mut self, random: &mut dyn Random) {
        let corners = self.scatter_corners();
        self.ghosts = shuffled(CATALOG.map(|m| m.id), random)
            .into_iter()
            .take(4)
            .enumerate()
            .map(|(index, mascot)| Ghost {
                mover: Mover {
                    tx: self.pen.x,
                    ty: self.pen.y,
                    ..Mover::default()
                },
                mascot,
                kind: index,
                state: GhostState::Pen,
                release_in: [0., 1600., 3600., 6000.][index],
                scatter: corners[index % corners.len()],
                steps: 0,
            })
            .collect();
    }
    fn respawn(&mut self, random: &mut dyn Random) {
        self.pac = Mover {
            tx: self.pac_home.x,
            ty: self.pac_home.y,
            ..Mover::default()
        };
        self.pending = None;
        self.chew = 0.;
        self.fright_left = 0.;
        self.eaten_streak = 0;
        self.scatter = true;
        self.phase_left = 7000.;
        self.place_ghosts(random);
        self.status = Status::Run;
        self.status_left = 0.;
    }
    fn fill_pellets(&mut self) {
        let Some(m) = &self.maze else {
            return;
        };
        self.pellets = m.wall.iter().map(|wall| u8::from(*wall != 1)).collect();
        for dy in -1..=1 {
            for dx in -1..=1 {
                let x = self.wrap_tx(self.pen.x + dx);
                let y = self.pen.y + dy;
                if y >= 0 && y < m.rows {
                    self.pellets[m.at(x, y)] = 0;
                }
            }
        }
        self.pellets[m.at(self.pac_home.x, self.pac_home.y)] = 0;
        for corner in self.scatter_corners() {
            let spot = self.nearest_open(corner.x, corner.y);
            self.pellets[self.maze.as_ref().unwrap().at(spot.x, spot.y)] = 2;
        }
        self.pellets_left = self.pellets.iter().filter(|p| **p != 0).count();
    }
    fn rebuild(&mut self, random: &mut dyn Random) {
        self.tile = if self.player { 4 } else { 3 };
        let odd_up = |n: i32| if n % 2 != 0 { n } else { n + 1 };
        let tc = odd_up((self.cols + self.tile - 1) / self.tile + 2);
        let tr = odd_up((self.rows + self.tile - 1) / self.tile + 2);
        if tc < 7 || tr < 5 {
            self.maze = None;
            self.ghosts.clear();
            self.pellets.clear();
            self.pellets_left = 0;
            self.logo = None;
            self.speech = None;
            return;
        }
        let maze = Maze::build(tc, tr, random);
        self.origin_x =
            -self.tile - (maze.cols * self.tile - 2 * self.tile - self.cols).div_euclid(2);
        self.origin_y =
            -self.tile - (maze.rows * self.tile - 2 * self.tile - self.rows).div_euclid(2);
        let (mc, mr) = (maze.cols, maze.rows);
        self.maze = Some(maze);
        self.pen = self.nearest_open(mc / 2, mr / 2);
        self.pac_home = self.nearest_open(mc / 2, (f64::from(mr) * 0.78).floor() as i32);
        self.fill_pellets();
        self.respawn(random);
        self.logo = None;
        self.logo_timer = Self::next_logo_delay(random);
        self.speech = None;
    }
    fn boot(&mut self, random: &mut dyn Random) {
        self.booted = 0.;
        self.score = 0;
        self.lives = 3;
        self.rebuild(random);
    }
    fn flood_into(
        &self,
        from: Cell,
        blocked: Option<&[u8]>,
        dist: &mut Vec<i32>,
        prev: &mut Vec<i32>,
        queue: &mut Vec<usize>,
    ) -> usize {
        let m = self.maze.as_ref().unwrap();
        let size = (m.cols * m.rows) as usize;
        dist.resize(size, -1);
        dist.fill(-1);
        prev.resize(size, -1);
        prev.fill(-1);
        queue.clear();
        let start = m.at(from.x, from.y);
        dist[start] = 0;
        queue.push(start);
        let mut head = 0;
        while head < queue.len() {
            let at = queue[head];
            head += 1;
            let x = at as i32 % m.cols;
            let y = at as i32 / m.cols;
            for dir in DIRS {
                let nx = self.wrap_tx(x + dir.x);
                let ny = y + dir.y;
                if !self.open(nx, ny) {
                    continue;
                }
                let next = m.at(nx, ny);
                if dist[next] != -1 || blocked.is_some_and(|b| b[next] != 0) {
                    continue;
                }
                dist[next] = dist[at] + 1;
                prev[next] = at as i32;
                queue.push(next);
            }
        }
        start
    }
    fn first_step(&self, prev: &[i32], start: usize, goal: usize) -> Option<Cell> {
        let m = self.maze.as_ref().unwrap();
        let mut at = goal;
        while prev[at] != -1 && prev[at] != start as i32 {
            at = prev[at] as usize;
        }
        if prev[at] == -1 {
            return None;
        }
        let (x, y) = (at as i32 % m.cols, at as i32 / m.cols);
        let (sx, sy) = (start as i32 % m.cols, start as i32 / m.cols);
        let span = self.span_x(f64::from(sx), f64::from(x)) as i32;
        Some(Cell::new(span.signum(), (y - sy).signum()))
    }
    fn think_pac(&mut self) {
        let mut scratch = std::mem::take(&mut self.ai);
        let heading = self.think_pac_with(&mut scratch);
        self.ai = scratch;
        if let Some(heading) = heading {
            if heading.x != 0 || heading.y != 0 {
                self.pac.dx = heading.x;
                self.pac.dy = heading.y;
            }
        }
    }
    fn think_pac_with(&self, scratch: &mut AiScratch) -> Option<Cell> {
        let m = self.maze.as_ref()?;
        scratch.danger.resize((m.cols * m.rows) as usize, 0);
        scratch.danger.fill(0);
        for ghost in &self.ghosts {
            if ghost.state != GhostState::Hunt {
                continue;
            }
            let gx = js_round(ghost.mover.at_x());
            let gy = js_round(ghost.mover.at_y());
            for dy in -2_i32..=2 {
                for dx in -2_i32..=2 {
                    if dx.abs() + dy.abs() > 2 {
                        continue;
                    }
                    let x = self.wrap_tx(gx + dx);
                    let y = gy + dy;
                    if y >= 0 && y < m.rows {
                        scratch.danger[m.at(x, y)] = 1;
                    }
                }
            }
        }
        scratch.danger[m.at(self.pac.tx, self.pac.ty)] = 0;
        scratch.targets.clear();
        if self.fright_left > 0. {
            for g in &self.ghosts {
                if g.state != GhostState::Fright {
                    continue;
                }
                let x = self.wrap_tx(js_round(g.mover.at_x()));
                let y = js_round(g.mover.at_y()).clamp(0, m.rows - 1);
                scratch.targets.push((m.at(x, y), 40.));
            }
        }
        if let Some(l) = &self.logo {
            scratch.targets.push((m.at(l.tx, l.ty), 60.));
        }
        for (i, pellet) in self.pellets.iter().enumerate() {
            if *pellet != 0 {
                scratch
                    .targets
                    .push((i, if *pellet == 2 { 2. } else { 1. }));
            }
        }
        if scratch.targets.is_empty() {
            return None;
        }
        self.search(scratch, true)
            .or_else(|| self.search(scratch, false))
    }
    fn search(&self, scratch: &mut AiScratch, blocked: bool) -> Option<Cell> {
        let start = self.flood_into(
            Cell::new(self.pac.tx, self.pac.ty),
            blocked.then_some(scratch.danger.as_slice()),
            &mut scratch.dist,
            &mut scratch.prev,
            &mut scratch.queue,
        );
        let mut best: Option<(usize, f64)> = None;
        for &(at, weight) in &scratch.targets {
            let steps = scratch.dist.get(at).copied().unwrap_or(-1);
            if steps < 0 {
                continue;
            }
            let score = weight / f64::from(steps + 1);
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((at, score));
            }
        }
        best.and_then(|(at, _)| self.first_step(&scratch.prev, start, at))
    }
    fn reverse(&self, m: &mut Mover) {
        if m.dx == 0 && m.dy == 0 {
            return;
        }
        if m.progress > 0. {
            m.tx = self.wrap_tx(m.tx + m.dx);
            m.ty += m.dy;
            m.progress = 1. - m.progress;
        }
        m.dx = -m.dx;
        m.dy = -m.dy;
    }
    fn eat_at(&mut self, tx: i32, ty: i32, random: &mut dyn Random) {
        let Some(m) = &self.maze else {
            return;
        };
        let at = m.at(tx, ty);
        let value = self.pellets[at];
        if value != 0 {
            self.pellets[at] = 0;
            self.pellets_left -= 1;
            self.score += if value == 2 { 50 } else { 10 };
            if value == 2 {
                self.fright_left = 7000.;
                self.eaten_streak = 0;
                for i in 0..self.ghosts.len() {
                    if self.ghosts[i].state == GhostState::Hunt {
                        let mut mover = self.ghosts[i].mover;
                        self.reverse(&mut mover);
                        self.ghosts[i].mover = mover;
                        self.ghosts[i].state = GhostState::Fright;
                    }
                }
            }
        }
        if self.logo.as_ref().is_some_and(|l| l.tx == tx && l.ty == ty) {
            self.clear_logo(random);
            self.score += 200;
            self.say(&FRUIT_CHATTER, Speaker::Pac, random);
        }
        if self.pellets_left == 0 {
            self.status = Status::Clear;
            self.status_left = 1200.;
            self.say(&["LEVEL CLEAR"], Speaker::Pac, random);
        }
    }
    fn steer_pac(&mut self) {
        if self.player {
            if let Some(p) = self.pending {
                if self.open(self.pac.tx + p.x, self.pac.ty + p.y) {
                    self.pac.dx = p.x;
                    self.pac.dy = p.y;
                    self.pending = None;
                    return;
                }
            }
            if self.open(self.pac.tx + self.pac.dx, self.pac.ty + self.pac.dy)
                && (self.pac.dx != 0 || self.pac.dy != 0)
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
    fn target_for(&self, g: &Ghost) -> Cell {
        let m = self.maze.as_ref().unwrap();
        if g.state == GhostState::Eyes {
            return self.pen;
        }
        if self.scatter {
            return g.scatter;
        }
        let (px, py) = (self.pac.tx, self.pac.ty);
        match g.kind {
            0 => Cell::new(px, py),
            1 => Cell::new(
                self.wrap_tx(px + self.pac.dx * 4),
                (py + self.pac.dy * 4).clamp(0, m.rows - 1),
            ),
            2 => {
                let ax = self.wrap_tx(px + self.pac.dx * 2);
                let ay = (py + self.pac.dy * 2).clamp(0, m.rows - 1);
                self.ghosts.first().map_or(Cell::new(ax, ay), |lead| {
                    Cell::new(
                        self.wrap_tx(
                            ax + self.span_x(f64::from(lead.mover.tx), f64::from(ax)) as i32,
                        ),
                        (ay + (ay - lead.mover.ty)).clamp(0, m.rows - 1),
                    )
                })
            }
            _ => {
                if self.tile_distance(Cell::new(g.mover.tx, g.mover.ty), Cell::new(px, py)) > 64. {
                    Cell::new(px, py)
                } else {
                    g.scatter
                }
            }
        }
    }
    fn steer_ghost(&self, g: &mut Ghost, random: &mut dyn Random) {
        let mut moves = [Cell::default(); 4];
        let mut count = 0;
        for d in DIRS {
            if self.open(g.mover.tx + d.x, g.mover.ty + d.y)
                && !(d.x == -g.mover.dx && d.y == -g.mover.dy)
            {
                moves[count] = d;
                count += 1;
            }
        }
        if count == 0 {
            for d in DIRS {
                if self.open(g.mover.tx + d.x, g.mover.ty + d.y) {
                    moves[count] = d;
                    count += 1;
                }
            }
        }
        if count == 0 {
            g.mover.dx = 0;
            g.mover.dy = 0;
            return;
        }
        let choice = if g.state == GhostState::Fright {
            moves[(random.next_f64() * count as f64).floor() as usize]
        } else {
            let target = self.target_for(g);
            let mut best = moves[0];
            let mut best_score = f64::INFINITY;
            for &dir in &moves[..count] {
                let score = self.tile_distance(
                    Cell::new(self.wrap_tx(g.mover.tx + dir.x), g.mover.ty + dir.y),
                    target,
                );
                if score < best_score {
                    best_score = score;
                    best = dir;
                }
            }
            best
        };
        g.mover.dx = choice.x;
        g.mover.dy = choice.y;
    }
    fn move_pac(&mut self, dt: f64, random: &mut dyn Random) {
        if self.pac.dx == 0 && self.pac.dy == 0 {
            self.steer_pac();
        }
        if self.pac.dx == 0 && self.pac.dy == 0 {
            return;
        }
        let tick = self.pac_tick();
        self.pac.progress += dt / tick;
        self.chew += dt / tick;
        while self.pac.progress >= 1. {
            self.pac.progress -= 1.;
            self.pac.tx = self.wrap_tx(self.pac.tx + self.pac.dx);
            self.pac.ty += self.pac.dy;
            self.eat_at(self.pac.tx, self.pac.ty, random);
            if self.status != Status::Run {
                self.pac.progress = 0.;
                return;
            }
            self.steer_pac();
            if self.pac.dx == 0 && self.pac.dy == 0 {
                self.pac.progress = 0.;
                break;
            }
        }
    }
    fn move_ghost(&self, g: &mut Ghost, dt: f64, random: &mut dyn Random) {
        if g.state == GhostState::Pen {
            g.release_in -= dt;
            if g.release_in > 0. {
                return;
            }
            g.state = if self.fright_left > 0. {
                GhostState::Fright
            } else {
                GhostState::Hunt
            };
            self.steer_ghost(g, random);
            if g.mover.dx == 0 && g.mover.dy == 0 {
                return;
            }
        }
        if g.mover.dx == 0 && g.mover.dy == 0 {
            self.steer_ghost(g, random);
            if g.mover.dx == 0 && g.mover.dy == 0 {
                return;
            }
        }
        let speed = match g.state {
            GhostState::Eyes => 2.4,
            GhostState::Fright => 0.55,
            _ => 0.92,
        };
        g.mover.progress += dt / (self.pac_tick() / speed);
        while g.mover.progress >= 1. {
            g.mover.progress -= 1.;
            g.mover.tx = self.wrap_tx(g.mover.tx + g.mover.dx);
            g.mover.ty += g.mover.dy;
            g.steps += 1;
            if g.state == GhostState::Eyes && g.mover.tx == self.pen.x && g.mover.ty == self.pen.y {
                g.state = if self.fright_left > 0. {
                    GhostState::Fright
                } else {
                    GhostState::Hunt
                };
            }
            self.steer_ghost(g, random);
            if g.mover.dx == 0 && g.mover.dy == 0 {
                g.mover.progress = 0.;
                break;
            }
        }
    }
    fn die(&mut self) {
        self.status = Status::Dying;
        self.status_left = if self.player { 1500. } else { 900. };
        if self.player {
            self.lives = self.lives.saturating_sub(1);
        }
    }
    fn collide(&mut self, random: &mut dyn Random) {
        for i in 0..self.ghosts.len() {
            let g = &self.ghosts[i];
            if g.state == GhostState::Eyes || g.state == GhostState::Pen {
                continue;
            }
            let dx = self.span_x(g.mover.at_x(), self.pac.at_x()).abs();
            let dy = (g.mover.at_y() - self.pac.at_y()).abs();
            if dx > 0.6 || dy > 0.6 {
                continue;
            }
            if g.state == GhostState::Fright {
                self.ghosts[i].state = GhostState::Eyes;
                self.score += [200, 400, 800, 1600][self.eaten_streak.min(3)];
                self.eaten_streak += 1;
                self.say(&CHOMP_CHATTER, Speaker::Pac, random);
                continue;
            }
            self.say(&CAUGHT_CHATTER, Speaker::Ghost(i), random);
            self.die();
            return;
        }
    }
    fn spawn_logo(&mut self, random: &mut dyn Random) {
        let Some(m) = &self.maze else {
            return;
        };
        // Unlike the old undefined-provider output, an empty host catalog safely
        // disables logos. For nonempty catalogs all draw/attempt order is kept.
        if self.providers.is_empty() {
            return;
        }
        let reach = (12000. * 0.3) / self.pac_tick();
        for _ in 0..60 {
            let x = 1 + (random.next_f64() * f64::from(m.cols - 2)).floor() as i32;
            let y = 1 + (random.next_f64() * f64::from(m.rows - 2)).floor() as i32;
            if !self.open(x, y) {
                continue;
            }
            let cells = self.tile.max(3);
            let cx = self.cell_x(f64::from(x));
            let cy = self.cell_y(f64::from(y));
            if cx < 0.
                || cx + f64::from(cells) > f64::from(self.cols)
                || cy < 0.
                || cy + f64::from(cells) > f64::from(self.rows)
            {
                continue;
            }
            let steps = self.span_x(f64::from(self.pac.tx), f64::from(x)).abs()
                + f64::from((y - self.pac.ty).abs());
            if steps < 4. || steps > reach {
                continue;
            }
            let provider = (random.next_f64() * self.providers.len() as f64).floor() as usize;
            self.logo = Some(Logo {
                provider,
                tx: x,
                ty: y,
                age: 0.,
            });
            return;
        }
    }
    pub fn resize(&mut self, cols: i32, rows: i32, random: &mut dyn Random) {
        assert!(
            cols >= 0 && rows >= 0,
            "grid dimensions must be nonnegative"
        );
        if cols == self.cols && rows == self.rows {
            return;
        }
        let keep = self.player && self.maze.is_some();
        self.cols = cols;
        self.rows = rows;
        if keep {
            self.rebuild(random);
        } else {
            self.boot(random);
        }
    }
    pub fn take_control(&mut self, random: &mut dyn Random) {
        if !self.player {
            self.player = true;
            self.boot(random);
        }
    }
    pub fn release_control(&mut self, random: &mut dyn Random) {
        if self.player {
            self.player = false;
            self.boot(random);
        }
    }
    pub fn steer(&mut self, x: i32, y: i32) {
        if !self.player
            || self.maze.is_none()
            || !Cell::new(x, y).is_cardinal()
            || self.status != Status::Run
        {
            return;
        }
        if x == -self.pac.dx && y == -self.pac.dy && (self.pac.dx != 0 || self.pac.dy != 0) {
            let mut p = self.pac;
            self.reverse(&mut p);
            self.pac = p;
            self.pending = None;
            return;
        }
        self.pending = Some(Cell::new(x, y));
    }
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }
    pub fn controlled(&self) -> bool {
        self.player
    }
    pub fn score(&self) -> u32 {
        self.score
    }
    pub fn lives(&self) -> u32 {
        self.lives
    }
    pub fn game_over(&self) -> bool {
        self.status == Status::Over
    }
    pub fn fade(&self) -> f64 {
        (self.booted / 420.).min(1.)
    }
    pub fn step(&mut self, dt: f64, random: &mut dyn Random) {
        self.booted += dt;
        self.blink = (self.blink + dt) % 900.;
        if self.maze.is_none() {
            return;
        }
        if let Some(speech) = &mut self.speech {
            speech.age += dt;
            if speech.age >= 2460. {
                self.speech = None;
            }
        }
        if self.status != Status::Run {
            self.status_left -= dt;
            if self.status_left > 0. {
                return;
            }
            match self.status {
                Status::Over => self.boot(random),
                Status::Clear => {
                    let (score, lives) = (self.score, self.lives);
                    self.rebuild(random);
                    self.score = score;
                    self.lives = lives;
                }
                _ => {
                    if self.player && self.lives == 0 {
                        self.status = Status::Over;
                        self.status_left = 2600.;
                        self.say(&["GAME OVER"], Speaker::Pac, random);
                    } else {
                        self.respawn(random);
                    }
                }
            }
            return;
        }
        if self.fright_left > 0. {
            self.fright_left -= dt;
            if self.fright_left <= 0. {
                self.fright_left = 0.;
                self.eaten_streak = 0;
                for g in &mut self.ghosts {
                    if g.state == GhostState::Fright {
                        g.state = GhostState::Hunt;
                    }
                }
            }
        } else {
            self.phase_left -= dt;
            if self.phase_left <= 0. {
                self.scatter = !self.scatter;
                self.phase_left = if self.scatter { 7000. } else { 20000. };
                for i in 0..self.ghosts.len() {
                    if self.ghosts[i].state == GhostState::Hunt {
                        let mut mover = self.ghosts[i].mover;
                        self.reverse(&mut mover);
                        self.ghosts[i].mover = mover;
                    }
                }
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
        self.move_pac(dt, random);
        if self.status != Status::Run {
            return;
        }
        // Sequential copies preserve the updated lead ghost seen by kind 2,
        // without aliasing the mutable ghosts vector during targeting.
        for i in 0..self.ghosts.len() {
            let mut g = self.ghosts[i].clone();
            self.move_ghost(&mut g, dt, random);
            self.ghosts[i] = g;
        }
        self.collide(random);
    }
    pub fn speech_bubble(&self) -> Option<Bubble> {
        let speech = self.speech.as_ref()?;
        self.maze.as_ref()?;
        let mover = match speech.from {
            Speaker::Pac => Some(self.pac),
            Speaker::Ghost(i) => self.ghosts.get(i).map(|g| g.mover),
        };
        let (x, y) = mover.map_or((0., 0.), |m| {
            (
                f64::from(js_round(
                    self.cell_x(m.at_x()) + f64::from(self.tile - 1) / 2.,
                )),
                f64::from(js_round(
                    self.cell_y(m.at_y()) + f64::from(self.tile - 1) / 2.,
                )),
            )
        });
        let alpha = (speech.age / 260.)
            .min(1.)
            .min((1. - (speech.age - 2200.) / 260.).max(0.));
        Some(Bubble {
            text: speech.text,
            x,
            y,
            alpha: alpha * self.fade(),
        })
    }
    pub fn logo_pickup(&self) -> Option<Pickup<'_>> {
        let logo = self.logo.as_ref()?;
        self.maze.as_ref()?;
        let cells = self.tile.max(3);
        let alpha = (logo.age / 500.)
            .min(1.)
            .min(((12000. - logo.age) / 500.).min(1.))
            .max(0.);
        Some(Pickup {
            provider: &self.providers[logo.provider],
            x: self.cell_x(f64::from(logo.tx)) + f64::from(self.tile - cells) / 2.,
            y: self.cell_y(f64::from(logo.ty)) + f64::from(self.tile - cells) / 2.,
            cells: f64::from(cells),
            alpha: alpha * self.fade(),
        })
    }
    pub fn write_sprites(&self, out: &mut Vec<Sprite>) {
        out.clear();
        if self.maze.is_none() {
            return;
        }
        let alpha = self.fade();
        let dying = self.status == Status::Dying;
        let mouth = if dying {
            (1. - self.status_left / if self.player { 1500. } else { 900. }).clamp(0., 1.)
        } else if self.status == Status::Over {
            1.
        } else {
            0.32 * (self.chew * std::f64::consts::PI).sin().abs()
        };
        out.push(Sprite {
            kind: SpriteKind::Pacman,
            cx: self.cell_x(self.pac.at_x()) + f64::from(self.tile - 1) / 2.,
            cy: self.cell_y(self.pac.at_y()) + f64::from(self.tile - 1) / 2.,
            size: f64::from(self.tile),
            dx: self.pac.dx,
            dy: self.pac.dy,
            alpha,
            mouth,
            mascot: None,
            talk: false,
            eyes: false,
        });
        if self.status == Status::Over {
            return;
        }
        let flashing = self.fright_left > 0.
            && self.fright_left < 2200.
            && (self.fright_left / 260.).floor() as i64 % 2 == 0;
        for g in &self.ghosts {
            if g.state == GhostState::Pen && g.release_in > 0. {
                continue;
            }
            out.push(Sprite {
                kind: SpriteKind::Ghost,
                cx: self.cell_x(g.mover.at_x()) + f64::from(self.tile - 1) / 2.,
                cy: self.cell_y(g.mover.at_y()) + f64::from(self.tile - 1) / 2.,
                size: f64::from(self.tile),
                dx: g.mover.dx,
                dy: g.mover.dy,
                alpha: alpha
                    * if g.state == GhostState::Fright && !flashing {
                        0.4
                    } else if dying {
                        0.5
                    } else {
                        1.
                    },
                mouth: 0.,
                mascot: Some(g.mascot),
                talk: g.steps % 2 != 0,
                eyes: g.state == GhostState::Eyes,
            });
        }
    }
    pub fn stamp(&self, out: &mut [f32], cols: usize, rows: usize) {
        assert!(out.len() >= cols * rows);
        let Some(m) = &self.maze else {
            return;
        };
        let alpha = self.fade();
        let dim = if self.status == Status::Over {
            0.35
        } else {
            1.
        };
        let mut light = |x: i32, y: i32, value: f64| {
            if x < 0 || y < 0 || x as usize >= cols || y as usize >= rows {
                return;
            }
            let index = y as usize * cols + x as usize;
            if f64::from(out[index]) < value {
                out[index] = value as f32;
            }
        };
        let pellet_offset = (self.tile - 1) / 2;
        for ty in 0..m.rows {
            for tx in 0..m.cols {
                let at = m.at(tx, ty);
                let px = self.origin_x + tx * self.tile;
                let py = self.origin_y + ty * self.tile;
                if m.wall[at] == 1 {
                    for dy in 0..self.tile {
                        for dx in 0..self.tile {
                            light(px + dx, py + dy, 0.26 * dim);
                        }
                    }
                    continue;
                }
                let pellet = self.pellets[at];
                if pellet == 0 {
                    continue;
                }
                if pellet == 2 {
                    let span = (self.tile - 1).max(2);
                    let pulse =
                        0.72 + 0.28 * ((self.blink / 900.) * std::f64::consts::PI * 2.).sin();
                    for dy in 0..span {
                        for dx in 0..span {
                            light(px + dx, py + dy, pulse * dim);
                        }
                    }
                } else {
                    light(px + pellet_offset, py + pellet_offset, 0.78 * dim);
                }
            }
        }
        if alpha < 0.999 {
            for value in out {
                *value = (f64::from(*value) * alpha) as f32;
            }
        }
    }
}
