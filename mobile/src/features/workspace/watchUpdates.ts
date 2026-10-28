import type { Turn } from "./types";

/** A frame of a watched conversation, from the desktop's watchUpdate. */
export type WatchFrame = {
  busy?: boolean;
  /** Whether the conversation has turns before the ones the watch carries. */
  older?: boolean;
  order?: string[];
  turns?: Turn[];
  patches?: { id: string; edits: WatchEdit[] }[];
  error?: string;
};

type WatchEdit = {
  path: (string | number)[];
  value?: unknown;
  append?: string;
  from?: number;
  remove?: true;
};

/** Copy only the edited branch, preserving untouched item identities. */
function editValue(value: unknown, edit: WatchEdit, depth = 0): unknown {
  if (depth === edit.path.length) {
    if (edit.append !== undefined) {
      if (typeof value !== "string" || value.length !== edit.from)
        throw new Error("Transcript append needs a fresh snapshot");
      return value + edit.append;
    }
    return edit.value;
  }
  const key = edit.path[depth];
  const last = depth === edit.path.length - 1;
  if (
    Array.isArray(value) &&
    typeof key === "number" &&
    Number.isInteger(key) &&
    key >= 0 &&
    (key < value.length || (key === value.length && last && "value" in edit))
  ) {
    const next = value.slice();
    if (last && edit.remove) next.splice(key, 1);
    else next[key] = editValue(value[key], edit, depth + 1);
    return next;
  }
  if (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    typeof key === "string"
  ) {
    const record = value as Record<string, unknown>;
    if (
      !Object.prototype.hasOwnProperty.call(record, key) &&
      !(last && "value" in edit)
    )
      throw new Error("Transcript path needs a fresh snapshot");
    if (last && edit.remove) {
      const next = { ...record };
      delete next[key];
      return next;
    }
    return { ...record, [key]: editValue(record[key], edit, depth + 1) };
  }
  throw new Error("Transcript path needs a fresh snapshot");
}

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
    try {
      for (const patch of frame.patches ?? []) {
        let turn = known.get(patch.id);
        if (!turn) return null;
        for (const edit of patch.edits) turn = editValue(turn, edit) as Turn;
        if (turn.id !== patch.id) return null;
        known.set(patch.id, turn);
      }
    } catch {
      return null;
    }
    const next: Turn[] = [];
    for (const id of frame.order) {
      const turn = known.get(id);
      if (!turn) return null;
      next.push(turn);
    }
    turns =
      next.length === turns.length &&
      next.every((turn, index) => turn === turns[index])
        ? turns
        : next;
  }
  return { turns, busy, older, error };
}
