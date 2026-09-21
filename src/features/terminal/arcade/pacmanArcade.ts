/**
 * Pac-man on the terminal grid, chased by four of the project mascots.
 *
 * Everything lives on two grids. The background grid is one cell per square,
 * which is what `stamp` paints the maze and the pellets into. On top of that
 * sits the maze lattice, `tile` cells to a side, which is what pac-man and the
 * mascots actually walk on — `sprites` hands their positions back in fractional
 * cells so the caller can draw them mid-step.
 */
import { PROJECT_MASCOTS } from "../../projects/model/projectMascots";
import { HARNESSES, type HarnessId } from "../../sessions/model/session";
import type {
  ArcadeMode,
  ArcadeSprite,
  GridArcade,
  LogoPickup,
  SpeechBubble,
} from "./gridArcade";

const BOOT_FADE_MS = 420;

/** Cells to a maze tile. The idle band gets a finer maze than a real game. */
const IDLE_TILE = 3;
const PLAYER_TILE = 4;

/** Smallest maze worth running; below this the surface stays a plain grid. */
const MIN_TILE_COLS = 7;
const MIN_TILE_ROWS = 5;

/** ms per tile at a player-sized tile — scaled down with the tile below. */
const MODE_TICK: Record<ArcadeMode, number> = { low: 250, mid: 185, hard: 135 };
const IDLE_TICK = 240;

/** Mascot pace as a fraction of pac-man's. Eyes race back to the pen. */
const GHOST_SPEED = 0.92;
const FRIGHT_SPEED = 0.55;
const EYES_SPEED = 2.4;

const FRIGHT_MS = 7000;
/** Tail end of a fright, where the mascots blink to warn they're coming back. */
const FRIGHT_FLASH_MS = 2200;
const FLASH_PERIOD_MS = 260;

const SCATTER_MS = 7000;
const CHASE_MS = 20000;

const DEATH_MS = 1500;
/** The idle board sits behind a pane, so it doesn't dwell on a loss. */
const IDLE_DEATH_MS = 900;
const OVER_MS = 2600;
const CLEAR_MS = 1200;
const START_LIVES = 3;

const PELLET_SCORE = 10;
const ENERGIZER_SCORE = 50;
const FRUIT_SCORE = 200;
const GHOST_SCORES = [200, 400, 800, 1600];

/**
 * What the idle brain goes out of its way for, against a pellet's 1. A logo is
 * only up for so long, so it outranks the pellet underfoot outright — that
 * detour is the whole point of putting one on the board.
 */
const LOGO_WEIGHT = 60;
const EDIBLE_WEIGHT = 40;
const ENERGIZER_WEIGHT = 2;

const GHOST_COUNT = 4;
/** Staggered starts, so all four don't pour out of the pen at once. */
const GHOST_RELEASE_MS = [0, 1600, 3600, 6000];

const WALL_VALUE = 0.26;
const PELLET_VALUE = 0.78;
const ENERGIZER_VALUE = 1;
/** Energizers breathe, so they read as more than a fat pellet. */
const BLINK_MS = 900;

const LOGO_FADE_MS = 500;
const LOGO_LIFE_MS = 12000;
const LOGO_GAP_MIN_MS = 4200;
const LOGO_GAP_MAX_MS = 10500;

const SPEECH_FADE_MS = 260;
/** How long a line stays up after whoever said it opened their mouth. */
const SPEECH_HOLD_MS = 2200;

/** What pac-man pipes up with once a provider logo goes down. */
const FRUIT_CHATTER = [
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

/** Pac-man, having turned the tables on a mascot. */
const CHOMP_CHATTER = [
  "GOTCHA",
  "SORRY, LITTLE GUY",
  "REVERSE UNO",
  "WHO'S CHASING NOW",
  "RESPAWN LATER",
  "TASTES LIKE PIXELS",
  "THAT'S ONE",
];

/** The mascot that just caught him. */
const CAUGHT_CHATTER = [
  "TAG, YOU'RE IT",
  "OUR TURN",
  "SNACK ACQUIRED",
  "MERGE CONFLICT",
  "SKILL ISSUE",
  "GOT ONE",
  "NOM",
];

const DIRS = [
  { x: 1, y: 0 },
  { x: -1, y: 0 },
  { x: 0, y: 1 },
  { x: 0, y: -1 },
] as const;

type Maze = {
  cols: number;
  rows: number;
  /** 1 wall, 0 corridor. */
  wall: Uint8Array;
};

type Mover = {
  tx: number;
  ty: number;
  dx: number;
  dy: number;
  /** 0 → 1 across the step from this tile to the next. */
  progress: number;
};

type Ghost = Mover & {
  mascot: string;
  kind: number;
  state: "pen" | "hunt" | "fright" | "eyes";
  releaseIn: number;
  scatter: { x: number; y: number };
  /** Tiles walked, for the two-frame shuffle. */
  steps: number;
};

type Speaker = { kind: "pac" } | { kind: "ghost"; index: number };

type Speech = { text: string; age: number; from: Speaker };

type Logo = { harness: HarnessId; tx: number; ty: number; age: number };

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

function pick<T>(items: readonly T[]): T {
  return items[Math.floor(Math.random() * items.length)]!;
}

function shuffled<T>(items: readonly T[]): T[] {
  const out = items.slice();
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [out[i], out[j]] = [out[j]!, out[i]!];
  }
  return out;
}

