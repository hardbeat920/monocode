import {
  getSession,
  shouldPersistSession,
  upsertSession,
} from "../../features/sessions/data/sessionStore";
import {
  formatSessionTitle,
  type Session,
} from "../../features/sessions/model/session";

/** Shared by sidebar and operator renames; never replace a live transcript. */
export async function renameSession(
  sessionId: string,
  displayTitle: string,
  getSessions: () => Session[],
  updateSessions: (update: (sessions: Session[]) => Session[]) => void,
): Promise<Session> {
  const title = displayTitle.trim();
  if (!title) throw new Error("Session title must not be empty");
  const stored =
    getSessions().find((session) => session.id === sessionId) ??
    (await getSession(sessionId));
  // A closed session may have opened while its record was loading.
  const current =
    getSessions().find((session) => session.id === sessionId) ?? stored;
  if (!current) throw new Error("Session was not found");
  const patch = {
    title: formatSessionTitle(current.harness, title),
    titleIsExplicit: true,
  };
  const updated = { ...current, ...patch };
  updateSessions((sessions) =>
    sessions.map((session) =>
      session.id === sessionId ? { ...session, ...patch } : session,
    ),
  );
  // Blank tabs are retained by the workspace snapshot, not project history.
  if (shouldPersistSession(updated) && !(await upsertSession(updated)))
    throw new Error("Session title could not be saved");
  return updated;
}
