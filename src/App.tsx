import { invoke } from "@tauri-apps/api/core";
import { orchestrator } from "./lib/orchestration";
import { DEFAULT_PROVIDER_ACCOUNT_ID } from "./lib/providerAccounts";
import type { RateLimitProvider } from "./lib/rateLimits";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { Sidebar } from "./chrome/Sidebar";
import { ApprovalToasts } from "./chrome/ApprovalToasts";
import { WhatsNewDialog } from "./chrome/WhatsNewDialog";
import { ProviderSignInDialog } from "./chrome/ProviderSignInDialog";
import { TitleBar } from "./chrome/TitleBar";
import { MenuBar } from "./chrome/MenuBar";
import { FilePicker } from "./chrome/FilePicker";
import { UsageFooter } from "./chrome/UsageFooter";
import {
  loadProjectRailOpen,
  loadSidebarTabOrder,
  type SidebarTabId,
} from "./lib/appearance";
import { HAS_NATIVE_GLASS, IS_MAC } from "./lib/platform";
import {
  applyUiScale,
  loadUiScale,
  saveUiScale,
  UI_SCALE_DEFAULT,
  zoomInUiScale,
  zoomOutUiScale,
} from "./lib/uiScale";
import {
  isFilesystemTab,
  leafIds,
  newTab,
  type WorkspaceTab,
} from "./lib/layout";
import {
  applyDockGridStyle,
  findProjectTerminal,
} from "./lib/projectTerminal";
import { type WindowTransferPayload } from "./lib/windowTransfer";
import { supportsHarnessLogin } from "./lib/harness/authSupport";
import { type EditorNavigationTarget, } from "./lib/search";
import {
  lastProjectPath,
  loadRecents,
  looksLikeProject,
  normalizeProjectPath,
  rememberProject,
  sameProjectPath,
} from "./lib/recents";
import {
  newDefaultSession,
  newSession,
  type HarnessId,
  type Session,
} from "./lib/session";

import {
  type SessionSummary,
} from "./lib/sessionStore";
import { ReminderNotices } from "./chrome/ReminderNotices";

import { PaneTree } from "./surfaces/PaneTree";
import { SessionPane } from "./surfaces/SessionPane";
import { SessionSurface } from "./surfaces/SessionSurface";
import { ProjectTerminalDock } from "./surfaces/ProjectTerminalDock";
import { SearchView } from "./surfaces/SearchView";
import { SettingsView, type SettingsAnchor } from "./surfaces/SettingsView";
import { InboxView, LinkedWorkItemPanel } from "./surfaces/InboxView";
import type { InboxSessionPortal } from "./surfaces/InboxDiscussionPanel";
import { NotesView } from "./surfaces/NotesView";
import type { LinkedSessionUpdate } from "./lib/linkedSessionUpdates";
import {
  loadLiveAgentsEnabled,
  loadNotesEnabled,
  loadSettingsSection,
  subscribeLiveAgentsEnabled,
  subscribeNotesEnabled,
  type SettingsSectionId,
} from "./lib/settings";
import type { InstalledUpdate } from "./lib/updateNotice";
import {
  type ResumedWorkspace,
} from "./lib/appLifecycle";

import { useProjectTerminals } from "./app/useProjectTerminals";
import {
  useAppViews,
  type LinkedWorkItemPanelState,
} from "./app/useAppViews";
import { useWorkspaceTabs } from "./app/useWorkspaceTabs";
import { useSessions } from "./app/useSessions";
import { useHistory } from "./app/useHistory";
import { useProjects } from "./app/useProjects";
import { useComposer } from "./app/useComposer";
import { useTurnActions } from "./app/useTurnActions";
import { useSessionBootstrap } from "./app/useSessionBootstrap";
import { registerBuiltinHarnesses } from "./lib/harness";
import { insertTabBesideActive } from "./lib/tabGroups";
import { projectName } from "./lib/paths";
import {
  providerSignInRequestKey,
  selectedChangeKind,
  selectedChangePath,
  selectedCommitSha,
} from "./app/tabHelpers";
import {
  OrchestrationActions,
  OrchestrationWorkers,
} from "./chrome/OrchestrationActions";
import { pickTextHarness } from "./lib/harness/textHarness";
import { latestTurnNeedsHarnessLogin } from "./lib/harness/authSupport";
import { useAppShortcuts } from "./app/useAppShortcuts";
import { useSessionSync } from "./app/useSessionSync";
import { useOrchestration } from "./app/useOrchestration";

// Register capabilities before composer hooks choose their discovery strategy.
registerBuiltinHarnesses();

