import { useCallback, useReducer, useRef } from "react";
import { nextUnseenReplyCounts } from "../model/sessionDone";

/**
 * Replies each tracked session finished while unfocused, plus any noted
 * from elsewhere (a habit's post), cleared once the session is focused.
 */
export function useUnseenReplyCounts(
  trackedIds: ReadonlySet<string>,
  busySessionIds: ReadonlySet<string>,
  activeSessionId?: string,
): {
  counts: ReadonlyMap<string, number>;
  noteReply: (sessionId: string) => void;
} {
  const [, rerender] = useReducer((tick: number) => tick + 1, 0);
  const busyRef = useRef(busySessionIds);
  const focusedRef = useRef(activeSessionId);
  const trackedRef = useRef(trackedIds);
  const countsRef = useRef<ReadonlyMap<string, number>>(new Map());
  if (
    busyRef.current !== busySessionIds ||
    focusedRef.current !== activeSessionId ||
    trackedRef.current !== trackedIds
  ) {
    countsRef.current = nextUnseenReplyCounts({
      previousBusyIds: busyRef.current,
      busyIds: busySessionIds,
      previousCounts: countsRef.current,
      focusedSessionId: activeSessionId,
      trackedIds,
    });
    busyRef.current = busySessionIds;
    focusedRef.current = activeSessionId;
    trackedRef.current = trackedIds;
  }
  const noteReply = useCallback((sessionId: string) => {
    if (sessionId === focusedRef.current || !trackedRef.current.has(sessionId))
      return;
    const next = new Map(countsRef.current);
    next.set(sessionId, (next.get(sessionId) ?? 0) + 1);
    countsRef.current = next;
    rerender();
  }, []);
  return { counts: countsRef.current, noteReply };
}
