import { useEffect, useMemo, useRef } from "react";
import { notifySession, pendingInputNotifications } from "../lib/notifications";
import type { Session } from "../lib/session";

export function useInputNotifications(
  sessions: Session[],
  activeSessionId: string | undefined,
) {
  const activeSessionIdRef = useRef(activeSessionId);
  activeSessionIdRef.current = activeSessionId;
  const inputNotifications = useMemo(
    () => pendingInputNotifications(sessions),
    [sessions],
  );
  const notifiedInputIdsRef = useRef(new Set<string>());
  useEffect(() => {
    const notified = notifiedInputIdsRef.current;
    for (const key of notified) {
      if (!inputNotifications.has(key)) notified.delete(key);
    }
    const notifiedSessions = new Set<string>();
    for (const [key, { session, event }] of inputNotifications) {
      if (notified.has(key) || notifiedSessions.has(session.id)) continue;
      notifiedSessions.add(session.id);
      // Coalesce this render's banners per session, but leave other requests
      // eligible for the next update (including resolution of the first one).
      notified.add(key);
      void notifySession(
        session,
        event,
        session.id === activeSessionIdRef.current,
      );
    }
  }, [inputNotifications]);
}
