import type { ChangedFile } from "@/features/workspace/types";

// The diffs the Changes page has handed to the diff screen, by an ID in the
// route, since a route's params can't carry a whole diff. A few are kept, so
// a screen still mounted (closing, say) can read its own.
const diffs = new Map<string, ChangedFile>();
const reviews = new Map<string, string>();
let next = 0;

/**
 * Keeps a diff for the diff screen and returns the ID to open it by. With
 * `reviewOf`, an Inbox item's key, its lines take review comments.
 */
export function keepDiff(file: ChangedFile, reviewOf?: string): string {
  const id = String(++next);
  diffs.set(id, file);
  if (reviewOf) reviews.set(id, reviewOf);
  for (const old of diffs.keys()) {
    if (diffs.size <= 4) break;
    diffs.delete(old);
    reviews.delete(old);
  }
  return id;
}

export function shownDiff(id: string): ChangedFile | undefined {
  return diffs.get(id);
}

/** The Inbox item whose review this diff's lines go to, if any. */
export function shownReview(id: string): string | undefined {
  return reviews.get(id);
}
