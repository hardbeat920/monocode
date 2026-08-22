import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Sidebar } from "./chrome/Sidebar";
import { ApprovalToasts } from "./chrome/ApprovalToasts";
import { TitleBar, type Tab as TitleTab } from "./chrome/TitleBar";
import { MenuBar } from "./chrome/MenuBar";
import { FilePicker } from "./chrome/FilePicker";
import {
  loadSidebarOpen,
  loadSidebarTabOrder,
  saveSidebarOpen,
  type SidebarTabId,
} from "./lib/appearance";
import { IS_MAC } from "./lib/platform";
import { displayAttachments, prepareAttachments } from "./lib/attachments";
import { basename, pickFolder } from "./lib/fs";
import {
  invalidateProjectFiles,
  prefetchProjectFiles,
  rememberOpenedFile,
  resolveOpenablePath,
} from "./lib/fileIndex";
import {
  closeLeaf,
  findSurfacePane,
  firstLeafId,
  focusedFileTab,
  isolateTerminalPanes,
  isPlanTab,
  isTerminalTab,
  leaf,
  leafIds,
  movePane,
  neighborLeafId,
  newFileTab,
  newPlanTab,
  newTab,
  newTerminalFile,
  newTerminalWorkspaceTab,
  nextTerminalTitle,
  openEditorTab,
  openTerminalTab,
  removePane,
  replaceLeafId,
  setSplitRatio,
  siblingLeafId,
  splitPane,
  surfacePanes,
  updateTerminalTab,
  withSurfacePanes,
  type EditorPane,
  type FilePaneTab,
  type FocusDir,
  type PaneEdge,
  type SplitDir,
  type WorkspaceTab,
} from "./lib/layout";
import { orderByIds } from "./lib/reorder";
import {
  addTabsToNewGroup,
  addTabToGroup,
  applyGroupedReorder,
  insertTabBesideActive,
  insertTabInGroup,
  joinTabOnto,
  newTabGroupId,
  removeTabFromGroup,
  tabGroupProject,
  ungroupTabs,
} from "./lib/tabGroups";
import {
  collectWindowTransfer,
  type WindowTransferPayload,
} from "./lib/windowTransfer";
import {
  confirmCloseTerminal,
  confirmCloseTerminals,
} from "./lib/terminalClose";
import { terminalTabLabel } from "./lib/terminalTab";
import {
  applyHarnessEvent,
  appendUser,
  appendSteerUser,
  bindHarnessSession,
  cancelHarnessTurn,
  canSteerHarness,
  forgetHarnessSession,
  generateHarnessTitle,
  isLiveHarness,
  probeHarnessAvailability,
  refreshHarnessCatalogs,
  registerBuiltinHarnesses,
  respondHarnessApproval,
  sendHarnessTurn,
  steerHarnessTurn,
  startHarnessBridge,
  stopStreaming,
  pickTextHarness,
  type ApprovalDecision,
  type HarnessEvent,
} from "./lib/harness";
import { isEditTool } from "./lib/harness/preview";
import {
  beginSessionTurn,
  captureSessionCheckpoint,
  keepSessionChanges,
  notifyReviewChanged,
  syncSessionCheckpoint,
} from "./lib/checkpoint";
import { notifyDirsChanged } from "./lib/fileTree";
import { nudgeWatchedFiles } from "./lib/fileWatch";
import { type EditorNavigationTarget, type OpenFileFn } from "./lib/search";
import {
  loadLastModelChoice,
  mergeModelSettings,
  preferredModelSettings,
  resolveModel,
  saveLastModelChoice,
  saveLastModelSettings,
} from "./lib/models";
import { planTitle } from "./lib/plan";
import {
  displayPath,
  isEqualOrInside,
  projectName,
  rebasePath,
  resolveWorkspacePath,
} from "./lib/paths";
import {
  lastProjectPath,
  loadRecents,
  looksLikeProject,
  normalizeProjectPath,
  rememberProject,
  sameProjectPath,
} from "./lib/recents";
import {
  HARNESS_LABEL,
  canReplaceSessionTitle,
  formatSessionTitle,
  hasPendingApproval,
  newSession,
  sessionDisplayTitle,
  titleFromPrompt,
  type Attachment,
  type HarnessId,
  type RuntimeMode,
  type Session,
} from "./lib/session";
import { dropContextWindow } from "./lib/contextUsage";
import {
  deleteSession,
  getSession,
  listSessionsByProject,
  persistFingerprint,
  replaceInFlightSessions,
  saveWorkspaceSnapshot,
  shouldPersistSession,
  upsertSession,
  type SessionSummary,
} from "./lib/sessionStore";
import { syncDockBadge } from "./lib/dockBadge";
import { hiddenApprovalNotices } from "./lib/approvalToast";
import { tabCommand } from "./lib/tabKeys";
import {
  canTabVisitBack,
  canTabVisitForward,
  emptyTabVisitHistory,
  pruneTabVisitHistory,
  recordTabVisit,
  tabVisitBack,
  tabVisitForward,
  type TabVisitHistory,
} from "./lib/tabVisitHistory";
import { applySkillsToTurn } from "./lib/skills";
import { PaneTree } from "./surfaces/PaneTree";
import { DiffPane } from "./surfaces/DiffPane";
import {
  handleEditorFindKey,
  openFindInActiveEditor,
} from "./surfaces/editorSearch";

import {
  mergeHistorySummary,
  historyWithLiveSessions,
} from "./lib/sessionHistory";
import {
  CONTINUE_PROMPT,
  canAutoContinue,
  inFlightRefs,
  inFlightSnapshotKey,
  shouldWriteInFlightSnapshot,
} from "./lib/inFlight";
import {
  collectWorkspaceSnapshot,
  workspaceSnapshotKey,
} from "./lib/workspaceSnapshot";
import {
  bindResumedSessions,
  hasInFlightSessions,
  hideCurrentWindow,
  closeCurrentWindow,
  isAppQuitting,
  persistLiveTranscripts,
  persistQuitState,
  reapWindowRuntime,
  setQuitWorkspace,
  type ResumedWorkspace,
} from "./lib/appLifecycle";

function setsEqual<T>(a: Set<T>, b: Set<T>): boolean {
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
}

type ScheduledFlush = { kind: "raf" | "timeout"; id: number };

function cancelScheduledFlush(handle: ScheduledFlush | null) {
  if (!handle) return;
  if (handle.kind === "raf") cancelAnimationFrame(handle.id);
  else clearTimeout(handle.id);
}

function scheduleHarnessFlush(run: () => void): ScheduledFlush {
  if (document.hidden) {
    return { kind: "timeout", id: window.setTimeout(run, 32) };
  }
  return { kind: "raf", id: requestAnimationFrame(run) };
}

function openSessionIds(tabs: WorkspaceTab[]): Set<string> {
  const ids = new Set<string>();
  for (const tab of tabs) {
    for (const id of leafIds(tab.layout)) ids.add(id);
  }
  return ids;
}

