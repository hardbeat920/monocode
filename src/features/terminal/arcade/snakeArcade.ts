import { HARNESSES, type HarnessId } from "../../sessions/model/session";
import type {
  ArcadeMode,
  ArcadeSprite,
  GridArcade,
  LogoPickup,
  SpeechBubble,
} from "./gridArcade";

const BOOT_FADE_MS = 420;

const TICK_MS = 70;
const TICK_FAST_MS = 52;
const FAST_AT_LENGTH = 10;

const PLAYER_TICK: Record<ArcadeMode, { base: number; fast: number }> = {
  low: { base: 72, fast: 54 },
  mid: { base: 38, fast: 26 },
  hard: { base: 22, fast: 16 },
};

/** How many grid cells across the logo pickup sits. */
export const LOGO_CELLS = 3;

const LOGO_FADE_MS = 500;
const LOGO_LIFE_MS = 12000;
const LOGO_GAP_MIN_MS = 4200;
const LOGO_GAP_MAX_MS = 10500;
const LOGO_GROWTH = 5;

const SPEECH_FADE_MS = 260;
/** How long the line stays up after the snake takes a logo. */
const SPEECH_HOLD_MS = 2200;

/** What the snake pipes up with once it has swallowed a logo. */
const CHATTER = [
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

/**
 * How far the snake can realistically travel before a logo times out. Its
 * steering is greedy, not optimal, so this budgets well under the straight-line
 * distance — on a wide monitor a logo parked at the far edge would just expire.
 */
const LOGO_REACH = Math.floor((LOGO_LIFE_MS * 0.7) / TICK_MS);

const PELLET_VALUE = 1;
const HEAD_VALUE = 0.9;

/** How far ahead of a fresh player snake the first pellet sits. */
const PLAYER_PELLET_AHEAD = 8;

function wrap(value: number, max: number) {
  if (max <= 0) return 0;
  return ((value % max) + max) % max;
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

function light(
  out: Float32Array,
  cols: number,
  rows: number,
  x: number,
  y: number,
  value: number,
) {
  if (x < 0 || y < 0 || x >= cols || y >= rows) return;
  const index = y * cols + x;
  if ((out[index] ?? 0) < value) out[index] = value;
}

type Cell = { x: number; y: number };

type Logo = { harness: HarnessId; x: number; y: number; age: number };

type Speech = { text: string; age: number };

export function createSnakeArcade(): GridArcade {
  let cols = 0;
  let rows = 0;
  let playRows = 0;

  let body: Cell[] = [];
  let dir = { x: 1, y: 0 };
  let pending: Cell | null = null;
  let grow = 0;
  let pellet: Cell = { x: 0, y: 0 };
  const occupied = new Set<string>();

  let logo: Logo | null = null;
  let logoTimer = 0;
  let speech: Speech | null = null;
  let lastChatter = -1;
  let tickAcc = 0;
  let booted = 0;
  let player = false;
  let score = 0;
  let mode: ArcadeMode = "mid";

  const key = (x: number, y: number) => `${x},${y}`;

  const computePlayRows = () =>
    player ? Math.max(8, rows) : Math.max(8, Math.floor(rows * 0.62));

  const syncOccupied = () => {
    occupied.clear();
    for (const cell of body) occupied.add(key(cell.x, cell.y));
  };

  const maxLength = () => clamp(Math.floor(cols * 0.4), 16, 80);

  const nextLogoDelay = () =>
    LOGO_GAP_MIN_MS + Math.random() * (LOGO_GAP_MAX_MS - LOGO_GAP_MIN_MS);

  /** Never the same line twice running. */
  const nextChatter = () => {
    let index = lastChatter;
    while (index === lastChatter && CHATTER.length > 1) {
      index = Math.floor(Math.random() * CHATTER.length);
    }
    lastChatter = index;
    return CHATTER[index] ?? CHATTER[0]!;
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

  /** True when (x, y) falls inside the logo's footprint. */
  const onLogo = (x: number, y: number) =>
    !!logo &&
    x >= logo.x &&
    x < logo.x + LOGO_CELLS &&
    y >= logo.y &&
    y < logo.y + LOGO_CELLS;

  const placePellet = () => {
    for (let attempt = 0; attempt < 60; attempt++) {
      const x = Math.floor(Math.random() * cols);
      const y = Math.floor(Math.random() * playRows);
      if (occupied.has(key(x, y)) || onLogo(x, y)) continue;
      pellet = { x, y };
      return;
    }
  };

  const spawnSnake = () => {
    const y = Math.max(1, Math.floor(playRows / 2));
    const x = Math.max(2, Math.floor(cols / 4));
    body = [
      { x, y },
      { x: x - 1, y },
      { x: x - 2, y },
    ];
    dir = { x: 1, y: 0 };
    pending = null;
    grow = 0;
    syncOccupied();
    if (player) {
      const ahead = wrap(x + PLAYER_PELLET_AHEAD, cols);
      if (!occupied.has(key(ahead, y)) && !onLogo(ahead, y)) {
        pellet = { x: ahead, y };
      } else {
        placePellet();
      }
    } else {
      placePellet();
    }
  };

  const spawnLogo = () => {
    const head = body[0];
    if (!head) return;
    if (cols < LOGO_CELLS + 6 || playRows < LOGO_CELLS + 3) return;

    const harness = HARNESSES[Math.floor(Math.random() * HARNESSES.length)];
    if (!harness) return;

    // Keep it landable: far enough away to be worth a detour, close enough that
    // the snake can actually get there before the logo times out.
    const minReach = LOGO_CELLS + 4;
    const maxReach = clamp(Math.floor(cols * 0.55), minReach + 6, LOGO_REACH);

    for (let attempt = 0; attempt < 60; attempt++) {
      const x = 1 + Math.floor(Math.random() * (cols - LOGO_CELLS - 2));
      const y = 1 + Math.floor(Math.random() * (playRows - LOGO_CELLS - 1));
      const cx = x + Math.floor(LOGO_CELLS / 2);
      const cy = y + Math.floor(LOGO_CELLS / 2);

      const dx = Math.min(Math.abs(cx - head.x), cols - Math.abs(cx - head.x));
      const reach = dx + Math.abs(cy - head.y);
      if (reach < minReach || reach > maxReach) continue;

      logo = { harness, x, y, age: 0 };
      if (onLogo(pellet.x, pellet.y)) placePellet();
      return;
    }
  };

  /**
   * Greedy steering. A logo on the board outranks the pellet outright — that
   * detour is the whole point.
   */
  const think = () => {
    const head = body[0];
    if (!head) return;

    const target = logo
      ? {
          x: logo.x + Math.floor(LOGO_CELLS / 2),
          y: logo.y + Math.floor(LOGO_CELLS / 2),
        }
      : pellet;
    const tail = body[body.length - 1];
    const options = [dir, { x: -dir.y, y: dir.x }, { x: dir.y, y: -dir.x }];

    let best = dir;
    let bestScore = -Infinity;
    for (const option of options) {
      if (option.x === -dir.x && option.y === -dir.y) continue;
      const nx = wrap(head.x + option.x, cols);
      const ny = wrap(head.y + option.y, playRows);
      const chasingTail = tail && nx === tail.x && ny === tail.y;
      if (occupied.has(key(nx, ny)) && !chasingTail) continue;

      const dx = Math.min(
        Math.abs(target.x - nx),
        cols - Math.abs(target.x - nx),
      );
      const dy = Math.min(
        Math.abs(target.y - ny),
        playRows - Math.abs(target.y - ny),
      );
      const score = -(dx + dy) + Math.random() * 0.15;
      if (score > bestScore) {
        bestScore = score;
        best = option;
      }
    }
    dir = best;
  };

  const advance = () => {
    if (player) {
      if (pending && !(pending.x === -dir.x && pending.y === -dir.y)) {
        dir = pending;
      }
      pending = null;
    } else {
      think();
    }
    const head = body[0];
    if (!head) return;

    const next = {
      x: wrap(head.x + dir.x, cols),
      y: wrap(head.y + dir.y, playRows),
    };
    const tail = body[body.length - 1];
    const hit =
      occupied.has(key(next.x, next.y)) &&
      !(tail && next.x === tail.x && next.y === tail.y);
    if (hit || body.length > maxLength()) {
      if (player) score = 0;
      spawnSnake();
      return;
    }

    body.unshift(next);

    if (onLogo(next.x, next.y)) {
      clearLogo();
      speech = { text: nextChatter(), age: 0 };
      grow += LOGO_GROWTH;
      if (player) score += LOGO_GROWTH;
    } else if (next.x === pellet.x && next.y === pellet.y) {
      grow += 1;
      if (player) score += 1;
      placePellet();
    }

    if (grow > 0) grow -= 1;
    else body.pop();

    syncOccupied();
  };

  const relayout = () => {
    const fit = (cell: Cell): Cell => ({
      x: wrap(cell.x, cols),
      y: wrap(cell.y, playRows),
    });
    const seen = new Set<string>();
    body = body.map(fit).filter((cell) => {
      const id = key(cell.x, cell.y);
      if (seen.has(id)) return false;
      seen.add(id);
      return true;
    });
    if (body.length === 0) {
      spawnSnake();
      return;
    }
    pellet = fit(pellet);
    if (logo) {
      if (cols < LOGO_CELLS || playRows < LOGO_CELLS) {
        logo = null;
      } else {
        logo = {
          ...logo,
          x: clamp(logo.x, 0, cols - LOGO_CELLS),
          y: clamp(logo.y, 0, playRows - LOGO_CELLS),
        };
      }
    }
    syncOccupied();
    if (occupied.has(key(pellet.x, pellet.y)) || onLogo(pellet.x, pellet.y)) {
      placePellet();
    }
  };

  const boot = () => {
    booted = 0;
    tickAcc = 0;
    score = 0;
    pending = null;
    logo = null;
    speech = null;
    logoTimer = nextLogoDelay();
    spawnSnake();
  };

  const playerTick = () => {
    const speeds = PLAYER_TICK[mode];
    return body.length > FAST_AT_LENGTH ? speeds.fast : speeds.base;
  };

  /** 0 while fading in, ramping to 1 — keeps resizes from popping. */
  const bootAlpha = () => Math.min(1, booted / BOOT_FADE_MS);

  return {
    resize(nextCols: number, nextRows: number) {
      if (nextCols === cols && nextRows === rows) return;
      const keepGame = player && body.length > 0 && cols > 0;
      cols = nextCols;
      rows = nextRows;
      playRows = computePlayRows();
      if (keepGame) relayout();
      else boot();
    },

    /** Hands the heading over to the caller; the idle brain goes quiet. */
    takeControl() {
      if (player) return;
      player = true;
      playRows = computePlayRows();
      boot();
    },

    /** Puts the idle brain back in the seat. */
    releaseControl() {
      if (!player) return;
      player = false;
      playRows = computePlayRows();
      boot();
    },

    /**
     * Queue the next heading. 180s against the current direction are ignored,
     * same as any other snake — you cannot reverse into yourself.
     */
    steer(x: number, y: number) {
      if (!player) return;
      if (Math.abs(x) + Math.abs(y) !== 1) return;
      if (x === -dir.x && y === -dir.y) return;
      pending = { x, y };
    },

    setMode(next: ArcadeMode) {
      if (mode === next) return;
      mode = next;
      // Don't dump a slow-mode remainder as extra steps on a faster tick.
      tickAcc = 0;
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
      return 0;
    },

    gameOver() {
      return false;
    },

    step(dt: number) {
      if (!cols || !playRows) return;
      booted += dt;

      if (speech) {
        speech.age += dt;
        if (speech.age >= SPEECH_HOLD_MS + SPEECH_FADE_MS) speech = null;
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

      tickAcc += dt;
      const tick = player
        ? playerTick()
        : body.length > FAST_AT_LENGTH
          ? TICK_FAST_MS
          : TICK_MS;
      while (tickAcc >= tick) {
        tickAcc -= tick;
        advance();
      }
    },

    /** The line to float above the head, set going by swallowing a logo. */
    speechBubble(): SpeechBubble | null {
      const head = body[0];
      if (!speech || !head) return null;
      return {
        text: speech.text,
        x: head.x,
        y: head.y,
        alpha: speechAlpha(speech) * bootAlpha(),
      };
    },

    /** The logo to paint over the grid, if one is on the board. */
    logoPickup(): LogoPickup | null {
      if (!logo) return null;
      const fadeIn = Math.min(1, logo.age / LOGO_FADE_MS);
      const fadeOut = Math.min(1, (LOGO_LIFE_MS - logo.age) / LOGO_FADE_MS);
      return {
        harness: logo.harness,
        x: logo.x,
        y: logo.y,
        cells: LOGO_CELLS,
        alpha: Math.max(0, Math.min(fadeIn, fadeOut)) * bootAlpha(),
      };
    },

    sprites(): ArcadeSprite[] {
      return [];
    },

    /** 0 → 1 over the first paint after mount or resize. */
    fade() {
      return bootAlpha();
    },

    stamp(out: Float32Array, stampCols: number, stampRows: number) {
      for (let i = 0; i < body.length; i++) {
        const cell = body[i];
        if (!cell) continue;
        const value = i === 0 ? HEAD_VALUE : Math.max(0.42, 0.72 - i * 0.018);
        light(out, stampCols, stampRows, cell.x, cell.y, value);
      }
      light(out, stampCols, stampRows, pellet.x, pellet.y, PELLET_VALUE);

      const alpha = bootAlpha();
      if (alpha >= 0.999) return;
      for (let i = 0; i < out.length; i++) out[i] *= alpha;
    },
  };
}
