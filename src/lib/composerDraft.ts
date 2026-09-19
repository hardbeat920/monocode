import { invoke } from "@tauri-apps/api/core";

const FLUSH_DELAY_MS = 500;

// Keyed by session id: multiple SessionPanes can be mounted at once (split
// view), so a single global pending slot would let one pane clobber or flush
// another session's draft.
const pending = new Map<string, { text: string; timer: ReturnType<typeof setTimeout> }>();

// At most one write per session at a time; each new write chains behind the
// previous one so a slow earlier write can never land after a newer one and
// resurrect stale text.
const inFlight = new Map<string, Promise<void>>();

function clearTimer(sessionId: string): void {
  const entry = pending.get(sessionId);
  if (entry) {
    clearTimeout(entry.timer);
    pending.delete(sessionId);
  }
}

function writeDraft(sessionId: string, text: string): Promise<void> {
  const prev = inFlight.get(sessionId) ?? Promise.resolve();
  const run = prev
    .catch(() => null)
    .then(() => invoke<void>("composer_draft_set", { sessionId, text }));
  inFlight.set(sessionId, run);
  void run.catch(() => null).then(() => {
    if (inFlight.get(sessionId) === run) inFlight.delete(sessionId);
  });
  return run;
}

/**
 * Persist the composer draft for a session, debounced per session. Only the
 * latest text for a session is written; rapid keystrokes collapse into a
 * single `composer_draft_set` invoke.
 */
export function saveSessionDraft(sessionId: string, text: string): void {
  clearTimer(sessionId);
  pending.set(sessionId, {
    text,
    timer: setTimeout(() => {
      pending.delete(sessionId);
      // Timer-fired writes stay best-effort: a failure re-pends the text
      // (unless a newer save already replaced it) so the next flush retries,
      // but never surfaces an unhandled rejection mid-typing.
      writeDraft(sessionId, text).catch(() => {
        if (!pending.has(sessionId)) saveSessionDraft(sessionId, text);
      });
    }, FLUSH_DELAY_MS),
  });
}

/**
 * Write every pending draft immediately (used before teardown). Keeps
 * draining until `pending` is empty — saves added while a write is in flight
 * are picked up on a later pass instead of being wiped by an early clear —
 * then waits out writes already in flight. A write that fails re-pends its
 * text (unless a newer save superseded it) for the next pass; if text is
 * still unwritten when the passes run out, this rejects so callers can stop
 * the lifecycle action instead of tearing down with the draft unsaved.
 */
export async function flushSessionDraft(): Promise<void> {
  for (let pass = 0; pass < 5; pass++) {
    const entries = [...pending.entries()];
    if (entries.length === 0) break;
    pending.clear();
    await Promise.all(
      entries.map(([sessionId, entry]) => {
        clearTimeout(entry.timer);
        return writeDraft(sessionId, entry.text).catch(() => {
          // Re-pend for the next pass, unless the user already saved newer
          // text for this session while the write was in flight.
          if (!pending.has(sessionId)) saveSessionDraft(sessionId, entry.text);
        });
      }),
    );
  }
  await Promise.all([...inFlight.values()].map((p) => p.catch(() => null)));
  if (pending.size > 0) {
    // Unwritten text is still in `pending`; callers must not treat teardown
    // (reload, close) as safe while that is the case.
    throw new Error("composer_draft_set failed");
  }
}

/** Drop any pending draft without writing it. */
export function discardPendingDraft(): void {
  for (const entry of pending.values()) clearTimeout(entry.timer);
  pending.clear();
}

/**
 * Drop one session's pending draft without writing it (used when a session
 * is deleted: the row is gone, so a late write would only fail).
 */
export function discardSessionDraft(sessionId: string): void {
  clearTimer(sessionId);
}

export async function loadSessionDraft(sessionId: string): Promise<string> {
  try {
    return (await invoke<string | null>("composer_draft_get", { sessionId })) ?? "";
  } catch {
    // A failed load must never block the composer; treat as no draft.
    return "";
  }
}
