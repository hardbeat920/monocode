import {
  NO_BRANCH_LABEL,
  type Worktree,
} from "../../features/source-control/model/worktrees";
import { useProjectWorktrees } from "../../features/source-control/hooks/useProjectWorktrees";
import {
  type WorktreeFocus,
  inWorktreeFocus,
  useWorktreeFocus,
} from "../../features/source-control/model/worktreeFocus";
import { SidebarWorktreeSwitcher } from "../../features/source-control/ui/SidebarWorktreeSwitcher";
import { OrchestrationSidebarAgents } from "../../features/orchestration/ui/OrchestrationSidebarAgents";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Archive,
  Chatting,
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleDashed,
  CircleDot,
  Clock,
  FileScript,
  Folder,
  GitBranch,
  GitPullRequest,
  Inbox,
  ListFilter,
  PanelLeft,
  Pin,
  Plus,
  Search,
  Share,
  Settings,
  StickyNote,
  Zap,
} from "../../shared/ui/icons";
import {
  memo,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ComponentProps,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import {
  loadSidebarTabOrder,
  saveSidebarTabOrder,
  type SidebarTabId,
} from "../../features/settings/model/appearance";
import { formatInteger } from "../../shared/lib/numbers";
import { pathKey } from "../../shared/lib/paths";
import {
  type GitFileDiffKind,
  type GitHistoryCommit,
} from "../../platform/tauri/fs";
import { IS_MAC, MOD } from "../../platform/tauri/platform";
import { copyText } from "../../platform/tauri/clipboard";
import { resolveModel } from "../../features/sessions/model/models";
import type { OpenFileFn } from "../../features/search/model/search";
import { sessionDisplayTitle } from "../../features/sessions/model/session";
import { ParticleText } from "../../shared/ui/ParticleText";
import { nextUnseenFinishedSessions } from "../../features/sessions/model/sessionDone";
import { orchestrationTaskLabel } from "../../features/orchestration/model/orchestrationSummary";
import {
  orderedSessionActionIds,
  pruneSessionSelection,
  toggleSessionSelection,
} from "../../features/sessions/model/sessionSelection";
import {
  paneDropFromPoint,
  setExternalPaneDrop,
} from "../../features/workspace/model/paneDrop";
import type { PaneEdge } from "../../features/workspace/model/layout";
import { suppressTextSelection } from "../../shared/lib/drag";
import {
  compareSessionSummaries,
  filterSessionsByArchive,
  filterSessionsByQuery,
} from "../../features/sessions/data/sessionHistory";
import {
  addSessionToFolder,
  applySessionListDrop,
  buildSessionList,
  createFolderWithSessions,
  dissolveFolder,
  folderAccent,
  folderContaining,
  folderShellFill,
  loadPinnedSessionsCollapsed,
  loadReminderSessionsCollapsed,
  loadSessionFolders,
  mergeFolderSessionSummaries,
  pruneSessionFolders,
  removeSessionFromFolder,
  renameFolder,
  reorderSessionFolders,
  savePinnedSessionsCollapsed,
  saveReminderSessionsCollapsed,
  saveSessionFolders,
  sessionListNavigationIds,
  setFolderCollapsed,
  setFolderColor,
  setFolderCustomColor,
  subscribeSessionFolders,
  ungroupedSessions,
  type SessionFolder,
  type SessionListDropTarget,
} from "../../features/sessions/model/sessionFolders";
import { LIST_PAGE_SIZE, listWindowSize } from "../../shared/lib/listWindow";
import {
  filterSessionsByHarness,
  filterSessionsByStatus,
  filterSessionsByTime,
  harnessesInSessions,
  hasActiveSessionFilters,
  loadSessionSidebarFilters,
  saveSessionSidebarFilters,
  type SessionSidebarFilters,
} from "../../features/sessions/model/sessionFilters";
import type {
  HarnessId,
  LinkedWorkItem,
} from "../../features/sessions/model/session";
import type { LiveAgent } from "../../features/sessions/model/liveAgents";
import type { SessionSummary } from "../../features/sessions/data/sessionStore";
import type { SettingsSectionId } from "../../features/settings/model/settings";
import type { InstalledUpdate } from "../model/updateNotice";
import { TAB_GROUP_COLORS } from "../../features/workspace/model/tabGroups";
import { useDragResize } from "../../shared/hooks/useDragResize";
import { useGitFileStatuses } from "../../features/source-control/hooks/useGitFileStatuses";
import { useLockOverscroll } from "../../shared/hooks/useLockOverscroll";
import { useProjectDiffStats } from "../../features/source-control/hooks/useProjectDiffStats";
import { useSortable } from "../../shared/hooks/useSortable";
import { useAnimatedReorder } from "../../shared/hooks/useAnimatedReorder";
import { normalizeHex } from "../../shared/lib/colorUtils";
import {
  collectRailProjects,
  looksLikeProject,
  isRemoteProjectPath,
  sameProjectPath,
  type RecentProject,
} from "../../features/projects/model/recents";
import {
  ColorPickerPopover,
  ColorSwatchRow,
} from "../../shared/ui/ColorPickerPopover";
import {
  ExplorerMenu,
  type ExplorerMenuItem,
} from "../../features/files/ui/ExplorerMenu";
import { FileTree } from "../../features/files/ui/FileTree";
import { HarnessIcon } from "../../features/sessions/ui/HarnessIcon";
import { LiveAgentsPreview } from "../../features/sessions/ui/LiveAgentsPreview";
import { ProjectRail } from "./ProjectRail";
import { InboxNotificationMenu } from "../../features/inbox/ui/InboxNotificationMenu";
import { RailAction } from "./RailAction";
import { TerminalSpinner } from "../../features/sessions/ui/TerminalSpinner";
import { DevModeSlot, IconButton, TabVisitNav } from "./TitleBar";
import { ProjectSearch } from "../../features/projects/ui/ProjectSearch";
import { Popover } from "../../shared/ui/Popover";
import { SearchableProjectPicker } from "../../features/projects/ui/SearchableProjectPicker";
import { useProjectMenu } from "./useProjectMenu";
import { SessionFiltersMenu } from "../../features/sessions/ui/SessionFiltersMenu";
import { LinkSessionWorkItemDialog } from "../../features/sessions/ui/LinkSessionWorkItemDialog";
import { sessionReminderPresets } from "../../features/sessions/ui/sessionReminderPresets";
import {
  formatReminderTime,
  reminderTime,
  type SessionReminder,
} from "../../features/sessions/model/sessionReminders";
import { SessionsEmpty } from "../../features/sessions/ui/SessionsEmpty";
import { SidebarUpdateFooter } from "./SidebarUpdate";
import { SourceControl } from "../../features/source-control/ui/SourceControl";
import { GithubStarPrompt } from "./GithubStarPrompt";
import {
  isMonoSession,
  listMonos,
  MONO_STATUS_LABEL,
  monoLook,
  monosSnapshot,
  subscribeMonos,
  type MonoStatus,
} from "../../features/monos/model/mono";
import { MonoRailMascot } from "../../features/monos/ui/MonoRailMascot";
import { useRailMonosPinned } from "../../features/settings/model/displayPrefs";
import type { PickerMonos } from "../../features/projects/ui/SearchableProjectPicker";
import { isHabitRun } from "../../features/monos/model/monoHabits";
import {
  unreadBadge,
  unreadLabel,
  type MonoRailProps,
} from "./MonoRailSection";
import {
  refreshRemoteProjectSessions,
  remoteRequest,
  remotePendingWorktree,
  remoteSessionFor,
  useRemoteProjectSessions,
} from "../../features/connections/model/connections";
import { parseRemotePath, remotePath, remoteProjectFor } from "../../features/connections/model/remoteProjects";

const MIN_WIDTH = 260;
const MAX_WIDTH = 560;
const DEFAULT_WIDTH = 260;
const REMINDERS_COLOR = "#f59e0b";

let rememberedWidth = DEFAULT_WIDTH;

type SidebarTab = SidebarTabId;

const TAB_LABELS: Record<SidebarTab, string> = {
  sessions: "Sessions",
  inbox: "Inbox",
  files: "Explorer",
  changes: "Changes",
};

const COMPACT_TAB_ICONS: Record<SidebarTab, typeof PanelLeft> = {
  sessions: Chatting,
  inbox: Inbox,
  files: FileScript,
  changes: GitBranch,
};

function projectPathBusy(
  paths: Iterable<string> | undefined,
  cwd: string,
): boolean {
  if (!paths) return false;
  for (const path of paths) {
    if (sameProjectPath(path, cwd)) return true;
  }
  return false;
}

type Props = {
  cwd: string;
  /** Working copy for Changes / explorer git. Falls back to `cwd`. */
  gitCwd?: string;
  /** Branch identity shown for a worktree whose folder has a temporary name. */
  explorerRootLabel?: string;
  /** Open tabs per worktree path key, for the worktree switcher. */
  worktreeTabStats?: ReadonlyMap<string, { tabs: number; busy: boolean }>;
  onSelectWorkspace?: (focus?: WorktreeFocus) => void;
  workspaceSwitchPending?: boolean;
  workspaceSwitchError?: string;
  open: boolean;
  sessions: SessionSummary[];
  busySessionIds: Set<string>;
  approvalSessionIds: Set<string>;
  activeSessionId?: string;
  /** Open tabs, including blank ones not yet in history. */
  openSessions?: readonly SessionSummary[];
  status: "idle" | "error";
  /** First listing for this project has not arrived yet. */
  pending: boolean;
  onSelectSession: (sessionId: string) => void;
  onSelectRemoteSession?: (project: string, sessionId: string) => void;
  onRemoteSessionDeleted?: (sessionId: string) => void;
  onSessionNavigationOrder?: (ids: readonly string[]) => void;
  onPrefetchSession?: (sessionId: string) => void;
  onPlaceSessionOnPane?: (
    sessionId: string,
    targetId: string,
    edge: PaneEdge,
  ) => void;
  onRenameSession?: (sessionId: string, title: string) => void;
  onArchiveSession?: (sessionId: string, archived: boolean) => void;
  onArchiveSessions?: (
    sessionIds: readonly string[],
    archived: boolean,
  ) => void;
  onPinSession?: (sessionId: string, pinned: boolean) => void;
  onPinSessions?: (sessionIds: readonly string[], pinned: boolean) => void;
  onSetSessionLinkedWorkItem?: (
    sessionId: string,
    item: LinkedWorkItem | undefined,
  ) => void;
  /** Rebind a conversation to another working copy of this project. */
  onMoveSessionToWorktree?: (sessionId: string, tree: Worktree) => void;
  reminders?: readonly SessionReminder[];
  onSetReminders?: (sessionIds: readonly string[], dueAt: number) => void;
  onCancelReminders?: (sessionIds: readonly string[]) => void;
  onDeleteSession?: (sessionId: string) => void;
  onDeleteSessions?: (sessionIds: readonly string[]) => void;
  onOpenFile: OpenFileFn;
  onOpenTerminal?: (cwd: string) => void;
  onFileMoved?: (from: string, to: string) => void;
  onFileDeleted?: (path: string) => void;
  tab: SidebarTab;
  onTabChange: (tab: SidebarTab) => void;
  filesSearchOpen: boolean;
  onFilesSearchOpenChange: (open: boolean) => void;
  onOpenFilesSearch?: () => void;
  searchFocusToken?: number;
  canGoBack?: boolean;
  canGoForward?: boolean;
  onGoBack?: () => void;
  onGoForward?: () => void;
  onOpenDiff?: (path: string, kind?: GitFileDiffKind, pin?: boolean) => void;
  onOpenAllChanges?: (kind: GitFileDiffKind) => void;
  onOpenCommit?: (commit: GitHistoryCommit, pin?: boolean) => void;
  selectedDiffPath?: string;
  selectedDiffKind?: GitFileDiffKind;
  selectedCommitSha?: string;
  textHarness?: HarnessId;
  recents?: RecentProject[];
  busyProjectPaths?: Iterable<string>;
  liveAgents?: LiveAgent[];
  onSelectAgent?: (sessionId: string) => void;
  onSelectProject?: (path: string) => void;
  onOpenProject?: () => void;
  onRemoveProject?: (path: string, options: { purgeData: boolean }) => void;
  onNew?: () => string | void;
  onNewTerminal?: () => void;
  onSearch?: () => void;
  onOpenInbox?: () => void;
  onOpenInboxItem?: (item: LinkedWorkItem, sessionId: string) => void;
  onOpenNotes?: () => void;
  onOpenAutomations?: () => void;
  onGoToFile?: () => void;
  searchActive?: boolean;
  inboxActive?: boolean;
  notesActive?: boolean;
  automationsActive?: boolean;
  notesEnabled?: boolean;
  onToggleProjectRail?: () => void;
  projectRailOpen?: boolean;
  compactProjectRail?: boolean;
  titleBarAbove?: boolean;
  unseenFinishedIds?: Set<string>;
  inboxUnseen?: boolean;
  /** Linked GitHub work changed after the session last advanced. */
  linkedSessionUpdateIds?: ReadonlySet<string>;
  settingsOpen?: boolean;
  settingsSection?: SettingsSectionId;
  onOpenSettings?: () => void;
  onOpenNotificationSettings?: (projectPath?: string) => void;
  onSelectSettingsSection?: (section: SettingsSectionId) => void;
  onCloseSettings?: () => void;
  updateNotice?: InstalledUpdate | null;
  onOpenWhatsNew?: (version: string) => void;
  onDismissUpdate?: () => void;
  /** The Monos on the rail; absent while Monos are off. */
  monos?: MonoRailProps;
  /** A Mono fills the main area, which has no project sidebar. */
  monoViewActive?: boolean;
};

function SidebarComponent({
  cwd,
  gitCwd,
  explorerRootLabel,
  worktreeTabStats,
  onSelectWorkspace,
  workspaceSwitchPending,
  workspaceSwitchError,
  open,
  sessions,
  busySessionIds,
  approvalSessionIds,
  activeSessionId,
  openSessions = [],
  status,
  pending,
  onSelectSession: onSelectLocalSession,
  onSelectRemoteSession,
  onRemoteSessionDeleted,
  onSessionNavigationOrder,
  onPrefetchSession: onPrefetchLocalSession,
  onPlaceSessionOnPane: onPlaceLocalSessionOnPane,
  onRenameSession: onRenameLocalSession,
  onArchiveSession: onArchiveLocalSession,
  onArchiveSessions: onArchiveLocalSessions,
  onPinSession: onPinLocalSession,
  onPinSessions: onPinLocalSessions,
  onSetSessionLinkedWorkItem: onSetLocalSessionLinkedWorkItem,
  onMoveSessionToWorktree: onMoveLocalSessionToWorktree,
  reminders = [],
  onSetReminders,
  onCancelReminders,
  onDeleteSession: onDeleteLocalSession,
  onDeleteSessions: onDeleteLocalSessions,
  onOpenFile,
  onOpenTerminal,
  onFileMoved,
  onFileDeleted,
  tab: requestedTab,
  onTabChange,
  filesSearchOpen,
  onFilesSearchOpenChange,
  onOpenFilesSearch,
  searchFocusToken = 0,
  canGoBack = false,
  canGoForward = false,
  onGoBack,
  onGoForward,
  onOpenDiff,
  onOpenAllChanges,
  onOpenCommit,
  selectedDiffPath,
  selectedDiffKind,
  selectedCommitSha,
  textHarness,
  recents = [],
  busyProjectPaths,
  liveAgents = [],
  onSelectAgent,
  onSelectProject,
  onOpenProject,
  onRemoveProject,
  onNew,
  onSearch,
  onOpenInbox,
  onOpenInboxItem,
  onOpenNotes,
  onOpenAutomations,
  onGoToFile,
  searchActive = false,
  inboxActive = false,
  notesActive = false,
  automationsActive = false,
  notesEnabled = true,
  onToggleProjectRail,
  projectRailOpen = true,
  compactProjectRail = true,
  titleBarAbove = false,
  unseenFinishedIds: unseenFinishedIdsProp,
  inboxUnseen = false,
  linkedSessionUpdateIds = new Set(),
  settingsOpen = false,
  settingsSection = "general",
  onOpenSettings,
  onOpenNotificationSettings,
  onSelectSettingsSection,
  onCloseSettings,
  updateNotice = null,
  onOpenWhatsNew,
  onDismissUpdate,
  monos,
  monoViewActive = false,
}: Props) {
  const remoteProject = isRemoteProjectPath(cwd);
  const tab: SidebarTabId = requestedTab;
  const remote = useRemoteProjectSessions(cwd, remoteProject);
  const hostProject = remoteProject ? remoteProjectFor(cwd) : undefined;
  const remoteChange = async (
    sessionId: string,
    patch: { title?: string; archived?: boolean; pinned?: boolean; linkedWorkItem?: LinkedWorkItem | null },
  ) => {
    if (!remote.machine || !hostProject) {
      window.alert("Connect this project's machine to change its sessions.");
      return;
    }
    try {
      await remoteRequest(remote.machine.id, "sessions.update", {
        projectId: hostProject.projectId,
        sessionId,
        ...patch,
      });
      refreshRemoteProjectSessions();
    } catch (error) {
      window.alert(`Could not update this session.\n\n${String(error)}`);
    }
  };
  const remoteDelete = async (sessionIds: readonly string[]) => {
    if (sessionIds.length === 0) return;
    if (!remote.machine || !hostProject) {
      window.alert("Connect this project's machine to delete its sessions.");
      return;
    }
    if (!window.confirm(
      `Delete ${sessionIds.length === 1 ? "this conversation" : `${sessionIds.length} conversations`}? This can’t be undone.`,
    )) return;
    try {
      for (const sessionId of sessionIds) {
        await remoteRequest(remote.machine.id, "sessions.delete", {
          projectId: hostProject.projectId,
          sessionId,
        });
        onRemoteSessionDeleted?.(sessionId);
      }
      refreshRemoteProjectSessions();
    } catch (error) {
      window.alert(`Could not delete this session.\n\n${String(error)}`);
      refreshRemoteProjectSessions();
    }
  };
  const onSelectSession = remoteProject
    ? (sessionId: string) => onSelectRemoteSession?.(cwd, sessionId)
    : onSelectLocalSession;
  const onPrefetchSession = remoteProject ? undefined : onPrefetchLocalSession;
  const onPlaceSessionOnPane = remoteProject ? undefined : onPlaceLocalSessionOnPane;
  const onRenameSession = remoteProject
    ? (sessionId: string, title: string) => { void remoteChange(sessionId, { title }); }
    : onRenameLocalSession;
  const onArchiveSession = remoteProject
    ? (sessionId: string, archived: boolean) => { void remoteChange(sessionId, { archived }); }
    : onArchiveLocalSession;
  const onArchiveSessions = remoteProject
    ? (sessionIds: readonly string[], archived: boolean) => {
        void Promise.all(sessionIds.map((id) => remoteChange(id, { archived })));
      }
    : onArchiveLocalSessions;
  const onPinSession = remoteProject
    ? (sessionId: string, pinned: boolean) => { void remoteChange(sessionId, { pinned }); }
    : onPinLocalSession;
  const onPinSessions = remoteProject
    ? (sessionIds: readonly string[], pinned: boolean) => {
        void Promise.all(sessionIds.map((id) => remoteChange(id, { pinned })));
      }
    : onPinLocalSessions;
  const onDeleteSession = remoteProject
    ? (sessionId: string) => { void remoteDelete([sessionId]); }
    : onDeleteLocalSession;
  const onDeleteSessions = remoteProject
    ? (sessionIds: readonly string[]) => { void remoteDelete(sessionIds); }
    : onDeleteLocalSessions;
  const onSetSessionLinkedWorkItem = remoteProject
    ? (sessionId: string, item: LinkedWorkItem | undefined) => {
        void remoteChange(sessionId, { linkedWorkItem: item ?? null });
      }
    : onSetLocalSessionLinkedWorkItem;
  const onMoveSessionToWorktree = remoteProject
    ? undefined
    : onMoveLocalSessionToWorktree;
  const projectWorktrees = useProjectWorktrees(
    cwd,
    !!onMoveSessionToWorktree,
  ).data?.worktrees.filter((tree) => !tree.missing);
  const activeRemoteId = activeSessionId
    ? remoteSessionFor(activeSessionId)
    : undefined;
  const activeListedSessionId = remoteProject ? activeRemoteId : activeSessionId;
  const listedBusySessionIds = remoteProject
    ? new Set(remote.sessions.filter((session) => session.status === "running" && !session.needsInput).map((session) => session.id))
    : busySessionIds;
  const listedApprovalSessionIds = remoteProject
    ? new Set(remote.sessions.filter((session) => session.needsInput).map((session) => session.id))
    : approvalSessionIds;
  const projectSessions: SessionSummary[] = useMemo(() => remoteProject
    ? remote.sessions.map((session) => ({
        id: session.id,
        cwd,
        harness: session.harness,
        model: session.model ?? "",
        runtimeMode: session.runtimeMode ?? "supervised",
        providerSessionId: session.providerSessionId ?? undefined,
        title: session.title,
        createdAt: session.createdAt ?? session.updatedAt,
        updatedAt: session.updatedAt,
        archived: session.archived,
        pinned: session.pinned,
        linkedWorkItem: session.linkedWorkItem,
        draft: session.draft,
        repo: session.repo,
        branch: session.branch,
        worktreeCwd: session.worktreeCwd,
      }))
    : sessions, [remoteProject, remote.sessions, sessions, cwd]);
  const remoteExecutionCwd =
    remote.sessions.find((session) => session.id === activeRemoteId)?.cwd ??
    (activeSessionId ? remotePendingWorktree(activeSessionId) : undefined) ??
    (remoteProject && gitCwd && gitCwd !== cwd
      ? parseRemotePath(gitCwd)?.hostPath ?? gitCwd
      : undefined) ??
    undefined;
  const gitRoot =
    remoteProject && hostProject
      ? remotePath(hostProject.environmentId, remoteExecutionCwd ?? hostProject.cwd)
      : gitCwd || cwd;
  const resize = useDragResize({
    min: MIN_WIDTH,
    max: () => Math.min(MAX_WIDTH, Math.floor(window.innerWidth * 0.5)),
    defaultWidth: DEFAULT_WIDTH,
    initial: rememberedWidth,
    onCommit: (next) => {
      rememberedWidth = next;
    },
  });
  const [tabOrder, setTabOrder] = useState<SidebarTab[]>(loadSidebarTabOrder);
  const [now, setNow] = useState(() => Date.now());
  const sessionsLock = useLockOverscroll<HTMLDivElement>();
  const sessionsScrollRef = useRef<HTMLDivElement>(null);
  const [sessionMenu, setSessionMenu] = useState<{
    x: number;
    y: number;
    sessionId: string;
  } | null>(null);
  const [linkingSession, setLinkingSession] = useState<SessionSummary | null>(
    null,
  );
  const [selectedSessionIds, setSelectedSessionIds] = useState<Set<string>>(
    () => new Set(),
  );
  const contextSelectionRef = useRef(false);
  const selectionAnchorRef = useRef<string | null>(null);
  const [folderMenu, setFolderMenu] = useState<{
    x: number;
    y: number;
    folderId: string;
  } | null>(null);
  const [renamingSessionId, setRenamingSessionId] = useState<string | null>(
    null,
  );
  const [renamingFolderId, setRenamingFolderId] = useState<string | null>(null);
  const [sessionFolders, setSessionFolders] = useState<SessionFolder[]>(() =>
    loadSessionFolders(cwd),
  );
  const [pinnedSessionsCollapsed, setPinnedSessionsCollapsed] = useState(() =>
    loadPinnedSessionsCollapsed(cwd),
  );
  const [reminderSessionsCollapsed, setReminderSessionsCollapsed] = useState(
    () => loadReminderSessionsCollapsed(cwd),
  );
  const [sessionDrop, setSessionDrop] = useState<SessionListDropTarget | null>(
    null,
  );
  const [sessionFilters, setSessionFilters] = useState(
    loadSessionSidebarFilters,
  );
  const [filterMenu, setFilterMenu] = useState<{ x: number; y: number } | null>(
    null,
  );
  const [searchQuery, setSearchQuery] = useState("");
  const [sessionListLimit, setSessionListLimit] = useState(LIST_PAGE_SIZE);
  const loadMoreRef = useRef<HTMLLIElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const pendingFolderSessionIds = useRef(new Set<string>());
  const busyIdsRef = useRef(busySessionIds);
  const focusedSessionIdRef = useRef(activeSessionId);
  const unseenFinishedLocalRef = useRef<Set<string>>(new Set());
  if (
    busyIdsRef.current !== busySessionIds ||
    focusedSessionIdRef.current !== activeSessionId
  ) {
    unseenFinishedLocalRef.current = nextUnseenFinishedSessions({
      previousBusyIds: busyIdsRef.current,
      busyIds: busySessionIds,
      previousUnseenIds: unseenFinishedLocalRef.current,
      focusedSessionId: activeSessionId,
    });
    busyIdsRef.current = busySessionIds;
    focusedSessionIdRef.current = activeSessionId;
  }
  const unseenFinishedIds =
    unseenFinishedIdsProp ?? unseenFinishedLocalRef.current;
  // Revisits render straight from cache, so this is only ever true the first
  // time a project is opened.
  const pendingFirstLoad = remoteProject
    ? !!remote.machine && !remote.loaded && projectSessions.length === 0
    : pending && sessions.length === 0;
  const worktreeFocus = useWorktreeFocus(cwd);
  const focusedWorktree = remoteProject ? undefined : worktreeFocus;
  useSyncExternalStore(subscribeMonos, monosSnapshot);
  const listedSessions = mergeFolderSessionSummaries(
    projectSessions,
    remoteProject ? [] : openSessions,
    sessionFolders,
  ).filter(
    (session) =>
      !isMonoSession(session.id) &&
      !session.sidebarHidden &&
      !("ephemeral" in session && session.ephemeral) &&
      !isHabitRun(session.id) &&
      !session.orchestrationLeadId &&
      inWorktreeFocus(session, focusedWorktree),
  );
  const visibleSessions = [
    ...filterSessionsByQuery(
      filterSessionsByStatus(
        filterSessionsByTime(
          filterSessionsByHarness(
            filterSessionsByArchive(
              listedSessions,
              sessionFilters.showArchived,
            ),
            sessionFilters.hiddenHarnesses,
          ),
          sessionFilters.time,
          now,
        ),
        sessionFilters.status,
        listedBusySessionIds,
        listedApprovalSessionIds,
        unseenFinishedIds,
      ),
      searchQuery,
    ),
  ].sort(compareSessionSummaries);
  const filtersActive = hasActiveSessionFilters(sessionFilters);
  const searchNarrowed = Boolean(searchQuery.trim());
  // Summaries for the whole project stay in `sessions` so filters still work.
  // Folders sit above the ungrouped list. Only a page of ungrouped cards
  // mounts; the sentinel below asks for the next page.
  const reminderIds = new Set(reminders.map((reminder) => reminder.sessionId));
  const reminderGroup = {
    sessionIds: [...reminders]
      .sort((a, b) => a.dueAt - b.dueAt)
      .map((reminder) => reminder.sessionId),
    collapsed: reminderSessionsCollapsed,
  };
  const ungroupedVisible = ungroupedSessions(
    visibleSessions,
    sessionFolders,
  ).filter((session) => !reminderIds.has(session.id));
  const activeUngroupedIndex = ungroupedVisible.findIndex(
    (session) => session.id === activeListedSessionId,
  );
  const shownUngroupedCount = listWindowSize(
    ungroupedVisible.length,
    sessionListLimit,
    activeUngroupedIndex,
  );
  const shownUngrouped = ungroupedVisible.slice(0, shownUngroupedCount);
  const fullSessionListEntries = buildSessionList(
    visibleSessions,
    sessionFolders,
    ungroupedVisible,
    pinnedSessionsCollapsed,
    reminderGroup,
  );
  const sessionListEntries = buildSessionList(
    visibleSessions,
    sessionFolders,
    shownUngrouped,
    pinnedSessionsCollapsed,
    reminderGroup,
  );
  const sessionNavigationIds = sessionListNavigationIds(
    fullSessionListEntries,
    searchNarrowed,
  );
  const sessionNavigationKey = sessionNavigationIds.join("\0");
  useEffect(() => {
    onSessionNavigationOrder?.(sessionNavigationIds);
  }, [onSessionNavigationOrder, sessionNavigationKey]);
  useEffect(() => {
    if (tab !== "sessions") {
      selectionAnchorRef.current = null;
      setSelectedSessionIds(new Set());
      return;
    }
    const available = new Set(sessionNavigationIds);
    if (
      selectionAnchorRef.current &&
      !available.has(selectionAnchorRef.current)
    ) {
      selectionAnchorRef.current = null;
    }
    setSelectedSessionIds((current) =>
      pruneSessionSelection(current, available),
    );
  }, [cwd, tab, sessionNavigationKey]);
  const hasMoreSessions = shownUngroupedCount < ungroupedVisible.length;
  const sessionListKey = `${cwd}\0${sessionFilters.showArchived}\0${sessionFilters.time}\0${sessionFilters.hiddenHarnesses.join(",")}\0${sessionFilters.status.working}\0${sessionFilters.status.needsApproval}\0${sessionFilters.status.done}\0${searchQuery}`;
  const sessionHarnesses = harnessesInSessions(projectSessions);
  const narrowedByUser = searchNarrowed || filtersActive;
  const visibleTabs = tabOrder.filter((itemId) => itemId !== "inbox");
  const sortable = useAnimatedReorder(visibleTabs, (ids) => {
    let index = 0;
    const next = tabOrder.map((itemId) =>
      itemId === "inbox" ? itemId : ids[index++],
    );
    setTabOrder(next);
    saveSidebarTabOrder(next);
  });
  const visibleFolderIds = sessionListEntries.flatMap((entry) =>
    entry.kind === "folder" ? [entry.folder.id] : [],
  );
  const folderSortable = useSortable(
    visibleFolderIds,
    (ids) => {
      setSessionFolders((current) => {
        const next = reorderSessionFolders(current, ids);
        if (next === current) return current;
        saveSessionFolders(cwd, next);
        return next;
      });
    },
    { axis: "y" },
  );
  const showProjectRail = Boolean(onSelectProject && onOpenProject);
  // Settings live in the rail slot, so they keep it visible even when the
  // project rail itself is collapsed.
  const railVisible = showProjectRail && (projectRailOpen || settingsOpen);
  // Keep the full rail mounted after its first reveal so reopening does not
  // recreate every project row and restart their Git-stat subscriptions.
  const railMounted = useRef(railVisible);
  if (railVisible) railMounted.current = true;
  const compactRailVisible =
    compactProjectRail && showProjectRail && !railVisible;
  const inProject = looksLikeProject(cwd);
  const showSidebarFooter = !projectRailOpen;
  const otherViewActive =
    searchActive ||
    inboxActive ||
    notesActive ||
    automationsActive ||
    settingsOpen;
  // A remembered Mono sits underneath these views; select it only while visible.
  const railMonos = monos
    ? { ...monos, activeId: otherViewActive ? undefined : monos.activeId }
    : undefined;
  // A blank session has no project to browse, so the shell stands alone until
  // one is picked — whether or not the rail is open.
  const sidebarAvailable =
    !otherViewActive && !monoViewActive && inProject;
  const sidebarVisible = open && sidebarAvailable;
  // With the sidebar collapsed beside the compact rail, its tab shortcuts
  // open the sidebar temporarily until the user clicks away.
  const drawerMode = compactRailVisible && !open;
  const [drawerOpen, setDrawerOpen] = useState(false);
  const drawerRef = useRef<HTMLDivElement>(null);
  const drawerVisible = drawerMode && drawerOpen && sidebarAvailable;
  // A dismissed drawer stays mounted while it slides shut. Anything that takes
  // its place (the pinned sidebar, another view) drops it at once.
  const [drawerMounted, setDrawerMounted] = useState(false);
  const drawerClosing =
    drawerMounted && !drawerVisible && drawerMode && sidebarAvailable;
  const drawerRendered = drawerVisible || drawerClosing;
  const drawerAnimation = useRef<Animation | null>(null);
  const panelOpen = open || drawerVisible;
  // Keep the hidden explorer intact when a chat tab changes worktrees. Its
  // rows and file icons only need rebuilding when Files is actually shown.
  const explorer = useRef<{ cwd: string; rootLabel?: string } | null>(null);
  if (panelOpen && tab === "files") {
    explorer.current = { cwd: gitRoot, rootLabel: explorerRootLabel };
  }
  const gitStatuses = useGitFileStatuses(gitRoot, panelOpen && tab === "files");
  const changeStats = useProjectDiffStats(gitRoot, panelOpen);

  useEffect(() => {
    if (!drawerMode || !sidebarAvailable) setDrawerOpen(false);
  }, [drawerMode, sidebarAvailable]);

  useEffect(() => {
    if (drawerVisible) setDrawerMounted(true);
    else if (!drawerClosing) setDrawerMounted(false);
  }, [drawerVisible, drawerClosing]);

  // Grow the drawer's width so the workspace is pushed along with it. A
  // reversal mid-slide starts from wherever the width currently is.
  useLayoutEffect(() => {
    const drawer = drawerRef.current;
    if (!drawerRendered || !drawer) {
      drawerAnimation.current = null;
      return;
    }
    const full =
      drawer.firstElementChild instanceof HTMLElement
        ? drawer.firstElementChild.offsetWidth
        : 0;
    const from = drawerAnimation.current
      ? drawer.getBoundingClientRect().width
      : drawerClosing
        ? full
        : 0;
    drawerAnimation.current?.cancel();
    drawerAnimation.current = null;
    const reduceMotion = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    if (typeof drawer.animate !== "function" || reduceMotion) {
      if (drawerClosing) setDrawerMounted(false);
      return;
    }
    const animation = drawer.animate(
      [{ width: `${from}px` }, { width: `${drawerClosing ? 0 : full}px` }],
      drawerClosing
        ? {
            duration: 160,
            easing: "cubic-bezier(0.4, 0, 1, 1)",
            fill: "forwards",
          }
        : { duration: 200, easing: "cubic-bezier(0.22, 1, 0.36, 1)" },
    );
    drawerAnimation.current = animation;
    animation.onfinish = () => {
      if (drawerAnimation.current !== animation) return;
      drawerAnimation.current = null;
      if (drawerClosing) setDrawerMounted(false);
    };
  }, [drawerRendered, drawerClosing]);

  useEffect(() => {
    if (!drawerVisible) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      setDrawerOpen(false);
    };
    // The rail's own shortcuts toggle the drawer, and menus opened from it
    // stay usable; anything else dismisses it.
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      const el = target instanceof Element ? target : null;
      if (drawerRef.current?.contains(el)) return;
      if (el?.closest("[data-compact-project-rail],[data-popover-side]")) {
        return;
      }
      setDrawerOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [drawerVisible]);

  useEffect(() => {
    setSessionListLimit(LIST_PAGE_SIZE);
    const scroller = sessionsScrollRef.current;
    if (scroller) scroller.scrollTop = 0;
  }, [sessionListKey]);

  useEffect(() => {
    if (tab !== "sessions" || !hasMoreSessions) return;
    const sentinel = loadMoreRef.current;
    const root = sessionsScrollRef.current;
    if (!sentinel || !root) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) return;
        setSessionListLimit((current) => current + LIST_PAGE_SIZE);
      },
      { root, rootMargin: "240px" },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [tab, hasMoreSessions, shownUngroupedCount]);

  useEffect(() => {
    setSessionFolders(loadSessionFolders(cwd));
    setPinnedSessionsCollapsed(loadPinnedSessionsCollapsed(cwd));
    setReminderSessionsCollapsed(loadReminderSessionsCollapsed(cwd));
    setRenamingFolderId(null);
    setFolderMenu(null);
    setSessionDrop(null);
    pendingFolderSessionIds.current.clear();
  }, [cwd]);

  useEffect(
    () =>
      subscribeSessionFolders(cwd, () => {
        setSessionFolders(loadSessionFolders(cwd));
      }),
    [cwd],
  );

  useEffect(() => {
    if (pending || status === "error") return;
    if (remoteProject && !remote.loaded) return;
    const known = new Set(projectSessions.map((session) => session.id));
    const completedFolderSessions = new Map<string, string>();
    for (const session of remoteProject ? [] : openSessions) known.add(session.id);
    if (activeListedSessionId) known.add(activeListedSessionId);
    if (remoteProject && activeSessionId) known.add(activeSessionId);
    if (remoteProject) {
      for (const folder of sessionFolders) {
        for (const shellId of folder.sessionIds) {
          const hostId = remoteSessionFor(shellId);
          if (hostId && known.has(hostId)) completedFolderSessions.set(shellId, hostId);
        }
      }
    }
    for (const id of pendingFolderSessionIds.current) {
      known.add(id);
      const hostId = remoteProject ? remoteSessionFor(id) : undefined;
      if (hostId && known.has(hostId)) {
        completedFolderSessions.set(id, hostId);
        pendingFolderSessionIds.current.delete(id);
        continue;
      }
      if (
        projectSessions.some((session) => session.id === id) ||
        openSessions.some((session) => session.id === id)
      ) {
        pendingFolderSessionIds.current.delete(id);
      }
    }
    setSessionFolders((current) => {
      const migrated = completedFolderSessions.size
        ? current.map((folder) => ({
            ...folder,
            sessionIds: folder.sessionIds.map((id) => completedFolderSessions.get(id) ?? id),
          }))
        : current;
      const next = pruneSessionFolders(migrated, known);
      if (next === current) return current;
      saveSessionFolders(cwd, next);
      return next;
    });
  }, [activeListedSessionId, activeSessionId, cwd, openSessions, pending, projectSessions, remoteProject, remote.loaded, sessionFolders, status]);

  useEffect(() => {
    if (tab !== "sessions") return;
    const id = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(id);
  }, [tab]);

  useEffect(() => {
    if (tab !== "sessions") {
      setFilterMenu(null);
      setSearchQuery("");
    }
  }, [tab]);

  useEffect(() => {
    if (!sessionMenu && !folderMenu && !filterMenu) return;
    const onScroll = () => {
      closeSessionMenu();
      setFolderMenu(null);
      setFilterMenu(null);
    };
    const scrollParent = sessionsScrollRef.current ?? window;
    scrollParent.addEventListener("scroll", onScroll, true);
    return () => scrollParent.removeEventListener("scroll", onScroll, true);
  }, [sessionMenu, folderMenu, filterMenu]);

  useEffect(() => {
    if (selectedSessionIds.size === 0) return;
    const clear = () => {
      selectionAnchorRef.current = null;
      contextSelectionRef.current = false;
      setSelectedSessionIds(new Set());
      setSessionMenu(null);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      clear();
    };
    // A pointer landing off the cards drops the selection; a menu acting on
    // it stays open, and the cards handle their own clicks.
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      const el = target instanceof Element ? target : null;
      if (el?.closest("[data-session-card],[data-popover-side]")) return;
      clear();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown);
    };
  }, [selectedSessionIds.size]);

  const commitSessionFolders = (next: SessionFolder[]) => {
    setSessionFolders(next);
    saveSessionFolders(cwd, next);
  };

  const onNewInFolder = (folderId: string) => {
    const sessionId = onNew?.();
    if (!sessionId) return;
    pendingFolderSessionIds.current.add(sessionId);
    setSearchQuery("");
    setSessionFolders((current) => {
      const next = setFolderCollapsed(
        addSessionToFolder(current, folderId, sessionId),
        folderId,
        false,
      );
      saveSessionFolders(cwd, next);
      return next;
    });
  };

  const menuSessionIds = sessionMenu
    ? orderedSessionActionIds(
        sessionMenu.sessionId,
        selectedSessionIds,
        sessionNavigationIds,
      )
    : [];
  const menuSessions = menuSessionIds.flatMap((sessionId) => {
    const session = listedSessions.find((entry) => entry.id === sessionId);
    return session ? [session] : [];
  });
  const multipleMenuSessions = menuSessionIds.length > 1;
  const menuReminderTimes = [
    ...new Set(
      reminders
        .filter((reminder) => menuSessionIds.includes(reminder.sessionId))
        .map((reminder) => reminder.dueAt),
    ),
  ];
  const allMenuSessionsPinned =
    menuSessions.length > 0 && menuSessions.every((session) => session.pinned);
  const allMenuSessionsArchived =
    menuSessions.length > 0 &&
    menuSessions.every((session) => session.archived);
  const menuSessionFolder =
    menuSessionIds.length === 1
      ? folderContaining(sessionFolders, menuSessionIds[0])
      : undefined;
  const anyMenuSessionFoldered = menuSessionIds.some((sessionId) =>
    sessionFolders.some((folder) => folder.sessionIds.includes(sessionId)),
  );
  const canRemoveMenuSessionsFromFolders = multipleMenuSessions
    ? anyMenuSessionFoldered
    : !!menuSessionFolder;
  const menuFolder = folderMenu
    ? sessionFolders.find((folder) => folder.id === folderMenu.folderId)
    : undefined;
  const folderMenuItems: ExplorerMenuItem[] = [
    { kind: "item", id: "rename", label: "Rename", shortcut: "F2" },
    { kind: "sep" },
    { kind: "item", id: "ungroup", label: "Ungroup" },
  ];
  const sessionMenuItems: ExplorerMenuItem[] = [
    ...(onCancelReminders && menuReminderTimes.length > 0
      ? [
          {
            kind: "item" as const,
            id: "reminder:cancel",
            label: "Cancel reminder",
            description:
              menuReminderTimes.length === 1
                ? formatReminderTime(menuReminderTimes[0])
                : "Multiple reminder times",
          },
          { kind: "sep" as const },
        ]
      : []),
    ...(onPinSession || onPinSessions
      ? [
          {
            kind: "item" as const,
            id: "pin",
            label: allMenuSessionsPinned ? "Unpin" : "Pin",
          },
        ]
      : []),
    ...(!multipleMenuSessions && onRenameSession
      ? [
          {
            kind: "item" as const,
            id: "rename",
            label: "Rename",
            shortcut: "F2",
          },
        ]
      : []),
    ...(!multipleMenuSessions
      ? [
          {
            kind: "item" as const,
            id: "copy-session-id",
            label: "Copy session ID",
            submenu: [
              {
                kind: "item" as const,
                id: "copy-harness-session-id",
                label: "Harness session ID",
                disabled: !menuSessions[0]?.providerSessionId,
              },
              {
                kind: "item" as const,
                id: "copy-monocode-session-id",
                label: "MonoCode session ID",
              },
            ],
          },
        ]
      : []),
    ...(!multipleMenuSessions && onSetSessionLinkedWorkItem
      ? [
          {
            kind: "item" as const,
            id: "link-work-item",
            label: menuSessions[0]?.linkedWorkItem
              ? "Edit GitHub issue or PR link…"
              : "Link GitHub issue or PR…",
          },
        ]
      : []),
    ...(!multipleMenuSessions &&
    onMoveSessionToWorktree &&
    projectWorktrees &&
    projectWorktrees.length > 1
      ? [
          {
            kind: "item" as const,
            id: "move-worktree",
            label: "Move to worktree",
            disabled:
              !menuSessions[0] || listedBusySessionIds.has(menuSessions[0].id),
            submenu: projectWorktrees.map((tree, index) => {
              const current =
                !!menuSessions[0] &&
                pathKey(menuSessions[0].worktreeCwd || menuSessions[0].cwd) ===
                  pathKey(tree.path);
              return {
                kind: "item" as const,
                id: `move-worktree:${index}`,
                label: tree.branch ?? "Detached worktree",
                description: tree.isMain ? "Main checkout" : tree.path,
                checked: current,
                disabled: current,
              };
            }),
          },
        ]
      : []),
    {
      kind: "item",
      id: "reminder",
      label: "Remind me",
      disabled: !onSetReminders,
      submenu: sessionReminderPresets(),
    },
    { kind: "sep" as const },
    { kind: "item" as const, id: "folder-new", label: "New folder" },
    ...(sessionFolders.length > 0 ? [{ kind: "sep" as const }] : []),
    ...sessionFolders.map((folder) => ({
      kind: "item" as const,
      id: `folder-add:${folder.id}`,
      label: `Add to ${folder.name}`,
      checked:
        menuSessionIds.length > 0 &&
        menuSessionIds.every((sessionId) =>
          folder.sessionIds.includes(sessionId),
        ),
    })),
    ...(canRemoveMenuSessionsFromFolders
      ? [
          {
            kind: "item" as const,
            id: "folder-remove",
            label: multipleMenuSessions
              ? "Remove from folders"
              : "Remove from folder",
          },
        ]
      : []),
    ...(onArchiveSession ||
    onArchiveSessions ||
    onDeleteSession ||
    onDeleteSessions
      ? [
          { kind: "sep" as const },
          ...(onArchiveSession || onArchiveSessions
            ? [
                {
                  kind: "item" as const,
                  id: "archive",
                  label: allMenuSessionsArchived ? "Unarchive" : "Archive",
                },
              ]
            : []),
          ...(onDeleteSession || onDeleteSessions
            ? [
                {
                  kind: "item" as const,
                  id: "delete",
                  label: "Delete",
                  shortcut: "⌫",
                  danger: true,
                },
              ]
            : []),
        ]
      : []),
  ];

  const onSessionContextMenu = (
    sessionId: string,
    e: ReactMouseEvent<HTMLDivElement>,
  ) => {
    e.preventDefault();
    e.stopPropagation();
    contextSelectionRef.current = !selectedSessionIds.has(sessionId);
    if (contextSelectionRef.current) {
      setSelectedSessionIds(new Set([sessionId]));
    }
    setFilterMenu(null);
    setFolderMenu(null);
    setSessionMenu({ x: e.clientX, y: e.clientY, sessionId });
  };

  const closeSessionMenu = () => {
    setSessionMenu(null);
    if (!contextSelectionRef.current) return;
    contextSelectionRef.current = false;
    selectionAnchorRef.current = null;
    setSelectedSessionIds(new Set());
  };

  const onFolderContextMenu = (
    folderId: string,
    e: ReactMouseEvent<HTMLButtonElement>,
  ) => {
    e.preventDefault();
    e.stopPropagation();
    setFilterMenu(null);
    setSessionMenu(null);
    setFolderMenu({ x: e.clientX, y: e.clientY, folderId });
  };

  const onSessionMenuPick = (id: string) => {
    if (!sessionMenu) return;
    const sessionId = sessionMenu.sessionId;
    const sessionIds = menuSessionIds;
    const providerSessionId = menuSessions[0]?.providerSessionId;
    const archived = allMenuSessionsArchived;
    const pinned = allMenuSessionsPinned;
    closeSessionMenu();
    if (id === "reminder:cancel") {
      onCancelReminders?.(sessionIds);
      return;
    }
    if (id.startsWith("reminder:")) {
      const dueAt = reminderTime(id);
      if (dueAt != null) {
        setReminderSessionsCollapsed(false);
        saveReminderSessionsCollapsed(cwd, false);
        onSetReminders?.(sessionIds, dueAt);
      }
      return;
    }
    if (id === "pin") {
      if (sessionIds.length > 1 && onPinSessions) {
        onPinSessions(sessionIds, !pinned);
      } else {
        for (const id of sessionIds) onPinSession?.(id, !pinned);
      }
      return;
    }
    if (id === "rename") {
      setRenamingSessionId(sessionId);
      return;
    }
    if (id === "copy-harness-session-id" || id === "copy-monocode-session-id") {
      const value =
        id === "copy-harness-session-id" ? providerSessionId : sessionId;
      if (value) {
        void copyText(value).catch((error) => {
          console.error("Failed to copy session ID:", error);
        });
      }
      return;
    }
    if (id.startsWith("move-worktree:")) {
      const tree = projectWorktrees?.[Number(id.slice("move-worktree:".length))];
      if (tree) onMoveSessionToWorktree?.(sessionId, tree);
      return;
    }
    if (id === "link-work-item") {
      setLinkingSession(menuSessions[0] ?? null);
      return;
    }
    if (id === "folder-new") {
      const { folders, id: createdId } = createFolderWithSessions(
        sessionFolders,
        sessionIds,
      );
      if (!createdId) return;
      commitSessionFolders(folders);
      setRenamingFolderId(createdId);
      return;
    }
    if (id.startsWith("folder-add:")) {
      const folderId = id.slice("folder-add:".length);
      const folders = sessionIds.reduce(
        (current, id) => addSessionToFolder(current, folderId, id),
        sessionFolders,
      );
      commitSessionFolders(setFolderCollapsed(folders, folderId, false));
      return;
    }
    if (id === "folder-remove") {
      commitSessionFolders(
        sessionIds.reduce(
          (current, id) => removeSessionFromFolder(current, id),
          sessionFolders,
        ),
      );
      return;
    }
    if (id === "archive") {
      if (sessionIds.length > 1 && onArchiveSessions) {
        onArchiveSessions(sessionIds, !archived);
      } else {
        for (const id of sessionIds) onArchiveSession?.(id, !archived);
      }
      return;
    }
    if (id === "delete") {
      if (sessionIds.length > 1 && onDeleteSessions) {
        onDeleteSessions(sessionIds);
      } else {
        for (const id of sessionIds) onDeleteSession?.(id);
      }
    }
  };

  const onFolderMenuPick = (id: string) => {
    if (!folderMenu) return;
    const folderId = folderMenu.folderId;
    setFolderMenu(null);
    if (id === "rename") {
      setRenamingFolderId(folderId);
      return;
    }
    if (id === "ungroup") {
      commitSessionFolders(dissolveFolder(sessionFolders, folderId));
    }
  };

  const onFolderColorChange = (colorIndex: number | null) => {
    if (!folderMenu) return;
    commitSessionFolders(
      setFolderColor(sessionFolders, folderMenu.folderId, colorIndex),
    );
  };

  const onFolderCustomColorChange = (color: string) => {
    if (!folderMenu) return;
    commitSessionFolders(
      setFolderCustomColor(sessionFolders, folderMenu.folderId, color),
    );
  };

  const onSessionListDrop = (
    draggedId: string,
    target: SessionListDropTarget,
  ) => {
    const { folders, createdId } = applySessionListDrop(
      sessionFolders,
      draggedId,
      target,
    );
    if (folders === sessionFolders) return;
    commitSessionFolders(folders);
    if (createdId) setRenamingFolderId(createdId);
  };

  const isSessionDrop = (kind: "folder" | "session", id: string) =>
    sessionDrop?.kind === kind && sessionDrop.id === id;

  const onSessionCardSelect = (
    sessionId: string,
    event: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean },
  ) => {
    contextSelectionRef.current = false;
    setSessionMenu(null);
    if (event.shiftKey) {
      const visibleIds = sessionListNavigationIds(
        sessionListEntries,
        searchNarrowed,
      );
      if (
        selectionAnchorRef.current &&
        !visibleIds.includes(selectionAnchorRef.current)
      ) {
        selectionAnchorRef.current = null;
      }
      const anchor = selectionAnchorRef.current ?? activeListedSessionId ?? sessionId;
      const start = visibleIds.indexOf(anchor);
      const end = visibleIds.indexOf(sessionId);
      const range =
        start < 0 || end < 0
          ? [sessionId]
          : visibleIds.slice(Math.min(start, end), Math.max(start, end) + 1);
      selectionAnchorRef.current = start < 0 ? sessionId : anchor;
      setSelectedSessionIds(
        (current) =>
          new Set(
            event.ctrlKey || event.metaKey ? [...current, ...range] : range,
          ),
      );
      return;
    }
    selectionAnchorRef.current = sessionId;
    if (event.ctrlKey || event.metaKey) {
      const next = toggleSessionSelection(selectedSessionIds, sessionId);
      if (next.size === 0) selectionAnchorRef.current = null;
      setSelectedSessionIds(next);
      return;
    }
    setSelectedSessionIds(new Set());
    setDrawerOpen(false);
    onSelectSession(sessionId);
  };

  // Cards are memoized. Their handlers go through one stable set that calls
  // the latest version, so a sidebar render no longer re-renders every card.
  const cardHandlers = useRef({
    onSessionCardSelect,
    onOpenInboxItem,
    onPrefetchSession,
    onPlaceSessionOnPane,
    onSessionListDrop,
    onSessionContextMenu,
    onArchiveSession,
    setRenamingSessionId,
    onDeleteSession,
  });
  cardHandlers.current = {
    onSessionCardSelect,
    onOpenInboxItem,
    onPrefetchSession,
    onPlaceSessionOnPane,
    onSessionListDrop,
    onSessionContextMenu,
    onArchiveSession,
    setRenamingSessionId,
    onDeleteSession,
  };
  const cardActions = useMemo(
    () => ({
      select: (
        sessionId: string,
        event: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean },
      ) => cardHandlers.current.onSessionCardSelect(sessionId, event),
      openWorkItem: (item: LinkedWorkItem, sessionId: string) =>
        cardHandlers.current.onOpenInboxItem?.(item, sessionId),
      prefetch: (sessionId: string) =>
        cardHandlers.current.onPrefetchSession?.(sessionId),
      placeOnPane: (sessionId: string, targetId: string, edge: PaneEdge) =>
        cardHandlers.current.onPlaceSessionOnPane?.(sessionId, targetId, edge),
      listDrop: (draggedId: string, target: SessionListDropTarget) =>
        cardHandlers.current.onSessionListDrop(draggedId, target),
      contextMenu: (sessionId: string, e: ReactMouseEvent<HTMLDivElement>) =>
        cardHandlers.current.onSessionContextMenu(sessionId, e),
      archive: (sessionId: string, archived: boolean) =>
        cardHandlers.current.onArchiveSession?.(sessionId, archived),
      rename: (sessionId: string) =>
        cardHandlers.current.setRenamingSessionId(sessionId),
      delete: (sessionId: string) =>
        cardHandlers.current.onDeleteSession?.(sessionId),
    }),
    [],
  );

  const sessionInsertMotion = useRef<SessionInsertMotion>({
    cwd: "",
    seen: new Set(),
  });
  // Runs after the rows' mount effects: a project's first paint never animates,
  // and rows that mount later (drawer opened, folder expanded) are not new.
  useLayoutEffect(() => {
    const motion = sessionInsertMotion.current;
    motion.cwd = cwd;
    for (const session of listedSessions) motion.seen.add(session.id);
  });

  const renderSessionCard = (session: SessionSummary, compact = false) =>
    renamingSessionId === session.id && onRenameSession ? (
      <SessionRenameRow
        session={session}
        isActive={session.id === activeListedSessionId}
        needsApproval={listedApprovalSessionIds.has(session.id)}
        onCommit={(title) => {
          onRenameSession(session.id, title);
          setRenamingSessionId(null);
        }}
        onCancel={() => setRenamingSessionId(null)}
      />
    ) : (
      <SessionCard
        session={session}
        isActive={session.id === activeListedSessionId}
        isSelected={selectedSessionIds.has(session.id)}
        busy={listedBusySessionIds.has(session.id)}
        done={unseenFinishedIds.has(session.id)}
        linkedUpdate={linkedSessionUpdateIds.has(session.id)}
        needsApproval={listedApprovalSessionIds.has(session.id)}
        dropTarget={isSessionDrop("session", session.id)}
        compact={compact}
        now={now}
        onSelect={cardActions.select}
        onOpenWorkItem={onOpenInboxItem && !remoteProject ? cardActions.openWorkItem : undefined}
        onPrefetch={onPrefetchSession ? cardActions.prefetch : undefined}
        onPlaceOnPane={
          onPlaceSessionOnPane ? cardActions.placeOnPane : undefined
        }
        onListDrop={
          reminderIds.has(session.id) ? undefined : cardActions.listDrop
        }
        onListDropTargetChange={setSessionDrop}
        onContextMenu={cardActions.contextMenu}
        onArchive={onArchiveSession ? cardActions.archive : undefined}
        onRename={onRenameSession ? cardActions.rename : undefined}
        onDelete={onDeleteSession ? cardActions.delete : undefined}
      />
    );

  const onSessionFiltersChange = (next: SessionSidebarFilters) => {
    setSessionFilters(next);
    saveSessionSidebarFilters(next);
  };

  const onFilterButtonClick = (event: ReactMouseEvent<HTMLButtonElement>) => {
    if (filterMenu) {
      setFilterMenu(null);
      return;
    }
    const rect = event.currentTarget.getBoundingClientRect();
    setSessionMenu(null);
    setFolderMenu(null);
    setFilterMenu({
      x: rect.right - 228,
      y: rect.bottom + 2,
    });
  };

  const sessionSearchInput = (
    <input
      ref={searchInputRef}
      type="text"
      value={searchQuery}
      placeholder="Search conversations..."
      aria-label="Search conversations"
      spellCheck={false}
      autoComplete="off"
      autoCorrect="off"
      autoCapitalize="off"
      onChange={(event) => setSearchQuery(event.target.value)}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        if (searchQuery) {
          setSearchQuery("");
        }
      }}
      className="h-full w-full min-w-0 rounded-md bg-transparent py-0 pl-7 pr-2 text-[12px] text-content outline-none placeholder:text-content/35"
    />
  );

  const onTabPick = (itemId: SidebarTab) => {
    onTabChange(itemId);
  };

  const onCompactTabPick = (itemId: SidebarTab) => {
    if (drawerMode) {
      setDrawerOpen(!(drawerVisible && tab === itemId));
    }
    onTabChange(itemId);
  };

  const changeAdditions = changeStats?.additions ?? 0;
  const changeDeletions = changeStats?.deletions ?? 0;
  const hasChanges = (changeStats?.files ?? 0) > 0;
  const hasChangeStats = changeAdditions > 0 || changeDeletions > 0;
  const changesLabel = hasChangeStats
    ? [
        "Changes",
        changeAdditions > 0 ? `+${changeAdditions}` : "",
        changeDeletions > 0 ? `-${changeDeletions}` : "",
      ]
        .filter(Boolean)
        .join(" ")
    : "Changes";

  const workspaceTabItems = visibleTabs.map((itemId) => {
    const active = tab === itemId;
    const isChangesTab = itemId === "changes";
    return (
      <div
        key={itemId}
        ref={(el) => sortable.setItemRef(itemId, el)}
        className="reorder-item workspace-tab relative flex min-w-0 flex-1 touch-none items-stretch"
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          sortable.onItemPointerDown(itemId, event);
        }}
      >
        <button
          type="button"
          role="tab"
          aria-selected={active}
          aria-label={isChangesTab ? changesLabel : undefined}
          data-tauri-drag-region="false"
          onClick={() => {
            if (sortable.consumeClick()) return;
            onTabPick(itemId);
          }}
          className={`flex h-6 min-w-0 flex-1 items-center justify-center self-center rounded-md px-2 text-[12px] leading-none ${
            active ? "bg-selection text-content" : "text-content/50"
          }`}
        >
          {isChangesTab && hasChangeStats ? (
            <DiffStat additions={changeAdditions} deletions={changeDeletions} />
          ) : (
            <span className="block truncate leading-label">
              {TAB_LABELS[itemId]}
            </span>
          )}
        </button>
      </div>
    );
  });

  const workspaceHeader = (
    <div
      className="flex h-10 shrink-0 select-none items-center gap-1 border-b border-stroke pl-3 pr-1.5"
      data-tauri-drag-region="deep"
    >
      <div className="flex min-w-0 flex-1 items-center">
        {!remoteProject && cwd && cwd !== "~" ? (
          <SidebarWorktreeSwitcher
            cwd={cwd}
            tabStats={worktreeTabStats}
            onSelect={onSelectWorkspace}
            pending={workspaceSwitchPending}
            switchError={workspaceSwitchError}
          />
        ) : (
          <span className="min-w-0 truncate text-sm font-medium leading-tight">
            Workspace
          </span>
        )}
      </div>
      <WorkspaceTitleActions onSearch={onGoToFile} onNew={onNew} />
    </div>
  );

  const sidebarContent = (
    <aside
      ref={resize.setPaneRef}
      className="body-glass relative flex h-full min-h-0 shrink-0 flex-col border-r border-stroke"
    >
      {railVisible ? (
        <>
          {workspaceHeader}
          <div
            role="tablist"
            aria-label="Workspace"
            className="flex h-9 shrink-0 items-center gap-px border-b border-stroke px-2"
          >
            {workspaceTabItems}
          </div>
        </>
      ) : (
        <>
          {titleBarAbove ? null : (
            <div
              className="flex h-10 shrink-0 select-none items-center border-b border-stroke pr-1.5"
              data-tauri-drag-region="deep"
            >
              {IS_MAC ? <div className="w-[78px] shrink-0" /> : null}
              <DevModeSlot />
              <TabVisitNav
                canGoBack={canGoBack}
                canGoForward={canGoForward}
                onGoBack={onGoBack}
                onGoForward={onGoForward}
                onTogglePanel={
                  compactProjectRail ? undefined : onToggleProjectRail
                }
              />
            </div>
          )}
          {compactRailVisible ? workspaceHeader : null}
          {onSelectProject && !compactRailVisible ? (
            <SidebarProjectPicker
              cwd={cwd}
              recents={recents}
              busy={projectPathBusy(busyProjectPaths, cwd)}
              onSelectProject={onSelectProject}
              onOpenProject={onOpenProject}
              onRemoveProject={onRemoveProject}
              onNew={onNew}
              onSearch={onSearch}
              onOpenInbox={onOpenInbox}
              onOpenNotificationSettings={onOpenNotificationSettings}
              onOpenNotes={notesEnabled ? onOpenNotes : undefined}
              onOpenAutomations={onOpenAutomations}
              searchActive={searchActive}
              inboxActive={inboxActive}
              notesActive={notesActive}
              automationsActive={automationsActive}
              inboxUnseen={inboxUnseen}
            />
          ) : null}
          {!compactRailVisible ? (
            <div
              role="tablist"
              aria-label="Workspace"
              className="flex h-9 shrink-0 items-center gap-px overflow-visible border-b border-stroke px-2"
            >
              {workspaceTabItems}
            </div>
          ) : null}
        </>
      )}
      <>
        <div
          className={`flex min-h-0 flex-1 flex-col overflow-hidden ${
            tab === "files" ? "" : "hidden"
          }`}
        >
          {filesSearchOpen ? (
            <ProjectSearch
              cwd={gitRoot}
              focusToken={searchFocusToken}
              onOpenFile={onOpenFile}
              onClose={() => onFilesSearchOpenChange(false)}
            />
          ) : cwd && cwd !== "~" ? (
            <div className="flex min-h-0 flex-1 flex-col">
              {explorer.current ? (
                <FileTree
                  key={explorer.current.cwd}
                  cwd={explorer.current.cwd}
                  rootLabel={explorer.current.rootLabel}
                  onOpenFile={onOpenFile}
                  onOpenTerminal={remoteProject ? undefined : onOpenTerminal}
                  onFileMoved={onFileMoved}
                  onFileDeleted={onFileDeleted}
                  onSearch={onOpenFilesSearch}
                  gitStatuses={gitStatuses}
                />
              ) : null}
            </div>
          ) : (
            <p className="px-3 py-2 text-[12px] text-content/50">
              No project folder
            </p>
          )}
        </div>
        {tab === "sessions" && cwd && cwd !== "~" ? (
          <div className="flex h-9 shrink-0 items-center gap-1 border-b border-stroke px-2">
            <div className="relative flex h-7 min-w-0 flex-1 items-center">
              <Search className="pointer-events-none absolute left-2 size-3 shrink-0 opacity-50" />
              {sessionSearchInput}
            </div>
            <SessionsHeaderButton
              label="Filter sessions"
              active={filtersActive}
              open={!!filterMenu}
              hasPopup
              onClick={onFilterButtonClick}
            >
              <ListFilter className="size-3" strokeWidth={1.75} />
            </SessionsHeaderButton>
          </div>
        ) : null}
        <div
          ref={(el) => {
            sessionsLock(el);
            sessionsScrollRef.current = el;
          }}
          className={`sidebar-session-scroll min-h-0 flex-1 overflow-y-auto overscroll-none ${
            tab === "sessions" ? "" : "hidden"
          }`}
        >
          {!cwd || cwd === "~" ? (
            <p className="px-3 py-2 text-[12px] text-content/50">
              No project folder
            </p>
          ) : (
            <div>
              {/*
              A project's first load stays deliberately blank. The listing is
              served from a covering index and resolves within a frame or two,
              so a placeholder only ever flashed — reading as a glitch rather
              than as progress. This is checked before the empty state so that
              cannot claim "No sessions yet" before the rows have landed.
            */}
              {pendingFirstLoad ? null : status === "error" &&
                projectSessions.length === 0 ? (
                <p className="px-3 py-2 text-[12px] text-content/50">
                  Couldn’t load sessions
                </p>
              ) : visibleSessions.length === 0 ? (
                // A narrowed-down result is a transient answer to what the user
                // just typed, so it stays a quiet line of text. Only the genuine
                // "this project has nothing in it" case earns the illustration.
                narrowedByUser ? (
                  <p className="px-3 py-2 text-[12px] text-content/50">
                    {searchNarrowed
                      ? "No matching sessions"
                      : "No sessions match these filters"}
                  </p>
                ) : remoteProject && !remote.machine ? (
                  <p className="px-3 py-2 text-[12px] text-content/45">
                    This project’s machine isn’t connected on this computer.
                  </p>
                ) : (
                  <SessionsEmpty message="Sessions you start will show up here" />
                )
              ) : (
                <ul data-session-list className="flex flex-col gap-0.5 p-1.5 pb-10">
                  {sessionListEntries.map((entry, index) => {
                    if (entry.kind === "pinned" || entry.kind === "reminders") {
                      const isReminders = entry.kind === "reminders";
                      const expanded = searchNarrowed || !entry.collapsed;
                      const beforeUngrouped =
                        sessionListEntries[index + 1]?.kind === "session";
                      return (
                        <li
                          key={`${entry.kind}-sessions`}
                          data-pinned-sessions={isReminders ? undefined : ""}
                          data-reminder-sessions={isReminders ? "" : undefined}
                          className={`relative ${
                            expanded || beforeUngrouped ? "mb-1.5" : ""
                          }`}
                        >
                          <div className="overflow-hidden rounded-md bg-content/5">
                            <FolderRow
                              folder={
                                isReminders
                                  ? {
                                      name: "Reminders",
                                      customColor: REMINDERS_COLOR,
                                    }
                                  : { name: "Pinned" }
                              }
                              sessions={entry.sessions}
                              expanded={expanded}
                              dropTarget={false}
                              busy={entry.sessions.some((session) =>
                                listedBusySessionIds.has(session.id),
                              )}
                              done={entry.sessions.some((session) =>
                                unseenFinishedIds.has(session.id),
                              )}
                              needsApproval={entry.sessions.some((session) =>
                                listedApprovalSessionIds.has(session.id),
                              )}
                              groupIcon={
                                isReminders ? (
                                  <Clock
                                    className="size-3.5"
                                    strokeWidth={1.75}
                                  />
                                ) : (
                                  <Pin
                                    className="size-3.5 text-content"
                                    strokeWidth={1.75}
                                  />
                                )
                              }
                              onToggle={() => {
                                if (searchNarrowed) return;
                                const collapsed = !entry.collapsed;
                                if (isReminders) {
                                  setReminderSessionsCollapsed(collapsed);
                                  saveReminderSessionsCollapsed(cwd, collapsed);
                                  return;
                                }
                                setPinnedSessionsCollapsed(collapsed);
                                savePinnedSessionsCollapsed(cwd, collapsed);
                              }}
                            />
                            {expanded ? (
                              <ul className="flex flex-col gap-px p-1">
                                {entry.sessions.map((session) => (
                                  <SessionListItem
                                    key={session.id}
                                    session={session}
                                    cwd={cwd}
                                    motion={sessionInsertMotion}
                                  >
                                    {renderSessionCard(session, true)}
                                  </SessionListItem>
                                ))}
                              </ul>
                            ) : null}
                          </div>
                        </li>
                      );
                    }
                    if (entry.kind === "folder") {
                      const expanded =
                        searchNarrowed || !entry.folder.collapsed;
                      const shellFill = folderShellFill(
                        entry.folder.colorIndex,
                        entry.folder.customColor,
                      );
                      const folderIndex = visibleFolderIds.indexOf(
                        entry.folder.id,
                      );
                      const beforeUngrouped =
                        sessionListEntries[index + 1]?.kind === "session";
                      const draggingFolder =
                        folderSortable.draggingId === entry.folder.id;
                      const showFolderDropStart =
                        folderSortable.draggingId &&
                        folderSortable.toIndex === folderIndex &&
                        folderSortable.fromIndex !== null &&
                        folderSortable.toIndex < folderSortable.fromIndex;
                      const showFolderDropEnd =
                        folderSortable.draggingId &&
                        folderSortable.toIndex === folderIndex &&
                        folderSortable.fromIndex !== null &&
                        folderSortable.toIndex > folderSortable.fromIndex;
                      return (
                        <li
                          key={entry.folder.id}
                          ref={(el) =>
                            folderSortable.setItemRef(entry.folder.id, el)
                          }
                          data-session-folder={entry.folder.id}
                          className={`relative ${
                            expanded || beforeUngrouped ? "mb-1.5" : ""
                          } ${draggingFolder ? "opacity-40" : ""}`}
                        >
                          {showFolderDropStart ? (
                            <div className="pointer-events-none absolute inset-x-1 top-0 z-20 h-0.5 rounded-full bg-accent" />
                          ) : null}
                          {showFolderDropEnd ? (
                            <div className="pointer-events-none absolute inset-x-1 bottom-0 z-20 h-0.5 rounded-full bg-accent" />
                          ) : null}
                          <div
                            className={`overflow-hidden rounded-md ${
                              shellFill ? "" : "bg-content/5"
                            }`}
                            style={
                              shellFill ? { background: shellFill } : undefined
                            }
                          >
                            {renamingFolderId === entry.folder.id ? (
                              <FolderRenameRow
                                folder={entry.folder}
                                memberCount={entry.sessions.length}
                                dropTarget={isSessionDrop(
                                  "folder",
                                  entry.folder.id,
                                )}
                                onCommit={(name) => {
                                  commitSessionFolders(
                                    renameFolder(
                                      sessionFolders,
                                      entry.folder.id,
                                      name,
                                    ),
                                  );
                                  setRenamingFolderId(null);
                                }}
                                onCancel={() => setRenamingFolderId(null)}
                              />
                            ) : (
                              <FolderRow
                                folder={entry.folder}
                                sessions={entry.sessions}
                                expanded={expanded}
                                dropTarget={isSessionDrop(
                                  "folder",
                                  entry.folder.id,
                                )}
                                busy={entry.sessions.some((session) =>
                                  listedBusySessionIds.has(session.id),
                                )}
                                done={entry.sessions.some((session) =>
                                  unseenFinishedIds.has(session.id),
                                )}
                                needsApproval={entry.sessions.some((session) =>
                                  listedApprovalSessionIds.has(session.id),
                                )}
                                onPointerDown={(event) =>
                                  folderSortable.onItemPointerDown(
                                    entry.folder.id,
                                    event,
                                  )
                                }
                                onToggle={() => {
                                  if (folderSortable.consumeClick()) return;
                                  if (searchNarrowed) return;
                                  commitSessionFolders(
                                    setFolderCollapsed(
                                      sessionFolders,
                                      entry.folder.id,
                                      !entry.folder.collapsed,
                                    ),
                                  );
                                }}
                                onContextMenu={(event) =>
                                  onFolderContextMenu(entry.folder.id, event)
                                }
                                onRename={() =>
                                  setRenamingFolderId(entry.folder.id)
                                }
                              />
                            )}
                            {expanded ? (
                              <>
                                <ul className="flex flex-col gap-px p-1">
                                  {entry.sessions.map((session) => (
                                    <SessionListItem
                                      key={session.id}
                                      session={session}
                                      cwd={cwd}
                                      motion={sessionInsertMotion}
                                    >
                                      {renderSessionCard(session, true)}
                                    </SessionListItem>
                                  ))}
                                </ul>
                                {onNew ? (
                                  <div className="border-t border-stroke p-1">
                                    <button
                                      type="button"
                                      data-no-drag
                                      data-tauri-drag-region="false"
                                      title="New session"
                                      aria-label="New session"
                                      onClick={() =>
                                        onNewInFolder(entry.folder.id)
                                      }
                                      className="relative flex w-full items-center gap-1 rounded-md border border-transparent px-2.5 py-1.5 text-left text-content/45 hover:bg-content/10 hover:text-content"
                                    >
                                      <Plus
                                        className="size-3 shrink-0"
                                        strokeWidth={1.75}
                                      />
                                      <span className="text-[13px] font-semibold leading-snug">
                                        New session
                                      </span>
                                    </button>
                                  </div>
                                ) : null}
                              </>
                            ) : null}
                          </div>
                        </li>
                      );
                    }
                    return (
                      <SessionListItem
                        key={entry.session.id}
                        session={entry.session}
                        cwd={cwd}
                        motion={sessionInsertMotion}
                      >
                        {renderSessionCard(entry.session)}
                      </SessionListItem>
                    );
                  })}
                  {hasMoreSessions ? (
                    <li
                      ref={loadMoreRef}
                      aria-hidden
                      className="h-px list-none"
                    />
                  ) : null}
                </ul>
              )}
            </div>
          )}
        </div>
        {tab === "changes" ? (
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
            <SourceControl
                cwd={gitRoot}
                enabled={panelOpen}
                textHarness={textHarness}
                selectedPath={selectedDiffPath}
                selectedKind={selectedDiffKind}
                selectedSha={selectedCommitSha}
                onOpenFile={
                  onOpenDiff ??
                  ((path) => onOpenFile(path, undefined, { exact: true }))
                }
                onOpenAllChanges={onOpenAllChanges ?? (() => {})}
                onOpenCommit={onOpenCommit ?? (() => {})}
              />
          </div>
        ) : null}
        {showSidebarFooter ? (
          <>
            <LiveAgentsPreview
              agents={liveAgents}
              activeSessionId={activeSessionId}
              onSelect={onSelectAgent}
              bottomSpacing={compactRailVisible}
            />
            <SidebarUpdateFooter
              update={updateNotice}
              onOpenWhatsNew={onOpenWhatsNew}
              onDismissUpdate={onDismissUpdate}
            />
            <div className="flex shrink-0 flex-col gap-px p-2 empty:hidden">
              <GithubStarPrompt />
              {!compactProjectRail ? (
                <RailAction
                  label="Settings"
                  icon={Settings}
                  onClick={onOpenSettings}
                  shortcut={`${MOD},`}
                  ariaLabel={`Settings (${MOD},)`}
                />
              ) : null}
            </div>
          </>
        ) : null}
      </>
      {sessionMenu ? (
        <ExplorerMenu
          x={sessionMenu.x}
          y={sessionMenu.y}
          items={sessionMenuItems}
          ariaLabel={
            multipleMenuSessions
              ? `${menuSessionIds.length} selected session actions`
              : "Session actions"
          }
          onPick={onSessionMenuPick}
          onClose={closeSessionMenu}
        />
      ) : null}
      {folderMenu ? (
        <ExplorerMenu
          x={folderMenu.x}
          y={folderMenu.y}
          items={folderMenuItems}
          ariaLabel="Folder actions"
          width={260}
          header={
            <FolderColorSwatches
              colorIndex={menuFolder?.colorIndex}
              customColor={menuFolder?.customColor}
              onChange={onFolderColorChange}
              onCustomChange={onFolderCustomColorChange}
            />
          }
          onPick={onFolderMenuPick}
          onClose={() => setFolderMenu(null)}
        />
      ) : null}
      {filterMenu ? (
        <SessionFiltersMenu
          x={filterMenu.x}
          y={filterMenu.y}
          harnesses={sessionHarnesses}
          filters={sessionFilters}
          onChange={onSessionFiltersChange}
          onClose={() => setFilterMenu(null)}
        />
      ) : null}
      {linkingSession ? (
        <LinkSessionWorkItemDialog
          initial={linkingSession.linkedWorkItem}
          sessionTitle={sessionDisplayTitle(
            linkingSession.title,
            linkingSession.harness,
          )}
          onSave={(item) => {
            onSetSessionLinkedWorkItem?.(linkingSession.id, item);
            setLinkingSession(null);
          }}
          onClose={() => setLinkingSession(null)}
        />
      ) : null}
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        aria-valuenow={resize.width}
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={MAX_WIDTH}
        className={`absolute inset-y-0 -right-px z-10 w-1.5 cursor-col-resize touch-none ${
          resize.dragging ? "bg-content/15" : "hover:bg-content/10"
        }`}
        onPointerDown={resize.onPointerDown}
        onDoubleClick={resize.onDoubleClick}
      />
    </aside>
  );

  return (
    <div
      className={`flex h-full shrink-0 ${
        railVisible || compactRailVisible || sidebarVisible ? "" : "hidden"
      }`}
    >
      {compactRailVisible ? (
        <CompactProjectRail
          cwd={cwd}
          recents={recents}
          busy={projectPathBusy(busyProjectPaths, cwd)}
          tabs={visibleTabs}
          activeTab={tab}
          tabShown={panelOpen}
          changesLabel={changesLabel}
          hasChanges={hasChanges}
          inboxUnseen={inboxUnseen}
          onSelectProject={onSelectProject}
          onOpenProject={onOpenProject}
          onRemoveProject={onRemoveProject}
          onTabChange={onCompactTabPick}
          onSearch={onSearch}
          searchActive={searchActive}
          onOpenInbox={onOpenInbox}
          inboxActive={inboxActive}
          onOpenNotificationSettings={onOpenNotificationSettings}
          onOpenNotes={notesEnabled ? onOpenNotes : undefined}
          notesActive={notesActive}
          onOpenAutomations={onOpenAutomations}
          automationsActive={automationsActive}
          onOpenSettings={onOpenSettings}
          onTogglePanel={onToggleProjectRail}
          onLeaveActive={onGoBack}
          titleBarAbove={titleBarAbove}
          monos={railMonos}
          monoViewActive={monoViewActive}
        />
      ) : null}
      {railMounted.current && onSelectProject && onOpenProject ? (
        <ProjectRail
          visible={railVisible}
          cwd={cwd}
          recents={recents}
          inboxUnseen={inboxUnseen}
          busyPaths={busyProjectPaths}
          liveAgents={liveAgents}
          activeSessionId={activeSessionId}
          onSelectAgent={onSelectAgent}
          canGoBack={canGoBack}
          canGoForward={canGoForward}
          onGoBack={onGoBack}
          onGoForward={onGoForward}
          onSearch={onSearch}
          searchActive={searchActive}
          onOpenInbox={onOpenInbox}
          inboxActive={inboxActive}
          notesEnabled={notesEnabled}
          onOpenNotes={onOpenNotes}
          notesActive={notesActive}
          onOpenAutomations={onOpenAutomations}
          automationsActive={automationsActive}
          onTogglePanel={onToggleProjectRail}
          onSelectProject={onSelectProject}
          onOpenProject={onOpenProject}
          onRemoveProject={onRemoveProject}
          settingsOpen={settingsOpen}
          settingsSection={settingsSection}
          onOpenSettings={onOpenSettings}
          onOpenNotificationSettings={onOpenNotificationSettings}
          onSelectSettingsSection={onSelectSettingsSection}
          onCloseSettings={onCloseSettings}
          updateNotice={updateNotice}
          onOpenWhatsNew={onOpenWhatsNew}
          onDismissUpdate={onDismissUpdate}
          monos={railMonos}
        />
      ) : null}
      {sidebarVisible ? sidebarContent : null}
      {drawerRendered ? (
        // Pinned to the right edge, so the sidebar slides in as the width grows.
        <div
          ref={drawerRef}
          data-sidebar-drawer={drawerClosing ? "closing" : "open"}
          inert={drawerClosing || undefined}
          className={`flex shrink-0 justify-end overflow-hidden ${
            drawerClosing ? "pointer-events-none" : ""
          }`}
        >
          {sidebarContent}
        </div>
      ) : null}
    </div>
  );
}