/**
 * A braided maze: a perfect maze carved depth-first, then opened up until no
 * dead ends are left. Dead ends are the one thing pac-man can't survive, and
 * the extra loops are what give the mascots somewhere to cut you off.
 */
function buildMaze(tileCols: number, tileRows: number): Maze {
  // Corridors sit on odd coordinates, walls on even, so both counts are odd.
  const cols = tileCols % 2 ? tileCols : tileCols - 1;
  const rows = tileRows % 2 ? tileRows : tileRows - 1;
  const wall = new Uint8Array(cols * rows).fill(1);
  const at = (x: number, y: number) => y * cols + x;
  const carve = (x: number, y: number) => {
    wall[at(x, y)] = 0;
  };
  const isWall = (x: number, y: number) => wall[at(x, y)] === 1;

  const stack: { x: number; y: number }[] = [{ x: 1, y: 1 }];
  carve(1, 1);
  while (stack.length) {
    const cur = stack[stack.length - 1]!;
    let moved = false;
    for (const dir of shuffled(DIRS)) {
      const nx = cur.x + dir.x * 2;
      const ny = cur.y + dir.y * 2;
      if (nx < 1 || ny < 1 || nx > cols - 2 || ny > rows - 2) continue;
      if (!isWall(nx, ny)) continue;
      carve(cur.x + dir.x, cur.y + dir.y);
      carve(nx, ny);
      stack.push({ x: nx, y: ny });
      moved = true;
      break;
    }
    if (!moved) stack.pop();
  }

  // Braid: every corridor with a single way out gets a second one.
  for (let y = 1; y < rows - 1; y += 2) {
    for (let x = 1; x < cols - 1; x += 2) {
      const exits = DIRS.filter((dir) => !isWall(x + dir.x, y + dir.y));
      if (exits.length > 1) continue;
      const options = shuffled(DIRS).filter(
        (dir) =>
          isWall(x + dir.x, y + dir.y) &&
          x + dir.x * 2 >= 1 &&
          y + dir.y * 2 >= 1 &&
          x + dir.x * 2 <= cols - 2 &&
          y + dir.y * 2 <= rows - 2,
      );
      const opened = options[0];
      if (opened) carve(x + opened.x, y + opened.y);
    }
  }

  // A few more loops on top, so the place doesn't read as a puzzle.
  for (let y = 1; y < rows - 1; y++) {
    for (let x = 1; x < cols - 1; x++) {
      if (!isWall(x, y)) continue;
      if ((x % 2 === 1) === (y % 2 === 1)) continue;
      if (Math.random() > 0.12) continue;
      carve(x, y);
    }
  }

  // The wrap-around tunnel, straight across the middle corridor row.
  const tunnel = clamp(
    Math.floor(rows / 2) % 2 === 1
      ? Math.floor(rows / 2)
      : Math.floor(rows / 2) + 1,
    1,
    rows - 2,
  );
  for (let x = 0; x < cols; x++) carve(x, tunnel);

  return { cols, rows, wall };
}

