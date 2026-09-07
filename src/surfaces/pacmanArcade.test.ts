import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HARNESSES } from "../lib/session";
import type { ArcadeSprite } from "./gridArcade";
import { createPacmanArcade } from "./pacmanArcade";

/** Roughly what a normal window gives us: 6px cells, a 192px-tall band. */
const COLS = 172;
const ROWS = 28;
const FRAME_MS = 33;
/** Matches the arcade's own logo lifetime. */
const LOGO_LIFE_MS = 12000;

/**
 * The maze, the logo drops, the mascot pen and the chatter all come off
 * Math.random, so a run is only as repeatable as that stream. Pin it, and the
 * same board plays out everywhere; leave it loose and a thin tail of unlucky
 * boards — pac-man walled off from a logo until it times out — turns the
 * counts below into a coin toss on CI. Override ARCADE_SEED to sweep other
 * boards and check the counts still hold.
 */
const SEED = Number(process.env.ARCADE_SEED) || 0x5eed;

/** mulberry32: small, fast, and good enough to play a maze with. */
function seededRandom(seed: number) {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

beforeEach(() => {
  vi.spyOn(Math, "random").mockImplementation(seededRandom(SEED));
});

afterEach(() => {
  vi.restoreAllMocks();
});

const HEADINGS = [
  { x: 1, y: 0 },
  { x: 0, y: 1 },
  { x: -1, y: 0 },
  { x: 0, y: -1 },
] as const;

type Arcade = ReturnType<typeof createPacmanArcade>;

/** Math.sign, minus the negative zero that trips strict equality. */
const sign = (value: number) => (value > 0 ? 1 : value < 0 ? -1 : 0);

function pacman(arcade: Arcade): ArcadeSprite {
  const sprite = arcade.sprites().find((it) => it.kind === "pacman");
  if (!sprite) throw new Error("no pac-man on the board");
  return sprite;
}

function ghosts(arcade: Arcade) {
  return arcade.sprites().filter((sprite) => sprite.kind === "ghost");
}

/**
 * Pac-man spawns stopped and only takes a heading that opens onto a corridor,
 * which the maze picks at random — so try each in turn until one lands.
 */
function startMoving(arcade: Arcade) {
  for (const heading of HEADINGS) {
    arcade.steer(heading.x, heading.y);
    const before = pacman(arcade);
    arcade.step(40);
    const after = pacman(arcade);
    if (after.cx !== before.cx || after.cy !== before.cy) return heading;
  }
  return null;
}

function idle(cols: number, rows: number, ms: number) {
  const arcade = createPacmanArcade();
  arcade.resize(cols, rows);
  const stamp = new Float32Array(cols * rows);

  let eaten = 0;
  let expired = 0;
  let onBoardFor = 0;
  let peak = 0;
  let frightened = 0;

  for (let elapsed = 0; elapsed < ms; elapsed += FRAME_MS) {
    arcade.step(FRAME_MS);
    stamp.fill(0);
    arcade.stamp(stamp, cols, rows);
    for (let i = 0; i < stamp.length; i++) {
      const value = stamp[i] ?? 0;
      if (value > peak) peak = value;
    }
    for (const ghost of ghosts(arcade)) {
      if (ghost.alpha < 0.9 || ghost.eyes) frightened++;
    }

    const pickup = arcade.logoPickup();
    if (pickup) {
      expect(HARNESSES).toContain(pickup.harness);
      expect(pickup.alpha).toBeGreaterThanOrEqual(0);
      expect(pickup.alpha).toBeLessThanOrEqual(1);
      expect(pickup.x).toBeGreaterThanOrEqual(0);
      expect(pickup.y).toBeGreaterThanOrEqual(0);
      expect(pickup.x + pickup.cells).toBeLessThanOrEqual(cols);
      expect(pickup.y + pickup.cells).toBeLessThanOrEqual(rows);
      onBoardFor += FRAME_MS;
    } else if (onBoardFor) {
      // Gone well before its lifetime ran out means pac-man got it.
      if (onBoardFor < LOGO_LIFE_MS - FRAME_MS * 2) eaten++;
      else expired++;
      onBoardFor = 0;
    }
  }

  return { arcade, eaten, expired, peak, frightened, score: arcade.score() };
}

describe("pac-man arcade", () => {
  it("works its way through the pellets on its own", () => {
    const { score } = idle(COLS, ROWS, 30_000);
    expect(score).toBeGreaterThan(500);
  });

  it("detours to swallow the provider logos", () => {
    // Logos arrive every 4-10s, so two minutes should clear a good handful.
    const { eaten } = idle(COLS, ROWS, 120_000);
    expect(eaten).toBeGreaterThanOrEqual(4);
  });

  it("eases the pickup in rather than popping it into place", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    let key = "";
    let alphas: number[] = [];

    for (let t = 0; t < 120_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      const pickup = arcade.logoPickup();
      if (!pickup) {
        key = "";
        alphas = [];
        continue;
      }

      const next = `${pickup.harness}:${pickup.x},${pickup.y}`;
      if (next !== key) {
        key = next;
        alphas = [];
      }
      alphas.push(pickup.alpha);
      if (pickup.alpha < 1) continue;

      // It starts invisible and walks up a frame at a time — the board pauses
      // on a death, so this is counted in frames rather than wall time.
      expect(alphas[0]).toBeLessThan(0.2);
      for (let i = 1; i < alphas.length; i++) {
        expect(alphas[i]).toBeGreaterThanOrEqual(alphas[i - 1]!);
        expect(alphas[i]! - alphas[i - 1]!).toBeLessThan(0.1);
      }
      return;
    }

    throw new Error("no logo stayed up long enough to finish fading in");
  });

  it("runs the maze off every edge of the pane", () => {
    for (const control of [false, true]) {
      const arcade = createPacmanArcade();
      if (control) arcade.takeControl();
      arcade.resize(COLS, ROWS);
      arcade.step(500);

      const stamp = new Float32Array(COLS * ROWS);
      arcade.stamp(stamp, COLS, ROWS);
      const lit = (x: number, y: number) => (stamp[y * COLS + x] ?? 0) > 0;

      const column = (x: number) =>
        Array.from({ length: ROWS }, (_, y) => y).some((y) => lit(x, y));
      const row = (y: number) =>
        Array.from({ length: COLS }, (_, x) => x).some((x) => lit(x, y));

      // The border ring hangs off the pane, so the edge lands mid-maze — a
      // tile's worth of corridor is as much dead margin as there should be.
      const margin = 4;
      expect(Array.from({ length: margin }, (_, x) => column(x))).toContain(
        true,
      );
      expect(
        Array.from({ length: margin }, (_, x) => column(COLS - 1 - x)),
      ).toContain(true);
      expect(Array.from({ length: margin }, (_, y) => row(y))).toContain(true);
      expect(
        Array.from({ length: margin }, (_, y) => row(ROWS - 1 - y)),
      ).toContain(true);
    }
  });

  it("keeps every stamped cell inside the intensity range", () => {
    const { peak } = idle(COLS, ROWS, 30_000);
    expect(peak).toBeGreaterThan(0);
    expect(peak).toBeLessThanOrEqual(1);
  });

  it("puts four different mascots on the board", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    let most = 0;

    // They leave the pen staggered, and a caught pac-man puts them back.
    for (let t = 0; t < 30_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      const names = ghosts(arcade).map((ghost) => ghost.mascot);
      for (const name of names) expect(name).toBeTruthy();
      expect(new Set(names).size).toBe(names.length);
      most = Math.max(most, names.length);
    }

    expect(most).toBe(4);
  });

  it("turns the mascots edible once an energizer goes down", () => {
    // A short board so the corners, where the energizers sit, come up quickly.
    const { frightened } = idle(90, 24, 180_000);
    expect(frightened).toBeGreaterThan(0);
  });

  it("sits the game out on grids too small to hold a maze", () => {
    const arcade = createPacmanArcade();
    arcade.resize(9, 6);
    for (let t = 0; t < 60_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      expect(arcade.logoPickup()).toBeNull();
      expect(arcade.sprites()).toEqual([]);
    }
  });

  it("survives small and awkward grids", () => {
    for (const [cols, rows] of [
      [8, 4],
      [12, 9],
      [20, 12],
    ] as const) {
      expect(() => idle(cols, rows, 10_000)).not.toThrow();
    }
  });

  it("handles being stepped before it has a size", () => {
    const arcade = createPacmanArcade();
    expect(() => {
      arcade.step(FRAME_MS);
      arcade.stamp(new Float32Array(0), 0, 0);
      expect(arcade.logoPickup()).toBeNull();
      expect(arcade.speechBubble()).toBeNull();
    }).not.toThrow();
  });

  it("pipes up over whoever is talking, and never twice with the same line", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    const said: string[] = [];
    let spots = new Set<string>();
    let travelled = 0;

    for (let t = 0; t < 600_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      const bubble = arcade.speechBubble();
      if (!bubble) {
        spots = new Set();
        continue;
      }
      if (bubble.text !== said[said.length - 1]) {
        said.push(bubble.text);
        spots = new Set();
      }
      // A line said on the move rides along with whoever said it.
      spots.add(`${bubble.x},${bubble.y}`);
      travelled = Math.max(travelled, spots.size);
    }

    expect(said.length).toBeGreaterThan(20);
    for (let i = 1; i < said.length; i++) {
      expect(said[i]).not.toBe(said[i - 1]);
    }
    expect(travelled).toBeGreaterThan(5);
  });

  it("fades the board in instead of painting at full strength", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    expect(arcade.fade()).toBe(0);
    arcade.step(FRAME_MS);
    expect(arcade.fade()).toBeGreaterThan(0);
    expect(arcade.fade()).toBeLessThan(1);
    arcade.step(500);
    expect(arcade.fade()).toBe(1);
  });

  it("reboots cleanly when the window is resized", () => {
    const arcade = createPacmanArcade();
    for (const [cols, rows] of [
      [COLS, ROWS],
      [40, 12],
      [300, 40],
      [COLS, ROWS],
    ] as const) {
      arcade.resize(cols, rows);
      for (let i = 0; i < 400; i++) arcade.step(FRAME_MS);
      const stamp = new Float32Array(cols * rows);
      expect(() => arcade.stamp(stamp, cols, rows)).not.toThrow();
    }
  });
});

