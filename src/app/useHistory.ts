import {
  useCallback,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import { message } from "@tauri-apps/plugin-dialog";
import {
  deleteSession,
  getSession,
  persistFingerprint,
  setSessionArchived,
  setSessionPinned,
  shouldPersistSession,
  upsertSession,
} from "../lib/sessionStore";
import {
  mergeHistorySummary,
  mergeProjectHistorySummary,
  summaryFromSession,
} from "../lib/sessionHistory";
import {
  newDefaultSession,
  newSession,
  formatSessionTitle,
  sessionDisplayTitle,
  type HarnessId,
  type Session,
} from "../lib/session";
import { newTab } from "../lib/layout";
import { forgetHarnessSession } from "../lib/harness";
import { orchestrator } from "../lib/orchestration";
import { flushSessionCheckpoint } from "../lib/checkpoint";
import { isBlankSession } from "../lib/projectReturn";
import { isFilesystemTab, leafIds, type PaneEdge } from "../lib/layout";
import {
  applyPlaceSessionOnPane,
  applyPlaceTabOnPane,
  type WorkspaceTabCloseScope,
} from "../lib/workspaceTabGroups";
import { markLinkedSessionUpdateSeen } from "../lib/linkedSessionSeen";
import { restoreSessionCheckout } from "../lib/fs";
import { runSessionRemoval } from "../lib/sessionRemoval";
import { sessionChildHarnesses } from "../lib/handoff";
import { confirmDiscardUnsaved } from "./workspaceEvents";
import { filesInWorkspaceTabs } from "./tabHelpers";
import { confirmCloseTerminals } from "../lib/terminalClose";
import { archiveFocusedSession } from "../lib/archiveShortcut";
import { releaseOrchestrationWorker } from "../lib/orchestrationWorkspace";
import { rememberLoadedSession } from "../lib/sessionCache";
import type { SessionSummary } from "../lib/sessionStore";
import type { SidebarTabId } from "../lib/appearance";
import type { LinkedSessionUpdate } from "../lib/linkedSessionUpdates";
import { useSessionReminders } from "../hooks/useSessionReminders";
import { rememberProject, type RecentProject } from "../lib/recents";

export interface HistoryDeps {
  sessions: Session[];
  history: SessionSummary[];
  activeTabId: string;
  projectCwd: string;
  sidebarCwd: string;
  tabCloseScope: WorkspaceTabCloseScope;
  sessionsRef: MutableRefObject<Session[]>;
  tabsRef: MutableRefObject<import("../lib/layout").WorkspaceTab[]>;
  dirtyFilesRef: MutableRefObject<Set<string>>;
  activeTabIdRef: MutableRefObject<string>;
  linkedSessionUpdatesRef: MutableRefObject<ReadonlyMap<string, LinkedSessionUpdate>>;
  lastPersisted: MutableRefObject<Map<string, string>>;
  projectCwdRef: MutableRefObject<string>;
  projectTerminalFocusedRef: MutableRefObject<boolean>;
  pendingPersist: MutableRefObject<Map<string, Session>>;
  loadedSessionCache: MutableRefObject<Map<string, Session>>;
  removingSessionIds: MutableRefObject<Set<string>>;
  sessionLoads: MutableRefObject<Map<string, Promise<Session | null>>>;
  whatsNewVersionRef: MutableRefObject<string | null>;
  settingsOpenRef: MutableRefObject<boolean>;
  filePickerOpenRef: MutableRefObject<boolean>;
  inboxViewOpenRef: MutableRefObject<boolean>;
  notesViewOpenRef: MutableRefObject<boolean>;
  searchViewOpenRef: MutableRefObject<boolean>;
  setSessions: Dispatch<SetStateAction<Session[]>>;
  setHistory: Dispatch<SetStateAction<SessionSummary[]>>;
  setStoredLinkedSessions: Dispatch<SetStateAction<SessionSummary[]>>;
  setActiveTabId: Dispatch<SetStateAction<string>>;
  setComposerFocused: Dispatch<SetStateAction<boolean>>;
  setSearchViewOpen: Dispatch<SetStateAction<boolean>>;
  setInboxViewOpen: Dispatch<SetStateAction<boolean>>;
  setNotesViewOpen: Dispatch<SetStateAction<boolean>>;
  setSettingsOpen: Dispatch<SetStateAction<boolean>>;
  setFilePickerOpen: Dispatch<SetStateAction<boolean>>;
  setSidebarTab: Dispatch<SetStateAction<SidebarTabId>>;
  setProjectCwd: Dispatch<SetStateAction<string>>;
  setRecents: Dispatch<SetStateAction<RecentProject[]>>;
  setInspectedWorkerId: Dispatch<SetStateAction<string | null>>;
  setTabs: Dispatch<SetStateAction<import("../lib/layout").WorkspaceTab[]>>;
  setDirtyFiles: Dispatch<SetStateAction<Set<string>>>;
  setProjectTerminalFocused: Dispatch<SetStateAction<boolean>>;
  invalidateLoadedSession: (sessionId: string) => void;
  setLinkedWorkItemUpdateCard: (
    sessionId: string,
    update: (
      card: import("../lib/linkedWorkItemActivity").LinkedWorkItemUpdateCard | undefined,
    ) => import("../lib/linkedWorkItemActivity").LinkedWorkItemUpdateCard | undefined,
  ) => void;
  appendTab: (tab: import("../lib/layout").WorkspaceTab, cwd?: string) => void;
  ensureOpenSession: (sessionId: string) => Promise<Session | null>;
  focusOpenSession: (sessionId: string) => boolean;
  replaceBlankPaneWithSession: (session: Session) => boolean;
  revealLinkedSessionUpdate: (sessionId: string, update: LinkedSessionUpdate) => void;
  stopSessionForRemoval: (sessionId: string) => Promise<Session | undefined>;
  refreshHistory: (cwd: string) => Promise<void>;
  persistSession: (session: Session | undefined) => void;
  activateTab: (id: string, paneId?: string) => void;
}

export function useHistory(deps: HistoryDeps) {
  const {
    sessions,
    history,
    sidebarCwd,
    tabCloseScope,
    sessionsRef,
    tabsRef,
    dirtyFilesRef,
    activeTabIdRef,
    linkedSessionUpdatesRef,
    lastPersisted,
    projectCwdRef,
    projectTerminalFocusedRef,
    pendingPersist,
    loadedSessionCache,
    removingSessionIds,
    sessionLoads,
    whatsNewVersionRef,
    settingsOpenRef,
    filePickerOpenRef,
    inboxViewOpenRef,
    notesViewOpenRef,
    searchViewOpenRef,
    setSessions,
    setHistory,
    setStoredLinkedSessions,
    setActiveTabId,
    setComposerFocused,
    setSearchViewOpen,
    setInboxViewOpen,
    setNotesViewOpen,
    setSettingsOpen,
    setFilePickerOpen,
    setSidebarTab,
    setProjectCwd,
    setRecents,
    setInspectedWorkerId,
    setTabs,
    setDirtyFiles,
    setProjectTerminalFocused,
    invalidateLoadedSession,
    setLinkedWorkItemUpdateCard,
    appendTab,
    ensureOpenSession,
    focusOpenSession,
    replaceBlankPaneWithSession,
    revealLinkedSessionUpdate,
    stopSessionForRemoval,
    refreshHistory,
    persistSession,
    activateTab,
  } = deps;
  const onSelectHistorySession = useCallback(
    async (sessionId: string) => {
      let session = await ensureOpenSession(sessionId);
      if (!session || session.inboxAsk) return;
      const parentId =
        session.orchestrationLeadId ??
        orchestrator.forSession(sessionId)?.leadId;
      if (parentId && parentId !== sessionId) {
        setInspectedWorkerId(sessionId);
        session = await ensureOpenSession(parentId);
        if (!session) return;
      }
      const linkedUpdate = linkedSessionUpdatesRef.current.get(session.id);
      if (focusOpenSession(session.id)) {
        if (linkedUpdate) revealLinkedSessionUpdate(session.id, linkedUpdate);
        return;
      }
      if (replaceBlankPaneWithSession(session)) {
        if (linkedUpdate) revealLinkedSessionUpdate(session.id, linkedUpdate);
        return;
      }
      const tab = newTab(session.id);
      appendTab(tab, session.cwd);
      setActiveTabId(tab.id);
      setComposerFocused(true);
      if (linkedUpdate) revealLinkedSessionUpdate(session.id, linkedUpdate);
    },
    [
      appendTab,
      ensureOpenSession,
      focusOpenSession,
      replaceBlankPaneWithSession,
      revealLinkedSessionUpdate,
    ],
  );

  const openReminderSession = useCallback(
    async (sessionId: string) => {
      const session = await ensureOpenSession(sessionId);
      if (!session)
        throw new Error("This conversation is no longer available.");
      setSearchViewOpen(false);
      setInboxViewOpen(false);
      setNotesViewOpen(false);
      setSettingsOpen(false);
      setFilePickerOpen(false);
      setSidebarTab("sessions");
      setProjectCwd(session.cwd);
      setRecents(rememberProject(session.cwd));
      await onSelectHistorySession(sessionId);
    },
    [ensureOpenSession, onSelectHistorySession],
  );

  const ensureReminderSessionsSaved = useCallback(
    async (ids: readonly string[]) => {
      for (const id of ids) {
        const session = sessionsRef.current.find(
          (session) => session.id === id,
        );
        if (session && !(await upsertSession(session))) {
          throw new Error(
            "Send a message in this conversation before setting a reminder.",
          );
        }
      }
    },
    [],
  );

  const sessionReminders = useSessionReminders(
    openReminderSession,
    ensureReminderSessionsSaved,
    sessions
      .filter((session) => !session.inboxAsk)
      .map((session) => session.id),
  );

  const dismissNoticesForContinuedSession = useCallback(
    (sessionId: string) => {
      void sessionReminders.dismissDue(sessionId);
      const updatedAt = sessionsRef.current.find(
        (session) => session.id === sessionId,
      )?.linkedWorkItemUpdateCard?.updatedAt;
      if (updatedAt == null) return;
      markLinkedSessionUpdateSeen(sessionId, updatedAt);
      setLinkedWorkItemUpdateCard(sessionId, (card) =>
        card?.updatedAt === updatedAt ? undefined : card,
      );
    },
    [sessionReminders.dismissDue, setLinkedWorkItemUpdateCard],
  );

  const onPlaceSessionOnPane = useCallback(
    async (sessionId: string, targetId: string, edge: PaneEdge) => {
      if (sessionId === targetId) return;
      const targetTab = tabsRef.current.find((tab) =>
        leafIds(tab.layout).includes(targetId),
      );
      if (!targetTab) return;

      const alreadyHere = leafIds(targetTab.layout).includes(sessionId);
      if (!alreadyHere) {
        const session = await ensureOpenSession(sessionId);
        if (!session) return;
      }

      const tab = tabsRef.current.find((entry) => entry.id === targetTab.id);
      if (!tab || !leafIds(tab.layout).includes(targetId)) return;

      const replaceTarget =
        !leafIds(tab.layout).includes(sessionId) &&
        isBlankSession(
          sessionsRef.current.find((entry) => entry.id === targetId),
        );

      if (replaceTarget) {
        lastPersisted.current.delete(targetId);
        const blank = sessionsRef.current.find(
          (entry) => entry.id === targetId,
        );
        if (blank) void forgetHarnessSession(blank.harness, targetId);
      }

      const result = applyPlaceSessionOnPane({
        tabs: tabsRef.current,
        sessions: sessionsRef.current,
        sessionId,
        targetId,
        edge,
        replaceTarget,
        scope: tabCloseScope,
        createReplacement: (seed) =>
          newDefaultSession(
            seed?.cwd ?? projectCwdRef.current,
            seed?.runtimeMode,
          ),
      });
      if (!result) return;

      sessionsRef.current = result.sessions;
      tabsRef.current = result.tabs;
      setSessions(result.sessions);
      setTabs(result.tabs);
      setActiveTabId(result.activeTabId);
      setProjectTerminalFocused(false);
      setComposerFocused(true);
    },
    [ensureOpenSession, tabCloseScope],
  );

  const onPlaceTabOnPane = useCallback(
    (sourceTabId: string, targetId: string, edge: PaneEdge) => {
      const targetTab = tabsRef.current.find((tab) =>
        leafIds(tab.layout).includes(targetId),
      );
      if (!targetTab || targetTab.id === sourceTabId) return;

      const blankTarget = sessionsRef.current.find(
        (session) => session.id === targetId && isBlankSession(session),
      );
      const result = applyPlaceTabOnPane({
        tabs: tabsRef.current,
        sessions: sessionsRef.current,
        sourceTabId,
        targetId,
        edge,
        replaceTarget: blankTarget != null,
      });
      if (!result) return;

      if (blankTarget) {
        lastPersisted.current.delete(blankTarget.id);
        void forgetHarnessSession(blankTarget.harness, blankTarget.id);
      }
      sessionsRef.current = result.sessions;
      tabsRef.current = result.tabs;
      setSessions(result.sessions);
      setTabs(result.tabs);
      setActiveTabId(result.activeTabId);
      setProjectTerminalFocused(false);
      setComposerFocused(
        result.sessions.some((session) => session.id === result.focusedId),
      );
    },
    [],
  );

  const onRenameHistorySession = useCallback(
    async (sessionId: string, displayTitle: string) => {
      const trimmed = displayTitle.trim();
      if (!trimmed) return;
      invalidateLoadedSession(sessionId);

      const open = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      if (open) {
        const title = formatSessionTitle(open.harness, trimmed);
        const updated = { ...open, title };
        setSessions((prev) =>
          prev.map((session) => (session.id === sessionId ? updated : session)),
        );
        loadedSessionCache.current.delete(sessionId);
        persistSession(updated);
      } else {
        const restored = await getSession(sessionId).catch(() => null);
        if (!restored) {
          void refreshHistory(sidebarCwd);
          return;
        }
        const updated = {
          ...restored,
          title: formatSessionTitle(restored.harness, trimmed),
        };
        const saved = await upsertSession(updated).catch(() => null);
        if (saved) {
          const cached = restoreSessionCheckout(updated);
          rememberLoadedSession(loadedSessionCache.current, cached);
          lastPersisted.current.set(sessionId, persistFingerprint(updated));
        }
      }
      void refreshHistory(sidebarCwd);
    },
    [invalidateLoadedSession, persistSession, refreshHistory, sidebarCwd],
  );

  const onRemoveHistorySession = useCallback(
    async (
      sessionId: string,
      mode: "archive" | "delete",
      skipDeleteConfirm = false,
    ): Promise<boolean> => {
      if (removingSessionIds.current.has(sessionId)) return false;
      const open = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      const summary = history.find((entry) => entry.id === sessionId);
      const seed = open ?? summary;
      const label = seed
        ? sessionDisplayTitle(seed.title, seed.harness)
        : "this session";
      if (
        mode === "delete" &&
        !skipDeleteConfirm &&
        !window.confirm(`Delete “${label}”?`)
      )
        return false;

      removingSessionIds.current.add(sessionId);
      invalidateLoadedSession(sessionId);
      pendingPersist.current.delete(sessionId);
      let savedSummary: SessionSummary | undefined;
      try {
        return await runSessionRemoval({
          sessionId,
          scope: tabCloseScope,
          readWorkspace: () => ({
            tabs: tabsRef.current,
            sessions: sessionsRef.current,
            activeTabId: activeTabIdRef.current,
            dirtyFiles: dirtyFilesRef.current,
          }),
          createReplacement: (latest) =>
            newSession(
              latest?.harness ?? seed?.harness ?? "cursor",
              latest?.cwd ?? seed?.cwd ?? sidebarCwd,
              latest?.model ?? seed?.model,
              latest?.runtimeMode ?? seed?.runtimeMode,
              latest?.modelSettings ?? open?.modelSettings,
            ),
          confirmClose: async (closedTabs) => {
            const files = filesInWorkspaceTabs(closedTabs);
            const unsaved = files.some(
              (file) =>
                isFilesystemTab(file) && dirtyFilesRef.current.has(file.id),
            );
            if (
              unsaved &&
              !(await confirmDiscardUnsaved(
                `${mode === "archive" ? "Archive" : "Delete"} this conversation with unsaved files?`,
              ))
            )
              return false;
            const terminals = files.filter((file) => file.terminal);
            return (
              terminals.length === 0 || (await confirmCloseTerminals(terminals))
            );
          },
          stop: async () => {
            const run =
              mode === "delete" ? orchestrator.forSession(sessionId) : undefined;
            if (run && (run.status === "active" || run.status === "paused"))
              await orchestrator.stopRun(run.leadId);
            await stopSessionForRemoval(sessionId);
          },
          updateSession: (stopped) => {
            const next = sessionsRef.current.map((session) =>
              session.id === sessionId ? stopped : session,
            );
            sessionsRef.current = next;
            setSessions(next);
          },
          persist: async (latest) => {
            if (latest) await flushSessionCheckpoint(sessionId);
            if (mode === "delete") {
              await orchestrator.deleteSession(sessionId, () =>
                deleteSession(sessionId),
              );
              const released = sessionsRef.current.map((session) =>
                releaseOrchestrationWorker(session, sessionId),
              );
              sessionsRef.current = released;
              setSessions(released);
              for (const [id, cached] of loadedSessionCache.current) {
                if (releaseOrchestrationWorker(cached, sessionId) !== cached)
                  invalidateLoadedSession(id);
              }
              // Pending reads may still carry the deleted lead's ownership.
              for (const id of sessionLoads.current.keys())
                invalidateLoadedSession(id);
              for (const [id, pending] of pendingPersist.current) {
                pendingPersist.current.set(
                  id,
                  releaseOrchestrationWorker(pending, sessionId),
                );
              }
              const releaseSummary = (entry: SessionSummary) =>
                entry.orchestrationLeadId === sessionId
                  ? { ...entry, orchestrationLeadId: undefined }
                  : entry;
              setHistory((current) => current.map(releaseSummary));
              setStoredLinkedSessions((current) => current.map(releaseSummary));
              return;
            }
            if (latest && shouldPersistSession(latest)) {
              const saved = await upsertSession(latest);
              if (!saved)
                throw new Error("The conversation could not be saved.");
              savedSummary = saved;
            }
            await setSessionArchived(sessionId, true);
          },
          commit: (removal) => {
            const latest = sessionsRef.current.find(
              (session) => session.id === sessionId,
            );
            const harnesses: HarnessId[] = latest
              ? sessionChildHarnesses(latest)
              : [seed?.harness ?? "cursor"];
            for (const harness of harnesses) {
              void forgetHarnessSession(harness, sessionId);
            }
            lastPersisted.current.delete(sessionId);
            pendingPersist.current.delete(sessionId);
            const closingFiles = filesInWorkspaceTabs(removal.closedTabs);
            setDirtyFiles((current) => {
              const next = new Set(current);
              for (const file of closingFiles) next.delete(file.id);
              return next;
            });
            sessionsRef.current = removal.sessions;
            tabsRef.current = removal.tabs;
            setSessions(removal.sessions);
            setTabs(removal.tabs);
            if (removal.activeTabId !== activeTabIdRef.current) {
              activateTab(removal.activeTabId);
            }
            const activeTab = removal.tabs.find(
              (tab) => tab.id === removal.activeTabId,
            );
            setComposerFocused(
              removal.sessions.some(
                (session) => session.id === activeTab?.focusedId,
              ),
            );
            if (mode === "archive") {
              if (latest && shouldPersistSession(latest)) {
                rememberLoadedSession(loadedSessionCache.current, latest);
              }
              const archived =
                savedSummary ??
                summary ??
                (latest && summaryFromSession(latest));
              if (archived) {
                setHistory((current) =>
                  mergeHistorySummary(current, { ...archived, archived: true }),
                );
              }
            } else {
              setHistory((current) =>
                current.filter((entry) => entry.id !== sessionId),
              );
              void refreshHistory(sidebarCwd);
            }
          },
        });
      } catch (error) {
        const detail = error instanceof Error ? error.message : String(error);
        void message(`Could not ${mode} this conversation.\n\n${detail}`, {
          title: "MonoCode",
          kind: "error",
        });
        return false;
      } finally {
        removingSessionIds.current.delete(sessionId);
      }
    },
    [
      activateTab,
      history,
      invalidateLoadedSession,
      refreshHistory,
      sidebarCwd,
      stopSessionForRemoval,
      tabCloseScope,
    ],
  );

  const onArchiveHistorySession = useCallback(
    async (sessionId: string, archived: boolean) => {
      if (archived) return onRemoveHistorySession(sessionId, "archive");
      if (removingSessionIds.current.has(sessionId)) return false;
      try {
        await setSessionArchived(sessionId, false);
        setHistory((current) =>
          current.map((entry) =>
            entry.id === sessionId ? { ...entry, archived: false } : entry,
          ),
        );
        return true;
      } catch (error) {
        void message(
          `Could not unarchive this conversation.\n\n${String(error)}`,
          {
            title: "MonoCode",
            kind: "error",
          },
        );
        return false;
      }
    },
    [onRemoveHistorySession],
  );

  const onArchiveFocusedSession = useCallback(
    (event: KeyboardEvent) => {
      archiveFocusedSession(
        event,
        {
          activeTabId: activeTabIdRef.current,
          tabs: tabsRef.current,
          sessions: sessionsRef.current,
          projectTerminalFocused: projectTerminalFocusedRef.current,
          surfaceOpen: Boolean(
            searchViewOpenRef.current ||
            inboxViewOpenRef.current ||
            notesViewOpenRef.current ||
            settingsOpenRef.current ||
            filePickerOpenRef.current ||
            whatsNewVersionRef.current,
          ),
        },
        (sessionId) => {
          void onArchiveHistorySession(sessionId, true);
        },
      );
    },
    [onArchiveHistorySession],
  );

  const onPinHistorySession = useCallback(
    async (sessionId: string, pinned: boolean) => {
      const open = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      if (open && shouldPersistSession(open)) {
        await upsertSession(open).catch(() => undefined);
      }
      await setSessionPinned(sessionId, pinned).catch(() => undefined);
      setHistory((current) => {
        const existing = current.find((entry) => entry.id === sessionId);
        if (existing) {
          return mergeProjectHistorySummary(current, { ...existing, pinned });
        }
        if (!open) return current;
        return mergeProjectHistorySummary(current, {
          ...summaryFromSession(open),
          pinned,
        });
      });
    },
    [],
  );

  const onArchiveHistorySessions = useCallback(
    async (sessionIds: readonly string[], archived: boolean) => {
      for (const sessionId of sessionIds) {
        if (!(await onArchiveHistorySession(sessionId, archived))) break;
      }
    },
    [onArchiveHistorySession],
  );

  const onPinHistorySessions = useCallback(
    async (sessionIds: readonly string[], pinned: boolean) => {
      await Promise.all(
        sessionIds.map((sessionId) => onPinHistorySession(sessionId, pinned)),
      );
    },
    [onPinHistorySession],
  );

  const onDeleteHistorySession = useCallback(
    (sessionId: string) => onRemoveHistorySession(sessionId, "delete"),
    [onRemoveHistorySession],
  );

  const onDeleteHistorySessions = useCallback(
    async (sessionIds: readonly string[]) => {
      if (sessionIds.length === 0) return;
      if (
        !window.confirm(
          `Delete ${sessionIds.length} selected conversations? This can’t be undone.`,
        )
      )
        return;
      for (const sessionId of sessionIds) {
        if (!(await onRemoveHistorySession(sessionId, "delete", true))) break;
      }
    },
    [onRemoveHistorySession],
  );

  return {
    sessionReminders,
    onArchiveFocusedSession,
    onSelectHistorySession,
    openReminderSession,
    ensureReminderSessionsSaved,
    dismissNoticesForContinuedSession,
    onPlaceSessionOnPane,
    onPlaceTabOnPane,
    onRenameHistorySession,
    onRemoveHistorySession,
    onArchiveHistorySession,
    onPinHistorySession,
    onArchiveHistorySessions,
    onPinHistorySessions,
    onDeleteHistorySession,
    onDeleteHistorySessions,
  };
}
