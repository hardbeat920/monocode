/** Pixel mascot that patrols the composer's top edge while a turn is in flight. */

import { mascotPath } from "./projectMascots";

export const RUNNER_SIZE = 16;
export const RUNNER_SPEED_PX = 160;
export const RUNNER_INSET = 10;
/** Start the jump this far before the obstacle, land the same distance after. */
export const JUMP_LEAD = 18;
const JUMP_CLEARANCE = 10;
const JUMP_MIN = 28;

export const COIN_SIZE = 12;
/** Coin center above the rim — high enough that the mascot jumps into it. */
export const COIN_HOVER = 42;
export const COIN_WIDTH = 8;
/** Longer than the chevron hop so the takeoff reads instead of twitching. */
export const COIN_JUMP_LEAD = 34;
export const COIN_GAP_MIN_MS = 7000;
export const COIN_GAP_MAX_MS = 18000;
export const COIN_FIRST_MIN_MS = 3500;
export const COIN_FIRST_MAX_MS = 9000;
export const COLLECT_X = 10;
export const COLLECT_POP_MS = 280;
export const COLLECT_POP_PX = 16;

export const EXIT_MS = 560;
export const EXIT_PEAK = 44;
export const EXIT_SINK = 20;
const EXIT_APEX = 0.38;

/** First chevron hit this turn: knock-back, stars, then the mascot learns the hop. */
export const CRASH_RECOIL_PX = 18;
export const CRASH_RECOIL_MS = 140;
export const CRASH_STUN_MS = 560;
export const CRASH_SHAKE_MS = 480;
export const STAR_SIZE = 8;
export const STAR_COUNT = 3;
export const STAR_ORBIT = 11;
/** One full star orbit. Longer than the old 280ms spin so it reads as a daze, not a blur. */
export const STAR_SPIN_MS = 520;

export const COIN_FACE_PATH = mascotPath([
  "........",
  "..####..",
  ".######.",
  "########",
  "########",
  ".######.",
  "..####..",
  "........",
]);

export const COIN_EDGE_PATH = mascotPath([
  "........",
  "...##...",
  "...##...",
  "...##...",
  "...##...",
  "...##...",
  "...##...",
  "........",
]);

export const STAR_FACE_PATH = mascotPath([
  "...##...",
  "...##...",
  "..####..",
  "########",
  "########",
  "..####..",
  "...##...",
  "...##...",
]);

export const STAR_EDGE_PATH = mascotPath([
  "........",
  "...##...",
  "...##...",
  "..####..",
  "..####..",
  "...##...",
  "...##...",
  "........",
]);

type Rect = {
  left: number;
  right: number;
  top: number;
  bottom: number;
  width?: number;
};

export type Obstacle = {
  /** Left edge of the hurdle, in box coordinates. */
  left: number;
  right: number;
  /** Peak of the jump arc, in px above the top border. */
  height: number;
};

export type Coin = {
  id: number;
  /** Center X in box coordinates. */
  x: number;
  /** How high the mascot must jump to grab it. */
  height: number;
};

export type RunnerPose = {
  /** Sprite center X, in box coordinates. */
  x: number;
  /** Feet height above the top border. Negative sinks behind the box. */
  y: number;
  facing: 1 | -1;
  airborne: boolean;
};

export function pingPong(
  distance: number,
  length: number,
): { t: number; facing: 1 | -1 } {
  if (length <= 0) return { t: 0, facing: 1 };
  const cycle = length * 2;
  const d = ((distance % cycle) + cycle) % cycle;
  if (d <= length) return { t: d, facing: 1 };
  return { t: cycle - d, facing: -1 };
}

function arc(
  x: number,
  left: number,
  right: number,
  height: number,
  lead = JUMP_LEAD,
): number {
  const start = left - lead;
  const end = right + lead;
  if (end <= start || x <= start || x >= end) return 0;
  const t = (x - start) / (end - start);
  return 4 * t * (1 - t) * height;
}

/** Feet peak so the sprite's body meets the coin instead of its shoes. */
export function coinJumpPeak(coin: Coin): number {
  return Math.max(0, coin.height - RUNNER_SIZE / 2);
}

