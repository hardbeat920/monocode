import type { GridArcade } from "./gridArcade";
import { createPacmanArcade } from "./pacmanArcade";
import { createSnakeArcade } from "./snakeArcade";

/** How long an idle board stays up before the slider moves on. */
export const SLIDE_HOLD_MS = 16_000;

export type GridGame = {
  id: string;
  label: string;
  playLabel: string;
  /**
   * Idle-band brightness vs a full game. Maze games fill the grid, so they
   * run quieter than a sparse one like snake.
   */
  idleDim: number;
  /** Whether the HUD should show lives. */
  lives: boolean;
  create: () => GridArcade;
};

/**
 * Games the empty-session slider rotates through. Push another entry to add
 * one — the track, dots, and take-control path all follow this list.
 */
export const GRID_GAMES: readonly GridGame[] = [
  {
    id: "pacman",
    label: "pac-man",
    playLabel: "Pac-man. Arrow keys or WASD to move. Escape to release.",
    idleDim: 0.2,
    lives: true,
    create: createPacmanArcade,
  },
  {
    id: "snake",
    label: "snake",
    playLabel: "Snake. Arrow keys or WASD to move. Escape to release.",
    idleDim: 0.65,
    lives: false,
    create: createSnakeArcade,
  },
];

/**
 * Next stop on the idle slider. Walks to the end and back so a wrap never
 * has to jump the track the long way around.
 */
export function stepSlider(
  index: number,
  dir: 1 | -1,
  count: number,
): { index: number; dir: 1 | -1 } {
  if (count <= 1) return { index: 0, dir: 1 };
  const next = index + dir;
  if (next >= count) return { index: count - 2, dir: -1 };
  if (next < 0) return { index: 1, dir: 1 };
  return { index: next, dir };
}
