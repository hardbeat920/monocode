/**
 * One join for every provider's streamed body text.
 *
 * Providers send either a new token or a resent snapshot of the whole message
 * so far. This function is the only place that distinguishes the two, so a new
 * harness does not need its own merge logic.
 *
 * Tokens append. A longer chunk that already starts with the current text is a
 * snapshot and replaces. Overlap matching is never used: it ate blank lines,
 * headings, table rows, and doubled letters.
 */
export function joinStreamText(existing: string, incoming: string): string {
  if (!incoming) return existing;
  if (!existing) return incoming;
  if (incoming === existing) {
    // `\n` then `\n` is a Markdown paragraph break, not a snapshot of a
    // one-character message. Longer exact repeats are completed snapshots.
    return incoming.length <= 1 ? existing + incoming : existing;
  }
  if (incoming.length > existing.length && incoming.startsWith(existing)) {
    return incoming;
  }
  if (
    existing.length > incoming.length &&
    existing.startsWith(incoming) &&
    existing.slice(incoming.length).trim() === ""
  ) {
    return existing;
  }
  return existing + incoming;
}

/**
 * How much of a completed snapshot to emit after tokens already landed.
 *
 * Claude and Codex send the full message again when a turn (or item) finishes.
 * If that copy is the same as — or already contained in — what we streamed,
 * emit nothing. If it only adds a suffix, emit the suffix. If it is a new
 * stretch (text after a tool, no tokens yet), emit the whole snapshot.
 */
export function snapshotRemainder(already: string, snapshot: string): string {
  if (!snapshot) return "";
  if (!already) return snapshot;
  if (snapshot === already) return "";
  if (snapshot.startsWith(already)) return snapshot.slice(already.length);
  if (already.startsWith(snapshot)) return "";
  return snapshot;
}

/** Body text from a stream. Whitespace is real content, not a missing field. */
export function streamTextDelta(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/** @deprecated Use joinStreamText. Kept so existing imports keep working. */
export const mergeStream = joinStreamText;
