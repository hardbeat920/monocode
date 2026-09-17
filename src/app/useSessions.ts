import {
  useCallback,
  useEffect,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  bindHarnessSession,
  forgetHarnessSession,
  isLiveHarness,
} from "../lib/harness";
import {
  newDefaultSession,
  newSession,
  sessionWorkCwd,
  type Session,
} from "../lib/session";
import {
  getSession,
} from "../lib/sessionStore";
import { persistFingerprint } from "../lib/sessionStore";
import { rememberLoadedSession } from "../lib/sessionCache";
import { restoreSessionCheckout } from "../lib/fs";
import { sessionChildHarnesses } from "../lib/handoff";
import type { InboxItem } from "../lib/githubTasks";
import { githubWorkItemThread } from "../lib/githubTasks";
import {
  gitlabWorkItemDetails,
  peekGitlabWorkItemDetails,
} from "../lib/gitlab";
import {
  linearIssueDetails,
  peekLinearIssueDetails,
} from "../lib/linear";
import { inboxAskKey } from "../lib/inboxAsk";
import type { LinkedSessionUpdate } from "../lib/linkedSessionUpdates";
import {
  completeLinkedWorkItemUpdateCard,
  failLinkedWorkItemUpdateCard,
  pendingLinkedWorkItemUpdateCard,
  type LinkedWorkItemUpdateCard,
} from "../lib/linkedWorkItemActivity";

export interface SessionsDeps {
  sidebarCwd: string;
  inboxViewOpen: boolean;
  inboxAskPortal: import("../surfaces/InboxDiscussionPanel").InboxSessionPortal | null;
  sessionsRef: MutableRefObject<Session[]>;
  openingInboxSessions: MutableRefObject<Map<string, Promise<string>>>;
  openingSessionIds: MutableRefObject<Set<string>>;
  sessionLoads: MutableRefObject<Map<string, Promise<Session | null>>>;
  sessionLoadEpochs: MutableRefObject<Map<string, number>>;
  removingSessionIds: MutableRefObject<Set<string>>;
  loadedSessionCache: MutableRefObject<Map<string, Session>>;
  lastPersisted: MutableRefObject<Map<string, string>>;
  activeSessionPrefetch: MutableRefObject<Promise<Session | null> | null>;
  linkedSessionUpdatesRef: MutableRefObject<ReadonlyMap<string, LinkedSessionUpdate>>;
  linkedWorkItemActivityFetches: MutableRefObject<Map<string, number>>;
  setSessions: Dispatch<SetStateAction<Session[]>>;
  setInboxAskPortal: Dispatch<
    SetStateAction<import("../surfaces/InboxDiscussionPanel").InboxSessionPortal | null>
  >;
  setComposerFocused: Dispatch<SetStateAction<boolean>>;
  setLinkedWorkItemUpdateCard: (
    sessionId: string,
    update: (
      card: LinkedWorkItemUpdateCard | undefined,
    ) => LinkedWorkItemUpdateCard | undefined,
  ) => void;
  stopSessionForRemoval: (sessionId: string) => Promise<Session | undefined>;
  refreshHistory: (cwd: string) => Promise<void>;
}

