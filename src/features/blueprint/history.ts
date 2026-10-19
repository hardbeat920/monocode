/** Undo/redo stack of whole-blueprint snapshots (serialized text). */
export type History = { past: string[]; present: string; future: string[] };

// ponytail: whole-file snapshots; fine for blueprints of a few hundred boxes.
const LIMIT = 100;

/** A history with `present` as its only state: nothing to undo or redo yet. */
export function createHistory(present: string): History {
  return { past: [], present, future: [] };
}

/** A new state: becomes the present, and anything undone is dropped. */
export function record(h: History, next: string): History {
  if (next === h.present) return h;
  return {
    past: [...h.past, h.present].slice(-LIMIT),
    present: next,
    future: [],
  };
}

/** Steps back to the previous state, keeping the current one for redo. */
export function undo(h: History): History {
  if (h.past.length === 0) return h;
  return {
    past: h.past.slice(0, -1),
    present: h.past[h.past.length - 1],
    future: [h.present, ...h.future],
  };
}

/** Steps forward to the next undone state. */
export function redo(h: History): History {
  if (h.future.length === 0) return h;
  return {
    past: [...h.past, h.present],
    present: h.future[0],
    future: h.future.slice(1),
  };
}
