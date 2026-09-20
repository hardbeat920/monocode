import { describe, expect, it } from "vitest";
import { HARNESSES } from "../../sessions/model/session";
import { LOGO_CELLS, createSnakeArcade } from "./snakeArcade";

/** Roughly what a normal window gives us: 6px cells, a 192px-tall band. */
const COLS = 172;
const ROWS = 28;
const FRAME_MS = 33;
/** Matches the arcade's own logo lifetime. */
const LOGO_LIFE_MS = 12000;

function run(cols: number, rows: number, ms: number) {
  const arcade = createSnakeArcade();
  arcade.resize(cols, rows);
  const stamp = new Float32Array(cols * rows);
  const playRows = Math.max(8, Math.floor(rows * 0.62));

  let eaten = 0;
  let expired = 0;
  let onBoardFor = 0;
  let peak = 0;

  for (let elapsed = 0; elapsed < ms; elapsed += FRAME_MS) {
    arcade.step(FRAME_MS);
    stamp.fill(0);
    arcade.stamp(stamp, cols, rows);
    for (let i = 0; i < stamp.length; i++) {
      const value = stamp[i] ?? 0;
      if (value > peak) peak = value;
    }

    const pickup = arcade.logoPickup();
    if (pickup) {
      expect(HARNESSES).toContain(pickup.harness);
      expect(pickup.alpha).toBeGreaterThanOrEqual(0);
      expect(pickup.alpha).toBeLessThanOrEqual(1);
      expect(pickup.cells).toBe(LOGO_CELLS);
      expect(pickup.x).toBeGreaterThanOrEqual(0);
      expect(pickup.y).toBeGreaterThanOrEqual(0);
      expect(pickup.x + pickup.cells).toBeLessThanOrEqual(cols);
      expect(pickup.y + pickup.cells).toBeLessThanOrEqual(playRows);
      onBoardFor += FRAME_MS;
    } else if (onBoardFor) {
      // Gone well before its lifetime ran out means the snake got it.
      if (onBoardFor < LOGO_LIFE_MS - FRAME_MS * 2) eaten++;
      else expired++;
      onBoardFor = 0;
    }
  }

  return { eaten, expired, peak };
}

