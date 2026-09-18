import { invoke } from "@tauri-apps/api/core";

const FLUSH_DELAY_MS = 500;

let flushTimer: ReturnType<typeof setTimeout> | null = null;
let pending: { sessionId: string; text: string } | null = null;

/**
 * Persist the composer draft for a session, debounced per session. Only the
 * latest text for a session is written; rapid keystrokes collapse into a
 * single `composer_draft_set` invoke.
 */
export function saveSessionDraft(sessionId: string, text: string): void {
  pending = { sessionId, text };
  if (flushTimer !== null) clearTimeout(flushTimer);
  flushTimer = setTimeout(() => {
    flushTimer = null;
    void flushDraft();
  }, FLUSH_DELAY_MS);
}

/** Write any pending draft immediately (used when the pane unmounts). */
export function flushSessionDraft(): void {
  if (flushTimer !== null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  void flushDraft();
}

/** Drop any pending draft without writing it. */
export function discardPendingDraft(): void {
  if (flushTimer !== null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  pending = null;
}

export async function loadSessionDraft(sessionId: string): Promise<string> {
  try {
    return (await invoke<string | null>("composer_draft_get", { sessionId })) ?? "";
  } catch {
    // A failed load must never block the composer; treat as no draft.
    return "";
  }
}

async function flushDraft(): Promise<void> {
  if (!pending) return;
  const { sessionId, text } = pending;
  pending = null;
  try {
    await invoke("composer_draft_set", { sessionId, text });
  } catch {
    // Persistence is best-effort: losing a draft on a failed write is
    // preferable to surfacing an error over every keystroke batch.
  }
}