export const Sidebar = memo(SidebarComponent);

/** Project picker whose rows open the shared project context menu. */
function SearchableProjectPickerWithMenu({
  onRemoveProject,
  onOpenNotificationSettings,
  ...pickerProps
}: Omit<
  ComponentProps<typeof SearchableProjectPicker>,
  "onProjectContextMenu" | "projectMenuActive"
> & {
  onRemoveProject?: Props["onRemoveProject"];
  onOpenNotificationSettings?: (projectPath?: string) => void;
}) {
  const projectMenu = useProjectMenu({
    onRemoveProject,
    onOpenNotificationSettings,
  });
  return (
    <>
      <SearchableProjectPicker
        {...pickerProps}
        onProjectContextMenu={projectMenu.open}
        projectMenuActive={projectMenu.isActive}
      />
      {projectMenu.element}
    </>
  );
}

function SidebarProjectPicker({
  cwd,
  recents,
  busy,
  onSelectProject,
  onOpenProject,
  onRemoveProject,
  onNew,
  onSearch,
  onOpenInbox,
  onOpenNotificationSettings,
  onOpenNotes,
  onOpenAutomations,
  searchActive = false,
  inboxActive = false,
  notesActive = false,
  automationsActive = false,
  inboxUnseen = false,
}: {
  cwd: string;
  recents: RecentProject[];
  busy: boolean;
  onSelectProject: (path: string) => void;
  onOpenProject?: () => void;
  onRemoveProject?: Props["onRemoveProject"];
  onNew?: () => string | void;
  onSearch?: () => void;
  onOpenInbox?: () => void;
  onOpenNotificationSettings?: (projectPath?: string) => void;
  onOpenNotes?: () => void;
  onOpenAutomations?: () => void;
  searchActive?: boolean;
  inboxActive?: boolean;
  notesActive?: boolean;
  automationsActive?: boolean;
  inboxUnseen?: boolean;
}) {
  const [inboxMenu, setInboxMenu] = useState<{ x: number; y: number } | null>(
    null,
  );
  const inboxTrigger = useRef<HTMLElement | null>(null);
  return (
    <div
      className="flex h-9 items-center gap-0.5 border-b border-stroke px-2"
      data-tauri-drag-region="deep"
    >
      <SearchableProjectPickerWithMenu
        cwd={cwd}
        recents={recents}
        busy={busy}
        className="flex-1"
        onSelectProject={onSelectProject}
        onOpenProject={onOpenProject}
        onRemoveProject={onRemoveProject}
        onOpenNotificationSettings={onOpenNotificationSettings}
      />
      <div className="ml-auto flex items-center">
        {onNew ? (
          <IconButton label={`New tab (${MOD}T)`} onClick={onNew}>
            <Plus className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
        {onSearch ? (
          <IconButton
            label={`Search (${MOD}K)`}
            active={searchActive}
            onClick={onSearch}
          >
            <Search className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
        {onOpenInbox ? (
          <IconButton
            label={inboxUnseen ? "Inbox, new items" : "Inbox"}
            active={inboxActive}
            onClick={onOpenInbox}
            onOpenContextMenu={(x, y) => {
              inboxTrigger.current =
                document.activeElement instanceof HTMLElement
                  ? document.activeElement
                  : null;
              setInboxMenu({ x, y });
            }}
          >
            <span className="relative">
              <Inbox className="size-3.5" strokeWidth={1.75} />
              {inboxUnseen ? (
                <span
                  aria-hidden
                  className="absolute -right-0.5 -top-0.5 size-1.5 rounded-full bg-accent"
                />
              ) : null}
            </span>
          </IconButton>
        ) : null}
        {onOpenNotes ? (
          <IconButton label="Notes" active={notesActive} onClick={onOpenNotes}>
            <StickyNote className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
        {onOpenAutomations ? (
          <IconButton
            label="Automations"
            active={automationsActive}
            onClick={onOpenAutomations}
          >
            <Zap className="size-3.5" strokeWidth={1.75} />
          </IconButton>
        ) : null}
      </div>
      {inboxMenu ? (
        <InboxNotificationMenu
          {...inboxMenu}
          projectPaths={[...collectRailProjects(recents, cwd).keys()]}
          onOpenSettings={onOpenNotificationSettings}
          onClose={() => {
            setInboxMenu(null);
            inboxTrigger.current?.focus();
          }}
        />
      ) : null}
    </div>
  );
}

function CompactProjectRail({
  cwd,
  recents,
  busy,
  tabs,
  activeTab,
  tabShown,
  changesLabel,
  hasChanges,
  inboxUnseen,
  onSelectProject,
  onOpenProject,
  onRemoveProject,
  onTabChange,
  onSearch,
  searchActive,
  onOpenInbox,
  inboxActive,
  onOpenNotificationSettings,
  onOpenNotes,
  notesActive,
  onOpenAutomations,
  automationsActive,
  onOpenSettings,
  onTogglePanel,
  onLeaveActive,
  titleBarAbove,
  monos,
  monoViewActive = false,
}: {
  cwd: string;
  recents: RecentProject[];
  busy: boolean;
  tabs: SidebarTab[];
  activeTab: SidebarTab;
  tabShown: boolean;
  changesLabel: string;
  hasChanges: boolean;
  inboxUnseen: boolean;
  onSelectProject?: (path: string) => void;
  onOpenProject?: () => void;
  onRemoveProject?: Props["onRemoveProject"];
  onTabChange: (tab: SidebarTab) => void;
  onSearch?: () => void;
  searchActive: boolean;
  onOpenInbox?: () => void;
  inboxActive: boolean;
  onOpenNotificationSettings?: (projectPath?: string) => void;
  onOpenNotes?: () => void;
  notesActive: boolean;
  onOpenAutomations?: () => void;
  automationsActive: boolean;
  onOpenSettings?: () => void;
  onTogglePanel?: () => void;
  onLeaveActive?: () => void;
  titleBarAbove: boolean;
  /** Monos have no row here, so the project button lists them too. */
  monos?: MonoRailProps;
  /** A Mono fills the main area: no workspace tab is the current one. */
  monoViewActive?: boolean;
}) {
  const monosSnap = useSyncExternalStore(subscribeMonos, monosSnapshot);
  const monosPinned = useRailMonosPinned();
  const monoItems = useMemo(() => {
    if (!monos) return [];
    return listMonos().map((mono) => ({
      id: mono.id,
      ...monoLook(mono),
      status: monos.states.get(mono.id)?.status ?? "idle",
    }));
    // The roster is read through its snapshot.
  }, [monos, monosSnap]);
  const pickerMonos = useMemo((): PickerMonos | undefined => {
    if (!monos) return undefined;
    return {
      // Pinned Monos have their own buttons; the picker keeps "New mono".
      items: monosPinned ? [] : monoItems,
      activeId: monos.activeId,
      onOpen: monos.onOpen,
      onCreate: monos.onCreate,
    };
  }, [monos, monoItems, monosPinned]);
  const [inboxMenu, setInboxMenu] = useState<{ x: number; y: number } | null>(
    null,
  );
  const inboxTrigger = useRef<HTMLElement | null>(null);
  const action = (active: boolean, open?: () => void) =>
    active && onLeaveActive ? onLeaveActive : open;
  const workspaceActive =
    !searchActive &&
    !inboxActive &&
    !notesActive &&
    !automationsActive &&
    !monoViewActive;
  const openWorkspaceTab = (nextTab: SidebarTab) => {
    if (!workspaceActive) onLeaveActive?.();
    onTabChange(nextTab);
  };

  return (
    <nav
      aria-label="Project shortcuts"
      data-compact-project-rail
      className="sidebar-glass relative flex h-full w-12 shrink-0 flex-col items-center"
    >
      {titleBarAbove ? null : (
        <div
          className="h-10 w-full shrink-0 border-b border-stroke"
          data-tauri-drag-region="deep"
        />
      )}
      <span
        aria-hidden
        data-compact-rail-divider
        className={`pointer-events-none absolute bottom-0 right-0 w-px bg-stroke ${
          titleBarAbove ? "top-0" : "top-10"
        }`}
      />
      <div
        data-compact-rail-actions
        className="flex w-full shrink-0 flex-col items-center gap-1.5 py-1.5"
      >
        <CompactRailAction
          label="Expand projects"
          icon={PanelLeft}
          onClick={onTogglePanel}
        />
        {monos && monosPinned && monoItems.length ? (
          <>
            <div
              role="group"
              aria-label="Monos"
              data-compact-rail-monos
              className="flex flex-col items-center gap-1.5"
            >
              {monoItems.map((mono) => (
                <CompactRailMono
                  key={mono.id}
                  name={mono.name}
                  mascot={mono.mascot}
                  color={mono.color}
                  status={mono.status}
                  active={mono.id === monos.activeId}
                  unseen={
                    mono.id === monos.activeId
                      ? 0
                      : (monos.unseenCounts?.get(mono.id) ?? 0)
                  }
                  onClick={() => monos.onOpen(mono.id)}
                />
              ))}
            </div>
            <span
              aria-hidden
              data-compact-rail-monos-divider
              className="h-px w-6 shrink-0 bg-stroke"
            />
          </>
        ) : null}
        {onSelectProject ? (
          <SearchableProjectPickerWithMenu
            cwd={cwd}
            recents={recents}
            busy={busy}
            compact
            className="w-full justify-center"
            onSelectProject={onSelectProject}
            onOpenProject={onOpenProject}
            onRemoveProject={onRemoveProject}
            onOpenNotificationSettings={onOpenNotificationSettings}
            monos={pickerMonos}
          />
        ) : null}
        <div
          role="tablist"
          aria-label="Workspace"
          aria-orientation="vertical"
          className="flex flex-col items-center gap-1.5"
        >
          {tabs.map((itemId) => (
            <CompactRailAction
              key={itemId}
              tab
              label={itemId === "changes" ? changesLabel : TAB_LABELS[itemId]}
              icon={COMPACT_TAB_ICONS[itemId]}
              active={workspaceActive && tabShown && activeTab === itemId}
              dot={itemId === "changes" && hasChanges}
              onClick={() => openWorkspaceTab(itemId)}
            />
          ))}
        </div>
        <CompactRailAction
          label={`Search (${MOD}K)`}
          icon={Search}
          active={searchActive}
          onClick={action(searchActive, onSearch)}
        />
        <CompactRailAction
          label={inboxUnseen ? "Inbox, new items" : "Inbox"}
          icon={Inbox}
          active={inboxActive}
          dot={inboxUnseen}
          onClick={action(inboxActive, onOpenInbox)}
          onOpenContextMenu={(x, y) => {
            inboxTrigger.current =
              document.activeElement instanceof HTMLElement
                ? document.activeElement
                : null;
            setInboxMenu({ x, y });
          }}
        />
        {onOpenNotes ? (
          <CompactRailAction
            label="Notes"
            icon={StickyNote}
            active={notesActive}
            onClick={action(notesActive, onOpenNotes)}
          />
        ) : null}
        <CompactRailAction
          label="Automations"
          icon={Zap}
          active={automationsActive}
          onClick={action(automationsActive, onOpenAutomations)}
        />
      </div>
      <div className="min-h-2 flex-1" />
      <div className="flex w-full flex-col items-center gap-1 py-1.5">
        <CompactRailAction
          label={`Settings (${MOD},)`}
          icon={Settings}
          onClick={onOpenSettings}
        />
      </div>
      {inboxMenu ? (
        <InboxNotificationMenu
          {...inboxMenu}
          projectPaths={[...collectRailProjects(recents, cwd).keys()]}
          onOpenSettings={onOpenNotificationSettings}
          onClose={() => {
            setInboxMenu(null);
            inboxTrigger.current?.focus();
          }}
        />
      ) : null}
    </nav>
  );
}

function CompactRailMono({
  name,
  mascot,
  color,
  status,
  active,
  unseen,
  onClick,
}: {
  name: string;
  mascot: string;
  color: string;
  status: MonoStatus;
  active: boolean;
  /** Replies the user has not read. */
  unseen: number;
  onClick: () => void;
}) {
  const label = `${name}, ${MONO_STATUS_LABEL[status]}`;
  return (
    <button
      type="button"
      title={`${name}\n${MONO_STATUS_LABEL[status]}`}
      aria-label={unseen ? `${label}, ${unreadLabel(unseen)}` : label}
      aria-current={active ? "true" : undefined}
      data-compact-rail-mono
      onClick={onClick}
      className={`relative grid size-8 shrink-0 place-items-center rounded-md active:scale-[0.97] ${
        active
          ? "bg-selection"
          : "opacity-65 hover:bg-content/10 hover:opacity-100"
      }`}
    >
      <MonoRailMascot name={mascot} color={color} status={status} />
      {unseen ? (
        <span
          aria-hidden
          data-mono-unread={unseen}
          className="absolute -right-0.5 -top-0.5 grid h-3.5 min-w-3.5 place-items-center rounded-full bg-background-base px-0.5 text-[9px] font-medium tabular-nums leading-none text-content/80 ring-1 ring-content/15"
        >
          {unreadBadge(unseen)}
        </span>
      ) : null}
    </button>
  );
}

function CompactRailAction({
  label,
  icon: Icon,
  tab = false,
  active = false,
  dot = false,
  onClick,
  onOpenContextMenu,
}: {
  label: string;
  icon: typeof PanelLeft;
  tab?: boolean;
  active?: boolean;
  dot?: boolean;
  onClick?: () => void;
  onOpenContextMenu?: (x: number, y: number) => void;
}) {
  return (
    <button
      type="button"
      role={tab ? "tab" : undefined}
      title={label}
      aria-label={label}
      aria-selected={tab ? active : undefined}
      aria-pressed={tab ? undefined : active}
      disabled={!onClick}
      onClick={onClick}
      onContextMenu={
        onOpenContextMenu
          ? (event) => {
              event.preventDefault();
              event.stopPropagation();
              event.currentTarget.focus();
              onOpenContextMenu(event.clientX, event.clientY);
            }
          : undefined
      }
      onKeyDown={
        onOpenContextMenu
          ? (event) => {
              if (
                event.key !== "ContextMenu" &&
                !(event.shiftKey && event.key === "F10")
              )
                return;
              event.preventDefault();
              event.stopPropagation();
              event.currentTarget.focus();
              const rect = event.currentTarget.getBoundingClientRect();
              onOpenContextMenu(rect.right, rect.top);
            }
          : undefined
      }
      className={`relative grid size-8 shrink-0 place-items-center rounded-md active:scale-[0.97] ${
        active
          ? "bg-selection text-content"
          : "text-content/50 hover:bg-content/10 hover:text-content"
      } disabled:cursor-default disabled:opacity-35`}
    >
      <Icon
        className={`size-4 ${dot ? "compact-rail-icon-with-dot" : ""}`}
        strokeWidth={1.75}
      />
      {dot ? (
        <span
          aria-hidden
          className="absolute right-1.5 top-1.5 size-1.5 rounded-full bg-accent"
        />
      ) : null}
    </button>
  );
}

function WorkspaceTitleActions({
  onSearch,
  onNew,
}: {
  onSearch?: () => void;
  onNew?: () => void;
}) {
  if (!onSearch && !onNew) return null;
  return (
    <div
      className="flex shrink-0 items-center gap-0.5"
      data-tauri-drag-region="false"
    >
      {onSearch ? (
        <IconButton label={`Go to File (${MOD}P)`} onClick={onSearch}>
          <Search className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      ) : null}
      {onNew ? (
        <IconButton label={`New session (${MOD}T)`} onClick={onNew}>
          <Plus className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      ) : null}
    </div>
  );
}

function SessionsHeaderButton({
  label,
  active = false,
  open = false,
  hasPopup = false,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  open?: boolean;
  hasPopup?: boolean;
  onClick: (event: ReactMouseEvent<HTMLButtonElement>) => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-expanded={open}
      aria-haspopup={hasPopup ? "menu" : undefined}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={onClick}
      className={`relative z-50 grid size-6 place-items-center rounded-md text-content/50 hover:bg-content/10 hover:text-content ${
        open || active ? "bg-selection text-content" : ""
      }`}
    >
      {children}
    </button>
  );
}

function sessionListDropFromPoint(
  x: number,
  y: number,
  draggedId: string,
): SessionListDropTarget | null {
  const el = document.elementFromPoint(x, y);
  if (!el) return null;
  if (el.closest("[data-reminder-sessions]")) return null;
  const card = el.closest("[data-session-card]") as HTMLElement | null;
  const cardId = card?.dataset.sessionCard;
  if (cardId === draggedId) return null;
  const folder = el.closest("[data-session-folder]") as HTMLElement | null;
  const folderId = folder?.dataset.sessionFolder;
  if (folderId && cardId && card && folder.contains(card)) {
    return { kind: "folder", id: folderId };
  }
  if (cardId) return { kind: "session", id: cardId };
  if (folderId) return { kind: "folder", id: folderId };
  return null;
}

function FolderColorSwatches({
  colorIndex,
  customColor,
  onChange,
  onCustomChange,
}: {
  colorIndex: number | undefined;
  customColor: string | undefined;
  onChange: (index: number | null) => void;
  onCustomChange: (color: string) => void;
}) {
  const paletteColor =
    colorIndex != null ? TAB_GROUP_COLORS[colorIndex] : TAB_GROUP_COLORS[0];
  const pickerValue =
    customColor ?? normalizeHex(paletteColor ?? TAB_GROUP_COLORS[0]);
  return (
    <div className="px-1 py-1">
      <ColorSwatchRow
        colors={TAB_GROUP_COLORS}
        colorIndex={colorIndex}
        customColor={customColor}
        customPickerOpen
        customHighlighted={customColor != null}
        onPickIndex={(index) => onChange(index === 0 ? null : index)}
      />
      <ColorPickerPopover value={pickerValue} onChange={onCustomChange} />
    </div>
  );
}

function FolderRow({
  folder,
  sessions,
  expanded,
  dropTarget,
  busy,
  done,
  needsApproval,
  groupIcon,
  onPointerDown,
  onToggle,
  onContextMenu,
  onRename,
}: {
  folder: Pick<SessionFolder, "name" | "colorIndex" | "customColor">;
  sessions: SessionSummary[];
  expanded: boolean;
  dropTarget: boolean;
  busy: boolean;
  done: boolean;
  needsApproval: boolean;
  groupIcon?: ReactNode;
  onPointerDown?: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onToggle: () => void;
  onContextMenu?: (event: ReactMouseEvent<HTMLButtonElement>) => void;
  onRename?: () => void;
}) {
  const count = sessions.length;
  const accent = folderAccent(folder.colorIndex, folder.customColor);
  return (
    <button
      type="button"
      title={folder.name}
      aria-expanded={expanded}
      data-tauri-drag-region="false"
      onPointerDown={onPointerDown}
      onClick={onToggle}
      onContextMenu={onContextMenu}
      onKeyDown={(event) => {
        if (event.key === "F2" && onRename) {
          event.preventDefault();
          onRename();
        }
      }}
      className={`group relative flex w-full touch-none items-center gap-1.5 px-2 h-8 text-left ${
        expanded ? "rounded-md" : ""
      } ${
        dropTarget
          ? "text-content"
          : expanded
            ? "text-content hover:bg-content/10"
            : "text-content/80 hover:bg-content/10 hover:text-content"
      }`}
    >
      {dropTarget ? (
        <div className="pointer-events-none absolute inset-0 rounded-md bg-accent/20" />
      ) : null}
      <span
        className={`relative grid size-4 shrink-0 place-items-center ${
          accent ? "" : "text-content/50"
        }`}
        style={accent ? { color: accent } : undefined}
      >
        {expanded ? (
          groupIcon ? (
            <>
              <span className="group-hover:hidden group-focus-visible:hidden">
                {groupIcon}
              </span>
              <ChevronDown
                className="hidden size-3.5 text-content group-hover:block group-focus-visible:block"
                strokeWidth={1.75}
              />
            </>
          ) : (
            <ChevronDown className="size-3.5 text-content" strokeWidth={1.75} />
          )
        ) : (
          <>
            <span className="group-hover:hidden group-focus-visible:hidden">
              {groupIcon ?? (
                <Folder className="size-3.5 text-content" strokeWidth={1.75} />
              )}
            </span>
            <ChevronRight
              className="hidden size-3.5 group-hover:block group-focus-visible:block text-content"
              strokeWidth={1.75}
            />
          </>
        )}
      </span>
      <span className="relative min-w-0 flex-1 truncate text-[13px] font-semibold leading-snug text-content">
        {folder.name}
      </span>
      <span className="relative flex shrink-0 items-center gap-1 text-[11px] tabular-nums text-content/45">
        {!expanded && needsApproval ? (
          <CircleAlert className="size-3 text-amber-400" strokeWidth={1.75} />
        ) : !expanded && busy ? (
          <TerminalSpinner className="inline-block w-3 select-none text-center text-[11px] leading-none text-accent" />
        ) : !expanded && done ? (
          <Check className="size-3 text-emerald-400" strokeWidth={2.25} />
        ) : null}
        <span>{count}</span>
      </span>
    </button>
  );
}

function FolderRenameRow({
  folder,
  memberCount,
  dropTarget,
  onCommit,
  onCancel,
}: {
  folder: SessionFolder;
  memberCount: number;
  dropTarget: boolean;
  onCommit: (name: string) => void;
  onCancel: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const finished = useRef(false);
  const [value, setValue] = useState(folder.name);

  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    input.select();
  }, []);

  const finish = (success: boolean) => {
    if (finished.current) return;
    if (success) {
      const trimmed = value.trim();
      if (!trimmed) {
        onCancel();
        return;
      }
      finished.current = true;
      onCommit(trimmed);
      return;
    }
    finished.current = true;
    onCancel();
  };

  return (
    <div
      className={`relative flex w-full items-center gap-1.5 px-2 py-1.5 ${
        dropTarget ? "" : "text-content"
      }`}
    >
      {dropTarget ? (
        <div className="pointer-events-none absolute inset-0 rounded-md bg-accent/20" />
      ) : null}
      <span className="relative grid size-4 shrink-0 place-items-center text-content/50">
        <ChevronDown className="size-3.5" strokeWidth={1.75} />
      </span>
      <input
        ref={inputRef}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onBlur={() => finish(true)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            finish(true);
            return;
          }
          if (event.key === "Escape") {
            event.preventDefault();
            finish(false);
          }
        }}
        className="relative min-w-0 flex-1 rounded bg-content/10 px-2 py-0.5 text-[13px] font-semibold leading-snug text-content outline-none ring-1 ring-accent/40"
      />
      <span className="relative shrink-0 text-[11px] tabular-nums text-content/45">
        {memberCount}
      </span>
    </div>
  );
}

const SESSION_PREFETCH_DELAY_MS = 120;
/** Rows created this recently slide in; older ones are just being listed. */
const SESSION_INSERT_WINDOW_MS = 15_000;

type SessionInsertMotion = { cwd: string; seen: Set<string> };

/** List row that grows open when a new session lands, pushing rows below it down. */
function SessionListItem({
  session,
  cwd,
  motion,
  children,
}: {
  session: SessionSummary;
  cwd: string;
  motion: RefObject<SessionInsertMotion>;
  children: ReactNode;
}) {
  const ref = useRef<HTMLLIElement>(null);
  // Decided once per row: effects can replay (StrictMode, reordering), and a
  // row that already slid in must not do it again.
  const played = useRef(false);
  useLayoutEffect(() => {
    if (played.current) return;
    played.current = true;
    const state = motion.current;
    const fresh =
      state.cwd === cwd &&
      !state.seen.has(session.id) &&
      (session.createdAt === 0 ||
        Date.now() - session.createdAt < SESSION_INSERT_WINDOW_MS);
    state.seen.add(session.id);
    const el = ref.current;
    const content = el?.firstElementChild;
    if (
      !fresh ||
      !el ||
      !(content instanceof HTMLElement) ||
      typeof el.animate !== "function" ||
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches
    )
      return;
    // The card takes its place at once; everything below starts where it was
    // and slides down, uncovering it as it fades in.
    const offset =
      el.offsetHeight +
      (parseFloat(getComputedStyle(el.parentElement ?? el).rowGap) || 0);
    const timing = {
      duration: 380,
      easing: "cubic-bezier(0.32, 0.72, 0, 1)",
    };
    for (
      let node: Element | null = el;
      node && !node.hasAttribute("data-session-list");
      node = node.parentElement
    ) {
      for (
        let below = node.nextElementSibling;
        below;
        below = below.nextElementSibling
      ) {
        if (!(below instanceof HTMLElement)) continue;
        below.animate(
          [{ transform: `translateY(${-offset}px)` }, { transform: "none" }],
          // Stack with a push already in flight instead of restarting it.
          { ...timing, composite: "add" },
        );
      }
    }
    content.animate([{ opacity: 0 }, { opacity: 1 }], {
      duration: 220,
      easing: "ease-out",
    });
  }, []);
  return <li ref={ref}>{children}</li>;
}

const SessionCard = memo(function SessionCard({
  session,
  isActive,
  isSelected,
  busy,
  done,
  linkedUpdate,
  needsApproval,
  dropTarget,
  compact = false,
  now,
  onSelect,
  onOpenWorkItem,
  onPrefetch,
  onPlaceOnPane,
  onListDrop,
  onListDropTargetChange,
  onContextMenu,
  onArchive,
  onRename,
  onDelete,
}: {
  session: SessionSummary;
  isActive: boolean;
  isSelected: boolean;
  busy: boolean;
  done: boolean;
  linkedUpdate: boolean;
  needsApproval: boolean;
  dropTarget?: boolean;
  compact?: boolean;
  now: number;
  onSelect: (
    sessionId: string,
    event: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean },
  ) => void;
  onOpenWorkItem?: (item: LinkedWorkItem, sessionId: string) => void;
  onPrefetch?: (sessionId: string) => void;
  onPlaceOnPane?: (sessionId: string, targetId: string, edge: PaneEdge) => void;
  onListDrop?: (draggedId: string, target: SessionListDropTarget) => void;
  onListDropTargetChange?: (target: SessionListDropTarget | null) => void;
  onContextMenu?: (
    sessionId: string,
    e: ReactMouseEvent<HTMLDivElement>,
  ) => void;
  onArchive?: (sessionId: string, archived: boolean) => void;
  onRename?: (sessionId: string) => void;
  onDelete?: (sessionId: string) => void;
}) {
  const skipClickUntil = useRef(0);
  const prefetchTimer = useRef<number | null>(null);
  const orchestrationTooltipRootRef = useRef<HTMLDivElement>(null);
  const orchestrationTooltipId = useId();
  const [dragging, setDragging] = useState(false);
  const [orchestrationTooltipOpen, setOrchestrationTooltipOpen] =
    useState(false);
  const orchestration = session.orchestration;
  const draft = !!session.draft;
  const orchestrationExpanded =
    !!orchestration && (isActive || isSelected || busy);
  const orchestrationDone =
    orchestration?.tasks.filter((task) => task.status === "completed").length ??
    0;
  const title = sessionDisplayTitle(session.title, session.harness);
  const gitLabel = session.worktreeRemoved
    ? NO_BRANCH_LABEL
    : formatGitLabel(session.repo, session.branch);
  const time = formatRelative(session.updatedAt, now);
  const model =
    compact && !orchestrationExpanded
      ? null
      : resolveModel(session.harness, session.model).name;
  const statusClass = needsApproval
    ? "text-amber-400"
    : busy
      ? "text-accent"
      : done
        ? "text-emerald-400"
        : draft
          ? "text-content/55"
          : "text-content/45";
  const status = (
    <span
      className={`flex shrink-0 items-center gap-1 text-[11px] tabular-nums ${statusClass}`}
    >
      {needsApproval ? (
        <>
          <CircleAlert className="size-3" strokeWidth={1.75} />
          <span>{orchestration ? "Needs input" : "Need approval"}</span>
        </>
      ) : busy ? (
        <>
          <TerminalSpinner className="inline-block w-3 select-none text-center text-[11px] leading-none text-accent" />
          <span>Working...</span>
        </>
      ) : done ? (
        <>
          <Check className="size-3" strokeWidth={2.25} />
          <span>Done</span>
        </>
      ) : draft ? (
        <>
          <CircleDashed className="size-3" strokeWidth={1.75} />
          <span>Draft</span>
        </>
      ) : (
        <span>{time}</span>
      )}
    </span>
  );

  const linkedWorkItem = session.linkedWorkItem;
  const linkedUpdateDot = linkedUpdate ? (
    <span
      title={`Linked ${linkedWorkItem?.kind === "pr" ? "PR" : "issue"} updated since this session`}
      aria-label="Linked work item updated"
      className="size-1.5 shrink-0 rounded-full bg-accent"
    />
  ) : null;
  const workItemBadge = linkedWorkItem ? (
    <button
      type="button"
      data-no-drag
      data-tauri-drag-region="false"
      title={`Open ${linkedWorkItem.kind === "pr" ? "PR" : "issue"} #${linkedWorkItem.number} beside this session (${MOD}-click for GitHub)`}
      aria-label={`Open ${linkedWorkItem.kind === "pr" ? "PR" : "issue"} #${linkedWorkItem.number}`}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.metaKey || event.ctrlKey) {
          void openUrl(linkedWorkItem.url).catch(() => undefined);
          return;
        }
        if (onOpenWorkItem) onOpenWorkItem(linkedWorkItem, session.id);
        else void openUrl(linkedWorkItem.url).catch(() => undefined);
      }}
      onAuxClick={(event) => {
        if (event.button !== 1) return;
        event.preventDefault();
        event.stopPropagation();
        void openUrl(linkedWorkItem.url).catch(() => undefined);
      }}
      className="flex shrink-0 cursor-pointer items-center gap-0.5 rounded px-0.5 text-[11px] tabular-nums text-accent hover:underline"
    >
      {linkedWorkItem.kind === "pr" ? (
        <GitPullRequest className="size-3" strokeWidth={1.75} />
      ) : (
        <CircleDot className="size-3" strokeWidth={1.75} />
      )}
      <span>#{linkedWorkItem.number}</span>
    </button>
  ) : null;

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onSelect(session.id, e);
      return;
    }
    if (e.key === "F2" && onRename) {
      e.preventDefault();
      onRename(session.id);
      return;
    }
    if ((e.key === "Delete" || e.key === "Backspace") && onDelete) {
      e.preventDefault();
      onDelete(session.id);
    }
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    // Warm the transcript during the press. Opening stays on click so a
    // drag-to-pane gesture does not switch conversations.
    if (prefetchTimer.current != null) {
      window.clearTimeout(prefetchTimer.current);
      prefetchTimer.current = null;
    }
    onPrefetch?.(session.id);
    if (!onPlaceOnPane && !onListDrop) return;
    const handle = event.currentTarget;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startY = event.clientY;
    let active = false;
    let lastX = startX;
    let lastY = startY;
    let lastList: SessionListDropTarget | null = null;
    handle.setPointerCapture(pointerId);
    const restoreSelection = suppressTextSelection();

    const setListTarget = (next: SessionListDropTarget | null) => {
      if (lastList?.kind === next?.kind && lastList?.id === next?.id) return;
      lastList = next;
      onListDropTargetChange?.(next);
    };

    const onMove = (ev: PointerEvent) => {
      lastX = ev.clientX;
      lastY = ev.clientY;
      if (!active) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 5) return;
        active = true;
        setDragging(true);
        if (onPlaceOnPane) {
          setExternalPaneDrop({
            fromId: session.id,
            overId: null,
            edge: "left",
          });
        }
      }
      setListTarget(
        onListDrop
          ? sessionListDropFromPoint(ev.clientX, ev.clientY, session.id)
          : null,
      );
      if (!onPlaceOnPane) return;
      const over = paneDropFromPoint(ev.clientX, ev.clientY);
      if (!over || over.id === session.id) {
        setExternalPaneDrop({
          fromId: session.id,
          overId: over?.id === session.id ? session.id : null,
          edge: over?.edge ?? "left",
        });
        return;
      }
      setExternalPaneDrop({
        fromId: session.id,
        overId: over.id,
        edge: over.edge,
      });
    };

    const onUp = () => finish(true);
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      finish(false);
    };

    function finish(commit: boolean) {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      window.removeEventListener("keydown", onKey);
      restoreSelection();
      setDragging(false);
      setExternalPaneDrop(null);
      setListTarget(null);
      try {
        handle.releasePointerCapture(pointerId);
      } catch {
        /* already released */
      }
      if (!active) return;
      skipClickUntil.current = performance.now() + 400;
      if (!commit) return;
      const listOver = onListDrop
        ? sessionListDropFromPoint(lastX, lastY, session.id)
        : null;
      if (listOver) {
        onListDrop?.(session.id, listOver);
        return;
      }
      const over = paneDropFromPoint(lastX, lastY);
      if (over && over.id !== session.id) {
        onPlaceOnPane?.(session.id, over.id, over.edge);
      }
    }

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    window.addEventListener("keydown", onKey);
  };

  useEffect(
    () => () => {
      if (prefetchTimer.current != null) {
        window.clearTimeout(prefetchTimer.current);
        prefetchTimer.current = null;
      }
    },
    [onPrefetch, session.id],
  );

  const schedulePrefetch = () => {
    if (!onPrefetch || prefetchTimer.current != null) return;
    prefetchTimer.current = window.setTimeout(() => {
      prefetchTimer.current = null;
      onPrefetch(session.id);
    }, SESSION_PREFETCH_DELAY_MS);
  };

  const cancelScheduledPrefetch = () => {
    if (prefetchTimer.current == null) return;
    window.clearTimeout(prefetchTimer.current);
    prefetchTimer.current = null;
  };

  const archiveLabel = session.archived ? "Unarchive" : "Archive";
  // Expanding an orchestration card must not move its existing header. Keep
  // the collapsed top inset and give only the new detail area extra room at
  // the bottom.
  const cardPaddingY = orchestrationExpanded
    ? compact
      ? "pb-2.5 pt-1.5"
      : "pb-2.5 pt-2"
    : compact
      ? "py-1.5"
      : "py-2";

  return (
    <div className="group relative">
      <div
        title={title}
        data-session-card={session.id}
        data-orchestration-card={orchestration ? "true" : undefined}
        data-session-selected={isSelected ? "true" : undefined}
        data-tauri-drag-region="false"
        onPointerDown={onPointerDown}
        onPointerEnter={schedulePrefetch}
        onPointerLeave={cancelScheduledPrefetch}
        onClick={(event) => {
          if (performance.now() < skipClickUntil.current) return;
          onSelect(session.id, event);
        }}
        onContextMenu={
          onContextMenu
            ? (event) => onContextMenu(session.id, event)
            : undefined
        }
        className={`relative border flex w-full cursor-default select-none touch-none flex-col rounded-md px-2.5 text-left ${cardPaddingY} ${
          dragging ? "opacity-40" : ""
        } ${
          dropTarget
            ? "text-content border-transparent"
            : isSelected
              ? `bg-accent/15 text-content ${draft ? "border-content/30 border-dashed" : "border-transparent"}`
              : needsApproval
                ? "bg-content/20 text-content border-content/30 border-dashed"
                : isActive
                  ? `bg-selection text-content ${draft ? "border-content/30 border-dashed" : "border-transparent"}`
                  : draft
                    ? "border-content/25 border-dashed text-content/80 hover:bg-content/5 hover:text-content"
                    : `text-content/80 hover:text-content border-transparent ${
                        orchestrationExpanded
                          ? "bg-content/5 hover:bg-content/10"
                          : "hover:bg-content/5"
                      }`
        }`}
      >
        {dropTarget ? (
          <div className="pointer-events-none absolute inset-0 rounded-md bg-accent/20" />
        ) : null}
        <div
          role="button"
          tabIndex={0}
          aria-current={isActive ? "true" : undefined}
          aria-pressed={isSelected}
          data-session-select={session.id}
          onKeyDown={onKeyDown}
          onMouseDown={(event) => {
            if (event.button !== 0) return;
            // Shift-click can trigger :focus-visible. Mouse selection should
            // only highlight the card; Tab can still focus this button.
            event.preventDefault();
            // Clear prior focus too, so shortcuts cannot target another card.
            const focused = event.currentTarget.ownerDocument.activeElement;
            if (focused instanceof HTMLElement) focused.blur();
          }}
          className="rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-accent/50"
        >
          {compact && !orchestrationExpanded ? null : (
            <span className="relative flex items-center gap-2">
              <span className="flex min-w-0 flex-1 items-center gap-1.5">
                <HarnessIcon
                  harness={session.harness}
                  className="size-3.5 shrink-0"
                />
                <span className="min-w-0 truncate text-[11px] text-content/50">
                  {model}
                </span>
              </span>
              <span className="flex shrink-0 items-center gap-1.5">
                {linkedUpdateDot}
                {status}
              </span>
            </span>
          )}
          <span
            className={`relative flex min-w-0 items-center gap-1.5 ${
              compact && !orchestrationExpanded ? "" : "mt-1"
            }`}
          >
            {session.pinned ? (
              <Pin
                className="size-3 shrink-0 text-content/45"
                strokeWidth={1.75}
              />
            ) : null}
            <ParticleText
              text={title}
              className="line-clamp-1 text-[13px] font-semibold leading-snug text-content"
            />
            {compact && !orchestrationExpanded ? (
              <span className="flex shrink-0 items-center gap-1.5">
                {linkedUpdateDot}
                {status}
              </span>
            ) : null}
          </span>
        </div>
        {orchestrationExpanded ? (
          <OrchestrationSidebarAgents
            leadId={session.id}
            summary={orchestration!}
          />
        ) : null}
        <span className="relative mt-1 flex items-center gap-2">
          {gitLabel ? (
            <span
              className="flex min-w-0 flex-1 items-center gap-1 text-[11px] text-content/45"
              title={session.worktreeCwd ? `${gitLabel}\n${session.worktreeCwd}` : gitLabel}
            >
              <GitBranch className="size-3 shrink-0" strokeWidth={1.75} />
              <span className="min-w-0 truncate">{gitLabel}</span>
            </span>
          ) : (
            <span className="min-w-0 flex-1" />
          )}
          <span className="relative flex shrink-0 items-center gap-px">
            {onArchive ? (
              <button
                type="button"
                data-no-drag
                data-tauri-drag-region="false"
                title={archiveLabel}
                aria-label={`${archiveLabel} ${title}`}
                onPointerDown={(event) => event.stopPropagation()}
                onClick={(event) => {
                  event.stopPropagation();
                  onArchive(session.id, !session.archived);
                }}
                className="pointer-events-none grid size-5 place-items-center rounded-md text-content/50 opacity-0 hover:bg-content/10 hover:text-content group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100"
              >
                <Archive className="size-3 shrink-0" strokeWidth={1.75} />
              </button>
            ) : null}
            {workItemBadge}
            {session.automationId ? (
              <span
                data-automation-icon
                role="img"
                title="Started by an automation"
                aria-label="Started by an automation"
                className="grid size-5 -mr-1 shrink-0 place-items-center text-amber-400"
              >
                <Zap className="size-3" strokeWidth={1.75} />
              </span>
            ) : null}
            {orchestration ? (
              <div
                ref={orchestrationTooltipRootRef}
                className="relative shrink-0"
                onMouseEnter={() => setOrchestrationTooltipOpen(true)}
                onMouseLeave={() => setOrchestrationTooltipOpen(false)}
              >
                <button
                  type="button"
                  data-no-drag
                  data-tauri-drag-region="false"
                  data-orchestration-icon
                  aria-label={`Orchestrator, ${orchestration.tasks.length} ${
                    orchestration.tasks.length === 1 ? "subagent" : "subagents"
                  }, ${orchestrationDone} done`}
                  aria-describedby={
                    orchestrationTooltipOpen
                      ? orchestrationTooltipId
                      : undefined
                  }
                  onPointerDown={(event) => event.stopPropagation()}
                  onFocus={() => setOrchestrationTooltipOpen(true)}
                  onBlur={() => setOrchestrationTooltipOpen(false)}
                  onClick={(event) => {
                    event.stopPropagation();
                    setOrchestrationTooltipOpen(false);
                    onSelect(session.id, event);
                  }}
                  className="grid size-5 shrink-0 place-items-center rounded-md text-fuchsia-300/65 hover:bg-content/10 hover:text-fuchsia-200/90"
                >
                  <Share className="size-3" />
                </button>
              </div>
            ) : null}
          </span>
        </span>
      </div>
      {orchestration && orchestrationTooltipOpen ? (
        <Popover
          anchor={orchestrationTooltipRootRef}
          side="right"
          align="end"
          width={248}
          maxHeight={320}
          role="tooltip"
          id={orchestrationTooltipId}
          className="pointer-events-none overflow-y-auto p-2.5"
        >
          <div className="flex items-center justify-between gap-3">
            <span className="text-[11px] font-semibold text-content/85">
              Subagents
            </span>
            <span className="shrink-0 text-[10px] tabular-nums text-content/45">
              {orchestrationDone}/{orchestration.tasks.length} done
            </span>
          </div>
          <div className="mt-1.5 flex flex-col gap-0.5">
            {orchestration.tasks.map((task) => {
              const label = orchestrationTaskLabel(task, orchestration);
              return (
                <div
                  key={task.sessionId}
                  className="flex min-w-0 items-center gap-1.5 rounded-md px-1 py-1"
                >
                  <HarnessIcon
                    harness={task.harness}
                    className="size-3.5 shrink-0 opacity-75"
                  />
                  <span className="min-w-0 flex-1 truncate text-[11px] text-content/75">
                    {task.title}
                  </span>
                  <span
                    className={`shrink-0 text-[10px] ${
                      task.needsInput ||
                      task.status === "failed" ||
                      task.status === "blocked" ||
                      task.status === "interrupted"
                        ? "text-amber-400"
                        : label === "Working"
                          ? "text-accent"
                          : task.status === "completed"
                            ? "text-emerald-400"
                            : "text-content/45"
                    }`}
                  >
                    {label}
                  </span>
                </div>
              );
            })}
          </div>
        </Popover>
      ) : null}
    </div>
  );
});