describe("snake arcade", () => {
  it("detours to swallow the provider logos", () => {
    // Logos arrive every 4-10s, so two minutes should clear a good handful.
    const { eaten, expired } = run(COLS, ROWS, 120_000);
    expect(eaten).toBeGreaterThanOrEqual(6);
    expect(expired).toBe(0);
  });

  it("eases the pickup in rather than popping it into place", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    let tracked: { key: string; shownFor: number } | null = null;

    for (let t = 0; t < 120_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      const pickup = arcade.logoPickup();
      if (!pickup) {
        tracked = null;
        continue;
      }

      const key = `${pickup.harness}:${pickup.x},${pickup.y}`;
      if (tracked?.key !== key) {
        // First frame of this logo: it starts invisible and fades up.
        expect(pickup.alpha).toBeLessThan(0.2);
        tracked = { key, shownFor: 0 };
        continue;
      }

      tracked.shownFor += FRAME_MS;
      if (tracked.shownFor > 1000) {
        expect(pickup.alpha).toBeCloseTo(1, 5);
        return;
      }
    }

    throw new Error("no logo stayed up long enough to finish fading in");
  });

  it("keeps every stamped cell inside the intensity range", () => {
    const { peak } = run(COLS, ROWS, 30_000);
    expect(peak).toBeGreaterThan(0);
    expect(peak).toBeLessThanOrEqual(1);
  });

  it("skips the logo entirely on grids too small to hold one", () => {
    const arcade = createSnakeArcade();
    arcade.resize(LOGO_CELLS + 2, 6);
    for (let t = 0; t < 60_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      expect(arcade.logoPickup()).toBeNull();
    }
  });

  it("survives small and awkward grids", () => {
    for (const [cols, rows] of [
      [8, 4],
      [12, 9],
      [20, 12],
    ] as const) {
      expect(() => run(cols, rows, 10_000)).not.toThrow();
    }
  });

  it("handles being stepped before it has a size", () => {
    const arcade = createSnakeArcade();
    expect(() => {
      arcade.step(FRAME_MS);
      arcade.stamp(new Float32Array(0), 0, 0);
      expect(arcade.logoPickup()).toBeNull();
    }).not.toThrow();
  });

  it("only speaks up on the frame it takes a logo", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    let onBoardFor = 0;
    let showing = false;
    let spoke = 0;

    for (let t = 0; t < 120_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      const logo = arcade.logoPickup();
      const bubble = arcade.speechBubble();
      const started = !!bubble && !showing;

      if (logo) {
        onBoardFor += FRAME_MS;
      } else if (onBoardFor) {
        // Gone well before its lifetime ran out means the snake got it, and
        // that is the only thing that should set a line going.
        const eaten = onBoardFor < LOGO_LIFE_MS - FRAME_MS * 2;
        expect(started).toBe(eaten);
        if (eaten) spoke++;
        onBoardFor = 0;
      } else {
        expect(started).toBe(false);
      }

      showing = !!bubble;
    }

    expect(spoke).toBeGreaterThanOrEqual(6);
  });

  it("rides along with the head rather than sitting still", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    const spots = new Set<string>();

    for (let t = 0; t < 120_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      const bubble = arcade.speechBubble();
      // Follow a single bubble from the moment it appears until it goes.
      if (!bubble) {
        if (spots.size) break;
        continue;
      }
      spots.add(`${bubble.x},${bubble.y}`);
    }

    // The snake keeps moving for the couple of seconds the line is up.
    expect(spots.size).toBeGreaterThan(5);
  });

  it("never says the same thing twice running", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    const said: string[] = [];

    for (let t = 0; t < 600_000; t += FRAME_MS) {
      arcade.step(FRAME_MS);
      const bubble = arcade.speechBubble();
      if (bubble && bubble.text !== said[said.length - 1])
        said.push(bubble.text);
    }

    expect(said.length).toBeGreaterThan(20);
    for (let i = 1; i < said.length; i++) {
      expect(said[i]).not.toBe(said[i - 1]);
    }
  });

  it("fades the board in instead of painting at full strength", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    expect(arcade.fade()).toBe(0);
    arcade.step(FRAME_MS);
    expect(arcade.fade()).toBeGreaterThan(0);
    expect(arcade.fade()).toBeLessThan(1);
    arcade.step(500);
    expect(arcade.fade()).toBe(1);
  });

  it("reboots cleanly when the window is resized", () => {
    const arcade = createSnakeArcade();
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

  it("has no sprites of its own — the body is stamped onto the grid", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    arcade.step(FRAME_MS);
    expect(arcade.sprites()).toEqual([]);
  });
});

/** Matches mid-mode's player tick. */
const TICK_MS = 38;
const IDLE_PLAY_ROWS = Math.max(8, Math.floor(ROWS * 0.62));
/** Matches the arcade's first pellet when a player takes over. */
const PLAYER_PELLET_AHEAD = 8;

function stampHead(
  arcade: ReturnType<typeof createSnakeArcade>,
  cols: number,
  rows: number,
) {
  const stamp = new Float32Array(cols * rows);
  arcade.stamp(stamp, cols, rows);
  // Pellet is always the brightest cell; the head is next, even while the
  // boot fade is still scaling both down.
  let pelletAt = 0;
  let pellet = -1;
  for (let i = 0; i < stamp.length; i++) {
    const value = stamp[i] ?? 0;
    if (value > pellet) {
      pellet = value;
      pelletAt = i;
    }
  }
  let best = -1;
  let at = 0;
  for (let i = 0; i < stamp.length; i++) {
    if (i === pelletAt) continue;
    const value = stamp[i] ?? 0;
    if (value > best) {
      best = value;
      at = i;
    }
  }
  return { x: at % cols, y: Math.floor(at / cols), value: best };
}

