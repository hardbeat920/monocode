import {
  useCallback,
  useMemo,
  useSyncExternalStore,
  type SetStateAction,
} from "react";
import type { ChatReference } from "./chatReferences";

/**
 * Cited passages waiting to be sent, per session. A store rather than pane
 * state because the transcript, the composer and any open document all mark
 * or edit the same references.
 */
const NONE: ChatReference[] = [];
const bySession = new Map<string, ChatReference[]>();
const listeners = new Set<() => void>();
let version = 0;

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The references waiting in a session's composer, oldest first. */
export function getChatReferences(sessionId: string): ChatReference[] {
  return bySession.get(sessionId) ?? NONE;
}

/** Replaces a session's references, or updates them from the current list. */
export function setChatReferences(
  sessionId: string,
  action: SetStateAction<ChatReference[]>,
): void {
  const current = getChatReferences(sessionId);
  const next = typeof action === "function" ? action(current) : action;
  if (next === current) return;
  if (next.length === 0) bySession.delete(sessionId);
  else bySession.set(sessionId, next);
  version += 1;
  for (const listener of [...listeners]) listener();
}

/** Drops a session's references, e.g. once the session itself is deleted. */
export function clearChatReferences(sessionId: string): void {
  setChatReferences(sessionId, NONE);
}

/** A session's references and a stable setter, like `useState`. */
export function useSessionChatReferences(sessionId: string) {
  const references = useSyncExternalStore(subscribe, () =>
    getChatReferences(sessionId),
  );
  const set = useCallback(
    (action: SetStateAction<ChatReference[]>) =>
      setChatReferences(sessionId, action),
    [sessionId],
  );
  return [references, set] as const;
}

export type DocumentChatReference = {
  sessionId: string;
  reference: ChatReference;
};

/** Every pending reference cited from this document, across sessions. */
export function useDocumentChatReferences(
  path: string,
): DocumentChatReference[] {
  const seen = useSyncExternalStore(subscribe, () => version);
  return useMemo(() => {
    void seen;
    return [...bySession].flatMap(([sessionId, references]) =>
      references
        .filter((reference) => reference.source?.path === path)
        .map((reference) => ({ sessionId, reference })),
    );
  }, [path, seen]);
}