function titleTabsEqual(a: TitleTab[], b: TitleTab[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((tab, index) => {
    const other = b[index];
    return (
      other != null &&
      tab.id === other.id &&
      tab.project === other.project &&
      tab.title === other.title &&
      tab.sessionCount === other.sessionCount &&
      tab.dirty === other.dirty &&
      tab.more.join("\u0000") === other.more.join("\u0000") &&
      tab.harnesses.join("\u0000") === other.harnesses.join("\u0000") &&
      tab.busyHarnesses.join("\u0000") === other.busyHarnesses.join("\u0000") &&
      tab.files.join("\u0000") === other.files.join("\u0000") &&
      tab.multiPane === other.multiPane &&
      tab.fileFocused === other.fileFocused &&
      tab.terminal === other.terminal &&
      tab.groupId === other.groupId
    );
  });
}

export default function App({
  windowTransfer = null,
  resumed = null,
}: {
  windowTransfer?: WindowTransferPayload | null;
  resumed?: ResumedWorkspace | null;
}) {
  const [projectCwd, setProjectCwd] = useState(
    () =>
      windowTransfer?.projectCwd ??
      resumed?.projectCwd ??
      lastProjectPath() ??
      "~",
  );
  const [recents, setRecents] = useState(() =>
    resumed?.projectCwd && looksLikeProject(resumed.projectCwd)
      ? rememberProject(resumed.projectCwd)
      : loadRecents(),
  );
  const [seed] = useState(() => {
    const cwd = lastProjectPath() ?? "~";
    const last = loadLastModelChoice();
    const session = last
      ? newSession(last.harness, cwd, last.model)
      : newSession("cursor", cwd);
    const tab = newTab(session.id);
    return { session, tab };
  });
  const [sessions, setSessions] = useState<Session[]>(
    () => windowTransfer?.sessions ?? resumed?.sessions ?? [seed.session],
  );
  const [tabs, setTabs] = useState<WorkspaceTab[]>(
    () => windowTransfer?.tabs ?? resumed?.tabs ?? [seed.tab],
  );
  const [activeTabId, setActiveTabId] = useState(
    () => windowTransfer?.activeTabId ?? resumed?.activeTabId ?? seed.tab.id,
  );
  const [composerFocused, setComposerFocused] = useState(() => {
    if (windowTransfer) return true;
    if (!resumed) return false;
    const tab =
      resumed.tabs.find((entry) => entry.id === resumed.activeTabId) ??
      resumed.tabs[0];
    return !!tab && resumed.sessions.some((session) => session.id === tab.focusedId);
  });
  /** Tab id -> project name, kept in sync with the rendered title tabs. */
  const tabProjectsRef = useRef(new Map<string, string>());
  const projectOfTab = useCallback(
    (id: string) => tabProjectsRef.current.get(id),
    [],
  );
  const [sidebarOpen, setSidebarOpen] = useState(loadSidebarOpen);
  const [sidebarTab, setSidebarTab] = useState<SidebarTabId>(
    () => loadSidebarTabOrder()[0] ?? "sessions",
  );
  const [filesSearchOpen, setFilesSearchOpen] = useState(false);
  const [searchFocusToken, setSearchFocusToken] = useState(0);
  const [editorNavigation, setEditorNavigation] =
    useState<EditorNavigationTarget | null>(null);
  const editorNavigationToken = useRef(0);
  const [filePickerOpen, setFilePickerOpen] = useState(false);
  const [dirtyFiles, setDirtyFiles] = useState<Set<string>>(
    () => new Set(windowTransfer?.dirtyFileIds ?? []),
  );
  // Not carried across a window transfer the way dirty state is: the editor
  // re-lints whatever it mounts, so the counts rebuild themselves.
  const [fileErrorCounts, setFileErrorCounts] = useState<Map<string, number>>(
    () => new Map(),
  );
  const [history, setHistory] = useState<SessionSummary[]>([]);
  const [historyStatus, setHistoryStatus] = useState<
    "idle" | "loading" | "error"
  >("idle");

  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const activeTabIdRef = useRef(activeTabId);
  activeTabIdRef.current = activeTabId;
  const projectCwdRef = useRef(projectCwd);
  projectCwdRef.current = projectCwd;
  const tabVisitRef = useRef(emptyTabVisitHistory(activeTabId));
  const tabVisitFromHistoryRef = useRef(false);
  const [tabVisitNav, setTabVisitNav] = useState({
    canBack: false,
    canForward: false,
  });
  const turnGen = useRef(new Map<string, number>());
  const lastPersisted = useRef(new Map<string, string>());
  const lastBoundProvider = useRef(new Map<string, string>());
  const lastPersistedUserBlock = useRef(new Map<string, string>());
  const inFlightSyncKey = useRef<string | null>(null);
  const sawInFlight = useRef(false);
  const workspaceSyncKey = useRef<string | null>(null);
  const observedSessions = useRef(new Map<string, Session>());
  const pendingPersist = useRef(new Map<string, Session>());
  // Tokens arrive many times per frame; apply them once so React/markdown aren't
  // recomputed for every delta.
  const harnessQueued = useRef(new Map<string, HarnessEvent[]>());
  const harnessFlush = useRef<ScheduledFlush | null>(null);
  const skipForgetSessionIds = useRef(new Set<string>());
  const importedSessionsApplied = useRef(false);

  useEffect(() => {
    if (importedSessionsApplied.current) return;
    const imported = windowTransfer?.sessions ?? resumed?.sessions;
    if (!imported?.length) return;
    importedSessionsApplied.current = true;
    for (const session of imported) {
      observedSessions.current.set(session.id, session);
      lastPersisted.current.set(session.id, persistFingerprint(session));
      const userId = lastUserBlockId(session);
      if (userId) lastPersistedUserBlock.current.set(session.id, userId);
      if (session.providerSessionId) {
        lastBoundProvider.current.set(session.id, session.providerSessionId);
      }
    }
  }, [windowTransfer, resumed]);

  const flushHarnessEvents = useCallback(() => {
    cancelScheduledFlush(harnessFlush.current);
    harnessFlush.current = null;
    const batches = harnessQueued.current;
    if (batches.size === 0) return;
    harnessQueued.current = new Map();
    const prev = sessionsRef.current;
    const next = prev.map((session) => {
      const events = batches.get(session.id);
      return events ? events.reduce(applyHarnessEvent, session) : session;
    });
    if (!next.some((session, index) => session !== prev[index])) return;
    sessionsRef.current = next;
    syncDockBadge(next);
    setSessions(next);
  }, []);

  const applyApprovalEvent = useCallback((sessionId: string, event: HarnessEvent) => {
    const queued = harnessQueued.current.get(sessionId) ?? [];
    harnessQueued.current.delete(sessionId);
    const events = [...queued, event];
    const prev = sessionsRef.current;
    const next = prev.map((session) =>
      session.id === sessionId
        ? events.reduce(applyHarnessEvent, session)
        : session,
    );
    if (!next.some((session, index) => session !== prev[index])) return;
    sessionsRef.current = next;
    syncDockBadge(next);
    setSessions(next);
  }, []);

  const enqueueHarnessEvent = useCallback(
    (sessionId: string, event: HarnessEvent) => {
      if (
        event.type === "approval.requested" ||
        event.type === "approval.resolved"
      ) {
        applyApprovalEvent(sessionId, event);
        return;
      }
      const queued = harnessQueued.current;
      const events = queued.get(sessionId);
      if (events) events.push(event);
      else queued.set(sessionId, [event]);
      if (!harnessFlush.current) {
        harnessFlush.current = scheduleHarnessFlush(flushHarnessEvents);
      }
    },
    [applyApprovalEvent, flushHarnessEvents],
  );

  useEffect(() => {
    registerBuiltinHarnesses();
    if (resumed?.sessions.length) bindResumedSessions(resumed.sessions);
    const stopBridge = startHarnessBridge();
    const reap = () => {
      if (isAppQuitting()) return;
      void persistQuitState(
        sessionsRef.current,
        tabsRef.current,
        activeTabIdRef.current,
        projectCwdRef.current,
        "unload",
      ).finally(() => {
        void reapWindowRuntime(sessionsRef.current, tabsRef.current);
      });
    };
    window.addEventListener("pagehide", reap);
    window.addEventListener("beforeunload", reap);
    return () => {
      window.removeEventListener("pagehide", reap);
      window.removeEventListener("beforeunload", reap);
      stopBridge();
      cancelScheduledFlush(harnessFlush.current);
      harnessFlush.current = null;
    };
  }, [resumed]);

  useEffect(() => {
    void probeHarnessAvailability();
    void refreshHarnessCatalogs().then(() => {
      setSessions((prev) =>
        prev.map((session) => {
          if (!isLiveHarness(session.harness)) return session;
          const resolved = resolveModel(session.harness, session.model);
          const modelSettings = mergeModelSettings(
            resolved,
            session.modelSettings,
          );
          if (
            resolved.id === session.model &&
            sameSettings(modelSettings, session.modelSettings)
          ) {
            return session;
          }
          return { ...session, model: resolved.id, modelSettings };
        }),
      );
    });
  }, []);

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0];
  const active =
    sessions.find((session) => session.id === activeTab?.focusedId) ??
    sessions.find(
      (session) => activeTab && leafIds(activeTab.layout).includes(session.id),
    );
  const sessionDefaults = active ?? sessions[0];
  const sidebarCwd =
    active?.cwd ??
    (activeTab ? focusedFileTab(activeTab)?.cwd : undefined) ??
    projectCwd;
  const sidebarCwdRef = useRef(sidebarCwd);
  sidebarCwdRef.current = sidebarCwd;

  const nextBusySessionIds = useMemo(() => {
    const ids = new Set<string>();
    for (const session of sessions) {
      if (session.busy) ids.add(session.id);
    }
    return ids;
  }, [sessions]);
  const busySessionIdsRef = useRef(nextBusySessionIds);
  if (!setsEqual(busySessionIdsRef.current, nextBusySessionIds)) {
    busySessionIdsRef.current = nextBusySessionIds;
  }
  const busySessionIds = busySessionIdsRef.current;

  const nextApprovalSessionIds = useMemo(() => {
    const ids = new Set<string>();
    for (const session of sessions) {
      if (hasPendingApproval(session.blocks)) ids.add(session.id);
    }
    return ids;
  }, [sessions]);
  const approvalSessionIdsRef = useRef(nextApprovalSessionIds);
  if (!setsEqual(approvalSessionIdsRef.current, nextApprovalSessionIds)) {
    approvalSessionIdsRef.current = nextApprovalSessionIds;
  }
  const approvalSessionIds = approvalSessionIdsRef.current;

  const hiddenApprovalToasts = useMemo(
    () =>
      hiddenApprovalNotices(
        sessions,
        activeTabId,
        tabs,
        composerFocused,
      ),
    [sessions, activeTabId, tabs, composerFocused],
  );

  useEffect(() => {
    syncDockBadge(sessions);
  }, [sessions]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) {
          flushHarnessEvents();
          syncDockBadge(sessionsRef.current);
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => {
      unlisten?.();
    };
  }, [flushHarnessEvents]);

  useEffect(() => {
    const onVisible = () => {
      if (!document.hidden) flushHarnessEvents();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, [flushHarnessEvents]);

  useEffect(() => {
    let unlistenClose: (() => void) | undefined;
    const releaseQuit = setQuitWorkspace(
      () => sessionsRef.current,
      () => tabsRef.current,
      () => activeTabIdRef.current,
      () => projectCwdRef.current,
      flushHarnessEvents,
    );
    void getCurrentWindow()
      .onCloseRequested((event) => {
        // Listening here makes close our job. Letting the default path run
        // calls JS `window.destroy`, which Tauri denies without a permission.
        event.preventDefault();
        if (hasInFlightSessions(sessionsRef.current)) {
          flushHarnessEvents();
          void persistLiveTranscripts(sessionsRef.current);
          void hideCurrentWindow();
          return;
        }
        void persistQuitState(
          sessionsRef.current,
          tabsRef.current,
          activeTabIdRef.current,
          projectCwdRef.current,
          "unload",
        ).finally(() => {
          void closeCurrentWindow();
        });
      })
      .then((fn) => {
        unlistenClose = fn;
      });
    return () => {
      releaseQuit();
      unlistenClose?.();
    };
  }, [flushHarnessEvents]);

  const refreshHistory = useCallback(async (cwd: string) => {
    if (!cwd || cwd === "~") {
      setHistory([]);
      setHistoryStatus("idle");
      return;
    }
    setHistory([]);
    setHistoryStatus("loading");
    try {
      const rows = await listSessionsByProject(cwd);
      setHistory(rows);
      setHistoryStatus("idle");
    } catch {
      setHistoryStatus("error");
    }
  }, []);

  useEffect(() => {
    void refreshHistory(sidebarCwd);
  }, [sidebarCwd, refreshHistory]);

  useEffect(() => {
    prefetchProjectFiles(sidebarCwd);
  }, [sidebarCwd]);

  useEffect(() => {
    const liveIds = new Set(sessions.map((session) => session.id));
    const visibleIds = openSessionIds(tabsRef.current);
    for (const session of sessions) {
      if (observedSessions.current.get(session.id) === session) continue;
      observedSessions.current.set(session.id, session);
      const parked = !visibleIds.has(session.id);
      const newlyBound =
        !!session.providerSessionId &&
        lastBoundProvider.current.get(session.id) !== session.providerSessionId;
      const lastUserId = lastUserBlockId(session);
      const newUserTurn =
        !!lastUserId &&
        lastPersistedUserBlock.current.get(session.id) !== lastUserId;
      if (newlyBound && session.providerSessionId) {
        lastBoundProvider.current.set(session.id, session.providerSessionId);
      }
      if (newUserTurn && lastUserId) {
        lastPersistedUserBlock.current.set(session.id, lastUserId);
      }
      if (
        (newlyBound || newUserTurn) &&
        shouldPersistSession(session)
      ) {
        const snapshot = session;
        void upsertSession(snapshot)
          .then((summary) => {
            if (!summary) return;
            lastPersisted.current.set(snapshot.id, persistFingerprint(snapshot));
          })
          .catch(() => undefined);
      }
      if (
        shouldPersistSession(session) &&
        (!session.busy ||
          parked ||
          newlyBound ||
          newUserTurn ||
          !lastPersisted.current.has(session.id))
      ) {
        pendingPersist.current.set(session.id, session);
      }
    }
    for (const sessionId of observedSessions.current.keys()) {
      if (liveIds.has(sessionId)) continue;
      observedSessions.current.delete(sessionId);
      pendingPersist.current.delete(sessionId);
    }
    if (pendingPersist.current.size === 0) return;

    const timer = window.setTimeout(() => {
      const dirty = [...pendingPersist.current.values()];
      pendingPersist.current.clear();
      void Promise.all(
        dirty.map(async (session) => {
          const fingerprint = persistFingerprint(session);
          if (lastPersisted.current.get(session.id) === fingerprint) return;
          const summary = await upsertSession(session).catch(() => null);
          if (!summary) return;
          lastPersisted.current.set(session.id, fingerprint);
          if (summary.cwd === sidebarCwdRef.current) {
            setHistory((current) =>
              mergeHistorySummary(
                current.filter((entry) =>
                  sameProjectPath(entry.cwd, summary.cwd),
                ),
                summary,
              ),
            );
          }
        }),
      );
    }, 650);
    return () => window.clearTimeout(timer);
  }, [sessions]);

  useEffect(() => {
    const refs = inFlightRefs(sessions, tabs);
    if (refs.length > 0) sawInFlight.current = true;
    const key = inFlightSnapshotKey(refs);
    if (
      !shouldWriteInFlightSnapshot(
        key,
        refs,
        inFlightSyncKey.current,
        sawInFlight.current,
      )
    ) {
      return;
    }
    inFlightSyncKey.current = key;
    void replaceInFlightSessions(refs).catch(() => undefined);
  }, [sessions, tabs]);

  useEffect(() => {
    if (windowTransfer) return;
    const snapshot = collectWorkspaceSnapshot(
      tabs,
      sessions,
      activeTabId,
      projectCwd,
    );
    const key = workspaceSnapshotKey(snapshot);
    if (workspaceSyncKey.current === key) return;
    workspaceSyncKey.current = key;
    const timer = window.setTimeout(() => {
      void saveWorkspaceSnapshot(snapshot).catch(() => undefined);
    }, 250);
    return () => window.clearTimeout(timer);
  }, [tabs, sessions, activeTabId, projectCwd, windowTransfer]);

  useEffect(() => {
    if (lastProjectPath()) return;
    void invoke<string>("default_cwd")
      .then((cwd) => {
        if (!looksLikeProject(cwd)) return;
        setProjectCwd(cwd);
        setRecents((prev) => (prev.length > 0 ? prev : rememberProject(cwd)));
        setSessions((prev) =>
          prev.map((s) => (s.cwd === "~" ? { ...s, cwd } : s)),
        );
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    setTabs((prev) => {
      let changed = false;
      const next = prev.map((tab) => {
        const isolated = isolateTerminalPanes(tab);
        if (isolated !== tab) changed = true;
        return isolated;
      });
      return changed ? next : prev;
    });
  }, [tabs]);

  const persistSession = useCallback((session: Session | undefined) => {
    if (!session || !shouldPersistSession(session)) return;
    const fingerprint = persistFingerprint(session);
    void upsertSession(session)
      .then((summary) => {
        if (!summary) return;
        lastPersisted.current.set(session.id, fingerprint);
        if (summary.cwd === sidebarCwdRef.current) {
          setHistory((current) =>
            mergeHistorySummary(
              current.filter((entry) =>
                sameProjectPath(entry.cwd, summary.cwd),
              ),
              summary,
            ),
          );
        }
      })
      .catch(() => undefined);
  }, []);

  // Tabs are views. A working agent stays in memory (and keeps its child)
  // until the turn finishes or the session is deleted.
  useEffect(() => {
    const visibleIds = openSessionIds(tabs);
    const idleDetached = sessions.filter(
      (session) => !visibleIds.has(session.id) && !session.busy,
    );
    if (idleDetached.length === 0) return;
    for (const session of idleDetached) {
      if (skipForgetSessionIds.current.has(session.id)) continue;
      persistSession(session);
      void forgetHarnessSession(session.harness, session.id);
    }
    setSessions((prev) =>
      prev.filter(
        (session) =>
          visibleIds.has(session.id) ||
          session.busy ||
          skipForgetSessionIds.current.has(session.id),
      ),
    );
  }, [sessions, tabs, persistSession]);

  const activateTab = useCallback((id: string) => {
    setActiveTabId(id);
    const tab = tabsRef.current.find((entry) => entry.id === id);
    setComposerFocused(
      !!tab &&
        sessionsRef.current.some((session) => session.id === tab.focusedId),
    );
  }, []);

  const commitTabVisit = useCallback((history: TabVisitHistory) => {
    tabVisitRef.current = history;
    const canBack = canTabVisitBack(history);
    const canForward = canTabVisitForward(history);
    setTabVisitNav((prev) =>
      prev.canBack === canBack && prev.canForward === canForward
        ? prev
        : { canBack, canForward },
    );
  }, []);

  useEffect(() => {
    const openIds = new Set(tabs.map((tab) => tab.id));
    let next = pruneTabVisitHistory(
      tabVisitRef.current,
      openIds,
      activeTabId,
    );
    if (tabVisitFromHistoryRef.current) {
      tabVisitFromHistoryRef.current = false;
    } else if (next.current !== activeTabId) {
      next = recordTabVisit(next, activeTabId);
    }
    commitTabVisit(pruneTabVisitHistory(next, openIds, activeTabId));
  }, [activeTabId, commitTabVisit, tabs]);

  /** `cwd` scopes group inheritance: a tab from another project starts alone. */
  const appendTab = useCallback(
    (tab: WorkspaceTab, cwd?: string) => {
      setTabs((prev) =>
        insertTabBesideActive(prev, tab, activeTabIdRef.current, (id) =>
          id === tab.id
            ? cwd
              ? projectName(cwd)
              : undefined
            : projectOfTab(id),
        ),
      );
    },
    [projectOfTab],
  );

  const onNew = useCallback(() => {
    const cwd = active?.cwd ?? sessionDefaults?.cwd ?? projectCwd;
    const session = newSession(
      sessionDefaults?.harness ?? "claude",
      cwd,
      sessionDefaults?.model,
      sessionDefaults?.runtimeMode,
      sessionDefaults?.modelSettings,
    );
    const tab = newTab(session.id);
    setSessions((prev) => [...prev, session]);
    appendTab(tab, cwd);
    setActiveTabId(tab.id);
    setComposerFocused(true);
  }, [
    active?.cwd,
    appendTab,
    sessionDefaults?.harness,
    sessionDefaults?.cwd,
    sessionDefaults?.model,
    sessionDefaults?.runtimeMode,
    sessionDefaults?.modelSettings,
    projectCwd,
  ]);

  const onSplit = useCallback(
    (dir: SplitDir) => {
      if (!activeTab) return;
      const session = newSession(
        sessionDefaults?.harness ?? "claude",
        sessionDefaults?.cwd ?? projectCwd,
        sessionDefaults?.model,
        sessionDefaults?.runtimeMode,
        sessionDefaults?.modelSettings,
      );
      setSessions((prev) => [...prev, session]);
      setTabs((prev) =>
        prev.map((t) => {
          if (t.id !== activeTab.id) return t;
          return {
            ...t,
            layout: splitPane(t.layout, t.focusedId, dir, session.id),
            focusedId: session.id,
          };
        }),
      );
      setComposerFocused(true);
    },
    [activeTab, projectCwd, sessionDefaults],
  );

  const onOpenTerminal = useCallback(
    (cwd: string, asWorkspaceTab = false, occupySessionId?: string) => {
      const workdir = cwd || active?.cwd || projectCwd;
      if (asWorkspaceTab || !activeTab) {
        const file = newTerminalFile(workdir);
        const tab = newTerminalWorkspaceTab(file);
        appendTab(tab, workdir);
        setActiveTabId(tab.id);
        setComposerFocused(false);
        return;
      }

      const occupying = sessionsRef.current.find(
        (session) => session.id === (occupySessionId ?? activeTab.focusedId),
      );
      const occupyPaneId =
        occupying && isBlankSession(occupying) ? occupying.id : undefined;
      if (occupyPaneId && occupying) {
        lastPersisted.current.delete(occupyPaneId);
        void forgetHarnessSession(occupying.harness, occupyPaneId);
        setSessions((prev) =>
          prev.filter((session) => session.id !== occupyPaneId),
        );
      }

      const file = newTerminalFile(workdir, nextTerminalTitle(activeTab, workdir));
      setTabs((prev) =>
        prev.map((tab) =>
          tab.id === activeTab.id
            ? openTerminalTab(tab, file, occupyPaneId)
            : tab,
        ),
      );
      setComposerFocused(false);
    },
    [active?.cwd, activeTab, appendTab, projectCwd],
  );

  const onNewTerminal = useCallback(() => {
    onOpenTerminal(active?.cwd ?? projectCwd);
  }, [active?.cwd, onOpenTerminal, projectCwd]);

  const onNewTerminalInSession = useCallback(
    (sessionId: string) => {
      const session = sessionsRef.current.find((entry) => entry.id === sessionId);
      onOpenTerminal(session?.cwd ?? projectCwd, false, sessionId);
    },
    [onOpenTerminal, projectCwd],
  );

  const onTerminalMetaChange = useCallback(
    (fileId: string, patch: { title?: string; cwd?: string }) => {
      setTabs((prev) =>
        prev.map((tab) => updateTerminalTab(tab, fileId, patch)),
      );
    },
    [],
  );

  const onNewTerminalTab = useCallback(() => {
    onOpenTerminal(active?.cwd ?? projectCwd, true);
  }, [active?.cwd, onOpenTerminal, projectCwd]);

  const onCloseTab = useCallback(
    (id: string, opts?: { confirmedTerminalIds?: string[] }) => {
      const current = tabsRef.current;
      const index = current.findIndex((t) => t.id === id);
      if (current.length < 2 || index < 0) return;
      const closing = current[index];
      const closingFiles = [
        ...closing.editorPanes.flatMap((pane) => pane.files),
        ...(closing.terminalPanes ?? []).flatMap((pane) => pane.files),
      ];
      const unsaved = closingFiles.filter((file) => dirtyFiles.has(file.id));
      if (
        unsaved.length > 0 &&
        !window.confirm("Close this tab with unsaved files?")
      ) {
        return;
      }

      const finishClose = () => {
        const next = current.filter((t) => t.id !== id);
        const gone = new Set(
          leafIds(closing.layout).filter((paneId) =>
            sessionsRef.current.some((session) => session.id === paneId),
          ),
        );
        for (const sessionId of gone) {
          persistSession(sessionsRef.current.find((s) => s.id === sessionId));
        }
        setDirtyFiles((prev) => {
          const updated = new Set(prev);
          for (const file of closingFiles) updated.delete(file.id);
          return updated;
        });
        setTabs(next);
        if (id === activeTabIdRef.current) {
          activateTab((next[Math.max(0, index - 1)] ?? next[0]).id);
        }
        void refreshHistory(sidebarCwd);
      };

      const confirmed = new Set(opts?.confirmedTerminalIds ?? []);
      const terminals = closingFiles.filter(
        (file) => file.terminal && !confirmed.has(file.id),
      );
      if (terminals.length > 0) {
        void confirmCloseTerminals(terminals).then((ok) => ok && finishClose());
        return;
      }
      finishClose();
    },
    [
      dirtyFiles,
      activateTab,
      persistSession,
      refreshHistory,
      sidebarCwd,
    ],
  );

  const onGroupNewTab = useCallback(
    (groupId: string) => {
      const groupTab = tabsRef.current.find((tab) => tab.groupId === groupId);
      const sessionInTab =
        groupTab
          ? sessionsRef.current.find((session) =>
              leafIds(groupTab.layout).includes(session.id),
            )
          : undefined;
      const cwd = sessionInTab?.cwd ?? active?.cwd ?? projectCwd;
      const session = newSession(
        sessionDefaults?.harness ?? "claude",
        cwd,
        sessionDefaults?.model,
        sessionDefaults?.runtimeMode,
        sessionDefaults?.modelSettings,
      );
      const tab = newTab(session.id);
      setSessions((prev) => [...prev, session]);
      setTabs((prev) => insertTabInGroup(prev, tab, groupId));
      setActiveTabId(tab.id);
      setComposerFocused(true);
    },
    [
      active?.cwd,
      projectCwd,
      sessionDefaults?.harness,
      sessionDefaults?.cwd,
      sessionDefaults?.model,
      sessionDefaults?.runtimeMode,
      sessionDefaults?.modelSettings,
    ],
  );

  const onGroupCloseTabs = useCallback(
    (tabIds: string[]) => {
      for (const id of tabIds) onCloseTab(id);
    },
    [onCloseTab],
  );

  const onGroupMoveToNewWindow = useCallback(
    async (tabIds: string[]) => {
      const payload = collectWindowTransfer(
        tabsRef.current,
        sessionsRef.current,
        tabIds,
        activeTabIdRef.current,
        dirtyFiles,
        projectCwd,
      );
      if (!payload) return;

      const sessionIds = new Set(payload.sessions.map((session) => session.id));
      for (const id of sessionIds) skipForgetSessionIds.current.add(id);

      try {
        await invoke("stage_window_transfer", {
          payload: JSON.stringify(payload),
        });
        await invoke("open_new_window");
      } catch {
        for (const id of sessionIds) skipForgetSessionIds.current.delete(id);
        return;
      }

      const remainingTabs = tabsRef.current.filter((tab) => !tabIds.includes(tab.id));
      if (remainingTabs.length === 0) {
        const seedSession = sessionsRef.current.find((session) =>
          sessionIds.has(session.id),
        );
        const session = newSession(
          seedSession?.harness ?? "claude",
          seedSession?.cwd ?? projectCwd,
          seedSession?.model,
          seedSession?.runtimeMode,
          seedSession?.modelSettings,
        );
        const tab = newTab(session.id);
        setSessions((prev) => [
          ...prev.filter((entry) => !sessionIds.has(entry.id)),
          session,
        ]);
        setTabs([tab]);
        setActiveTabId(tab.id);
      } else {
        setTabs(remainingTabs);
        setSessions((prev) =>
          prev.filter((session) => !sessionIds.has(session.id)),
        );
        if (tabIds.includes(activeTabIdRef.current)) {
          activateTab(remainingTabs[0]?.id ?? activeTabIdRef.current);
        }
      }

      setDirtyFiles((prev) => {
        const next = new Set(prev);
        for (const id of payload.dirtyFileIds) next.delete(id);
        return next;
      });

      for (const id of sessionIds) skipForgetSessionIds.current.delete(id);
    },
    [activateTab, dirtyFiles, projectCwd],
  );

  const onCloseFile = useCallback(
    (paneId: string, fileId: string) => {
      const tab = tabsRef.current.find((entry) => findSurfacePane(entry, paneId));
      if (!tab) return;
      const found = findSurfacePane(tab, paneId);
      if (!found) return;
      const { kind, pane } = found;
      const index = pane.files.findIndex((file) => file.id === fileId);
      if (index < 0) return;
      const file = pane.files[index];
      if (
        !file.plan &&
        !file.terminal &&
        dirtyFiles.has(fileId) &&
        !window.confirm(`Close ${basename(file.path)} without saving?`)
      ) {
        return;
      }

      const finishClose = () => {
        const files = pane.files.filter((entry) => entry.id !== fileId);
        let nextFocus = tab.focusedId;
        let nextLayout = tab.layout;
        let nextPanes = surfacePanes(tab, kind);
        if (files.length > 0) {
          nextFocus = paneId;
          const activeFileId =
            pane.activeFileId === fileId
              ? files[Math.min(index, files.length - 1)].id
              : pane.activeFileId;
          nextPanes = nextPanes.map((entry) =>
            entry.id === paneId ? { ...entry, files, activeFileId } : entry,
          );
        } else {
          const sibling = siblingLeafId(tab.layout, paneId);
          const withoutPane = removePane(tab.layout, paneId);
          if (!withoutPane) {
            setDirtyFiles((prev) => {
              const next = new Set(prev);
              next.delete(fileId);
              return next;
            });
            if (tabsRef.current.length > 1) {
              onCloseTab(
                tab.id,
                file.terminal ? { confirmedTerminalIds: [fileId] } : undefined,
              );
              return;
            }
            const seed = sessionsRef.current[0];
            const session = newSession(
              seed?.harness ?? "claude",
              file.cwd || projectCwd,
              seed?.model,
              seed?.runtimeMode,
              seed?.modelSettings,
            );
            setSessions((prev) => [...prev, session]);
            setTabs((prev) =>
              prev.map((entry) =>
                entry.id === tab.id
                  ? {
                      ...entry,
                      layout: leaf(session.id),
                      focusedId: session.id,
                      editorPanes: [],
                      terminalPanes: [],
                      diffOpen: false,
                      diffFocused: false,
                    }
                  : entry,
              ),
            );
            setComposerFocused(true);
            return;
          }
          nextLayout = withoutPane;
          nextFocus =
            tab.focusedId === paneId
              ? (sibling ?? firstLeafId(withoutPane))
              : tab.focusedId;
          nextPanes = nextPanes.filter((entry) => entry.id !== paneId);
        }

        setTabs((prev) =>
          prev.map((entry) =>
            entry.id === tab.id
              ? withSurfacePanes(
                  {
                    ...entry,
                    layout: nextLayout,
                    focusedId: nextFocus,
                  },
                  kind,
                  nextPanes,
                )
              : entry,
          ),
        );
        setDirtyFiles((prev) => {
          const next = new Set(prev);
          next.delete(fileId);
          return next;
        });
        if (tab.id === activeTabId && files.length === 0) {
          setComposerFocused(
            sessionsRef.current.some((session) => session.id === nextFocus),
          );
        }
      };

      if (file.terminal) {
        void confirmCloseTerminal(file).then((ok) => ok && finishClose());
        return;
      }
      finishClose();
    },
    [activeTabId, dirtyFiles, onCloseTab, projectCwd],
  );

  const onClearTabSession = useCallback(
    (id: string) => {
      const tab = tabs.find((entry) => entry.id === id);
      if (!tab || isBlankWorkspaceTab(tab, sessionsRef.current))
        return;

      const closingFiles = [
        ...tab.editorPanes.flatMap((pane) => pane.files),
        ...(tab.terminalPanes ?? []).flatMap((pane) => pane.files),
      ];
      const unsaved = closingFiles.filter((file) => dirtyFiles.has(file.id));
      if (
        unsaved.length > 0 &&
        !window.confirm("Close this conversation with unsaved files?")
      ) {
        return;
      }

      const oldSessionId = leafIds(tab.layout).find((paneId) =>
        sessionsRef.current.some((session) => session.id === paneId),
      );
      const oldSession = sessionsRef.current.find(
        (session) => session.id === oldSessionId,
      );
      if (!oldSession) return;

      persistSession(oldSession);

      const session = newSession(
        oldSession.harness,
        oldSession.cwd,
        oldSession.model,
        oldSession.runtimeMode,
        oldSession.modelSettings,
      );

      setSessions((prev) => [...prev, session]);
      setDirtyFiles((prev) => {
        const updated = new Set(prev);
        for (const file of closingFiles) updated.delete(file.id);
        return updated;
      });
      setTabs((prev) =>
        prev.map((entry) =>
          entry.id === id
            ? {
                ...entry,
                layout: leaf(session.id),
                focusedId: session.id,
                editorPanes: [],
                terminalPanes: [],
                diffOpen: false,
                diffFocused: false,
              }
            : entry,
        ),
      );
      setComposerFocused(true);
      void refreshHistory(sidebarCwd);
    },
    [tabs, dirtyFiles, persistSession, refreshHistory, sidebarCwd],
  );

  const onClosePane = useCallback(
    (sessionId?: string) => {
      if (!activeTab) return;
      if (
        sessionId === undefined &&
        activeTab.diffOpen &&
        activeTab.diffFocused
      ) {
        setTabs((prev) =>
          prev.map((tab) =>
            tab.id === activeTab.id
              ? {
                  ...tab,
                  diffOpen: false,
                  diffFocused: false,
                }
              : tab,
          ),
        );
        return;
      }
      const focusedSurface = findSurfacePane(activeTab, activeTab.focusedId);
      if (sessionId === undefined && focusedSurface) {
        onCloseFile(focusedSurface.pane.id, focusedSurface.pane.activeFileId);
        return;
      }
      const closingId = sessionId ?? activeTab.focusedId;
      const ids = leafIds(activeTab.layout);
      const sessionIds = ids.filter((paneId) =>
        sessionsRef.current.some((session) => session.id === paneId),
      );
      if (!sessionIds.includes(closingId)) return;
      const nextTab = closeLeaf(activeTab, closingId);
      if (!nextTab) {
        if (tabs.length < 2) onClearTabSession(activeTab.id);
        else onCloseTab(activeTab.id);
        return;
      }
      persistSession(sessionsRef.current.find((s) => s.id === closingId));
      setTabs((prev) =>
        prev.map((t) =>
          t.id === activeTab.id
            ? { ...t, layout: nextTab.layout, focusedId: nextTab.focusedId }
            : t,
        ),
      );
      if (closingId === activeTab.focusedId) {
        setComposerFocused(
          nextTab &&
            sessionsRef.current.some(
              (session) => session.id === nextTab.focusedId,
            ),
        );
      }
      void refreshHistory(sidebarCwd);
    },
    [
      activeTab,
      onCloseFile,
      onCloseTab,
      onClearTabSession,
      persistSession,
      refreshHistory,
      sidebarCwd,
      tabs.length,
    ],
  );

  const onNext = useCallback(() => {
    const index = tabs.findIndex((t) => t.id === activeTabId);
    if (index >= 0) activateTab(tabs[(index + 1) % tabs.length].id);
  }, [activateTab, activeTabId, tabs]);

  const onPrev = useCallback(() => {
    const index = tabs.findIndex((t) => t.id === activeTabId);
    if (index >= 0) {
      activateTab(tabs[(index - 1 + tabs.length) % tabs.length].id);
    }
  }, [activateTab, activeTabId, tabs]);

  const onVisitBack = useCallback(() => {
    const openIds = new Set(tabsRef.current.map((tab) => tab.id));
    const pruned = pruneTabVisitHistory(
      tabVisitRef.current,
      openIds,
      activeTabIdRef.current,
    );
    const next = tabVisitBack(pruned);
    if (!next || !openIds.has(next.current)) return;
    tabVisitFromHistoryRef.current = true;
    commitTabVisit(next);
    activateTab(next.current);
  }, [activateTab, commitTabVisit]);

  const onVisitForward = useCallback(() => {
    const openIds = new Set(tabsRef.current.map((tab) => tab.id));
    const pruned = pruneTabVisitHistory(
      tabVisitRef.current,
      openIds,
      activeTabIdRef.current,
    );
    const next = tabVisitForward(pruned);
    if (!next || !openIds.has(next.current)) return;
    tabVisitFromHistoryRef.current = true;
    commitTabVisit(next);
    activateTab(next.current);
  }, [activateTab, commitTabVisit]);

  const onActivate = useCallback(
    (slot: number) => {
      const tab = slot < 0 ? tabs[tabs.length - 1] : tabs[slot];
      if (tab) activateTab(tab.id);
    },
    [activateTab, tabs],
  );

  const onFocusPane = useCallback(
    (paneId: string) => {
      setTabs((prev) =>
        prev.map((t) =>
          t.id === activeTabId
            ? { ...t, focusedId: paneId, diffFocused: false }
            : t,
        ),
      );
      setComposerFocused(
        sessionsRef.current.some((session) => session.id === paneId),
      );
    },
    [activeTabId],
  );

  const onOpenDiff = useCallback(
    (path?: string) => {
      void (async () => {
        const resolved = path
          ? ((await resolveOpenablePath(sidebarCwd, path)) ?? path)
          : undefined;
        if (resolved) rememberOpenedFile(sidebarCwd, resolved);
        setTabs((prev) =>
          prev.map((tab) => {
            if (tab.id !== activeTabId) return tab;
            const opened = resolved
              ? openEditorTab(tab, newFileTab(resolved, sidebarCwd, true))
              : tab;
            return {
              ...opened,
              diffOpen: true,
              diffFocused: !resolved,
            };
          }),
        );
        setComposerFocused(false);
      })();
    },
    [activeTabId, sidebarCwd],
  );

  const onToggleDiff = useCallback(() => {
    setTabs((prev) =>
      prev.map((tab) =>
        tab.id === activeTabId
          ? {
              ...tab,
              diffOpen: !tab.diffOpen,
              diffFocused: !tab.diffOpen,
            }
          : tab,
      ),
    );
    setComposerFocused(false);
  }, [activeTabId]);

  const onCloseDiff = useCallback(() => {
    setTabs((prev) =>
      prev.map((tab) =>
        tab.id === activeTabId
          ? {
              ...tab,
              diffOpen: false,
              diffFocused: false,
            }
          : tab,
      ),
    );
  }, [activeTabId]);

  const onFocusDiff = useCallback(() => {
    setTabs((prev) =>
      prev.map((tab) =>
        tab.id === activeTabId
          ? { ...tab, diffFocused: true }
          : tab,
      ),
    );
    setComposerFocused(false);
  }, [activeTabId]);

  const onReorderTabs = useCallback(
    (ids: string[], movedId?: string) => {
      setTabs((prev) => {
        if (movedId) {
          return applyGroupedReorder(prev, ids, movedId, projectOfTab) ?? prev;
        }
        return orderByIds(prev, ids);
      });
    },
    [projectOfTab],
  );

  const onJoinTab = useCallback(
    (draggedId: string, targetId: string) => {
      setTabs(
        (prev) =>
          joinTabOnto(prev, draggedId, targetId, undefined, projectOfTab)
            ?.tabs ?? prev,
      );
    },
    [projectOfTab],
  );

  const onJoinTabToGroup = useCallback(
    (tabId: string, groupId: string) => {
      setTabs((prev) => addTabToGroup(prev, tabId, groupId, projectOfTab));
    },
    [projectOfTab],
  );

  const onAddToNewGroup = useCallback((tabId: string) => {
    const groupId = newTabGroupId();
    setTabs((prev) => addTabsToNewGroup(prev, [tabId], groupId));
  }, []);

  const onAddToGroup = useCallback(
    (tabId: string, groupId: string) => {
      setTabs((prev) => addTabToGroup(prev, tabId, groupId, projectOfTab));
    },
    [projectOfTab],
  );

  const onRemoveFromGroup = useCallback((tabId: string) => {
    setTabs((prev) => removeTabFromGroup(prev, tabId));
  }, []);

  const onUngroup = useCallback((groupId: string) => {
    setTabs((prev) => ungroupTabs(prev, groupId));
  }, []);

  const onReorderFiles = useCallback((paneId: string, ids: string[]) => {
    setTabs((prev) =>
      prev.map((tab) => {
        const found = findSurfacePane(tab, paneId);
        if (!found) return tab;
        return withSurfacePanes(
          tab,
          found.kind,
          surfacePanes(tab, found.kind).map((pane) =>
            pane.id === paneId
              ? { ...pane, files: orderByIds(pane.files, ids) }
              : pane,
          ),
        );
      }),
    );
  }, []);

  const onMovePane = useCallback(
    (fromId: string, toId: string, edge: PaneEdge) => {
      setTabs((prev) =>
        prev.map((tab) => {
          return leafIds(tab.layout).includes(fromId)
            ? {
                ...tab,
                layout: movePane(tab.layout, fromId, toId, edge),
                focusedId: fromId,
              }
            : tab;
        }),
      );
    },
    [],
  );

  const focusOpenSession = useCallback((sessionId: string) => {
    const tab = tabsRef.current.find(
      (entry) =>
        leafIds(entry.layout).includes(sessionId),
    );
    if (!tab) return false;
    setActiveTabId(tab.id);
    setTabs((prev) =>
      prev.map((entry) =>
        entry.id === tab.id
          ? { ...entry, focusedId: sessionId }
          : entry,
      ),
    );
    setComposerFocused(true);
    return true;
  }, []);

  const replaceBlankPaneWithSession = useCallback((session: Session) => {
    const tab =
      tabsRef.current.find((entry) => entry.id === activeTabIdRef.current) ??
      tabsRef.current[0];
    if (!tab) return false;

    const paneId = isBlankSession(
      sessionsRef.current.find((entry) => entry.id === tab.focusedId),
    )
      ? tab.focusedId
      : leafIds(tab.layout).find((id) =>
          isBlankSession(sessionsRef.current.find((entry) => entry.id === id)),
        );
    if (!paneId || paneId === session.id) return false;

    lastPersisted.current.delete(paneId);
    {
      const blank = sessionsRef.current.find((entry) => entry.id === paneId);
      if (blank) void forgetHarnessSession(blank.harness, paneId);
    }
    setSessions((prev) => {
      const next = prev.filter((entry) => entry.id !== paneId);
      return next.some((entry) => entry.id === session.id)
        ? next
        : [...next, session];
    });
    setTabs((prev) =>
      prev.map((entry) =>
        entry.id === tab.id
          ? {
              ...entry,
              layout: replaceLeafId(entry.layout, paneId, session.id),
              focusedId: session.id,
            }
          : entry,
      ),
    );
    setActiveTabId(tab.id);
    setComposerFocused(true);
    return true;
  }, []);

  const onSelectHistorySession = useCallback(
    async (sessionId: string) => {
      if (focusOpenSession(sessionId)) return;
      const open = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      if (open) {
        if (replaceBlankPaneWithSession(open)) return;
        const tab = newTab(open.id);
        appendTab(tab, open.cwd);
        setActiveTabId(tab.id);
        setComposerFocused(true);
        return;
      }

      const restored = await getSession(sessionId).catch(() => null);
      if (!restored) {
        void refreshHistory(sidebarCwd);
        return;
      }
      if (restored.providerSessionId && isLiveHarness(restored.harness)) {
        bindHarnessSession(
          restored.harness,
          restored.id,
          restored.providerSessionId,
          restored.cwd,
        );
      }
      lastPersisted.current.set(restored.id, persistFingerprint(restored));
      if (replaceBlankPaneWithSession(restored)) return;
      const tab = newTab(restored.id);
      setSessions((prev) =>
        prev.some((session) => session.id === restored.id)
          ? prev
          : [...prev, restored],
      );
      appendTab(tab, restored.cwd);
      setActiveTabId(tab.id);
      setComposerFocused(true);
    },
    [appendTab, focusOpenSession, refreshHistory, replaceBlankPaneWithSession, sidebarCwd],
  );

  const onRenameHistorySession = useCallback(
    async (sessionId: string, displayTitle: string) => {
      const trimmed = displayTitle.trim();
      if (!trimmed) return;

      const open = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      if (open) {
        const title = formatSessionTitle(open.harness, trimmed);
        const updated = { ...open, title };
        setSessions((prev) =>
          prev.map((session) => (session.id === sessionId ? updated : session)),
        );
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
        await upsertSession(updated).catch(() => undefined);
        lastPersisted.current.set(sessionId, persistFingerprint(updated));
      }
      void refreshHistory(sidebarCwd);
    },
    [persistSession, refreshHistory, sidebarCwd],
  );

  const onDeleteHistorySession = useCallback(
    async (sessionId: string) => {
      const open = sessionsRef.current.find(
        (session) => session.id === sessionId,
      );
      const summary =
        history.find((entry) => entry.id === sessionId) ?? open ?? null;
      const label = summary
        ? sessionDisplayTitle(summary.title, summary.harness)
        : "this session";

      if (!window.confirm(`Delete “${label}”?`)) return;

      if (open?.busy) {
        turnGen.current.set(
          sessionId,
          (turnGen.current.get(sessionId) ?? 0) + 1,
        );
        void cancelHarnessTurn(open.harness, sessionId);
      }

      const harness = open?.harness ?? summary?.harness ?? "cursor";
      void forgetHarnessSession(harness, sessionId);
      lastPersisted.current.delete(sessionId);
      await deleteSession(sessionId).catch(() => undefined);

      const affectedTabs = tabsRef.current.filter(
        (tab) =>
          leafIds(tab.layout).includes(sessionId),
      );

      if (affectedTabs.length === 0) {
        setSessions((prev) =>
          prev.filter((session) => session.id !== sessionId),
        );
        void refreshHistory(sidebarCwd);
        return;
      }

      let nextTabs = [...tabsRef.current];
      let nextSessions = sessionsRef.current.filter(
        (session) => session.id !== sessionId,
      );
      let nextActiveTabId = activeTabIdRef.current;

      for (const tab of affectedTabs) {
        const tabIndex = nextTabs.findIndex((entry) => entry.id === tab.id);
        const nextTab = closeLeaf(tab, sessionId);
        if (nextTab) {
          nextTabs[tabIndex] = nextTab;
          continue;
        }

        if (nextTabs.length > 1) {
          nextTabs = nextTabs.filter((entry) => entry.id !== tab.id);
          if (tab.id === nextActiveTabId) {
            nextActiveTabId =
              (nextTabs[Math.max(0, tabIndex - 1)] ?? nextTabs[0])?.id ??
              nextActiveTabId;
          }
          continue;
        }

        const replacement = newSession(
          open?.harness ?? harness,
          open?.cwd ?? summary?.cwd ?? sidebarCwd,
          open?.model ?? summary?.model,
          open?.runtimeMode ?? summary?.runtimeMode,
          open?.modelSettings,
        );
        nextSessions = [
          ...nextSessions.filter((session) => session.id !== sessionId),
          replacement,
        ];
        nextTabs[0] = {
          ...(tab as Extract<WorkspaceTab, { kind: "session" }>),
          layout: leaf(replacement.id),
          focusedId: replacement.id,
          editorPanes: [],
          terminalPanes: [],
          diffOpen: false,
          diffFocused: false,
        };
        nextActiveTabId = tab.id;
      }

      setSessions(nextSessions);
      setTabs(nextTabs);
      if (nextActiveTabId !== activeTabIdRef.current) {
        setActiveTabId(nextActiveTabId);
      }
      setComposerFocused(
        nextSessions.some((session) => {
          const tab = nextTabs.find((entry) => entry.id === nextActiveTabId);
          return !!tab && session.id === tab.focusedId;
        }),
      );
      void refreshHistory(sidebarCwd);
    },
    [history, refreshHistory, sidebarCwd],
  );

  const onFocusDir = useCallback(
    (dir: FocusDir) => {
      if (!activeTab) return;
      const next = neighborLeafId(activeTab.layout, activeTab.focusedId, dir);
      if (next) onFocusPane(next);
    },
    [activeTab, onFocusPane],
  );

  const onRatio = useCallback(
    (tabId: string, splitId: string, index: number, ratio: number) => {
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tabId
            ? { ...t, layout: setSplitRatio(t.layout, splitId, index, ratio) }
            : t,
        ),
      );
    },
    [],
  );

  const onCwdChange = useCallback((sessionId: string, cwd: string) => {
    const normalized = normalizeProjectPath(cwd);
    const current = sessionsRef.current.find((s) => s.id === sessionId);
    const previous = current?.cwd;
    // Threads stay bound to their project. Switching from the composer opens a
    // new tab instead of retargeting the conversation.
    if (
      current &&
      previous &&
      looksLikeProject(previous) &&
      !sameProjectPath(previous, normalized) &&
      !isBlankSession(current)
    ) {
      setProjectCwd(normalized);
      setRecents(rememberProject(normalized));
      const session = newSession(
        current.harness,
        normalized,
        current.model,
        current.runtimeMode,
        current.modelSettings,
      );
      const tab = newTab(session.id);
      setSessions((prev) => [...prev, session]);
      appendTab(tab, normalized);
      setActiveTabId(tab.id);
      setComposerFocused(true);
      return;
    }
    if (previous && !sameProjectPath(previous, normalized) && previous !== "~") {
      void keepSessionChanges(sessionId, previous).catch(() => undefined);
    }
    setProjectCwd(normalized);
    setRecents(rememberProject(normalized));
    setSessions((prev) =>
      prev.map((s) => (s.id === sessionId ? { ...s, cwd: normalized } : s)),
    );
    // The session's project just moved in place; a group only holds tabs that
    // share one project, so drop this tab out if it no longer matches.
    setTabs((prev) => {
      const tab = prev.find((t) => leafIds(t.layout).includes(sessionId));
      // The tab's visible project follows its focused pane; a background
      // pane changing project doesn't change what the group check should see.
      if (!tab?.groupId || tab.focusedId !== sessionId) return prev;
      const newProject = projectName(normalized);
      const othersProject = tabGroupProject(
        prev.filter((t) => t.id !== tab.id),
        tab.groupId,
        projectOfTab,
      );
      if (othersProject && newProject && othersProject !== newProject) {
        return removeTabFromGroup(prev, tab.id);
      }
      return prev;
    });
    notifyReviewChanged(sessionId);
  }, [appendTab, projectOfTab]);

  const pickProject = useCallback(async () => {
    const path = await pickFolder();
    const target = active ?? sessionsRef.current[0];
    if (!path || !target) return;
    onCwdChange(target.id, path);
  }, [active, onCwdChange]);

  const onFileMoved = useCallback((from: string, to: string) => {
    invalidateProjectFiles();
    setTabs((prev) =>
      prev.map((tab) => {
        return {
          ...tab,
          editorPanes: tab.editorPanes.map((pane) => ({
            ...pane,
            files: pane.files.map((file) =>
              file.plan || file.terminal
                ? file
                : {
                    ...file,
                    path: rebasePath(file.path, from, to),
                  },
            ),
          })),
        };
      }),
    );
  }, []);

  const onFileDeleted = useCallback((path: string) => {
    invalidateProjectFiles();
    const dropped = new Set<string>();
    for (const tab of tabsRef.current) {
      for (const pane of tab.editorPanes) {
        for (const file of pane.files) {
          if (isEqualOrInside(file.path, path)) dropped.add(file.id);
        }
      }
    }
    setTabs((prev) =>
      prev.map((tab) =>
        dropOpenFiles(tab, (filePath) => isEqualOrInside(filePath, path)),
      ),
    );
    if (dropped.size === 0) return;
    setDirtyFiles((prev) => {
      const next = new Set(prev);
      for (const id of dropped) next.delete(id);
      return next;
    });
  }, []);

  const onOpenFile = useCallback<OpenFileFn>(
    (path, navigation) => {
      void (async () => {
        const resolved =
          (await resolveOpenablePath(sidebarCwd, path)) ?? path;
        rememberOpenedFile(sidebarCwd, resolved);
        const tab = tabsRef.current.find((entry) => entry.id === activeTabId);
        if (!tab) return;
        const file = newFileTab(resolved, sidebarCwd);
        setTabs((prev) =>
          prev.map((entry) =>
            entry.id === tab.id ? openEditorTab(entry, file) : entry,
          ),
        );
        if (navigation) {
          editorNavigationToken.current += 1;
          setEditorNavigation({
            path: resolved,
            ...navigation,
            token: editorNavigationToken.current,
          });
        }
        setComposerFocused(false);
      })();
    },
    [activeTabId, sidebarCwd],
  );

  const onOpenPlan = useCallback(
    (sessionId: string, blockId: string) => {
      const tab = tabsRef.current.find((entry) => entry.id === activeTabId);
      const session = sessionsRef.current.find(
        (entry) => entry.id === sessionId,
      );
      const block = session?.blocks.find((entry) => entry.id === blockId);
      if (!tab || !session || !block) return;
      const file = newPlanTab(
        session.id,
        block.id,
        planTitle(block.text),
        session.cwd,
      );
      setTabs((prev) =>
        prev.map((entry) =>
          entry.id === tab.id ? openEditorTab(entry, file) : entry,
        ),
      );
      setComposerFocused(false);
    },
    [activeTabId],
  );

  const onFileDirtyChange = useCallback((fileId: string, dirty: boolean) => {
    setDirtyFiles((prev) => {
      if (prev.has(fileId) === dirty) return prev;
      const next = new Set(prev);
      if (dirty) next.add(fileId);
      else next.delete(fileId);
      return next;
    });
  }, []);

  /** The editor reports 0 as it unmounts, so closed tabs drop out on their own. */
  const onFileErrorCountChange = useCallback(
    (fileId: string, count: number) => {
      setFileErrorCounts((prev) => {
        if ((prev.get(fileId) ?? 0) === count) return prev;
        const next = new Map(prev);
        if (count > 0) next.set(fileId, count);
        else next.delete(fileId);
        return next;
      });
    },
    [],
  );

  const onSelectFileSurface = useCallback((paneId: string, fileId: string) => {
    setTabs((prev) =>
      prev.map((tab) => {
        const found = findSurfacePane(tab, paneId);
        if (!found) return tab;
        return withSurfacePanes(
          { ...tab, focusedId: paneId },
          found.kind,
          surfacePanes(tab, found.kind).map((pane) =>
            pane.id === paneId ? { ...pane, activeFileId: fileId } : pane,
          ),
        );
      }),
    );
    setComposerFocused(false);
  }, []);

  const onModelChange = useCallback(
    (sessionId: string, harness: HarnessId, model: string) => {
      const current = sessionsRef.current.find((s) => s.id === sessionId);
      if (current && current.harness !== harness) {
        void forgetHarnessSession(current.harness, sessionId);
      }
      const resolved = resolveModel(harness, model);
      if (current?.modelSettings) {
        saveLastModelSettings(current.modelSettings, "fill");
      }
      const modelSettings = preferredModelSettings(
        resolved,
        current?.modelSettings,
      );
      saveLastModelChoice(harness, resolved.id);
      setSessions((prev) =>
        prev.map((s) => {
          if (s.id !== sessionId) return s;
          return {
            ...s,
            harness,
            model: resolved.id,
            modelSettings,
            title: s.blocks.length === 0 ? HARNESS_LABEL[harness] : s.title,
            // The window belongs to the old model; keep the level and let the
            // next turn re-report the window rather than showing a stale one.
            ...(s.model === resolved.id
              ? {}
              : { context: dropContextWindow(s.context) }),
            ...(current?.harness === harness
              ? {}
              : { providerSessionId: undefined }),
          };
        }),
      );
    },
    [],
  );

  const onModelSettingsChange = useCallback(
    (sessionId: string, modelSettings: Record<string, string>) => {
      saveLastModelSettings(modelSettings);
      setSessions((prev) =>
        prev.map((s) => (s.id === sessionId ? { ...s, modelSettings } : s)),
      );
    },
    [],
  );

  const onRuntimeModeChange = useCallback(
    (sessionId: string, runtimeMode: RuntimeMode) => {
      setSessions((prev) =>
        prev.map((s) => (s.id === sessionId ? { ...s, runtimeMode } : s)),
      );
    },
    [],
  );

  const onSubmit = useCallback(
    (sessionId: string, text: string, attachments: Attachment[] = []) => {
      const current = sessionsRef.current.find((s) => s.id === sessionId);
      if (!current) return;
      if (!text.trim() && attachments.length === 0) return;

      if (current.busy) {
        if (!isLiveHarness(current.harness) || !canSteerHarness(current.harness)) {
          return;
        }
        const visible = displayAttachments(attachments);
        setSessions((prev) =>
          prev.map((s) =>
            s.id === sessionId ? appendSteerUser(s, text, visible) : s,
          ),
        );
        void (async () => {
          try {
            const prepared = await prepareAttachments(attachments);
            const prompt = await applySkillsToTurn(text, current.cwd);
            await steerHarnessTurn({
              harness: current.harness,
              sessionId,
              cwd: current.cwd,
              model: current.model,
              modelSettings: current.modelSettings,
              text: prompt,
              attachments: prepared,
            });
          } catch (error: unknown) {
            const message =
              error instanceof Error
                ? error.message
                : `${current.harness} could not steer the active turn`;
            enqueueHarnessEvent(sessionId, {
              type: "session.error",
              message,
            });
            flushHarnessEvents();
          }
        })();
        return;
      }

      const gen = (turnGen.current.get(sessionId) ?? 0) + 1;
      turnGen.current.set(sessionId, gen);
      const isFirstTurn = current.blocks.length === 0;
      const titleSeed = isFirstTurn
        ? titleFromPrompt(text, current.harness, attachments)
        : current.title;
      const visible = displayAttachments(attachments);
      const live = isLiveHarness(current.harness);

      setSessions((prev) =>
        prev.map((s) => {
          if (s.id !== sessionId) return s;
          const titled = isFirstTurn ? titleSeed : s.title;
          if (!live) {
            return {
              ...s,
              title: titled,
              busy: false,
              blocks: [
                ...s.blocks,
                {
                  id: crypto.randomUUID(),
                  role: "user",
                  text,
                  ...(visible.length > 0 ? { attachments: visible } : {}),
                },
                {
                  id: crypto.randomUUID(),
                  role: "system",
                  text: `${s.harness} is not connected yet — Claude Code, Cursor, Codex, and OpenCode are the live adapters.`,
                },
              ],
            };
          }
          return appendUser({ ...s, title: titled }, text, visible);
        }),
      );

      if (isFirstTurn && live) {
        void generateHarnessTitle(current.harness, {
          sessionId,
          cwd: current.cwd,
          message: text || attachments.map((file) => file.name).join(", "),
        })
          .then((title) => {
            if (!title) return;
            setSessions((prev) =>
              prev.map((s) => {
                if (s.id !== sessionId) return s;
                if (!canReplaceSessionTitle(s.title, s.harness, titleSeed)) {
                  return s;
                }
                return { ...s, title: formatSessionTitle(s.harness, title) };
              }),
            );
          })
          .catch(() => undefined);
      }

      if (!live) return;

      void (async () => {
        await beginSessionTurn(sessionId, current.cwd).catch(() => undefined);
        if (turnGen.current.get(sessionId) !== gen) return;
        try {
          const prepared = await prepareAttachments(attachments);
          const prompt = await applySkillsToTurn(text, current.cwd);
          await sendHarnessTurn({
            harness: current.harness,
            sessionId,
            cwd: current.cwd,
            model: current.model,
            modelSettings: current.modelSettings,
            runtimeMode: current.runtimeMode,
            text: prompt,
            attachments: prepared,
            onEvent: (event) => {
              if (turnGen.current.get(sessionId) !== gen) return;
              nudgeOpenEditors(event, current.cwd);
              trackSessionEdits(sessionId, current.cwd, event);
              enqueueHarnessEvent(sessionId, event);
            },
          });
        } catch (error: unknown) {
          if (turnGen.current.get(sessionId) !== gen) return;
          const message =
            error instanceof Error
              ? error.message
              : `${current.harness} adapter failed`;
          enqueueHarnessEvent(sessionId, {
            type: "session.error",
            message,
          });
        } finally {
          if (turnGen.current.get(sessionId) !== gen) return;
          flushHarnessEvents();
          setSessions((prev) =>
            prev.map((s) => (s.id === sessionId ? stopStreaming(s) : s)),
          );
          await syncSessionCheckpoint(sessionId, current.cwd).catch(
            () => undefined,
          );
          notifyReviewChanged(sessionId);
          nudgeWorkspace(current.cwd);
          nudgeWatchedFiles();
          window.setTimeout(() => nudgeWatchedFiles(), 150);
        }
      })();
    },
    [enqueueHarnessEvent, flushHarnessEvents],
  );

  const autoContinueKey = sessions
    .filter(
      (session) =>
        canAutoContinue(session) && isLiveHarness(session.harness),
    )
    .map((session) => session.id)
    .join("\n");

  useEffect(() => {
    if (!autoContinueKey) return;
    const ids = autoContinueKey.split("\n");
    // Delay past React StrictMode's dev remount so Continue is not claimed
    // against a discarded tree (sessionStorage also survives Vite reloads).
    const timer = window.setTimeout(() => {
      for (const id of ids) {
        const session = sessionsRef.current.find((entry) => entry.id === id);
        if (
          !session ||
          !canAutoContinue(session) ||
          !isLiveHarness(session.harness)
        ) {
          continue;
        }
        onSubmit(id, CONTINUE_PROMPT);
      }
    }, 0);
    return () => window.clearTimeout(timer);
  }, [autoContinueKey, onSubmit]);

  const onStop = useCallback((sessionId: string) => {
    turnGen.current.set(sessionId, (turnGen.current.get(sessionId) ?? 0) + 1);
    flushHarnessEvents();
    const session = sessionsRef.current.find((s) => s.id === sessionId);
    if (session) void cancelHarnessTurn(session.harness, sessionId);
    setSessions((prev) =>
      prev.map((s) => (s.id === sessionId ? stopStreaming(s) : s)),
    );
    if (session) {
      void syncSessionCheckpoint(sessionId, session.cwd)
        .catch(() => undefined)
        .then(() => notifyReviewChanged(sessionId));
      nudgeWorkspace(session.cwd);
      nudgeWatchedFiles();
      window.setTimeout(() => nudgeWatchedFiles(), 150);
    } else {
      notifyReviewChanged(sessionId);
    }
  }, [flushHarnessEvents]);

  const onApproval = useCallback(
    (sessionId: string, requestId: number, decision: ApprovalDecision) => {
      const session = sessionsRef.current.find((s) => s.id === sessionId);
      if (!session) return;
      respondHarnessApproval(session.harness, sessionId, requestId, decision);
    },
    [],
  );

  const onOpenApprovalSession = useCallback(
    (sessionId: string) => {
      if (!focusOpenSession(sessionId)) {
        void onSelectHistorySession(sessionId);
      }
    },
    [focusOpenSession, onSelectHistorySession],
  );

  const nextTitleTabs: TitleTab[] = tabs.map((tab) =>
    toTitleTab(tab, sessions, dirtyFiles),
  );
  tabProjectsRef.current = new Map(
    nextTitleTabs.map((tab) => [tab.id, tab.project]),
  );
  const titleTabsRef = useRef(nextTitleTabs);
  if (!titleTabsEqual(titleTabsRef.current, nextTitleTabs)) {
    titleTabsRef.current = nextTitleTabs;
  }
  const titleTabs = titleTabsRef.current;

  const sidebarHistory = useMemo(
    () => historyWithLiveSessions(history, sessions, sidebarCwd),
    [history, sessions, sidebarCwd],
  );

  const onToggleSidebar = useCallback(() => {
    setSidebarOpen((open) => {
      const next = !open;
      saveSidebarOpen(next);
      return next;
    });
  }, []);

  const onGoToFile = useCallback(() => {
    setFilePickerOpen(true);
  }, []);

  const onFindInProject = useCallback(() => {
    setSidebarOpen(true);
    saveSidebarOpen(true);
    setSidebarTab("files");
    setFilesSearchOpen(true);
    setSearchFocusToken((token) => token + 1);
  }, []);

  const openFilePaths = useMemo(() => {
    const paths: string[] = [];
    const seen = new Set<string>();
    for (const tab of tabs) {
      for (const pane of tab.editorPanes) {
        for (const file of pane.files) {
          if (file.plan || file.terminal || seen.has(file.path)) continue;
          seen.add(file.path);
          paths.push(file.path);
        }
      }
    }
    return paths;
  }, [tabs]);

  useEffect(() => {
    void invoke("set_traffic_lights_visible", { visible: true }).catch(
      () => {},
    );
  }, []);

  const actions = useRef({
    onNew,
    onClosePane,
    onNext,
    onPrev,
    onVisitBack,
    onVisitForward,
    onActivate,
    onSplit,
    onFocusDir,
    onToggleSidebar,
    onGoToFile,
    onFindInProject,
    pickProject,
    onNewTerminal,
    onNewTerminalTab,
  });
  actions.current = {
    onNew,
    onClosePane,
    onNext,
    onPrev,
    onVisitBack,
    onVisitForward,
    onActivate,
    onSplit,
    onFocusDir,
    onToggleSidebar,
    onGoToFile,
    onFindInProject,
    pickProject,
    onNewTerminal,
    onNewTerminalTab,
  };

  const debounce = useRef({ name: "", at: 0 });
  const run = useCallback((name: string, fn: () => void) => {
    const now = performance.now();
    if (name === debounce.current.name && now - debounce.current.at < 80)
      return;
    debounce.current = { name, at: now };
    fn();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const cmd = tabCommand(e);
      if (cmd) {
        const target = e.target instanceof Element ? e.target : null;
        if (
          target?.closest(".monocode-terminal") &&
          e.ctrlKey &&
          !e.metaKey &&
          (cmd === "back" ||
            cmd === "forward" ||
            /Mac|iPhone|iPad/.test(navigator.platform))
        ) {
          return;
        }
        if (
          (cmd === "split-right" || cmd === "split-down") &&
          target?.closest(".cm-editor")
        ) {
          return;
        }
        const inPicker =
          target &&
          target.closest(
            "[data-model-picker], [data-file-picker], [data-branch-picker], [data-skill-picker]",
          );
        if (inPicker && typeof cmd === "object" && "activate" in cmd) {
          return;
        }
        e.preventDefault();
        e.stopPropagation();
        const a = actions.current;
        if (cmd === "new") run("new", a.onNew);
        else if (cmd === "close") run("close", a.onClosePane);
        else if (cmd === "next") run("next", a.onNext);
        else if (cmd === "prev") run("prev", a.onPrev);
        else if (cmd === "back") run("back", a.onVisitBack);
        else if (cmd === "forward") run("forward", a.onVisitForward);
        else if (cmd === "split-right")
          run("split-right", () => a.onSplit("right"));
        else if (cmd === "split-down")
          run("split-down", () => a.onSplit("down"));
        else if (cmd === "new-terminal") run("new-terminal", a.onNewTerminal);
        else if (cmd === "new-terminal-tab")
          run("new-terminal-tab", a.onNewTerminalTab);
        else if ("focus" in cmd)
          run(`focus-${cmd.focus}`, () => a.onFocusDir(cmd.focus));
        else run(`activate-${cmd.activate}`, () => a.onActivate(cmd.activate));
        return;
      }
      if (handleEditorFindKey(e)) {
        e.stopPropagation();
        return;
      }
      const mod = e.metaKey || e.ctrlKey;
      if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "b") {
        e.preventDefault();
        e.stopPropagation();
        run("toggle_sidebar", actions.current.onToggleSidebar);
        return;
      }
      if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "p") {
        e.preventDefault();
        e.stopPropagation();
        run("go_to_file", actions.current.onGoToFile);
        return;
      }
      if (mod && e.shiftKey && !e.altKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        e.stopPropagation();
        run("find_in_project", actions.current.onFindInProject);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [run]);

  useEffect(() => {
    const unlisten: Array<Promise<() => void>> = [
      listen("new_tab", () => run("new", actions.current.onNew)),
      listen("close_tab", () => run("close", actions.current.onClosePane)),
      listen("next_tab", () => run("next", actions.current.onNext)),
      listen("prev_tab", () => run("prev", actions.current.onPrev)),
      listen("back_tab", () => run("back", actions.current.onVisitBack)),
      listen("forward_tab", () => run("forward", actions.current.onVisitForward)),
      listen("split_right", () =>
        run("split-right", () => actions.current.onSplit("right")),
      ),
      listen("split_down", () =>
        run("split-down", () => actions.current.onSplit("down")),
      ),
      listen("new_terminal", () =>
        run("new-terminal", actions.current.onNewTerminal),
      ),
      listen("new_terminal_tab", () =>
        run("new-terminal-tab", actions.current.onNewTerminalTab),
      ),
      listen("focus_left", () =>
        run("focus-left", () => actions.current.onFocusDir("left")),
      ),
      listen("focus_right", () =>
        run("focus-right", () => actions.current.onFocusDir("right")),
      ),
      listen("focus_up", () =>
        run("focus-up", () => actions.current.onFocusDir("up")),
      ),
      listen("focus_down", () =>
        run("focus-down", () => actions.current.onFocusDir("down")),
      ),
      listen("toggle_sidebar", () =>
        run("toggle_sidebar", actions.current.onToggleSidebar),
      ),
      listen("open_project", () => {
        void actions.current.pickProject();
      }),
      listen("go_to_file", () => actions.current.onGoToFile()),
      listen("find_in_project", () => actions.current.onFindInProject()),
      listen("find", () => {
        openFindInActiveEditor();
      }),
    ];
    return () => {
      void Promise.all(unlisten).then((fns) => fns.forEach((fn) => fn()));
    };
  }, [run]);

  return (
    <div
      className={`flex h-full text-content ${
        IS_MAC ? "bg-background-base/40" : "bg-background-base"
      }`}
    >
      <Sidebar
        cwd={sidebarCwd}
        open={sidebarOpen}
        tab={sidebarTab}
        onTabChange={setSidebarTab}
        filesSearchOpen={filesSearchOpen}
        onFilesSearchOpenChange={setFilesSearchOpen}
        onOpenFilesSearch={onFindInProject}
        searchFocusToken={searchFocusToken}
        sessions={sidebarHistory}
        busySessionIds={busySessionIds}
        approvalSessionIds={approvalSessionIds}
        activeSessionId={active?.id}
        status={historyStatus}
        onSelectSession={onSelectHistorySession}
        onRenameSession={onRenameHistorySession}
        onDeleteSession={onDeleteHistorySession}
        onOpenFile={onOpenFile}
        onOpenTerminal={(cwd) => onOpenTerminal(cwd)}
        onFileMoved={onFileMoved}
        onFileDeleted={onFileDeleted}
        diffOpen={!!activeTab?.diffOpen}
        onToggleDiff={onToggleDiff}
        canGoBack={tabVisitNav.canBack}
        canGoForward={tabVisitNav.canForward}
        onGoBack={onVisitBack}
        onGoForward={onVisitForward}
      />

      <div className="body-glass flex min-h-0 min-w-0 flex-1 flex-col">
        {!IS_MAC ? (
          <MenuBar
            onNew={onNew}
            onNewTerminal={onNewTerminal}
            onGoToFile={onGoToFile}
            onToggleSidebar={onToggleSidebar}
            onToggleDiff={onToggleDiff}
            onCloseCurrentTab={activeTabId ? () => onCloseTab(activeTabId) : undefined}
            onPickProject={pickProject}
            onFindInProject={onFindInProject}
          />
        ) : null}
        <TitleBar
          tabs={titleTabs}
          activeId={activeTabId}
          cwd={sidebarCwd}
          sidebarOpen={sidebarOpen}
          diffOpen={!!activeTab?.diffOpen}
          onToggleSidebar={onToggleSidebar}
          onToggleDiff={onToggleDiff}
          onSelect={activateTab}
          canGoBack={tabVisitNav.canBack}
          canGoForward={tabVisitNav.canForward}
          onGoBack={onVisitBack}
          onGoForward={onVisitForward}
          onNew={onNew}
          onNewTerminal={onNewTerminal}
          onClose={onCloseTab}
          onReorder={onReorderTabs}
          onGoToFile={onGoToFile}
          onJoinTab={onJoinTab}
          onJoinTabToGroup={onJoinTabToGroup}
          onAddToNewGroup={onAddToNewGroup}
          onAddToGroup={onAddToGroup}
          onRemoveFromGroup={onRemoveFromGroup}
          onUngroup={onUngroup}
          onGroupNewTab={onGroupNewTab}
          onGroupClose={onGroupCloseTabs}
          onGroupMoveToNewWindow={onGroupMoveToNewWindow}
        />

        <main className="relative min-h-0 min-w-0 flex-1">
          {tabs.map((tab) => (
            <div
              key={tab.id}
              aria-hidden={tab.id !== activeTabId}
              className={
                tab.id === activeTabId
                  ? "absolute inset-0 flex h-full min-h-0 flex-row"
                  : "hidden"
              }
            >
              <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col">
                <PaneTree
                  visible={tab.id === activeTabId}
                  layout={tab.layout}
                  sessions={sessions}
                  editorPanes={[
                    ...tab.editorPanes,
                    ...(tab.terminalPanes ?? []),
                  ]}
                  dirtyFileIds={dirtyFiles}
                  fileErrorCounts={fileErrorCounts}
                  focusedId={
                    tab.id === activeTabId && !tab.diffFocused
                      ? tab.focusedId
                      : ""
                  }
                  composerFocused={composerFocused}
                  recents={recents}
                  onFocus={onFocusPane}
                  onClose={onClosePane}
                  onSelectFile={onSelectFileSurface}
                  onCloseFile={onCloseFile}
                  onReorderFiles={onReorderFiles}
                  onFileDirtyChange={onFileDirtyChange}
                  onFileErrorCountChange={onFileErrorCountChange}
                  onRatio={(splitId, index, ratio) =>
                    onRatio(tab.id, splitId, index, ratio)
                  }
                  onCwdChange={onCwdChange}
                  onModelChange={onModelChange}
                  onModelSettingsChange={onModelSettingsChange}
                  onRuntimeModeChange={onRuntimeModeChange}
                  onSubmit={onSubmit}
                  onStop={onStop}
                  onApproval={onApproval}
                  onOpenFile={onOpenFile}
                  editorNavigation={editorNavigation}
                  onOpenDiff={onOpenDiff}
                  onOpenPlan={onOpenPlan}
                  onMovePane={onMovePane}
                  onNewTerminal={onNewTerminalInSession}
                  onTerminalMetaChange={onTerminalMetaChange}
                />
              </div>
              {tab.diffOpen && tab.id === activeTabId ? (
                <DiffPane
                  cwd={sidebarCwd}
                  textHarness={pickTextHarness(active?.harness)}
                  selectedPath={selectedChangePath(tab)}
                  focused={tab.id === activeTabId && !!tab.diffFocused}
                  onFocus={onFocusDiff}
                  onClose={onCloseDiff}
                  onOpenFile={onOpenDiff}
                />
              ) : null}
            </div>
          ))}
        </main>
      </div>

      {filePickerOpen ? (
        <FilePicker
          open
          cwd={sidebarCwd}
          openPaths={openFilePaths}
          onOpenFile={onOpenFile}
          onClose={() => setFilePickerOpen(false)}
        />
      ) : null}

      <ApprovalToasts
        notices={hiddenApprovalToasts}
        onFocusSession={onOpenApprovalSession}
        onApproval={onApproval}
      />
    </div>
  );
}

function conversationTitle(session: Session): string {
  const title = sessionDisplayTitle(session.title, session.harness);
  return title === "New session" ? "" : title;
}

function lastUserBlockId(session: Session): string | undefined {
  for (let i = session.blocks.length - 1; i >= 0; i--) {
    if (session.blocks[i]?.role === "user") return session.blocks[i]?.id;
  }
  return undefined;
}

function isBlankSession(session: Session | undefined): boolean {
  if (!session || session.busy) return false;
  return !session.blocks.some((block) => block.role === "user");
}

function selectedChangePath(tab: WorkspaceTab): string | undefined {
  const file = focusedFileTab(tab);
  if (!file || isPlanTab(file) || isTerminalTab(file) || !file.review) return undefined;
  return displayPath(file.path, file.cwd);
}

function isBlankWorkspaceTab(tab: WorkspaceTab, sessions: Session[]): boolean {
  if (tab.editorPanes.some((pane) => pane.files.length > 0)) return false;
  if ((tab.terminalPanes ?? []).some((pane) => pane.files.length > 0))
    return false;
  const ids = leafIds(tab.layout);
  if (ids.length !== 1) return false;
  return isBlankSession(sessions.find((entry) => entry.id === ids[0]));
}

function toTitleTab(
  tab: WorkspaceTab,
  sessions: Session[],
  dirtyFiles: Set<string>,
): TitleTab {
  const paneIds = leafIds(tab.layout);
  const multiPane = paneIds.length > 1;
  const tabSessions = paneIds
    .map((id) => sessions.find((session) => session.id === id))
    .filter((session): session is Session => session != null);
  const sessionFocused = tabSessions.some(
    (session) => session.id === tab.focusedId,
  );
  const fileFocused =
    !sessionFocused &&
    (tab.editorPanes.some((pane) => pane.id === tab.focusedId) ||
      (tab.terminalPanes ?? []).some((pane) => pane.id === tab.focusedId));
  const focused =
    sessions.find((session) => session.id === tab.focusedId) ?? tabSessions[0];

  const seen = new Set<HarnessId>();
  const harnesses: HarnessId[] = [];
  const busySeen = new Set<HarnessId>();
  const busyHarnesses: HarnessId[] = [];
  const ordered = focused
    ? [focused, ...tabSessions.filter((session) => session.id !== focused.id)]
    : tabSessions;
  for (const session of ordered) {
    if (
      session.busy &&
      !hasPendingApproval(session.blocks) &&
      !busySeen.has(session.harness)
    ) {
      busySeen.add(session.harness);
      busyHarnesses.push(session.harness);
    }
    if (seen.has(session.harness)) continue;
    seen.add(session.harness);
    harnesses.push(session.harness);
  }

  const files: string[] = [];
  const seenKeys = new Set<string>();
  const pushFile = (file: FilePaneTab) => {
    const key = file.terminal
      ? `terminal:${file.id}`
      : file.plan
        ? `plan:${file.plan.blockId}`
        : file.path;
    if (seenKeys.has(key)) return;
    seenKeys.add(key);
    files.push(
      file.plan?.title?.trim() ||
        (file.terminal ? terminalTabLabel(file) : basename(file.path)),
    );
  };
  const focusedPane =
    tab.editorPanes.find((pane) => pane.id === tab.focusedId) ??
    (tab.terminalPanes ?? []).find((pane) => pane.id === tab.focusedId);
  const otherPanes = [
    ...tab.editorPanes.filter((pane) => pane.id !== focusedPane?.id),
    ...(tab.terminalPanes ?? []).filter((pane) => pane.id !== focusedPane?.id),
  ];
  const panes = focusedPane ? [focusedPane, ...otherPanes] : otherPanes;
  for (const pane of panes) {
    const active = pane.files.find((file) => file.id === pane.activeFileId);
    if (active) pushFile(active);
  }
  for (const pane of panes) {
    for (const file of pane.files) pushFile(file);
  }

  const more = tabSessions
    .filter((session) => session.id !== focused?.id)
    .map(conversationTitle)
    .filter(Boolean);

  const hasTerminal = (tab.terminalPanes ?? []).some((pane) =>
    pane.files.some(isTerminalTab),
  );
  const focusedFile = focusedFileTab(tab);

  return {
    id: tab.id,
    project: focused
      ? projectName(focused.cwd)
      : focusedFile
        ? projectName(focusedFile.cwd)
        : "~",
    title: focused ? conversationTitle(focused) : "",
    more,
    sessionCount: tabSessions.length,
    harnesses,
    busyHarnesses,
    files,
    multiPane,
    fileFocused,
    dirty: tab.editorPanes.some((pane) =>
      pane.files.some((file) => dirtyFiles.has(file.id)),
    ),
    terminal: hasTerminal && harnesses.length === 0,
    groupId: tab.groupId,
  };
}

function dropOpenFiles(
  tab: WorkspaceTab,
  shouldDrop: (path: string) => boolean,
): WorkspaceTab {
  let layout = tab.layout;
  let focusedId = tab.focusedId;
  const editorPanes: EditorPane[] = [];
  for (const pane of tab.editorPanes) {
    const files = pane.files.filter((file) => !shouldDrop(file.path));
    if (files.length === 0) {
      const sibling = siblingLeafId(layout, pane.id);
      const withoutPane = removePane(layout, pane.id);
      if (withoutPane) {
        layout = withoutPane;
        if (focusedId === pane.id)
          focusedId = sibling ?? firstLeafId(withoutPane);
      }
      continue;
    }
    editorPanes.push({
      ...pane,
      files,
      activeFileId: files.some((file) => file.id === pane.activeFileId)
        ? pane.activeFileId
        : files[0].id,
    });
  }
  return { ...tab, layout, focusedId, editorPanes };
}

function trackSessionEdits(
  sessionId: string,
  cwd: string,
  event: HarnessEvent,
) {
  if (event.type !== "tool.updated") return;
  const completed =
    event.status === "completed" || event.status === "success";
  if (!completed) return;
  const kind = event.kind?.trim().toLowerCase();
  if (kind === "execute" || event.preview?.kind === "shell") {
    void syncSessionCheckpoint(sessionId, cwd)
      .catch(() => undefined)
      .then(() => notifyReviewChanged(sessionId));
    return;
  }
  if (!isEditTool(event.kind, event.title, event.preview)) return;
  const path = event.preview?.path;
  if (path && cwd !== "~") {
    void captureSessionCheckpoint(sessionId, cwd, [path])
      .catch(() => undefined)
      .then(() => notifyReviewChanged(sessionId));
    return;
  }
  notifyReviewChanged(sessionId);
}

function nudgeWorkspace(cwd?: string) {
  invalidateProjectFiles(cwd);
  notifyDirsChanged();
}

function nudgeOpenEditors(event: HarnessEvent, cwd: string) {
  if (event.type !== "tool.updated") return;
  const completed =
    event.status === "completed" || event.status === "success";

  const kind = event.kind?.trim().toLowerCase();
  if (kind === "execute" || event.preview?.kind === "shell") {
    if (!completed) return;
    nudgeWatchedFiles();
    window.setTimeout(() => nudgeWatchedFiles(), 150);
    nudgeWorkspace(cwd);
    window.setTimeout(() => nudgeWorkspace(cwd), 150);
    return;
  }

  if (!isEditTool(event.kind, event.title, event.preview)) return;
  const raw = event.preview?.path;
  const resolved = raw ? (resolveWorkspacePath(raw, cwd) ?? raw) : undefined;
  if (resolved) {
    nudgeWatchedFiles([resolved]);
  } else if (completed) {
    nudgeWatchedFiles();
  }
  if (completed) {
    window.setTimeout(() => nudgeWatchedFiles(), 150);
    nudgeWorkspace(cwd);
  }
}

function sameSettings(
  a: Record<string, string> | undefined,
  b: Record<string, string> | undefined,
): boolean {
  const left = a ?? {};
  const right = b ?? {};
  const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
  for (const key of keys) {
    if (left[key] !== right[key]) return false;
  }
  return true;
}