/** Mario parabola: 0 at the ends, `height` at the midpoint. */
export function jumpHeight(
  x: number,
  obstacle: Obstacle | null,
  coins: readonly Coin[] = [],
): number {
  let height = obstacle
    ? arc(x, obstacle.left, obstacle.right, obstacle.height)
    : 0;
  for (const coin of coins) {
    height = Math.max(
      height,
      arc(
        x,
        coin.x - COIN_WIDTH / 2,
        coin.x + COIN_WIDTH / 2,
        coinJumpPeak(coin),
        COIN_JUMP_LEAD,
      ),
    );
  }
  return height;
}

export function runnerPose(
  distance: number,
  boxWidth: number,
  obstacle: Obstacle | null,
  coins: readonly Coin[] = [],
  inset = RUNNER_INSET,
): RunnerPose {
  const trackWidth = Math.max(0, boxWidth - inset * 2);
  const { t, facing } = pingPong(distance, trackWidth);
  const x = inset + t;
  const y = jumpHeight(x, obstacle, coins);
  return { x, y, facing, airborne: y > 0.5 };
}

/** Keep a position in the same relative spot when the composer width changes. */
export function scaleTrackX(x: number, fromWidth: number, toWidth: number): number {
  if (fromWidth <= 0) return 0;
  return x * (toWidth / fromWidth);
}

export function stepAlong(
  along: number,
  facing: 1 | -1,
  dtMs: number,
  trackWidth: number,
  speed = RUNNER_SPEED_PX,
): { along: number; facing: 1 | -1 } {
  if (trackWidth <= 0) return { along: 0, facing: 1 };
  let next = along + facing * speed * (dtMs / 1000);
  let dir: 1 | -1 = facing;
  if (next >= trackWidth) {
    next = trackWidth;
    dir = -1;
  } else if (next <= 0) {
    next = 0;
    dir = 1;
  }
  return { along: next, facing: dir };
}

export function poseAt(
  along: number,
  facing: 1 | -1,
  boxWidth: number,
  obstacle: Obstacle | null,
  coins: readonly Coin[] = [],
  inset = RUNNER_INSET,
): RunnerPose {
  const trackWidth = Math.max(0, boxWidth - inset * 2);
  const x = inset + Math.min(trackWidth, Math.max(0, along));
  const y = jumpHeight(x, obstacle, coins);
  return { x, y, facing, airborne: y > 0.5 };
}

/** First contact with the chevron this turn — skip if they already learned, or are hopping a coin. */
export function hitsChevron(
  x: number,
  y: number,
  facing: 1 | -1,
  obstacle: Obstacle | null,
  learned: boolean,
): boolean {
  if (learned || !obstacle || y > 0.5) return false;
  const half = RUNNER_SIZE / 2;
  if (facing === 1) {
    return x + half >= obstacle.left && x - half < obstacle.right;
  }
  return x - half <= obstacle.right && x + half > obstacle.left;
}

/** Knocked back from the hurdle, easing out, clamped to the track. */
export function recoilAlong(
  hitAlong: number,
  facing: 1 | -1,
  elapsedMs: number,
  trackWidth: number,
): number {
  const t = Math.min(1, Math.max(0, elapsedMs / CRASH_RECOIL_MS));
  const eased = 1 - (1 - t) * (1 - t);
  const next = hitAlong - facing * CRASH_RECOIL_PX * eased;
  return Math.min(trackWidth, Math.max(0, next));
}

export function stunShake(elapsedMs: number): { x: number; y: number } {
  if (elapsedMs <= 0 || elapsedMs >= CRASH_SHAKE_MS) return { x: 0, y: 0 };
  const decay = 1 - elapsedMs / CRASH_SHAKE_MS;
  return {
    x: Math.round(Math.sin(elapsedMs / 32) * 3 * decay),
    y: Math.round(Math.cos(elapsedMs / 26) * 2 * decay),
  };
}

export function stunDone(elapsedMs: number): boolean {
  return elapsedMs >= CRASH_STUN_MS;
}