export function useSessions(deps: SessionsDeps) {
  const {
    sidebarCwd,
    inboxViewOpen,
    inboxAskPortal,
    sessionsRef,
    openingInboxSessions,
    openingSessionIds,
    sessionLoads,
    sessionLoadEpochs,
    removingSessionIds,
    loadedSessionCache,
    lastPersisted,
    activeSessionPrefetch,
    linkedSessionUpdatesRef,
    linkedWorkItemActivityFetches,
    setSessions,
    setInboxAskPortal,
    setComposerFocused,
    setLinkedWorkItemUpdateCard,
    stopSessionForRemoval,
    refreshHistory,
  } = deps;
  const invalidateLoadedSession = useCallback((sessionId: string) => {
    openingSessionIds.current.delete(sessionId);
    loadedSessionCache.current.delete(sessionId);
    sessionLoads.current.delete(sessionId);
    sessionLoadEpochs.current.set(
      sessionId,
      (sessionLoadEpochs.current.get(sessionId) ?? 0) + 1,
    );
  }, []);

  const loadStoredSession = useCallback(
    (sessionId: string): Promise<Session | null> => {
      const cached = loadedSessionCache.current.get(sessionId);
      if (cached) {
        // The cache owns closed sessions only. Transfer this reference into
        // live state instead of retaining a stale duplicate while it changes.
        loadedSessionCache.current.delete(sessionId);
        return Promise.resolve(cached);
      }

      const pending = sessionLoads.current.get(sessionId);
      if (pending) return pending;

      const epoch = sessionLoadEpochs.current.get(sessionId) ?? 0;
      const loading = getSession(sessionId)
        .then((loaded) => {
          if (
            !loaded ||
            removingSessionIds.current.has(sessionId) ||
            (sessionLoadEpochs.current.get(sessionId) ?? 0) !== epoch
          ) {
            return null;
          }
          const restored = restoreSessionCheckout(loaded);
          return restored;
        })
        .catch(() => null);
      sessionLoads.current.set(sessionId, loading);
      void loading.then(() => {
        if (sessionLoads.current.get(sessionId) === loading) {
          sessionLoads.current.delete(sessionId);
        }
      });
      return loading;
    },
    [],
  );

  const ensureOpenSession = useCallback(
    async (sessionId: string): Promise<Session | null> => {
      const open = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      if (open) return open;

      openingSessionIds.current.add(sessionId);
      const restored = await loadStoredSession(sessionId);
      if (!restored || removingSessionIds.current.has(sessionId)) {
        openingSessionIds.current.delete(sessionId);
        void refreshHistory(sidebarCwd);
        return null;
      }
      loadedSessionCache.current.delete(sessionId);
      const appeared = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      if (appeared) return appeared;
      if (restored.providerSessionId && isLiveHarness(restored.harness)) {
        bindHarnessSession(
          restored.harness,
          restored.id,
          restored.providerSessionId,
          sessionWorkCwd(restored),
          restored.providerAccountId,
        );
      }
      lastPersisted.current.set(restored.id, persistFingerprint(restored));
      if (!sessionsRef.current.some((session) => session.id === restored.id)) {
        const next = [...sessionsRef.current, restored];
        sessionsRef.current = next;
        setSessions(next);
      }
      return restored;
    },
    [loadStoredSession, refreshHistory, sidebarCwd],
  );

  const onPrefetchHistorySession = useCallback(
    (sessionId: string) => {
      if (
        removingSessionIds.current.has(sessionId) ||
        sessionsRef.current.some((session) => session.id === sessionId) ||
        loadedSessionCache.current.has(sessionId) ||
        sessionLoads.current.has(sessionId) ||
        activeSessionPrefetch.current
      ) {
        return;
      }
      const loading = loadStoredSession(sessionId);
      activeSessionPrefetch.current = loading;
      void loading.then((loaded) => {
        if (
          loaded &&
          !removingSessionIds.current.has(sessionId) &&
          !sessionsRef.current.some((session) => session.id === sessionId)
        ) {
          rememberLoadedSession(loadedSessionCache.current, loaded);
        }
        if (activeSessionPrefetch.current === loading) {
          activeSessionPrefetch.current = null;
        }
      });
    },
    [loadStoredSession],
  );

  const revealLinkedSessionUpdate = useCallback(
    (sessionId: string, update: LinkedSessionUpdate) => {
      const session = sessionsRef.current.find(
        (entry) => entry.id === sessionId,
      );
      if (!session?.linkedWorkItem) return;
      if (
        session.linkedWorkItemUpdateCard?.updatedAt === update.updatedAt &&
        session.linkedWorkItemUpdateCard.status !== "error"
      ) {
        return;
      }
      if (
        linkedWorkItemActivityFetches.current.get(sessionId) ===
        update.updatedAt
      ) {
        return;
      }
      const pending = pendingLinkedWorkItemUpdateCard(update);
      linkedWorkItemActivityFetches.current.set(sessionId, update.updatedAt);
      // A stale/error card should not remain visible while fresh details load.
      // The session itself is already open; this request stays fully detached
      // from the navigation path.
      setLinkedWorkItemUpdateCard(sessionId, (current) =>
        current?.updatedAt === update.updatedAt && current.status === "ready"
          ? current
          : undefined,
      );

      void githubWorkItemThread(
        session.cwd,
        session.linkedWorkItem.repo,
        session.linkedWorkItem.kind,
        session.linkedWorkItem.number,
        { force: true },
      ).then(
        (thread) => {
          if (
            linkedWorkItemActivityFetches.current.get(sessionId) !==
            pending.updatedAt
          ) {
            return;
          }
          linkedWorkItemActivityFetches.current.delete(sessionId);
          if (
            linkedSessionUpdatesRef.current.get(sessionId)?.updatedAt !==
            pending.updatedAt
          ) {
            return;
          }
          setLinkedWorkItemUpdateCard(sessionId, () =>
            completeLinkedWorkItemUpdateCard(pending, thread),
          );
        },
        () => {
          if (
            linkedWorkItemActivityFetches.current.get(sessionId) !==
            pending.updatedAt
          ) {
            return;
          }
          linkedWorkItemActivityFetches.current.delete(sessionId);
          if (
            linkedSessionUpdatesRef.current.get(sessionId)?.updatedAt !==
            pending.updatedAt
          ) {
            return;
          }
          setLinkedWorkItemUpdateCard(sessionId, () =>
            failLinkedWorkItemUpdateCard(pending),
          );
        },
      );
    },
    [setLinkedWorkItemUpdateCard],
  );

  const onAskInboxItem = useCallback(
    (item: InboxItem): Promise<string> => {
      const key = inboxAskKey(item);
      const pending = openingInboxSessions.current.get(key);
      if (pending) return pending;
      const opening = (async () => {
        let session = sessionsRef.current.find(
          (entry) => entry.inboxAsk?.key === key,
        );
        if (!session) {
          const candidate = item.projectPath || sidebarCwd;
          const cwd =
            candidate && candidate !== "~"
              ? candidate
              : await invoke<string>("default_cwd");
          const description =
            item.provider === "linear" && item.id
              ? (
                  peekLinearIssueDetails(item.id) ??
                  (await linearIssueDetails(item.id))
                ).body
              : item.provider === "gitlab" &&
                  (item.kind === "issue" || item.kind === "pr")
                ? (
                    peekGitlabWorkItemDetails(
                      item.repo,
                      item.kind,
                      item.number,
                    ) ??
                    (await gitlabWorkItemDetails(
                      item.repo,
                      item.kind,
                      item.number,
                    ))
                  ).body
                : undefined;
          session = {
            ...newDefaultSession(cwd),
            title: `Ask · ${item.title}`,
            inboxAsk: {
              key,
              title: item.title,
              url: item.url,
              provider: item.provider,
              description,
            },
          };
          sessionsRef.current = [...sessionsRef.current, session];
          setSessions(sessionsRef.current);
        }
        return session.id;
      })();
      openingInboxSessions.current.set(key, opening);
      void opening.then(
        () => openingInboxSessions.current.delete(key),
        () => openingInboxSessions.current.delete(key),
      );
      return opening;
    },
    [sidebarCwd],
  );

  const onRestartInboxAsk = useCallback(
    async (item: InboxItem): Promise<string> => {
      const id = await onAskInboxItem(item);
      const current = sessionsRef.current.find((session) => session.id === id)!;
      removingSessionIds.current.add(id);
      try {
        await stopSessionForRemoval(id);
        await Promise.all(
          sessionChildHarnesses(current).map((harness) =>
            forgetHarnessSession(harness, id),
          ),
        );
        const fresh = {
          ...newSession(
            current.harness,
            current.cwd,
            current.model,
            current.runtimeMode,
            current.modelSettings,
          ),
          title: current.title,
          inboxAsk: current.inboxAsk,
        };
        const next = sessionsRef.current.map((session) =>
          session.id === id ? fresh : session,
        );
        sessionsRef.current = next;
        setSessions(next);
        setInboxAskPortal((portal) =>
          portal?.sessionId === id
            ? { ...portal, sessionId: fresh.id }
            : portal,
        );
        return fresh.id;
      } finally {
        removingSessionIds.current.delete(id);
      }
    },
    [onAskInboxItem, stopSessionForRemoval],
  );

  useEffect(() => {
    if (!inboxAskPortal || !inboxViewOpen) return;
    setComposerFocused(true);
  }, [inboxAskPortal, inboxViewOpen]);
  return {
    invalidateLoadedSession,
    loadStoredSession,
    ensureOpenSession,
    onPrefetchHistorySession,
    revealLinkedSessionUpdate,
    onAskInboxItem,
    onRestartInboxAsk,
  };
}
