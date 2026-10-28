import type { Dispatch, RefObject, SetStateAction } from "react";
import type { Session } from "../../features/sessions/model/session";

/** Publish a session change to React and the runtime's synchronous snapshot. */
export function updateSessionState(
  sessionsRef: RefObject<Session[]>,
  setSessions: Dispatch<SetStateAction<Session[]>>,
  update: (sessions: Session[]) => Session[],
): Session[] {
  const current = sessionsRef.current;
  const next = update(current);
  // Another runtime callback may run before React renders this change.
  sessionsRef.current = next;
  // Ordinary UI setters can already have changes queued ahead of this one.
  // Reapply to that state instead of replacing it with the ref's snapshot.
  setSessions((pending) => (pending === current ? next : update(pending)));
  return next;
}
