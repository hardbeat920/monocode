import { invoke } from "@tauri-apps/api/core";

const FLUSH_DELAY_MS = 500;

// Keyed by session id: multiple SessionPanes can be mounted at once (split
// view), so a single global pending slot would let one pane clobber or flush
// another session's draft.
const pending = new Map<string, { text: string; timer: ReturnType<typeof setTimeout> }>();

function clearTimer(sessionId: string): void {
  const entry = pending.get(sessionId);
  if (entry) {
    clearTimeout(entry.timer);
    pending.delete(sessionId);
  }
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
      void flushDraft(sessionId, text);
    }, FLUSH_DELAY_MS),
  });
}

/** Write any pending drafts immediately (used when the pane unmounts). */
export function flushSessionDraft(): Promise<void> {
  const entries = [...pending.entries()];
  pending.clear();
  return Promise.all(
    entries.map(([sessionId, entry]) => {
      clearTimeout(entry.timer);
      return flushDraft(sessionId, entry.text);
    }),
  ).then(() => undefined);
}

/** Drop any pending draft without writing it. */
export function discardPendingDraft(): void {
  for (const entry of pending.values()) clearTimeout(entry.timer);
  pending.clear();
}

export async function loadSessionDraft(sessionId: string): Promise<string> {
  try {
    return (await invoke<string | null>("composer_draft_get", { sessionId })) ?? "";
  } catch {
    // A failed load must never block the composer; treat as no draft.
    return "";
  }
}

async function flushDraft(sessionId: string, text: string): Promise<void> {
  if (!text) {
    // Nothing typed means nothing to keep; clear any stale persisted draft.
    await invoke("composer_draft_set", { sessionId, text: "" }).catch(() => null);
    return;
  }
  await invoke("composer_draft_set", { sessionId, text }).catch(() => null);
}
