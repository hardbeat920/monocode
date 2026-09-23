import { summaryFromSession } from "../../sessions/data/sessionHistory";
import type { SessionSummary } from "../../sessions/data/sessionStore";
import { isPreparingHandoff } from "../../sessions/model/handoff";
import type { Session } from "../../sessions/model/session";

export function ciRepairSessions(
  history: readonly SessionSummary[],
  sessions: readonly Session[],
): SessionSummary[] {
  const unavailable = new Set(
    sessions
      .filter(
        (session) =>
          session.busy || session.pendingSwitch || isPreparingHandoff(session),
      )
      .map((session) => session.id),
  );
  return [
    ...new Map(
      [
        ...history,
        ...sessions
          .filter((session) => !session.inboxAsk)
          .map((session) => summaryFromSession(session)),
      ].map((session) => [session.id, session]),
    ).values(),
  ].filter((session) => !unavailable.has(session.id));
}
