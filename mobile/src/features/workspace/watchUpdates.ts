import type { Turn } from "./types";

/** A frame of a watched conversation, from the desktop's watchUpdate. */
export type WatchFrame = {
  busy?: boolean;
  /** Whether the conversation has turns before the ones the watch carries. */
  older?: boolean;
  order?: string[];
  turns?: Turn[];
  error?: string;
};

/**
 * Applies frames to the turns the phone has: the order says which turns there
 * are, and only the ones that changed come with it. Unchanged turns keep their
 * identity, so their rows skip rendering. Null when a frame names a turn the
 * phone never got, which needs a fresh watch.
 */
export function applyFrames(
  previous: Turn[],
  frames: WatchFrame[],
): { turns: Turn[]; busy?: boolean; older?: boolean; error?: string } | null {
  let turns = previous;
  let busy: boolean | undefined;
  let older: boolean | undefined;
  let error: string | undefined;
  for (const frame of frames) {
    if (frame.error) {
      error = frame.error;
      continue;
    }
    error = undefined;
    if (frame.busy !== undefined) busy = frame.busy;
    if (frame.older !== undefined) older = frame.older;
    if (!frame.order) continue;
    const known = new Map(turns.map((turn) => [turn.id, turn]));
    for (const turn of frame.turns ?? []) known.set(turn.id, turn);
    const next: Turn[] = [];
    for (const id of frame.order) {
      const turn = known.get(id);
      if (!turn) return null;
      next.push(turn);
    }
    turns =
      next.length === turns.length && next.every((turn, index) => turn === turns[index])
        ? turns
        : next;
  }
  return { turns, busy, older, error };
}