export default function App({
  windowTransfer = null,
  resumed = null,
  installedUpdate = null,
  history: bootHistory = [],
  historyCwd: bootHistoryCwd = null,
}: {
  windowTransfer?: WindowTransferPayload | null;
  resumed?: ResumedWorkspace | null;
  installedUpdate?: InstalledUpdate | null;
  history?: SessionSummary[];
  historyCwd?: string | null;
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
    const session = newDefaultSession(cwd);
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
  const activeTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0];
  const active =
    sessions.find((session) => session.id === activeTab?.focusedId) ??
    sessions.find(
      (session) => activeTab && leafIds(activeTab.layout).includes(session.id),
    );
  const sessionDefaults = active ?? sessions[0];
  const [composerFocused, setComposerFocused] = useState(() => {
    if (windowTransfer) return true;
    if (!resumed) return false;
    const tab =
      resumed.tabs.find((entry) => entry.id === resumed.activeTabId) ??
      resumed.tabs[0];
    return (
      !!tab && resumed.sessions.some((session) => session.id === tab.focusedId)
    );
  });
  const [composerFocusToken, setComposerFocusToken] = useState(0);
  /** Tab id -> project name, kept in sync with the rendered title tabs. */
  const tabProjectsRef = useRef(new Map<string, string>());
  const projectOfTab = useCallback(
    (id: string) => tabProjectsRef.current.get(id),
    [],
  );
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

const onSelectProviderAccount = useCallback(
    (provider: RateLimitProvider, accountId: string) => {
      if (!active || active.harness !== provider) return;
      const currentId = active.providerAccountId ?? DEFAULT_PROVIDER_ACCOUNT_ID;
      if (currentId === accountId) return;

      if (active.blocks.length === 0 && !active.busy) {
        setSessions((current) =>
          current.map((session) =>
            session.id === active.id
              ? { ...session, providerAccountId: accountId }
              : session,
          ),
        );
        return;
      }

      // Provider thread ids are account-owned. Keep the current conversation
      // pinned to its account and open a clean one for the selected profile.
      const session = {
        ...newSession(
          active.harness,
          active.cwd,
          active.model,
          active.runtimeMode,
          active.modelSettings,
        ),
        providerAccountId: accountId,
      };
      const tab = newTab(session.id);
      setSessions((current) => [...current, session]);
      appendTab(tab, active.cwd);
      setActiveTabId(tab.id);
      setComposerFocused(true);
    },
    [active, appendTab],
  );

  const [projectRailOpen, setProjectRailOpen] = useState(loadProjectRailOpen);
  const tabCloseScope = "project" as const;
  const [sidebarTab, setSidebarTab] = useState<SidebarTabId>(
    () => loadSidebarTabOrder()[0] ?? "sessions",
  );
  const [filesSearchOpen, setFilesSearchOpen] = useState(false);
  const [searchFocusToken, setSearchFocusToken] = useState(0);
  const [searchViewOpen, setSearchViewOpen] = useState(false);
  const [searchViewFocusToken, setSearchViewFocusToken] = useState(0);
  const [inboxViewOpen, setInboxViewOpen] = useState(false);
  const [linkedWorkItemPanels, setLinkedWorkItemPanels] = useState<
    ReadonlyMap<string, LinkedWorkItemPanelState>
  >(() => new Map());
  const [inboxAskPortal, setInboxAskPortal] =
    useState<InboxSessionPortal | null>(null);
  const openingInboxSessions = useRef(new Map<string, Promise<string>>());
  const [notesViewOpen, setNotesViewOpen] = useState(false);
  const orchestrationRuns = useSyncExternalStore(
    orchestrator.subscribe,
    orchestrator.snapshot,
    orchestrator.snapshot,
  );
  const notesEnabled = useSyncExternalStore(
    subscribeNotesEnabled,
    loadNotesEnabled,
    () => true,
  );
  const liveAgentsEnabled = useSyncExternalStore(
    subscribeLiveAgentsEnabled,
    loadLiveAgentsEnabled,
    () => true,
  );
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [updateNotice, setUpdateNotice] = useState(installedUpdate);
  const [whatsNewVersion, setWhatsNewVersion] = useState<string | null>(null);
  const [providerSignInRequest, setProviderSignInRequest] = useState<{
    key: string;
    sessionId: string;
    harness: HarnessId;
  } | null>(null);
  const seenProviderSignInRequestsRef = useRef<Set<string> | null>(null);
  const seenProviderSignInRequests =
    seenProviderSignInRequestsRef.current ??
    (seenProviderSignInRequestsRef.current = new Set(
      sessions.flatMap((session) => {
        if (
          !supportsHarnessLogin(session.harness) ||
          !latestTurnNeedsHarnessLogin(session.blocks)
        ) {
          return [];
        }
        return [providerSignInRequestKey(session)];
      }),
    ));
  const [settingsSection, setSettingsSection] =
    useState<SettingsSectionId>(loadSettingsSection);
  const [settingsAnchor, setSettingsAnchor] = useState<SettingsAnchor | null>(
    null,
  );
  const [notificationProjectPath, setNotificationProjectPath] = useState<string | null>(null);
  const [notificationSettingsRequest, setNotificationSettingsRequest] = useState(0);
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
  const [history, setHistory] = useState<SessionSummary[]>(() => bootHistory);
  const [storedLinkedSessions, setStoredLinkedSessions] = useState<
    SessionSummary[]
  >(() => bootHistory.filter((session) => session.linkedWorkItem));
  /**
   * Projects whose rows are already in `history`. This has to be state, not a
   * ref: `sidebarCwd` is derived during render, so the frame that first shows
   * a new project must already know the listing has not arrived yet.
   */
  const [loadedProjects, setLoadedProjects] = useState<ReadonlySet<string>>(
    () =>
      bootHistoryCwd
        ? new Set([normalizeProjectPath(bootHistoryCwd)])
        : new Set(),
  );
  const loadedProjectsRef = useRef(loadedProjects);
  loadedProjectsRef.current = loadedProjects;
  /** Project whose listing failed, so the error cannot leak to another one. */
  const [historyErrorCwd, setHistoryErrorCwd] = useState<string | null>(null);

  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const linkedSessionUpdatesRef = useRef<
    ReadonlyMap<string, LinkedSessionUpdate>
  >(new Map());
  const linkedWorkItemActivityFetches = useRef(new Map<string, number>());
  const queueDispatchingRef = useRef(new Set<string>());
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const dirtyFilesRef = useRef(dirtyFiles);
  dirtyFilesRef.current = dirtyFiles;
  const projectCwdRef = useRef(projectCwd);
  projectCwdRef.current = projectCwd;
  const activeTabIdRef = useRef(activeTabId);
  activeTabIdRef.current = activeTabId;
  const searchViewOpenRef = useRef(searchViewOpen);
  searchViewOpenRef.current = searchViewOpen;
  const inboxViewOpenRef = useRef(inboxViewOpen);
  inboxViewOpenRef.current = inboxViewOpen;
  const notesViewOpenRef = useRef(notesViewOpen);
  notesViewOpenRef.current = notesViewOpen;
  const settingsOpenRef = useRef(settingsOpen);
  settingsOpenRef.current = settingsOpen;
  const sessionNavigationIdsRef = useRef<readonly string[]>([]);
  const filePickerOpenRef = useRef(filePickerOpen);
  filePickerOpenRef.current = filePickerOpen;
  const whatsNewVersionRef = useRef(whatsNewVersion);
  whatsNewVersionRef.current = whatsNewVersion;

  const lastPersisted = useRef(new Map<string, string>());

  const {
    projectTerminals,
    setProjectTerminals,
    projectTerminalFocused,
    setProjectTerminalFocused,
    projectTerminalsRef,
    projectTerminalFocusedRef,
    currentProjectDock,
    dockVisible,
    runningTerminals,
    runningTerminalOpen,
    onSplit,
    focusProjectTerminal,
    onOpenTerminal,
    onNewTerminal,
    onShowProjectTerminal,
    onNewTerminalInSession,
    onToggleProjectTerminal,
    onHideProjectTerminal,
    onProjectTerminalSide,
    onProjectTerminalSize,
    onSelectProjectTerminal,
    onReorderProjectTerminals,
    onCloseProjectTerminal,
    onCloseOtherProjectTerminals,
    onTerminalMetaChange,
    onToggleRunningTerminal,
    onNewTerminalTab,
  } = useProjectTerminals({
    initialTerminals:
      windowTransfer?.projectTerminals ?? resumed?.projectTerminals ?? [],
    projectCwd,
    projectCwdRef,
    activeTab,
    active,
    sessionDefaults,
    sessionsRef,
    tabsRef,
    activeTabIdRef,
    tabs,
    appendTab,
    lastPersisted,
    setSessions,
    setTabs,
    setActiveTabId,
    setComposerFocused,
  });

  const {
    readProjectReturnMemory,
    tabVisitRef,
    tabVisitFromHistoryRef,
    turnGen,
    pendingPersist,
    removingSessionIds,
    loadedSessionCache,
    sessionLoads,
    sessionLoadEpochs,
    openingSessionIds,
    activeSessionPrefetch,
    flushHarnessEvents,
    stopSessionForRemoval,
    enqueueHarnessEvent,
    activeLinkedWorkItemPanel,
    sidebarCwd,
    sidebarCwdRef,
    historyFailed,
    historyPending,
    gitCwd,
    gitCwdRef,
    projectBranches,
    busySessionIds,
    usageProviders,
    usageSession,
    approvalSessionIds,
    activeSessionIdRef,
    tabVisitNav,
    reminderNoticesHeight,
    setReminderNoticesHeight,
    unseenFinishedIds,
    liveAgents,
    hiddenApprovalToasts,
    refreshHistory,
    persistSession,
    activateTab,
    commitTabVisit,
  } = useSessionSync({
    sessions,
    tabs,
    activeTabId,
    activeTab,
    active,
    projectCwd,
    history,
    recents,
    windowTransfer,
    resumed,
    notesEnabled,
    liveAgentsEnabled,
    orchestrationRuns,
    sessionDefaults,
    seenProviderSignInRequests,
    providerSignInRequest,
    inboxViewOpen,
    inboxAskPortal,
    composerFocused,
    setComposerFocusToken,
    projectTerminalFocusedRef,
    searchViewOpenRef,
    inboxViewOpenRef,
    notesViewOpenRef,
    settingsOpenRef,
    loadedProjectsRef,
    historyErrorCwd,
    loadedProjects,
    activeTabIdRef,
    projectCwdRef,
    sessionsRef,
    tabsRef,
    setSessions,
    setTabs,
    setActiveTabId,
    setComposerFocused,
    setHistory,
    setHistoryErrorCwd,
    setLinkedWorkItemPanels,
    linkedWorkItemPanels,
    setLoadedProjects,
    setNotesViewOpen,
    setProjectCwd,
    setProviderSignInRequest,
    setRecents,
    setStoredLinkedSessions,
    lastPersisted,
    projectTerminals,
    projectTerminalsRef,
  });
  const {
    onOpenWhatsNew,
    onNew,
    onStartInboxItem,
    onInboxCardDismiss,
    setLinkedWorkItemUpdateCard,
    onLinkedWorkItemUpdateCardDismiss,
    onNoteCardDismiss,
    onHandoffCardDismiss,
  } = useSessionBootstrap({
    active,
    projectCwd,
    sessionDefaults,
    sessionsRef,
    setSessions,
    setWhatsNewVersion,
    setActiveTabId,
    setComposerFocused,
    setSearchViewOpen,
    setInboxViewOpen,
    setNotesViewOpen,
    setSidebarTab,
    appendTab,
  });
  const {
    onCloseTab,
    onCloseTabs,
    onCloseOtherTabs,
    onCloseFile,
    onCloseOtherFiles,
    onCloseAllTabs,
    onClosePane,
    onCloseTitleTab,
    deckProjectTabs,
    onNext,
    onPrev,
    onVisitBack,
    onVisitForward,
    onActivate,
    onFocusPane,
    onOpenDiff,
    onOpenWorkingTreeDiff,
    onOpenAllChanges,
    onOpenCommit,
    onToggleChanges,
    onReorderTabs,
    onReorderFiles,
    onMovePane,
    onDetachPane,
    focusOpenSession,
    replaceBlankPaneWithSession,
  } = useWorkspaceTabs({
    tabs,
    sessions,
    activeTabId,
    activeTab,
    projectCwd,
    sidebarCwd,
    tabCloseScope,
    inboxAskPortal,
    tabsRef,
    sessionsRef,
    dirtyFilesRef,
    activeTabIdRef,
    tabVisitRef,
    tabVisitFromHistoryRef,
    gitCwdRef,
    sidebarCwdRef,
    lastPersisted,
    loadedSessionCache,
    setTabs,
    setSessions,
    setDirtyFiles,
    setActiveTabId,
    setComposerFocused,
    setProjectTerminalFocused,
    setSidebarTab,
    projectOfTab,
    activateTab,
    persistSession,
    refreshHistory,
    commitTabVisit,
  });
  const {
    invalidateLoadedSession,
    ensureOpenSession,
    onPrefetchHistorySession,
    revealLinkedSessionUpdate,
    onAskInboxItem,
    onRestartInboxAsk,
  } = useSessions({
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
  });

  const [inspectedWorkerId, setInspectedWorkerId] = useState<string | null>(
    null,
  );
  const {
    sessionReminders,
    onArchiveFocusedSession,
    onSelectHistorySession,
    dismissNoticesForContinuedSession,
    onPlaceSessionOnPane,
    onPlaceTabOnPane,
    onRenameHistorySession,
    onArchiveHistorySession,
    onPinHistorySession,
    onArchiveHistorySessions,
    onPinHistorySessions,
    onDeleteHistorySession,
    onDeleteHistorySessions,
  } = useHistory({
    sessions,
    history,
    activeTabId,
    projectCwd,
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
  });
  const {
    onPlaceSessionInFolder,
    onFileDirtyChange,
    onFileErrorCountChange,
    onSelectFileSurface,
    onFocusDir,
    onRatio,
    onCwdChange,
    onBranchChange,
    onSelectProject,
    pickProject,
    onRemoveProject,
    onRestoreProject,
    onFileMoved,
    onFileDeleted,
    onOpenFile,
    onOpenPlan,
  } = useProjects({
    activeTab,
    projectCwd,
    sidebarCwd,
    sessions,
    tabs,
    sessionsRef,
    tabsRef,
    activeTabIdRef,
    projectCwdRef,
    sidebarCwdRef,
    gitCwdRef,
    editorNavigationToken,
    turnGen,
    sessionLoads,
    pendingPersist,
    loadedSessionCache,
    lastPersisted,
    setSessions,
    setTabs,
    setDirtyFiles,
    setFileErrorCounts,
    setActiveTabId,
    setComposerFocused,
    setProjectCwd,
    setSearchViewOpen,
    setInboxViewOpen,
    setNotesViewOpen,
    setRecents,
    setHistory,
    setEditorNavigation,
    setProjectTerminalFocused,
    projectOfTab,
    activateTab,
    onFocusPane,
    appendTab,
    persistSession,
    refreshHistory,
    readProjectReturnMemory,
    focusOpenSession,
    invalidateLoadedSession,
    activeTabId,
    setProjectTerminals,
    onSelectHistorySession,
  });
  const { onModelChange, onModelSettingsChange, onRuntimeModeChange, onSubmit } =
    useComposer({
      sessionsRef,
      activeSessionIdRef,
      turnGen,
      removingSessionIds,
      setSessions,
      enqueueHarnessEvent,
      flushHarnessEvents,
      dismissNoticesForContinuedSession,
    });
  const {
    confirmingOrchestration,
    onUpdatePlan,
    onBuildPlan,
    onDeleteQueuedMessage,
    onQueuedMessageEditingChange,
    onEditQueuedMessage,
    onSteerQueuedMessage,
    onResumeQueue,
    onSecondOpinion,
    onHandoff,
    onCompactContext,
    onStop,
    onApproval,
    onQuestionReply,
    onQuestionInteraction,
    onOpenApprovalSession,
  } = useTurnActions({
    sessions,
    tabs,
    activeTabId,
    active,
    sessionsRef,
    tabsRef,
    activeTabIdRef,
    queueDispatchingRef,
    turnGen,
    orchestrationRuns,
    setSessions,
    setTabs,
    setActiveTabId,
    setComposerFocused,
    setInspectedWorkerId,
    setProjectTerminalFocused,
    enqueueHarnessEvent,
    flushHarnessEvents,
    onSubmit,
    focusOpenSession,
    onSelectHistorySession,
    ensureOpenSession,
    appendTab,
    projectTerminalFocusedRef,
  });
  const {
    orchestrationWorkers,
    orchestrationActions,
    onSelectLiveAgent,
    titleTabs,
    projectHistory,
    sidebarHistory,
    inboxUnseen,
    linkedSessionUpdateIds,
    inboxRelatedSessions,
    openProjectSessions,
  } = useOrchestration({
    tabs,
    sessions,
    dirtyFiles,
    history,
    recents,
    sidebarCwd,
    deckProjectTabs,
    unseenFinishedIds,
    orchestrationRuns,
    projectBranches,
    sessionsRef,
    linkedSessionUpdatesRef,
    tabProjectsRef,
    projectCwdRef,
    setTabs,
    setSessions,
    setActiveTabId,
    setComposerFocused,
    inspectedWorkerId,
    setInspectedWorkerId,
    setSearchViewOpen,
    setInboxViewOpen,
    setNotesViewOpen,
    setProjectTerminalFocused,
    focusOpenSession,
    onSelectHistorySession,
    ensureOpenSession,
    onOpenApprovalSession,
    onSubmit,
    confirmingOrchestration,
    storedLinkedSessions,
  });
  const {
    closeLinkedWorkItemPanel,
    onToggleSidebar,
    onToggleProjectRail,
    onGoToFile,
    onFindInProject,
    onOpenSearch,
    onLeaveSearch,
    onOpenInbox,
    onOpenLinkedWorkItem,
    onLeaveInbox,
    onOpenInboxSession,
    onOpenNotes,
    onLeaveNotes,
    openSettings,
    onOpenSettings,
    onOpenNotificationSettings,
    onOpenInboxIntegrations,
    onCloseSettings,
    onSelectSettingsSection,
    onOpenArchivedSession,
    onRailBack,
    onRailForward,
  } = useAppViews({
    history,
    sidebarCwd,
    sidebarTab,
    sessionsRef,
    searchViewOpen,
    inboxViewOpen,
    notesViewOpen,
    settingsOpen,
    dockVisible,
    setProjectRailOpen,
    setSidebarTab,
    setFilesSearchOpen,
    setSearchFocusToken,
    setSearchViewOpen,
    setSearchViewFocusToken,
    setInboxViewOpen,
    setLinkedWorkItemPanels,
    setNotesViewOpen,
    setSettingsOpen,
    setSettingsSection,
    setSettingsAnchor,
    setNotificationProjectPath,
    setNotificationSettingsRequest,
    setFilePickerOpen,
    setProjectTerminalFocused,
    onSelectHistorySession,
    onVisitBack,
    onVisitForward,
  });

  const openFilePaths = useMemo(() => {
    const paths: string[] = [];
    const seen = new Set<string>();
    for (const tab of tabs) {
      for (const pane of tab.editorPanes) {
        for (const file of pane.files) {
          if (!isFilesystemTab(file) || seen.has(file.path)) continue;
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

  const { onSessionNavigationOrder } = useAppShortcuts({
      tabs,
      tabsRef,
      sessionsRef,
      activeTabIdRef,
      projectCwdRef,
      filePickerOpenRef,
      searchViewOpenRef,
      inboxViewOpenRef,
      notesViewOpenRef,
      settingsOpenRef,
      whatsNewVersionRef,
      sessionNavigationIdsRef,
      setSidebarTab,
      onSelectHistorySession,
      onSelectProject,
      actions: {
        onNew,
        onArchiveFocusedSession,
        onCloseOtherTabs,
        onCloseAllTabs,
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
        onOpenSearch,
        onOpenInbox,
        onOpenNotes,
        pickProject,
        onNewTerminal,
        onNewTerminalTab,
        onToggleProjectTerminal,
        onOpenApprovalSession,
        openSettings,
      },
    });
  const dockGridRef = useRef<HTMLDivElement>(null);
  const dockDragSize = useRef<number | null>(null);
  const paintDockSize = useCallback((size: number) => {
    const dock = findProjectTerminal(
      projectTerminalsRef.current,
      projectCwdRef.current,
    );
    const el = dockGridRef.current;
    if (!dock || !el) return;
    dockDragSize.current = size;
    applyDockGridStyle(el, dock.side, size);
  }, []);
  const commitDockSize = useCallback(
    (size: number) => {
      dockDragSize.current = null;
      onProjectTerminalSize(size);
    },
    [onProjectTerminalSize],
  );
  useLayoutEffect(() => {
    if (dockDragSize.current != null) return;
    const el = dockGridRef.current;
    if (!el) return;
    applyDockGridStyle(
      el,
      dockVisible && currentProjectDock ? currentProjectDock.side : null,
      currentProjectDock?.size ?? 0,
    );
  }, [currentProjectDock, dockVisible]);

  const sessionPaneProps = {
    recents,
    hideProjectPicker: true,
    onFocus: onFocusPane,
    onClose: onClosePane,
    onCwdChange,
    onBranchChange,
    onModelChange,
    onModelSettingsChange,
    onRuntimeModeChange,
    onSubmit,
    onStop,
    onCompactContext,
    onPlaceSessionInFolder,
    onDeleteQueuedMessage,
    onEditQueuedMessage,
    onQueuedMessageEditingChange,
    onSteerQueuedMessage,
    onResumeQueue,
    onInboxCardDismiss,
    onLinkedWorkItemUpdateCardDismiss,
    onNoteCardDismiss,
    onHandoffCardDismiss,
    onOpenLinkedWorkItem,
    onArchiveSession: onArchiveHistorySession,
    onDeleteSession: onDeleteHistorySession,
    onApproval,
    onQuestionReply,
    onQuestionInteraction,
    onOpenFile,
    onOpenDiff,
    onOpenPlan,
    onBuildPlan,
    onSecondOpinion,
    onHandoff,
    onNewTerminal: onNewTerminalInSession,
  };

  return (
    <OrchestrationActions.Provider value={orchestrationActions}>
      <OrchestrationWorkers.Provider value={orchestrationWorkers}>
        <div
          className={`flex h-full text-content ${
            HAS_NATIVE_GLASS ? "bg-background-base/40" : "bg-background-base"
          }`}
        >
          <Sidebar
            cwd={sidebarCwd}
            gitCwd={gitCwd}
            open
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
            status={historyFailed ? "error" : "idle"}
            pending={historyPending}
            onSelectSession={onSelectHistorySession}
            onPrefetchSession={onPrefetchHistorySession}
            onSessionNavigationOrder={onSessionNavigationOrder}
            onPlaceSessionOnPane={onPlaceSessionOnPane}
            onRenameSession={onRenameHistorySession}
            onArchiveSession={onArchiveHistorySession}
            onArchiveSessions={onArchiveHistorySessions}
            onPinSession={onPinHistorySession}
            onPinSessions={onPinHistorySessions}
            reminders={sessionReminders.reminders}
            onSetReminders={sessionReminders.schedule}
            onCancelReminders={sessionReminders.cancel}
            onDeleteSession={onDeleteHistorySession}
            onDeleteSessions={onDeleteHistorySessions}
            onOpenFile={onOpenFile}
            onOpenTerminal={onOpenTerminal}
            onFileMoved={onFileMoved}
            onFileDeleted={onFileDeleted}
            canGoBack={
              tabVisitNav.canBack ||
              searchViewOpen ||
              settingsOpen ||
              inboxViewOpen ||
              notesViewOpen
            }
            canGoForward={tabVisitNav.canForward}
            onGoBack={onRailBack}
            onGoForward={onRailForward}
            onOpenDiff={onOpenWorkingTreeDiff}
            onOpenAllChanges={onOpenAllChanges}
            onOpenCommit={onOpenCommit}
            onShowSourceControl={onToggleChanges}
            selectedDiffPath={
              activeTab ? selectedChangePath(activeTab, gitCwd) : undefined
            }
            selectedDiffKind={
              activeTab ? selectedChangeKind(activeTab) : undefined
            }
            selectedCommitSha={
              activeTab ? selectedCommitSha(activeTab) : undefined
            }
            textHarness={pickTextHarness(active?.harness)}
            recents={recents}
            busyProjectPaths={sessions.flatMap((session) =>
              session.busy && session.cwd ? [session.cwd] : [],
            )}
            liveAgents={liveAgents}
            onSelectAgent={onSelectLiveAgent}
            onSelectProject={onSelectProject}
            onOpenProject={pickProject}
            onRemoveProject={onRemoveProject}
            onNew={onNew}
            openSessions={openProjectSessions}
            onNewTerminal={onNewTerminal}
            onSearch={onOpenSearch}
            onOpenInbox={onOpenInbox}
            onOpenInboxItem={onOpenLinkedWorkItem}
            onOpenNotes={notesEnabled ? onOpenNotes : undefined}
            onGoToFile={onGoToFile}
            searchActive={searchViewOpen}
            inboxActive={inboxViewOpen}
            notesActive={notesViewOpen}
            notesEnabled={notesEnabled}
            projectRailOpen={projectRailOpen}
            onToggleProjectRail={onToggleProjectRail}
            unseenFinishedIds={unseenFinishedIds}
            inboxUnseen={inboxUnseen}
            linkedSessionUpdateIds={linkedSessionUpdateIds}
            settingsOpen={settingsOpen}
            settingsSection={settingsSection}
            onOpenSettings={onOpenSettings}
            onOpenNotificationSettings={onOpenNotificationSettings}
            onSelectSettingsSection={onSelectSettingsSection}
            onCloseSettings={onCloseSettings}
            updateNotice={updateNotice}
            onOpenWhatsNew={onOpenWhatsNew}
            onDismissUpdate={() => setUpdateNotice(null)}
          />

          <div className="body-glass flex min-h-0 min-w-0 flex-1 flex-col">
            <div
              className={
                searchViewOpen || settingsOpen || inboxViewOpen || notesViewOpen
                  ? "hidden"
                  : "flex min-h-0 min-w-0 flex-1 flex-col"
              }
              aria-hidden={
                searchViewOpen || settingsOpen || inboxViewOpen || notesViewOpen
              }
              inert={
                searchViewOpen ||
                settingsOpen ||
                inboxViewOpen ||
                notesViewOpen ||
                undefined
              }
            >
              {!IS_MAC ? (
                <MenuBar
                  onNew={onNew}
                  onNewTerminal={onNewTerminal}
                  onToggleTerminal={onToggleProjectTerminal}
                  onGoToFile={onGoToFile}
                  onToggleSidebar={onToggleSidebar}
                  onShowSourceControl={onToggleChanges}
                  onCloseCurrentTab={
                    activeTabId ? () => onCloseTab(activeTabId) : undefined
                  }
                  onCloseOtherTabs={onCloseOtherTabs}
                  onCloseAllTabs={onCloseAllTabs}
                  onPickProject={pickProject}
                  onFindInProject={onFindInProject}
                  onSearch={onOpenSearch}
                  onOpenInbox={onOpenInbox}
                  onOpenNotes={notesEnabled ? onOpenNotes : undefined}
                  onZoomIn={() => {
                    const next = saveUiScale(zoomInUiScale(loadUiScale()));
                    void applyUiScale(next);
                  }}
                  onZoomOut={() => {
                    const next = saveUiScale(zoomOutUiScale(loadUiScale()));
                    void applyUiScale(next);
                  }}
                  onZoomReset={() => {
                    saveUiScale(UI_SCALE_DEFAULT);
                    void applyUiScale(UI_SCALE_DEFAULT);
                  }}
                />
              ) : null}
              <TitleBar
                tabs={titleTabs}
                activeId={activeTabId}
                cwd={sidebarCwd}
                projectRailOpen={projectRailOpen}
                onToggleSidebar={onToggleSidebar}
                onSelect={activateTab}
                onNew={onNew}
                onNewTerminal={onNewTerminal}
                onOpenSettings={onOpenSettings}
                onOpenInbox={onOpenInbox}
                onOpenNotes={notesEnabled ? onOpenNotes : undefined}
                onClose={onCloseTitleTab}
                onCloseMany={onCloseTabs}
                onReorder={onReorderTabs}
                onPlaceOnPane={onPlaceTabOnPane}
                onGoToFile={onGoToFile}
                recents={recents}
                onSelectProject={onSelectProject}
              />

              <main className="relative flex min-h-0 min-w-0 flex-1">
                <div
                  ref={dockGridRef}
                  className="grid h-full min-h-0 min-w-0 flex-1"
                >
                  {projectTerminals.map((dock) => {
                    const show =
                      dock.open &&
                      sameProjectPath(dock.projectPath, projectCwd);
                    return (
                      <div
                        key={dock.projectPath}
                        className={
                          show
                            ? "h-full min-h-0 min-w-0 w-full overflow-hidden"
                            : "hidden"
                        }
                        style={show ? { gridArea: "dock" } : undefined}
                        aria-hidden={!show}
                      >
                        <ProjectTerminalDock
                          dock={dock}
                          focused={show && projectTerminalFocused}
                          onFocus={focusProjectTerminal}
                          onHide={onHideProjectTerminal}
                          onSideChange={onProjectTerminalSide}
                          onSizePaint={paintDockSize}
                          onSizeCommit={commitDockSize}
                          onAddTerminal={() =>
                            onOpenTerminal(active?.cwd ?? projectCwd)
                          }
                          onSelectTerminal={onSelectProjectTerminal}
                          onCloseTerminal={onCloseProjectTerminal}
                          onCloseOtherTerminals={onCloseOtherProjectTerminals}
                          onReorderTerminals={onReorderProjectTerminals}
                          onTerminalMetaChange={onTerminalMetaChange}
                        />
                      </div>
                    );
                  })}
                  <div
                    className="relative flex min-h-0 min-w-0 flex-row"
                    style={{ gridArea: "main" }}
                  >
                    <div className="relative min-h-0 min-w-0 flex-1">
                      {tabs.map((tab) => (
                        <div
                          key={tab.id}
                          aria-hidden={tab.id !== activeTabId}
                          className={
                            tab.id === activeTabId
                              ? "absolute inset-0 flex h-full min-h-0 flex-col"
                              : "hidden"
                          }
                        >
                          <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col">
                            <PaneTree
                              {...sessionPaneProps}
                              visible={tab.id === activeTabId && !inboxViewOpen}
                              layout={tab.layout}
                              sessions={sessions}
                              editorPanes={[
                                ...tab.editorPanes,
                                ...(tab.terminalPanes ?? []),
                              ]}
                              dirtyFileIds={dirtyFiles}
                              fileErrorCounts={fileErrorCounts}
                              focusedId={
                                tab.id === activeTabId &&
                                !inboxViewOpen &&
                                !tab.diffFocused &&
                                !projectTerminalFocused
                                  ? tab.focusedId
                                  : ""
                              }
                              addToChatSessionId={
                                tab.id === activeTabId ? active?.id : undefined
                              }
                              composerFocused={
                                composerFocused && !projectTerminalFocused
                              }
                              composerFocusToken={composerFocusToken}
                              onSelectFile={onSelectFileSurface}
                              onCloseFile={onCloseFile}
                              onCloseOtherFiles={onCloseOtherFiles}
                              onReorderFiles={onReorderFiles}
                              onFileDirtyChange={onFileDirtyChange}
                              onFileErrorCountChange={onFileErrorCountChange}
                              onRatio={(splitId, index, ratio) =>
                                onRatio(tab.id, splitId, index, ratio)
                              }
                              editorNavigation={editorNavigation}
                              onUpdatePlan={onUpdatePlan}
                              onMovePane={onMovePane}
                              onDetachPane={onDetachPane}
                              onTerminalMetaChange={onTerminalMetaChange}
                            />
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
                {[...linkedWorkItemPanels.values()].map((panel) => (
                  <LinkedWorkItemPanel
                    key={panel.sessionId}
                    target={panel.item}
                    cwd={panel.cwd}
                    recents={recents}
                    visible={
                      !searchViewOpen &&
                      !settingsOpen &&
                      !inboxViewOpen &&
                      !notesViewOpen &&
                      activeLinkedWorkItemPanel?.sessionId === panel.sessionId
                    }
                    onClose={() => closeLinkedWorkItemPanel(panel.sessionId)}
                  />
                ))}
              </main>
            </div>
            {searchViewOpen ? (
              <SearchView
                open
                cwd={sidebarCwd}
                recents={recents}
                history={projectHistory}
                sessions={sessions.filter((session) => !session.inboxAsk)}
                focusToken={searchViewFocusToken}
                besideRail={projectRailOpen}
                onClose={onLeaveSearch}
                onToggleSidebar={onToggleSidebar}
                onOpenFile={onOpenFile}
                onOpenSession={onSelectHistorySession}
                onOpenProject={onSelectProject}
              />
            ) : null}
            <div className="hidden" aria-hidden>
              {sessions
                .filter((session) => session.inboxAsk)
                .map((session) => {
                  const visible =
                    inboxViewOpen && inboxAskPortal?.sessionId === session.id;
                  return (
                    <SessionSurface
                      key={session.id}
                      host={visible ? inboxAskPortal.host : undefined}
                    >
                      <SessionPane
                        {...sessionPaneProps}
                        session={session}
                        visible={visible}
                        focused={visible}
                        inSplit={false}
                        composerFocused={composerFocused}
                        composerFocusToken={composerFocusToken}
                      />
                    </SessionSurface>
                  );
                })}
            </div>
            {inboxViewOpen ? (
              <InboxView
                cwd={sidebarCwd}
                recents={recents}
                besideRail={projectRailOpen}
                onClose={onLeaveInbox}
                onToggleSidebar={onToggleSidebar}
                onStart={onStartInboxItem}
                onAsk={onAskInboxItem}
                onAskRestart={onRestartInboxAsk}
                onAskMount={setInboxAskPortal}
                sessions={inboxRelatedSessions}
                onOpenSession={onOpenInboxSession}
                onOpenIntegrations={onOpenInboxIntegrations}
              />
            ) : null}
            {notesViewOpen ? (
              <NotesView
                besideRail={projectRailOpen}
                cwd={projectCwd}
                recents={recents}
                onClose={onLeaveNotes}
                onToggleSidebar={onToggleSidebar}
              />
            ) : null}
            {settingsOpen ? (
              <SettingsView
                section={settingsSection}
                anchor={settingsAnchor}
                notificationProjectPath={notificationProjectPath}
                notificationSettingsRequest={notificationSettingsRequest}
                recents={recents}
                cwd={sidebarCwd}
                sessions={sidebarHistory}
                besideRail
                onClose={onCloseSettings}
                onSelectSection={onSelectSettingsSection}
                onOpenSession={onOpenArchivedSession}
                onArchiveSession={onArchiveHistorySession}
                onDeleteSession={onDeleteHistorySession}
                onRestoreProject={onRestoreProject}
                onDeleteProject={(path) =>
                  onRemoveProject(path, { purgeData: true })
                }
                onOpenWhatsNew={onOpenWhatsNew}
              />
            ) : null}
            {searchViewOpen ||
            inboxViewOpen ||
            notesViewOpen ||
            settingsOpen ? null : (
              <UsageFooter
                providers={usageProviders}
                session={usageSession}
                project={active?.cwd ?? projectCwd}
                onSelectAccount={onSelectProviderAccount}
                terminals={runningTerminals}
                terminalOpen={runningTerminalOpen}
                onToggleTerminal={onToggleRunningTerminal}
                onNewTerminal={
                  looksLikeProject(projectCwd) ? onNewTerminal : undefined
                }
                onShowTerminal={
                  looksLikeProject(projectCwd)
                    ? onShowProjectTerminal
                    : undefined
                }
                projectTerminalActive={
                  !!currentProjectDock &&
                  currentProjectDock.pane.files.length > 0
                }
              />
            )}
          </div>

          {filePickerOpen ? (
            <FilePicker
              open
              cwd={gitCwd}
              openPaths={openFilePaths}
              onOpenFile={onOpenFile}
              onClose={() => setFilePickerOpen(false)}
            />
          ) : null}

          <ApprovalToasts
            notices={hiddenApprovalToasts}
            topOffset={
              12 + (reminderNoticesHeight ? reminderNoticesHeight + 8 : 0)
            }
            onFocusSession={onOpenApprovalSession}
            onApproval={onApproval}
          />
          <ReminderNotices
            reminders={sessionReminders.due}
            error={sessionReminders.error}
            onOpen={sessionReminders.open}
            onSnooze={sessionReminders.schedule}
            onDismiss={sessionReminders.cancel}
            onRetry={sessionReminders.refresh}
            onOpenSettings={() => openSettings("general", "notifications")}
            onHeightChange={setReminderNoticesHeight}
          />
          {whatsNewVersion ? (
            <WhatsNewDialog
              version={whatsNewVersion}
              onClose={() => setWhatsNewVersion(null)}
            />
          ) : null}
          {providerSignInRequest ? (
            <ProviderSignInDialog
              key={providerSignInRequest.key}
              harness={providerSignInRequest.harness}
              onClose={() => setProviderSignInRequest(null)}
            />
          ) : null}
        </div>
      </OrchestrationWorkers.Provider>
    </OrchestrationActions.Provider>
  );
}