/** Pixel stars orbiting the sprite while it is stunned. Offsets are from the sprite top-left. */
export function stunStars(
  elapsedMs: number,
): { dx: number; dy: number; opacity: number }[] {
  if (elapsedMs < 0 || elapsedMs >= CRASH_STUN_MS) return [];
  const fadeAt = CRASH_STUN_MS - 140;
  const opacity =
    elapsedMs < fadeAt ? 1 : Math.max(0, 1 - (elapsedMs - fadeAt) / 140);
  const originX = (RUNNER_SIZE - STAR_SIZE) / 2;
  const originY = (RUNNER_SIZE - STAR_SIZE) / 2 - 5;
  const angle = (elapsedMs / STAR_SPIN_MS) * Math.PI * 2;
  const stars: { dx: number; dy: number; opacity: number }[] = [];
  for (let i = 0; i < STAR_COUNT; i++) {
    const a = angle + (i * (Math.PI * 2)) / STAR_COUNT;
    stars.push({
      dx: Math.round(originX + Math.cos(a) * STAR_ORBIT),
      dy: Math.round(originY + Math.sin(a) * STAR_ORBIT),
      opacity,
    });
  }
  return stars;
}

export function coinCollected(pose: RunnerPose, coin: Coin): boolean {
  if (Math.abs(pose.x - coin.x) > COLLECT_X) return false;
  const mascotTop = pose.y + RUNNER_SIZE;
  const mascotBottom = pose.y;
  const coinTop = coin.height + COIN_SIZE / 2;
  const coinBottom = coin.height - COIN_SIZE / 2;
  return mascotTop >= coinBottom && mascotBottom <= coinTop;
}

export function nextCoinDelay(first: boolean, random = Math.random): number {
  const min = first ? COIN_FIRST_MIN_MS : COIN_GAP_MIN_MS;
  const max = first ? COIN_FIRST_MAX_MS : COIN_GAP_MAX_MS;
  return min + random() * (max - min);
}

/** Place a coin on the track, away from the runner and the chevron when we can. */
export function pickCoinX(
  boxWidth: number,
  runnerX: number,
  obstacle: Obstacle | null,
  random = Math.random,
): number | null {
  const min = RUNNER_INSET + COIN_JUMP_LEAD + 8;
  const max = boxWidth - RUNNER_INSET - COIN_JUMP_LEAD - 8;
  if (max <= min) return null;

  for (let i = 0; i < 8; i++) {
    const x = min + random() * (max - min);
    if (Math.abs(x - runnerX) < 40) continue;
    if (obstacle && x >= obstacle.left - 6 && x <= obstacle.right + 6) continue;
    return x;
  }
  return min + random() * (max - min);
}

/**
 * Vertical hop that peaks, then drops below the rim so the sprite can clip
 * away behind the composer.
 */
export function exitJumpY(
  t: number,
  peak = EXIT_PEAK,
  sink = EXIT_SINK,
): number {
  if (t <= 0) return 0;
  if (t >= 1) return -sink;
  if (t < EXIT_APEX) {
    const u = t / EXIT_APEX;
    return peak * (1 - (1 - u) * (1 - u));
  }
  const u = (t - EXIT_APEX) / (1 - EXIT_APEX);
  return peak + (-sink - peak) * u * u;
}

/** How many pixels to clip off the sprite bottom as it sinks behind the rim. */
export function spriteClipBottom(y: number, size = RUNNER_SIZE): number {
  if (y >= 0) return 0;
  return Math.min(size, Math.ceil(-y));
}

/**
 * Treat a control as a hurdle when it sits on (or just above) the composer's
 * top border and overlaps it horizontally — the jump-to-latest chevron.
 */
export function obstacleFromRects(
  box: Rect,
  button: Rect | null,
): Obstacle | null {
  if (!button) return null;
  if (button.right <= box.left || button.left >= box.right) return null;
  if (button.bottom < box.top - 48 || button.top > box.top + 12) return null;

  return {
    left: button.left - box.left,
    right: button.right - box.left,
    height: Math.max(JUMP_MIN, box.top - button.top + JUMP_CLEARANCE),
  };
}

export type RunnerTrack = {
  left: number;
  top: number;
  width: number;
};

/** Prefer the top edge of a control stacked on the composer. */
export function runnerTrack(box: Rect, ledge: Rect | null): RunnerTrack {
  const ledgeWidth = ledge
    ? (ledge.width ?? ledge.right - ledge.left)
    : 0;
  if (!ledge || ledgeWidth <= 0) {
    return {
      left: box.left,
      top: box.top,
      width: box.width ?? box.right - box.left,
    };
  }
  return {
    left: ledge.left,
    top: ledge.top,
    width: ledgeWidth,
  };
}
