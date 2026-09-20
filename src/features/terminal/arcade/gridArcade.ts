/**
 * Shared types for the empty-session grid games.
 *
 * Pac-man, snake, and anything that joins later all speak `GridArcade` so the
 * idle slider can host them. Each game lives in its own module and plugs into
 * `GRID_GAMES`.
 */
import type { HarnessId } from "../../sessions/model/session";

export const ARCADE_MODES = ["low", "mid", "hard"] as const;
export type ArcadeMode = (typeof ARCADE_MODES)[number];

/** A pixel speech bubble trailing whoever is talking. */
export type SpeechBubble = {
  text: string;
  /** Grid cell the tail points at. */
  x: number;
  y: number;
  alpha: number;
};

/** Where to draw a provider logo in place of the grid squares. */
export type LogoPickup = {
  harness: HarnessId;
  /** Top-left grid cell of the pickup. */
  x: number;
  y: number;
  /** Footprint in grid cells, square. */
  cells: number;
  /** Eases in on arrival and out on expiry. */
  alpha: number;
};

/** A drawn piece on top of the stamped grid — pac-man, a mascot, … */
export type ArcadeSprite = {
  kind: "pacman" | "ghost";
  /** Centre of the sprite, in grid cells. */
  cx: number;
  cy: number;
  /** Side of the sprite, in grid cells. */
  size: number;
  /** Heading, for pac-man's mouth and the mascots' eyes. */
  dx: number;
  dy: number;
  alpha: number;
  /** Pac-man only: 0 shut, 1 gone — chewing, or the death spin. */
  mouth: number;
  /** Mascots only. */
  mascot?: string;
  /** Mascots only: which of the mascot's two frames to paint. */
  frame?: "rest" | "talk";
  /** Mascots only: eaten ones are drawn as a pair of eyes going home. */
  eyes?: boolean;
};

/**
 * One idle-or-playable board on the empty-session grid. Pac-man, snake, and
 * anything that joins later all speak this so the slider can host them.
 */
export type GridArcade = {
  resize(nextCols: number, nextRows: number): void;
  takeControl(): void;
  releaseControl(): void;
  steer(x: number, y: number): void;
  setMode(next: ArcadeMode): void;
  mode(): ArcadeMode;
  controlled(): boolean;
  score(): number;
  lives(): number;
  gameOver(): boolean;
  step(dt: number): void;
  speechBubble(): SpeechBubble | null;
  logoPickup(): LogoPickup | null;
  sprites(): ArcadeSprite[];
  fade(): number;
  stamp(out: Float32Array, stampCols: number, stampRows: number): void;
};
