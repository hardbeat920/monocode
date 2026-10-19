import { useEffect, useSyncExternalStore } from "react";

import type { Turn, TurnItem } from "@/features/workspace/types";

export type SessionErrorNotice = Extract<TurnItem, { type: "notice" }>;

/** Only the latest turn can interrupt the reader; paged-in history never does. */
export function latestSessionError(turns: Turn[]): SessionErrorNotice | undefined {
  return turns.at(-1)?.items.findLast(
    (item): item is SessionErrorNotice => item.type === "notice" && !!item.error,
  );
}

/** Desktop errors sometimes refer to controls that only exist on the desktop. */
export function sessionErrorContent(text: string) {
  if (/\bremoved provider account\b/i.test(text)) {
    return {
      title: "Account unavailable",
      message: "The account used for this conversation has been removed.",
      recovery: "Tap the model in the composer, open Account, and choose another account to start a new conversation.",
    };
  }
  return { title: "Couldn’t continue", message: text, recovery: undefined };
}

// The latest error of each open session, as its transcript last reported it,
// for the composer to show in place of the field; and those the user has
// tapped away, which stay away for as long as the app runs.
type Reported = { id: string; text: string };
const reported = new Map<string, Reported>();
const dismissed = new Set<string>();
const listeners = new Set<() => void>();
let version = 0;
const changed = () => {
  version += 1;
  listeners.forEach((listener) => listener());
};
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => void listeners.delete(listener);
};
const keyOf = (notice: Reported) => `${notice.id}\n${notice.text}`;

/** Reports a session's latest error, or that it has none, while mounted. */
export function useReportSessionError(
  sessionId: string | undefined,
  notice: SessionErrorNotice | null | undefined,
) {
  // By value: the native transcript hands over a new object with each status.
  const id = notice?.id;
  const text = notice?.text;
  useEffect(() => {
    if (!sessionId || id === undefined || text === undefined) return;
    const entry = { id, text };
    reported.set(sessionId, entry);
    changed();
    return () => {
      if (reported.get(sessionId) !== entry) return;
      reported.delete(sessionId);
      changed();
    };
  }, [sessionId, id, text]);
}

/**
 * The error the session's composer shows, with the field off, until the user
 * taps it away or the session moves past it.
 */
export function useSessionErrorNotice(sessionId: string | undefined) {
  useSyncExternalStore(subscribe, () => version);
  const notice = sessionId ? reported.get(sessionId) : undefined;
  if (!notice || dismissed.has(keyOf(notice))) return null;
  return {
    text: sessionErrorContent(notice.text).message,
    dismiss: () => {
      dismissed.add(keyOf(notice));
      changed();
    },
  };
}
