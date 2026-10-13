import {
  useCallback,
  useEffect,
  useRef,
  type Dispatch,
  type RefObject,
  type SetStateAction,
} from "react";
import type { OrchestrationRun } from "../../features/orchestration/model/orchestration";
import { rememberLoadedSession } from "../../features/sessions/data/sessionCache";
import { shouldPersistSession } from "../../features/sessions/data/sessionStore";
import { sessionChildHarnesses } from "../../features/sessions/model/handoff";
import type { Session } from "../../features/sessions/model/session";
import {
  leafIds,
  type WorkspaceTab,
} from "../../features/workspace/model/layout";
import { forgetHarnessSession } from "../../integrations/harness/core/registry";
import { isMonoSession } from "../../features/monos/model/mono";
import { isHabitRun } from "../../features/monos/model/monoHabits";

const SESSION_DETACH_DELAY_MS = 250;

export function useIdleSessionDetach({
  sessions,
  sessionsRef,
  tabs,
  tabsRef,
  orchestrationRuns,
  liveAgentsEnabled,
  unseenFinishedIds,
  openingSessionIds,
  loadedSessionCache,
  skipForgetSessionIds,
  persistSession,
  setSessions,
}: {
  sessions: Session[];
  sessionsRef: RefObject<Session[]>;
  tabs: WorkspaceTab[];
  tabsRef: RefObject<WorkspaceTab[]>;
  orchestrationRuns: OrchestrationRun[];
  liveAgentsEnabled: boolean;
  unseenFinishedIds: ReadonlySet<string>;
  openingSessionIds: RefObject<Set<string>>;
  loadedSessionCache: RefObject<Map<string, Session>>;
  skipForgetSessionIds: RefObject<Set<string>>;
  persistSession: (session: Session | undefined) => void;
  setSessions: Dispatch<SetStateAction<Session[]>>;
}) {
  // Dropping a session re-renders the whole app, so it waits until a switch
  // has painted, and a burst of switches pays for it once.
  const detachInputs = useRef({ orchestrationRuns, liveAgentsEnabled });
  detachInputs.current = { orchestrationRuns, liveAgentsEnabled };
  const unseenFinishedRef = useRef(unseenFinishedIds);
  unseenFinishedRef.current = unseenFinishedIds;
  const detachTimer = useRef<number | null>(null);
  const detachIdleSessions = useCallback(() => {
    detachTimer.current = null;
    const sessions = sessionsRef.current;
    const { orchestrationRuns, liveAgentsEnabled } = detachInputs.current;
    const visibleIds = new Set(
      tabsRef.current.flatMap((tab) => leafIds(tab.layout)),
    );
    // Inbox and resident agents own panes independently of project tabs. Keep
    // their drafts and attachments mounted when the user switches views.
    for (const session of sessions) {
      // A habit's hidden run is removed by its scheduler when it ends.
      if (
        session.inboxAsk ||
        isMonoSession(session.id) ||
        isHabitRun(session.id)
      )
        visibleIds.add(session.id);
    }
    // Internal workers stay attached to the lead, even while idle between
    // turns. They must not be discarded merely because they have no tab.
    for (const session of sessions) {
      if (
        session.orchestrationLeadId &&
        (visibleIds.has(session.orchestrationLeadId) ||
          orchestrationRuns.some(
            (run) =>
              run.leadId === session.orchestrationLeadId &&
              ["active", "paused"].includes(run.status),
          ))
      )
        visibleIds.add(session.id);
    }
    for (const sessionId of visibleIds) {
      openingSessionIds.current.delete(sessionId);
      loadedSessionCache.current.delete(sessionId);
    }
    const keepUnseen = liveAgentsEnabled;
    const idleDetached = sessions.filter(
      (session) =>
        !visibleIds.has(session.id) &&
        !session.busy &&
        !openingSessionIds.current.has(session.id) &&
        !(keepUnseen && unseenFinishedRef.current.has(session.id)),
    );
    if (idleDetached.length === 0) return;
    for (const session of idleDetached) {
      if (skipForgetSessionIds.current.has(session.id)) continue;
      if (shouldPersistSession(session)) {
        rememberLoadedSession(loadedSessionCache.current, session);
      }
      persistSession(session);
      for (const harness of sessionChildHarnesses(session)) {
        void forgetHarnessSession(harness, session.id);
      }
    }
    setSessions((prev) =>
      prev.filter(
        (session) =>
          visibleIds.has(session.id) ||
          session.busy ||
          openingSessionIds.current.has(session.id) ||
          (keepUnseen && unseenFinishedRef.current.has(session.id)) ||
          skipForgetSessionIds.current.has(session.id),
      ),
    );
  }, [
    sessionsRef,
    tabsRef,
    openingSessionIds,
    loadedSessionCache,
    skipForgetSessionIds,
    persistSession,
    setSessions,
  ]);

  useEffect(() => {
    if (detachTimer.current != null) return;
    detachTimer.current = window.setTimeout(
      detachIdleSessions,
      SESSION_DETACH_DELAY_MS,
    );
  }, [
    sessions,
    tabs,
    liveAgentsEnabled,
    orchestrationRuns,
    detachIdleSessions,
  ]);

  useEffect(
    () => () => {
      if (detachTimer.current != null) window.clearTimeout(detachTimer.current);
    },
    [],
  );
}