function SessionRenameRow({
  session,
  isActive,
  needsApproval,
  onCommit,
  onCancel,
}: {
  session: SessionSummary;
  isActive: boolean;
  needsApproval: boolean;
  onCommit: (title: string) => void;
  onCancel: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const finished = useRef(false);
  const [value, setValue] = useState(() =>
    sessionDisplayTitle(session.title, session.harness),
  );

  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    input.select();
  }, []);

  const finish = (success: boolean) => {
    if (finished.current) return;
    if (success) {
      const trimmed = value.trim();
      if (!trimmed) {
        onCancel();
        return;
      }
      finished.current = true;
      onCommit(trimmed);
      return;
    }
    finished.current = true;
    onCancel();
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      finish(true);
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      finish(false);
    }
  };

  return (
    <div
      className={`flex w-full flex-col rounded-md px-2.5 py-2 ${
        needsApproval
          ? "bg-amber-400/10 text-content"
          : isActive
            ? "bg-selection text-content"
            : "text-content/80"
      }`}
    >
      <input
        ref={inputRef}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={() => finish(true)}
        onKeyDown={onKeyDown}
        className="w-full rounded bg-content/10 px-2 py-1 text-[13px] font-semibold leading-snug text-content outline-none ring-1 ring-accent/40"
      />
    </div>
  );
}