export function createPacmanArcade(): GridArcade {
  let cols = 0;
  let rows = 0;
  let tile = IDLE_TILE;
  /** Cell offset of the maze's top-left corner, keeping it centred. */
  let originX = 0;
  let originY = 0;

  let maze: Maze | null = null;
  /** 0 empty, 1 pellet, 2 energizer, one entry per maze tile. */
  let pellets = new Uint8Array(0);
  let pelletsLeft = 0;

  let pac: Mover = { tx: 0, ty: 0, dx: 0, dy: 0, progress: 0 };
  let pacHome = { x: 0, y: 0 };
  let pending: { x: number; y: number } | null = null;
  let chew = 0;

  let ghosts: Ghost[] = [];
  let pen = { x: 0, y: 0 };
  let ghostPhase: "scatter" | "chase" = "scatter";
  let phaseLeft = SCATTER_MS;
  let frightLeft = 0;
  let eatenStreak = 0;

  let status: "run" | "dying" | "clear" | "over" = "run";
  let statusLeft = 0;

  let logo: Logo | null = null;
  let logoTimer = 0;
  let speech: Speech | null = null;
  let lastLine = "";

  let booted = 0;
  let blink = 0;
  let player = false;
  let score = 0;
  let lives = START_LIVES;
  let mode: ArcadeMode = "mid";

  const wrapTx = (x: number) => {
    if (!maze) return 0;
    const span = maze.cols;
    return ((x % span) + span) % span;
  };

  const open = (x: number, y: number) => {
    if (!maze) return false;
    if (y < 0 || y >= maze.rows) return false;
    return maze.wall[y * maze.cols + wrapTx(x)] === 0;
  };

  /** Shortest signed run from a to b, going the way the tunnel allows. */
  const spanX = (a: number, b: number) => {
    if (!maze) return 0;
    const direct = b - a;
    const around = direct - Math.sign(direct) * maze.cols;
    return Math.abs(direct) <= Math.abs(around) ? direct : around;
  };

  const tileDistance = (ax: number, ay: number, bx: number, by: number) => {
    const dx = spanX(ax, bx);
    const dy = by - ay;
    return dx * dx + dy * dy;
  };

  /** Nearest corridor to a point we'd like to put something on. */
  const nearestOpen = (x: number, y: number) => {
    if (!maze) return { x: 0, y: 0 };
    const start = {
      x: clamp(x, 1, maze.cols - 2),
      y: clamp(y, 1, maze.rows - 2),
    };
    if (open(start.x, start.y)) return start;
    for (let radius = 1; radius < Math.max(maze.cols, maze.rows); radius++) {
      for (let dy = -radius; dy <= radius; dy++) {
        for (let dx = -radius; dx <= radius; dx++) {
          const nx = start.x + dx;
          const ny = start.y + dy;
          if (ny < 1 || ny > maze.rows - 2) continue;
          if (open(nx, ny)) return { x: wrapTx(nx), y: ny };
        }
      }
    }
    return start;
  };

  const cellX = (tx: number) => originX + tx * tile;
  const cellY = (ty: number) => originY + ty * tile;

  /** Where a mover actually is, in fractional tiles. */
  const atX = (m: Mover) => m.tx + m.dx * m.progress;
  const atY = (m: Mover) => m.ty + m.dy * m.progress;

  const pacTick = () =>
    (player ? MODE_TICK[mode] : IDLE_TICK) * (tile / PLAYER_TILE);

  const ghostTick = (ghost: Ghost) => {
    const speed =
      ghost.state === "eyes"
        ? EYES_SPEED
        : ghost.state === "fright"
          ? FRIGHT_SPEED
          : GHOST_SPEED;
    return pacTick() / speed;
  };

  const nextLogoDelay = () =>
    LOGO_GAP_MIN_MS + Math.random() * (LOGO_GAP_MAX_MS - LOGO_GAP_MIN_MS);

  /** Never the same line twice running, whoever says it. */
  const say = (lines: readonly string[], from: Speaker) => {
    let text = lastLine;
    while (text === lastLine && lines.length > 1) text = pick(lines);
    lastLine = text;
    speech = { text, age: 0, from };
  };

  const clearLogo = () => {
    logo = null;
    logoTimer = nextLogoDelay();
  };

  const speechAlpha = (state: Speech) => {
    const fadeIn = Math.min(1, state.age / SPEECH_FADE_MS);
    const fadeOut = Math.max(
      0,
      1 - (state.age - SPEECH_HOLD_MS) / SPEECH_FADE_MS,
    );
    return Math.min(fadeIn, fadeOut);
  };

  const scatterCorners = () => {
    const m = maze;
    if (!m) return [{ x: 0, y: 0 }];
    return [
      { x: m.cols - 2, y: 1 },
      { x: 1, y: 1 },
      { x: m.cols - 2, y: m.rows - 2 },
      { x: 1, y: m.rows - 2 },
    ];
  };

  const placeGhosts = () => {
    const corners = scatterCorners();
    const mascots = shuffled(PROJECT_MASCOTS).slice(0, GHOST_COUNT);
    ghosts = mascots.map((mascot, index) => ({
      mascot: mascot.name,
      kind: index,
      tx: pen.x,
      ty: pen.y,
      dx: 0,
      dy: 0,
      progress: 0,
      state: "pen",
      releaseIn: GHOST_RELEASE_MS[index] ?? 0,
      scatter: corners[index % corners.length]!,
      steps: 0,
    }));
  };

  /** Puts everyone back on their marks without touching the pellets. */
  const respawn = () => {
    pac = { tx: pacHome.x, ty: pacHome.y, dx: 0, dy: 0, progress: 0 };
    pending = null;
    chew = 0;
    frightLeft = 0;
    eatenStreak = 0;
    ghostPhase = "scatter";
    phaseLeft = SCATTER_MS;
    placeGhosts();
    status = "run";
    statusLeft = 0;
  };

  const fillPellets = () => {
    const m = maze;
    if (!m) return;
    pellets = new Uint8Array(m.cols * m.rows);
    for (let y = 0; y < m.rows; y++) {
      for (let x = 0; x < m.cols; x++) {
        if (m.wall[y * m.cols + x] === 1) continue;
        pellets[y * m.cols + x] = 1;
      }
    }
    // Nothing to hoover up in the pen or under pac-man's feet.
    for (let dy = -1; dy <= 1; dy++) {
      for (let dx = -1; dx <= 1; dx++) {
        const x = wrapTx(pen.x + dx);
        const y = pen.y + dy;
        if (y < 0 || y >= m.rows) continue;
        pellets[y * m.cols + x] = 0;
      }
    }
    pellets[pacHome.y * m.cols + pacHome.x] = 0;

    for (const corner of scatterCorners()) {
      const spot = nearestOpen(corner.x, corner.y);
      pellets[spot.y * m.cols + spot.x] = 2;
    }

    pelletsLeft = 0;
    for (const value of pellets) if (value) pelletsLeft++;
  };

  const rebuild = () => {
    tile = player ? PLAYER_TILE : IDLE_TILE;
    // Sized so the maze's own border wall falls just outside the pane on all
    // four sides: the outer corridor ring lands flush with the edge, and the
    // maze reads as running past the surface instead of sitting in a margin.
    const oddUp = (count: number) => (count % 2 ? count : count + 1);
    const tileCols = oddUp(Math.ceil(cols / tile) + 2);
    const tileRows = oddUp(Math.ceil(rows / tile) + 2);
    if (tileCols < MIN_TILE_COLS || tileRows < MIN_TILE_ROWS) {
      maze = null;
      ghosts = [];
      pellets = new Uint8Array(0);
      pelletsLeft = 0;
      logo = null;
      speech = null;
      return;
    }

    maze = buildMaze(tileCols, tileRows);
    // One tile out on each side for the border ring, then the slack split.
    originX = -tile - Math.floor((maze.cols * tile - 2 * tile - cols) / 2);
    originY = -tile - Math.floor((maze.rows * tile - 2 * tile - rows) / 2);

    pen = nearestOpen(Math.floor(maze.cols / 2), Math.floor(maze.rows / 2));
    pacHome = nearestOpen(
      Math.floor(maze.cols / 2),
      Math.floor(maze.rows * 0.78),
    );
    fillPellets();
    respawn();
    logo = null;
    logoTimer = nextLogoDelay();
    speech = null;
  };

  const boot = () => {
    booted = 0;
    score = 0;
    lives = START_LIVES;
    rebuild();
  };

  /** Flood the maze, optionally treating a few tiles as walls. */
  const flood = (fromX: number, fromY: number, blocked: Uint8Array | null) => {
    const m = maze!;
    const size = m.cols * m.rows;
    const dist = new Int32Array(size).fill(-1);
    const prev = new Int32Array(size).fill(-1);
    const queue = new Int32Array(size);
    let head = 0;
    let tail = 0;

    const start = fromY * m.cols + fromX;
    dist[start] = 0;
    queue[tail++] = start;
    while (head < tail) {
      const at = queue[head++]!;
      const x = at % m.cols;
      const y = (at - x) / m.cols;
      for (const dir of DIRS) {
        const nx = wrapTx(x + dir.x);
        const ny = y + dir.y;
        if (!open(nx, ny)) continue;
        const next = ny * m.cols + nx;
        if (dist[next] !== -1) continue;
        if (blocked && blocked[next]) continue;
        dist[next] = dist[at]! + 1;
        prev[next] = at;
        queue[tail++] = next;
      }
    }
    return { dist, prev, start };
  };

  /** The heading that starts the walk from `start` towards `goal`. */
  const firstStep = (prev: Int32Array, start: number, goal: number) => {
    const m = maze!;
    let at = goal;
    while (prev[at] !== -1 && prev[at] !== start) at = prev[at]!;
    if (prev[at] === -1) return null;
    const x = at % m.cols;
    const y = (at - x) / m.cols;
    const sx = start % m.cols;
    const sy = (start - sx) / m.cols;
    return { x: Math.sign(spanX(sx, x)), y: Math.sign(y - sy) };
  };

  /**
   * The idle brain. It runs for pellets, detours for a logo, and only goes near
   * a mascot once one is edible — the tiles around a hunting mascot are walled
   * off as far as the search is concerned.
   */
  const thinkPac = () => {
    const m = maze;
    if (!m) return;

    const danger = new Uint8Array(m.cols * m.rows);
    for (const ghost of ghosts) {
      if (ghost.state !== "hunt") continue;
      const gx = Math.round(atX(ghost));
      const gy = Math.round(atY(ghost));
      for (let dy = -2; dy <= 2; dy++) {
        for (let dx = -2; dx <= 2; dx++) {
          if (Math.abs(dx) + Math.abs(dy) > 2) continue;
          const x = wrapTx(gx + dx);
          const y = gy + dy;
          if (y < 0 || y >= m.rows) continue;
          danger[y * m.cols + x] = 1;
        }
      }
    }
    danger[pac.ty * m.cols + pac.tx] = 0;

    const goals = () => {
      const list: { at: number; weight: number }[] = [];
      if (frightLeft > 0) {
        for (const ghost of ghosts) {
          if (ghost.state !== "fright") continue;
          const x = wrapTx(Math.round(atX(ghost)));
          const y = clamp(Math.round(atY(ghost)), 0, m.rows - 1);
          list.push({ at: y * m.cols + x, weight: EDIBLE_WEIGHT });
        }
      }
      if (logo)
        list.push({ at: logo.ty * m.cols + logo.tx, weight: LOGO_WEIGHT });
      for (let i = 0; i < pellets.length; i++) {
        const value = pellets[i];
        if (!value) continue;
        list.push({ at: i, weight: value === 2 ? ENERGIZER_WEIGHT : 1 });
      }
      return list;
    };

    const targets = goals();
    if (!targets.length) return;

    const search = (blocked: Uint8Array | null) => {
      const { dist, prev, start } = flood(pac.tx, pac.ty, blocked);
      let best: { at: number; score: number } | null = null;
      for (const goal of targets) {
        const steps = dist[goal.at] ?? -1;
        if (steps < 0) continue;
        const score = goal.weight / (steps + 1);
        if (!best || score > best.score) best = { at: goal.at, score };
      }
      if (!best) return null;
      return firstStep(prev, start, best.at);
    };

    const heading = search(danger) ?? search(null);
    if (!heading) return;
    if (heading.x === 0 && heading.y === 0) return;
    pac.dx = heading.x;
    pac.dy = heading.y;
  };

  const eatAt = (tx: number, ty: number) => {
    const m = maze;
    if (!m) return;
    const at = ty * m.cols + tx;
    const value = pellets[at];
    if (value) {
      pellets[at] = 0;
      pelletsLeft--;
      score += value === 2 ? ENERGIZER_SCORE : PELLET_SCORE;
      if (value === 2) {
        frightLeft = FRIGHT_MS;
        eatenStreak = 0;
        for (const ghost of ghosts) {
          if (ghost.state === "hunt") {
            ghost.state = "fright";
            reverse(ghost);
          }
        }
      }
    }

    if (logo && logo.tx === tx && logo.ty === ty) {
      clearLogo();
      score += FRUIT_SCORE;
      say(FRUIT_CHATTER, { kind: "pac" });
    }

    if (pelletsLeft <= 0) {
      status = "clear";
      statusLeft = CLEAR_MS;
      say(["LEVEL CLEAR"], { kind: "pac" });
    }
  };

  /** Turn on the spot, keeping the sprite where it already is. */
  const reverse = (m: Mover) => {
    if (!m.dx && !m.dy) return;
    if (m.progress > 0) {
      m.tx = wrapTx(m.tx + m.dx);
      m.ty = m.ty + m.dy;
      m.progress = 1 - m.progress;
    }
    m.dx = -m.dx;
    m.dy = -m.dy;
  };

  const steerPac = () => {
    if (player) {
      if (pending && open(pac.tx + pending.x, pac.ty + pending.y)) {
        pac.dx = pending.x;
        pac.dy = pending.y;
        pending = null;
        return;
      }
      if (open(pac.tx + pac.dx, pac.ty + pac.dy) && (pac.dx || pac.dy)) return;
      pac.dx = 0;
      pac.dy = 0;
      return;
    }
    thinkPac();
    if (!open(pac.tx + pac.dx, pac.ty + pac.dy)) {
      pac.dx = 0;
      pac.dy = 0;
    }
  };

  const targetFor = (ghost: Ghost) => {
    const m = maze!;
    if (ghost.state === "eyes") return pen;
    if (ghostPhase === "scatter") return ghost.scatter;

    const px = pac.tx;
    const py = pac.ty;
    switch (ghost.kind) {
      case 0:
        return { x: px, y: py };
      case 1:
        // Cuts the corner, aiming four tiles up the road.
        return {
          x: wrapTx(px + pac.dx * 4),
          y: clamp(py + pac.dy * 4, 0, m.rows - 1),
        };
      case 2: {
        // Plays off the lead mascot, so the pair pincer instead of queue up.
        const lead = ghosts[0];
        const ax = wrapTx(px + pac.dx * 2);
        const ay = clamp(py + pac.dy * 2, 0, m.rows - 1);
        if (!lead) return { x: ax, y: ay };
        return {
          x: wrapTx(ax + spanX(lead.tx, ax)),
          y: clamp(ay + (ay - lead.ty), 0, m.rows - 1),
        };
      }
      default:
        // Loses its nerve up close and heads for its corner instead.
        return tileDistance(ghost.tx, ghost.ty, px, py) > 64
          ? { x: px, y: py }
          : ghost.scatter;
    }
  };

  const steerGhost = (ghost: Ghost) => {
    const options = DIRS.filter(
      (dir) =>
        open(ghost.tx + dir.x, ghost.ty + dir.y) &&
        !(dir.x === -ghost.dx && dir.y === -ghost.dy),
    );
    const moves = options.length
      ? options
      : DIRS.filter((dir) => open(ghost.tx + dir.x, ghost.ty + dir.y));
    if (!moves.length) {
      ghost.dx = 0;
      ghost.dy = 0;
      return;
    }

    if (ghost.state === "fright") {
      const choice = pick(moves);
      ghost.dx = choice.x;
      ghost.dy = choice.y;
      return;
    }

    const target = targetFor(ghost);
    let best = moves[0]!;
    let bestScore = Infinity;
    for (const dir of moves) {
      const score = tileDistance(
        wrapTx(ghost.tx + dir.x),
        ghost.ty + dir.y,
        target.x,
        target.y,
      );
      if (score < bestScore) {
        bestScore = score;
        best = dir;
      }
    }
    ghost.dx = best.x;
    ghost.dy = best.y;
  };

  const movePac = (dt: number) => {
    steerPacIfStalled();
    if (!pac.dx && !pac.dy) return;

    const tick = pacTick();
    pac.progress += dt / tick;
    chew += dt / tick;
    while (pac.progress >= 1) {
      pac.progress -= 1;
      pac.tx = wrapTx(pac.tx + pac.dx);
      pac.ty = pac.ty + pac.dy;
      eatAt(pac.tx, pac.ty);
      if (status !== "run") {
        pac.progress = 0;
        return;
      }
      steerPac();
      if (!pac.dx && !pac.dy) {
        pac.progress = 0;
        break;
      }
    }
  };

  /** A stopped pac-man keeps trying the buffered turn every frame. */
  const steerPacIfStalled = () => {
    if (pac.dx || pac.dy) return;
    steerPac();
  };

  const moveGhost = (ghost: Ghost, dt: number) => {
    if (ghost.state === "pen") {
      ghost.releaseIn -= dt;
      if (ghost.releaseIn > 0) return;
      ghost.state = frightLeft > 0 ? "fright" : "hunt";
      steerGhost(ghost);
      if (!ghost.dx && !ghost.dy) return;
    }
    if (!ghost.dx && !ghost.dy) {
      steerGhost(ghost);
      if (!ghost.dx && !ghost.dy) return;
    }

    ghost.progress += dt / ghostTick(ghost);
    while (ghost.progress >= 1) {
      ghost.progress -= 1;
      ghost.tx = wrapTx(ghost.tx + ghost.dx);
      ghost.ty = ghost.ty + ghost.dy;
      ghost.steps++;
      if (ghost.state === "eyes" && ghost.tx === pen.x && ghost.ty === pen.y) {
        ghost.state = frightLeft > 0 ? "fright" : "hunt";
      }
      steerGhost(ghost);
      if (!ghost.dx && !ghost.dy) {
        ghost.progress = 0;
        break;
      }
    }
  };

  const die = () => {
    status = "dying";
    statusLeft = player ? DEATH_MS : IDLE_DEATH_MS;
    if (player) lives = Math.max(0, lives - 1);
  };

  const collide = () => {
    for (const ghost of ghosts) {
      if (ghost.state === "eyes" || ghost.state === "pen") continue;
      const dx = Math.abs(spanX(atX(ghost), atX(pac)));
      const dy = Math.abs(atY(ghost) - atY(pac));
      if (dx > 0.6 || dy > 0.6) continue;

      if (ghost.state === "fright") {
        ghost.state = "eyes";
        score += GHOST_SCORES[Math.min(eatenStreak, GHOST_SCORES.length - 1)]!;
        eatenStreak++;
        say(CHOMP_CHATTER, { kind: "pac" });
        continue;
      }

      say(CAUGHT_CHATTER, { kind: "ghost", index: ghosts.indexOf(ghost) });
      die();
      return;
    }
  };

  const spawnLogo = () => {
    const m = maze;
    if (!m) return;
    // Far enough to be worth a detour, close enough that pac-man can get there
    // before it times out — walls make the real walk longer than the crow flies.
    const reach = (LOGO_LIFE_MS * 0.3) / pacTick();
    for (let attempt = 0; attempt < 60; attempt++) {
      const x = 1 + Math.floor(Math.random() * (m.cols - 2));
      const y = 1 + Math.floor(Math.random() * (m.rows - 2));
      if (!open(x, y)) continue;
      // The maze overhangs the pane, so keep the logo where it fits whole.
      const cells = Math.max(3, tile);
      if (cellX(x) < 0 || cellX(x) + cells > cols) continue;
      if (cellY(y) < 0 || cellY(y) + cells > rows) continue;
      const steps = Math.abs(spanX(pac.tx, x)) + Math.abs(y - pac.ty);
      if (steps < 4 || steps > reach) continue;
      logo = { harness: pick(HARNESSES), tx: x, ty: y, age: 0 };
      return;
    }
  };

  /** 0 while fading in, ramping to 1 — keeps resizes from popping. */
  const bootAlpha = () => Math.min(1, booted / BOOT_FADE_MS);

  const speakerCell = (from: Speaker) => {
    const m = from.kind === "pac" ? pac : ghosts[from.index];
    if (!m) return { x: 0, y: 0 };
    return {
      x: Math.round(cellX(atX(m)) + (tile - 1) / 2),
      y: Math.round(cellY(atY(m)) + (tile - 1) / 2),
    };
  };

  return {
    resize(nextCols: number, nextRows: number) {
      if (nextCols === cols && nextRows === rows) return;
      const keepGame = player && !!maze;
      cols = nextCols;
      rows = nextRows;
      if (keepGame) rebuild();
      else boot();
    },

    /** Hands pac-man over to the caller; the idle brain goes quiet. */
    takeControl() {
      if (player) return;
      player = true;
      boot();
    },

    /** Puts the idle brain back in the seat. */
    releaseControl() {
      if (!player) return;
      player = false;
      boot();
    },

    /**
     * Queue the next heading. Held until pac-man reaches a tile that opens that
     * way, except for a turn on the spot, which lands immediately.
     */
    steer(x: number, y: number) {
      if (!player || !maze) return;
      if (Math.abs(x) + Math.abs(y) !== 1) return;
      if (status !== "run") return;
      if (x === -pac.dx && y === -pac.dy && (pac.dx || pac.dy)) {
        reverse(pac);
        pending = null;
        return;
      }
      pending = { x, y };
    },

    setMode(next: ArcadeMode) {
      mode = next;
    },

    mode() {
      return mode;
    },

    controlled() {
      return player;
    },

    score() {
      return score;
    },

    lives() {
      return lives;
    },

    /** True while the last life is spent and the board is sitting dark. */
    gameOver() {
      return status === "over";
    },

    step(dt: number) {
      booted += dt;
      blink = (blink + dt) % BLINK_MS;
      if (!maze) return;

      if (speech) {
        speech.age += dt;
        if (speech.age >= SPEECH_HOLD_MS + SPEECH_FADE_MS) speech = null;
      }

      if (status !== "run") {
        statusLeft -= dt;
        if (statusLeft > 0) return;
        if (status === "over") {
          boot();
        } else if (status === "clear") {
          const keptScore = score;
          const keptLives = lives;
          rebuild();
          score = keptScore;
          lives = keptLives;
        } else if (player && lives <= 0) {
          status = "over";
          statusLeft = OVER_MS;
          say(["GAME OVER"], { kind: "pac" });
        } else {
          respawn();
        }
        return;
      }

      if (frightLeft > 0) {
        frightLeft -= dt;
        if (frightLeft <= 0) {
          frightLeft = 0;
          eatenStreak = 0;
          for (const ghost of ghosts) {
            if (ghost.state === "fright") ghost.state = "hunt";
          }
        }
      } else {
        phaseLeft -= dt;
        if (phaseLeft <= 0) {
          ghostPhase = ghostPhase === "scatter" ? "chase" : "scatter";
          phaseLeft = ghostPhase === "scatter" ? SCATTER_MS : CHASE_MS;
          for (const ghost of ghosts) {
            if (ghost.state === "hunt") reverse(ghost);
          }
        }
      }

      if (logo) {
        logo.age += dt;
        if (logo.age >= LOGO_LIFE_MS) clearLogo();
      } else {
        logoTimer -= dt;
        if (logoTimer <= 0) {
          spawnLogo();
          // If nowhere suitable turned up, wait out another gap rather than
          // re-running the search on every frame.
          if (!logo) logoTimer = nextLogoDelay();
        }
      }

      movePac(dt);
      if (status !== "run") return;
      for (const ghost of ghosts) moveGhost(ghost, dt);
      collide();
    },

    /** The line to float over whoever said it. */
    speechBubble(): SpeechBubble | null {
      if (!speech || !maze) return null;
      const cell = speakerCell(speech.from);
      return {
        text: speech.text,
        x: cell.x,
        y: cell.y,
        alpha: speechAlpha(speech) * bootAlpha(),
      };
    },

    /** The logo to paint over the grid, if one is on the board. */
    logoPickup(): LogoPickup | null {
      if (!logo || !maze) return null;
      const fadeIn = Math.min(1, logo.age / LOGO_FADE_MS);
      const fadeOut = Math.min(1, (LOGO_LIFE_MS - logo.age) / LOGO_FADE_MS);
      const cells = Math.max(3, tile);
      return {
        harness: logo.harness,
        x: cellX(logo.tx) + (tile - cells) / 2,
        y: cellY(logo.ty) + (tile - cells) / 2,
        cells,
        alpha: Math.max(0, Math.min(fadeIn, fadeOut)) * bootAlpha(),
      };
    },

    /** Pac-man and the mascots, ready to draw. */
    sprites(): ArcadeSprite[] {
      if (!maze) return [];
      const alpha = bootAlpha();
      const out: ArcadeSprite[] = [];

      const dying = status === "dying";
      const mouth = dying
        ? clamp(1 - statusLeft / (player ? DEATH_MS : IDLE_DEATH_MS), 0, 1)
        : status === "over"
          ? 1
          : 0.32 * Math.abs(Math.sin(chew * Math.PI));
      out.push({
        kind: "pacman",
        cx: cellX(atX(pac)) + (tile - 1) / 2,
        cy: cellY(atY(pac)) + (tile - 1) / 2,
        size: tile,
        dx: pac.dx || 1,
        dy: pac.dy,
        alpha,
        mouth,
      });

      if (status === "over") return out;

      const flashing =
        frightLeft > 0 &&
        frightLeft < FRIGHT_FLASH_MS &&
        Math.floor(frightLeft / FLASH_PERIOD_MS) % 2 === 0;
      for (const ghost of ghosts) {
        if (ghost.state === "pen" && ghost.releaseIn > 0) continue;
        const frightened = ghost.state === "fright";
        out.push({
          kind: "ghost",
          cx: cellX(atX(ghost)) + (tile - 1) / 2,
          cy: cellY(atY(ghost)) + (tile - 1) / 2,
          size: tile,
          dx: ghost.dx,
          dy: ghost.dy,
          alpha: alpha * (frightened && !flashing ? 0.4 : dying ? 0.5 : 1),
          mouth: 0,
          mascot: ghost.mascot,
          frame: ghost.steps % 2 === 0 ? "rest" : "talk",
          eyes: ghost.state === "eyes",
        });
      }
      return out;
    },

    /** 0 → 1 over the first paint after mount or resize. */
    fade() {
      return bootAlpha();
    },

    stamp(out: Float32Array, stampCols: number, stampRows: number) {
      const m = maze;
      if (!m) return;
      const alpha = bootAlpha();
      const dim = status === "over" ? 0.35 : 1;

      const light = (x: number, y: number, value: number) => {
        if (x < 0 || y < 0 || x >= stampCols || y >= stampRows) return;
        const index = y * stampCols + x;
        if ((out[index] ?? 0) < value) out[index] = value;
      };

      const pelletOffset = Math.floor((tile - 1) / 2);
      for (let ty = 0; ty < m.rows; ty++) {
        for (let tx = 0; tx < m.cols; tx++) {
          const at = ty * m.cols + tx;
          const px = cellX(tx);
          const py = cellY(ty);
          if (m.wall[at] === 1) {
            for (let dy = 0; dy < tile; dy++) {
              for (let dx = 0; dx < tile; dx++) {
                light(px + dx, py + dy, WALL_VALUE * dim);
              }
            }
            continue;
          }
          const pellet = pellets[at];
          if (!pellet) continue;
          if (pellet === 2) {
            const span = Math.max(2, tile - 1);
            const pulse =
              0.72 + 0.28 * Math.sin((blink / BLINK_MS) * Math.PI * 2);
            for (let dy = 0; dy < span; dy++) {
              for (let dx = 0; dx < span; dx++) {
                light(px + dx, py + dy, ENERGIZER_VALUE * pulse * dim);
              }
            }
          } else {
            light(px + pelletOffset, py + pelletOffset, PELLET_VALUE * dim);
          }
        }
      }

      if (alpha >= 0.999) return;
      for (let i = 0; i < out.length; i++) out[i] *= alpha;
    },
  };
}
