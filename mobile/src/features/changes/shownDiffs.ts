import type { ChangedFile } from "@/features/workspace/types";

// The diffs the Changes page has handed to the diff screen, by an ID in the
// route, since a route's params can't carry a whole diff. A few are kept, so
// a screen still mounted (closing, say) can read its own.
const diffs = new Map<string, ChangedFile>();
let next = 0;

/** Keeps a diff for the diff screen and returns the ID to open it by. */
export function keepDiff(file: ChangedFile): string {
  const id = String(++next);
  diffs.set(id, file);
  for (const old of diffs.keys()) {
    if (diffs.size <= 4) break;
    diffs.delete(old);
  }
  return id;
}

export function shownDiff(id: string): ChangedFile | undefined {
  return diffs.get(id);
}