function DiffStat({
  additions,
  deletions,
}: {
  additions: number;
  deletions: number;
}) {
  const containerRef = useRef<HTMLSpanElement>(null);
  const contentRef = useRef<HTMLSpanElement>(null);

  useLayoutEffect(() => {
    const container = containerRef.current;
    const content = contentRef.current;
    if (!container || !content) return;

    const fit = () => {
      const availableWidth = container.getBoundingClientRect().width;
      if (availableWidth <= 0) return;

      // Measure at the normal size so the text can grow again after resizing.
      let maxFontSize = 11;
      content.style.fontSize = `${maxFontSize}px`;
      if (content.getBoundingClientRect().width <= availableWidth) return;

      // Font metrics can change at small sizes, so check the rendered width.
      let minFontSize = 0;
      while (maxFontSize - minFontSize > 0.1) {
        const fontSize = (minFontSize + maxFontSize) / 2;
        content.style.fontSize = `${fontSize}px`;
        if (content.getBoundingClientRect().width > availableWidth) {
          maxFontSize = fontSize;
        } else {
          minFontSize = fontSize;
        }
      }
      content.style.fontSize = `${minFontSize}px`;
    };

    fit();
    // Sidebar dragging writes its width directly to the DOM, without a render.
    const observer = new ResizeObserver(fit);
    observer.observe(container);
    return () => observer.disconnect();
  }, [additions, deletions]);

  if (additions <= 0 && deletions <= 0) return null;

  const label = [
    additions > 0 ? `+${formatInteger(additions)}` : "",
    deletions > 0 ? `-${formatInteger(deletions)}` : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <span
      ref={containerRef}
      title={`${label} uncommitted`}
      className="flex h-full w-full min-w-0 items-center justify-center overflow-hidden"
    >
      <span
        ref={contentRef}
        className="flex shrink-0 items-center gap-[0.55em] whitespace-nowrap font-sans text-[11px] font-semibold tabular-nums"
      >
        {additions > 0 ? (
          <span className="text-diff-add-fg">
            +<TightDiffNumber value={additions} />
          </span>
        ) : null}
        {deletions > 0 ? (
          <span className="text-diff-del-fg">
            -<TightDiffNumber value={deletions} />
          </span>
        ) : null}
      </span>
    </span>
  );
}

function TightDiffNumber({ value }: { value: number }) {
  const [first, ...rest] = formatInteger(value).split(",");
  return (
    <>
      {first}
      {rest.map((part, index) => (
        <span key={index}>
          <span className="-mr-px">,</span>
          {part}
        </span>
      ))}
    </>
  );
}

function formatGitLabel(repo?: string, branch?: string): string {
  if (repo && branch) return `${repo}/${branch}`;
  return branch || repo || "";
}

function formatRelative(value: number, now: number): string {
  if (!Number.isFinite(value) || value <= 0) return "";
  const seconds = Math.max(0, Math.round((now - value) / 1000));
  if (seconds < 60) return "now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    const rest = minutes % 60;
    return rest ? `${hours}h ${rest}m` : `${hours}h`;
  }
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  try {
    return new Intl.DateTimeFormat(undefined, {
      month: "short",
      day: "numeric",
    }).format(new Date(value));
  } catch {
    return "";
  }
}
