import type { Session } from "../../features/sessions/model/session";

export function persistAfterFailedSessionRemoval(
  sessionId: string,
  removed: boolean,
  current: Session | undefined,
  pending: Map<string, Session>,
  persist: (session: Session) => void,
): void {
  if (removed) return;
  const session = current ?? pending.get(sessionId);
  if (!session) return;
  pending.set(sessionId, session);
  persist(session);
}