describe("snake player control", () => {
  it("steers the snake instead of thinking for itself", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    expect(arcade.controlled()).toBe(true);

    arcade.steer(0, 1);
    for (let i = 0; i < 5; i++) arcade.step(TICK_MS);

    const head = stampHead(arcade, COLS, ROWS);
    const spawnY = Math.max(1, Math.floor(ROWS / 2));
    const spawnX = Math.max(2, Math.floor(COLS / 4));
    expect(head.x).toBe(spawnX);
    expect(head.y).toBe(spawnY + 5);
  });

  it("ignores a reverse into the body", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    arcade.steer(-1, 0);
    for (let i = 0; i < 5; i++) arcade.step(TICK_MS);

    const head = stampHead(arcade, COLS, ROWS);
    const spawnY = Math.max(1, Math.floor(ROWS / 2));
    const spawnX = Math.max(2, Math.floor(COLS / 4));
    expect(head.x).toBe(spawnX + 5);
    expect(head.y).toBe(spawnY);
  });

  it("ignores steering until someone takes control", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    expect(arcade.controlled()).toBe(false);
    arcade.steer(0, 1);
    expect(arcade.score()).toBe(0);
  });

  it("scores the pellet waiting in front of a fresh player snake", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    expect(arcade.score()).toBe(0);
    for (let i = 0; i < PLAYER_PELLET_AHEAD; i++) arcade.step(TICK_MS);
    expect(arcade.score()).toBe(1);
  });

  it("uses the full board rather than the faded idle strip", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    arcade.steer(0, 1);
    for (let i = 0; i < 6; i++) arcade.step(TICK_MS);

    const head = stampHead(arcade, COLS, ROWS);
    expect(head.y).toBeGreaterThan(IDLE_PLAY_ROWS - 1);
  });

  it("hands the idle brain back when control is released", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    for (let i = 0; i < PLAYER_PELLET_AHEAD; i++) arcade.step(TICK_MS);
    expect(arcade.score()).toBe(1);

    arcade.releaseControl();
    expect(arcade.controlled()).toBe(false);
    expect(arcade.score()).toBe(0);
  });

  it("starts on mid and lets hard outrun low", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    expect(arcade.mode()).toBe("mid");

    const runMode = (mode: "low" | "mid" | "hard") => {
      const next = createSnakeArcade();
      next.resize(COLS, ROWS);
      next.setMode(mode);
      next.takeControl();
      next.step(300);
      return stampHead(next, COLS, ROWS).x;
    };

    const spawnX = Math.max(2, Math.floor(COLS / 4));
    expect(runMode("low")).toBeGreaterThan(spawnX);
    expect(runMode("mid")).toBeGreaterThan(runMode("low"));
    expect(runMode("hard")).toBeGreaterThan(runMode("mid"));
  });

  it("keeps a live game when the pane is resized", () => {
    const arcade = createSnakeArcade();
    arcade.resize(COLS, ROWS);
    arcade.takeControl();
    for (let i = 0; i < PLAYER_PELLET_AHEAD; i++) arcade.step(TICK_MS);
    expect(arcade.score()).toBe(1);

    arcade.resize(COLS - 40, ROWS + 12);
    expect(arcade.controlled()).toBe(true);
    expect(arcade.score()).toBe(1);
    expect(arcade.mode()).toBe("mid");

    const next = stampHead(arcade, COLS - 40, ROWS + 12);
    expect(next.value).toBeGreaterThan(0);
    expect(next.x).toBeGreaterThanOrEqual(0);
    expect(next.x).toBeLessThan(COLS - 40);
    expect(next.y).toBeGreaterThanOrEqual(0);
    expect(next.y).toBeLessThan(ROWS + 12);
  });
});