describe("player control", () => {
  it("steers pac-man instead of thinking for him", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    expect(arcade.controlled()).toBe(true);

    const before = pacman(arcade);
    const heading = startMoving(arcade);
    expect(heading).not.toBeNull();

    const after = pacman(arcade);
    // He goes exactly the way he was pointed, and nowhere else.
    expect(sign(after.cx - before.cx)).toBe(heading!.x);
    expect(sign(after.cy - before.cy)).toBe(heading!.y);
  });

  it("stands still until someone points him somewhere", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();

    const before = pacman(arcade);
    for (let i = 0; i < 20; i++) arcade.step(FRAME_MS);
    const after = pacman(arcade);
    expect(after.cx).toBe(before.cx);
    expect(after.cy).toBe(before.cy);
    expect(arcade.score()).toBe(0);
  });

  it("turns on the spot when reversed mid-corridor", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    const heading = startMoving(arcade);
    expect(heading).not.toBeNull();

    const turn = pacman(arcade);
    arcade.steer(-heading!.x, -heading!.y);
    arcade.step(60);
    const after = pacman(arcade);
    expect(sign(after.cx - turn.cx)).toBe(-heading!.x || 0);
    expect(sign(after.cy - turn.cy)).toBe(-heading!.y || 0);
  });

  it("ignores steering until someone takes control", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    expect(arcade.controlled()).toBe(false);
    arcade.steer(0, 1);
    expect(arcade.score()).toBe(0);
  });

  it("starts on mid and lets hard outrun low", () => {
    const travel = (mode: "low" | "mid" | "hard") => {
      const arcade = createPacmanArcade();
      arcade.resize(COLS, ROWS);
      arcade.setMode(mode);
      arcade.takeControl();
      const heading = startMoving(arcade);
      expect(heading).not.toBeNull();

      const before = pacman(arcade);
      // Short enough that he can't reach the next tile and turn.
      arcade.step(60);
      const after = pacman(arcade);
      return Math.abs(after.cx - before.cx) + Math.abs(after.cy - before.cy);
    };

    const fresh = createPacmanArcade();
    expect(fresh.mode()).toBe("mid");
    expect(travel("mid")).toBeGreaterThan(travel("low"));
    expect(travel("hard")).toBeGreaterThan(travel("mid"));
  });

  it("gives the player three lives", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    expect(arcade.lives()).toBe(3);
    expect(arcade.gameOver()).toBe(false);
  });

  it("keeps a live game when the pane is resized", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    startMoving(arcade);
    for (let i = 0; i < 20; i++) arcade.step(FRAME_MS);
    const scored = arcade.score();
    expect(scored).toBeGreaterThan(0);

    arcade.resize(COLS - 40, ROWS + 12);
    expect(arcade.controlled()).toBe(true);
    expect(arcade.score()).toBe(scored);
    expect(arcade.mode()).toBe("mid");

    const pac = pacman(arcade);
    expect(pac.cx).toBeGreaterThanOrEqual(0);
    expect(pac.cx).toBeLessThan(COLS - 40);
    expect(pac.cy).toBeGreaterThanOrEqual(0);
    expect(pac.cy).toBeLessThan(ROWS + 12);
  });

  it("hands the idle brain back when control is released", () => {
    const arcade = createPacmanArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    startMoving(arcade);
    for (let i = 0; i < 20; i++) arcade.step(FRAME_MS);
    expect(arcade.score()).toBeGreaterThan(0);

    arcade.releaseControl();
    expect(arcade.controlled()).toBe(false);
    expect(arcade.score()).toBe(0);
  });
});
